//! Discrete-event simulation core: clock, event priority queue, node state, RNG, and
//! gossip/mining handlers. Persisted outputs live in [`export`]; sampling helpers in [`init`].

mod chain;
mod event_queue;
mod export;
mod handlers;
mod init;
mod node;
mod propagation;

use self::event_queue::EventQueue;
use self::init::{pick_genesis_minter, sample_mining_power, sample_node_inits};
use self::node::NodeState;
use self::propagation::PropagationTracker;
use crate::block::{BlockId, BlockRecord, BlockStore};
use crate::config::{NetworkConfig, SimulationConfig};
use crate::consensus::work_target_for_genesis;
use crate::event::SimEvent;
use crate::output::{ev_add_block, ev_add_link, ev_add_node, ev_simulation_end, EventLog};
use crate::routing::init_outbound_links;
use crate::types::NodeId;
use num_bigint::BigUint;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use std::collections::{HashSet, VecDeque};

/// Maximum number of blocks kept for propagation export; oldest is evicted when exceeded.
const OBSERVED_PROPAGATION_BLOCK_LIMIT: usize = 10;

/// Owns clock, event queue, nodes, blocks, RNG, and the JSON event log.
pub struct Simulation {
    pub sim: SimulationConfig,
    pub net: NetworkConfig,
    now_ms: u64,
    queue: EventQueue,
    rng: ChaCha8Rng,
    store: BlockStore,
    nodes: Vec<NodeState>,
    propagation: PropagationTracker,
    log: EventLog,
    finished: bool,
    /// Counter (starts at 1) used with parent block height to decide early stop and graph labels.
    mint_stop_layer: u32,
    /// Labels used for `graph/<n>.txt` snapshots during [`Self::run`].
    graph_snapshot_heights: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct RunStats {
    pub final_time_ms: u64,
    pub blocks: usize,
    pub events_logged: usize,
}

impl Simulation {
    pub fn new(sim: SimulationConfig, net: NetworkConfig) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(sim.rng_seed);
        let n = sim.num_nodes as usize;
        let inits = sample_node_inits(&mut rng, n, &net, &sim);

        let mut nodes = Vec::with_capacity(n);
        let mut log = EventLog::default();
        for (i, init) in inits.iter().enumerate() {
            let id = (i + 1) as NodeId;
            nodes.push(NodeState {
                id,
                region: init.region,
                mining_power: sample_mining_power(
                    &mut rng,
                    sim.average_mining_power,
                    sim.stdev_mining_power,
                ),
                use_cbr: init.use_cbr,
                is_churn: init.is_churn,
                max_outbound: init.max_outbound,
                outbound: Vec::new(),
                inbound: Vec::new(),
                tip: None,
                pending_mining: None,
                orphans: Vec::new(),
                block_send_queue: VecDeque::new(),
                sending_block: false,
                downloading: HashSet::new(),
            });
            log.push(ev_add_node(0, id, init.region));
        }

        let max_out: Vec<usize> = nodes.iter().map(|x| x.max_outbound).collect();
        let mut out: Vec<Vec<NodeId>> = vec![Vec::new(); n];
        let mut inn: Vec<Vec<NodeId>> = vec![Vec::new(); n];
        init_outbound_links(&mut rng, &max_out, &mut out, &mut inn);
        for (i, node) in nodes.iter_mut().enumerate() {
            node.outbound = std::mem::take(&mut out[i]);
            node.inbound = std::mem::take(&mut inn[i]);
        }

        let mut sim_obj = Self {
            sim,
            net,
            now_ms: 0,
            queue: EventQueue::new(),
            rng,
            store: BlockStore::default(),
            nodes,
            propagation: PropagationTracker::with_capacity(OBSERVED_PROPAGATION_BLOCK_LIMIT),
            log,
            finished: false,
            mint_stop_layer: 1,
            graph_snapshot_heights: Vec::new(),
        };

        sim_obj.log_links_at_time(0);
        sim_obj.bootstrap_genesis();
        sim_obj
    }

    fn log_links_at_time(&mut self, ts: u64) {
        for node in &self.nodes {
            for nbr in node.neighbors_sorted() {
                if node.id < nbr {
                    self.log.push(ev_add_link(ts, node.id, nbr));
                }
            }
        }
    }

    #[inline]
    fn idx(&self, id: NodeId) -> usize {
        id as usize - 1
    }

    fn enqueue_at(&mut self, at_ms: u64, ev: SimEvent) -> (u64, u64) {
        self.queue.push(at_ms, ev)
    }

    fn schedule(&mut self, at_ms: u64, ev: SimEvent) {
        self.enqueue_at(at_ms, ev);
    }

    /// Mark the node's pending mining event as stale (tombstone). The entry is skipped on pop.
    fn cancel_mining_for_node(&mut self, node: NodeId) {
        let i = self.idx(node);
        let Some((_, seq)) = self.nodes[i].pending_mining.take() else {
            return;
        };
        self.queue.cancel(seq);
    }

    fn bootstrap_genesis(&mut self) {
        let minter = pick_genesis_minter(&mut self.rng, &self.nodes);
        let total_pow: u128 = self.nodes.iter().map(|n| n.mining_power as u128).sum();
        let next_d = work_target_for_genesis(total_pow, self.sim.target_block_interval_ms);
        let gen = BlockRecord {
            parent: None,
            height: 0,
            time_ms: 0,
            minter,
            difficulty: BigUint::from(0u32),
            total_difficulty: BigUint::from(0u32),
            next_difficulty: next_d,
        };
        let gid = self.store.insert(gen);
        self.record_propagation(gid, minter, 0);
        let mi = self.idx(minter);
        self.nodes[mi].tip = Some(gid);
        self.log.push(ev_add_block(0, minter, gid.0));
        self.schedule_mining(minter, gid);
        self.send_inv(minter, gid);
    }

    fn record_propagation(&mut self, block: BlockId, node: NodeId, delay: u64) {
        self.propagation.record(block, node, delay);
    }

    /// Runs the discrete-event loop until the queue is empty.
    ///
    /// When [`Self::mint_stop_layer`] would exceed [`SimulationConfig::end_block_height`] after a
    /// valid mint, the pending event queue is cleared and the run ends without processing further
    /// events.
    pub fn run(&mut self) -> RunStats {
        self.run_observer(0, |_, _| {})
    }

    /// Same as [`Self::run`], but invokes `on_step` every `every` processed events (and once at the
    /// end) when `every > 0`. Use for CLI progress (`every == 0` skips callbacks).
    pub fn run_observer(
        &mut self,
        every: u64,
        mut on_step: impl FnMut(u64, &Simulation),
    ) -> RunStats {
        let mut i = 0u64;
        while let Some(te) = self.queue.pop_live() {
            self.now_ms = te.at_ms();
            match te.inner {
                SimEvent::MiningFinished {
                    node,
                    parent,
                    difficulty,
                } => self.on_mining_finished(node, parent, difficulty),
                SimEvent::InvArrive { to, from, block } => {
                    self.on_inv_arrive(to, from, block);
                }
                SimEvent::RecArrive { to, from, block } => {
                    self.on_rec_arrive(to, from, block);
                }
                SimEvent::GetBlockTxnArrive { to, from, block } => {
                    self.on_get_block_txn_arrive(to, from, block);
                }
                SimEvent::BlockPayloadArrive {
                    recipient,
                    sender,
                    block,
                    depart_ms,
                    kind,
                } => self.on_block_payload_arrive(recipient, sender, block, depart_ms, kind),
            }
            i += 1;
            if every > 0 && i.is_multiple_of(every) {
                on_step(i, self);
            }
        }
        if every > 0 {
            on_step(i, self);
        }
        self.log.push(ev_simulation_end(self.now_ms));
        RunStats {
            final_time_ms: self.now_ms,
            blocks: self.store.len(),
            events_logged: self.log.events.len(),
        }
    }

    /// Simulated clock (ms) after the last processed event.
    pub fn clock_ms(&self) -> u64 {
        self.now_ms
    }

    /// Number of pending discrete events.
    pub fn pending_events(&self) -> usize {
        self.queue.len()
    }

    /// Layer milestone used with [`SimulationConfig::end_block_height`] to stop the run.
    pub fn mint_stop_layer(&self) -> u32 {
        self.mint_stop_layer
    }

    /// Number of blocks in the store (genesis, chain, orphans).
    pub fn block_store_len(&self) -> usize {
        self.store.len()
    }

    /// Maximum block height among all node tips (genesis is height 0).
    pub fn max_tip_height(&self) -> u32 {
        self.nodes
            .iter()
            .filter_map(|n| n.tip.and_then(|b| self.store.get(b).map(|r| r.height)))
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::EventLog;

    fn count_event_kind(log: &EventLog, kind: &str) -> usize {
        log.events
            .iter()
            .filter(|e| e.get("kind").and_then(|k| k.as_str()) == Some(kind))
            .count()
    }

    #[test]
    fn tiny_sim_reaches_end_height() {
        let mut s = Simulation::new(SimulationConfig::tiny(), NetworkConfig::default());
        let st = s.run();
        assert!(st.blocks >= 1);
        assert!(st.final_time_ms > 0);
        assert!(st.events_logged >= 3);
    }

    #[test]
    fn tiny_sim_writes_graph_and_block_list() {
        let dir = std::env::temp_dir().join(format!("simblock-rs-analysis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut s = Simulation::new(SimulationConfig::tiny(), NetworkConfig::default());
        s.run();
        s.write_graph_dir(&dir).unwrap();
        s.write_block_list_txt(&dir.join("blockList.txt")).unwrap();

        assert!(dir.join("graph/2.txt").exists());
        let max_h = s.max_tip_height();
        assert!(max_h >= 2, "tiny sim should reach at least height 2");
        assert!(!dir.join("graph/0.txt").exists());
        let bl = std::fs::read_to_string(dir.join("blockList.txt")).unwrap();
        assert!(bl.contains("OnChain"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tiny_sim_writes_static_propagation_csv_and_human() {
        let dir =
            std::env::temp_dir().join(format!("simblock-rs-propagation-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut s = Simulation::new(SimulationConfig::tiny(), NetworkConfig::default());
        s.write_static_json(&dir.join("static.json")).unwrap();
        s.run();
        s.write_propagation_csv(&dir.join("propagation.csv"))
            .unwrap();

        let static_raw = std::fs::read_to_string(dir.join("static.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&static_raw).unwrap();
        assert!(v.get("region").and_then(|r| r.as_array()).is_some());

        let csv = std::fs::read_to_string(dir.join("propagation.csv")).unwrap();
        assert!(csv.starts_with("block_id,height,node_id,delay_ms\n"));

        let mut human = Vec::new();
        s.write_propagation_human(&mut human).unwrap();
        let h = String::from_utf8(human).unwrap();
        assert!(h.contains(":0\n") || h.contains(":1\n"));
        assert!(h.contains(','));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn propagation_csv_respects_observed_block_cap() {
        let cfg = SimulationConfig {
            end_block_height: 40,
            ..Default::default()
        };
        let mut s = Simulation::new(cfg, NetworkConfig::default());
        s.run();
        let dir = std::env::temp_dir().join(format!(
            "simblock-rs-prop-observed-cap-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        s.write_propagation_csv(&dir.join("propagation.csv"))
            .unwrap();
        let csv = std::fs::read_to_string(dir.join("propagation.csv")).unwrap();
        let mut block_ids = HashSet::new();
        for line in csv.lines().skip(1) {
            if line.is_empty() {
                continue;
            }
            if let Some(id) = line.split(',').next() {
                block_ids.insert(id.to_string());
            }
        }
        assert!(
            block_ids.len() <= 11,
            "at most 11 distinct blocks in propagation export; got {}",
            block_ids.len()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// INV→Rec→ペイロード経路では `flow-block` が記録される（旧モデルの単一結合ホップとは別）。
    #[test]
    fn gossip_emits_flow_block_for_block_payload() {
        let mut s = Simulation::new(SimulationConfig::tiny(), NetworkConfig::default());
        s.run();
        let n = count_event_kind(&s.log, "flow-block");
        assert!(
            n >= 1,
            "expected at least one flow-block (compact/full payload), got {n}"
        );
    }

    /// ジェネシスを複数ピアへ広げると、ピア数に応じて複数の `flow-block` が付く。
    #[test]
    fn gossip_genesis_fanout_multiple_flow_blocks() {
        let mut s = Simulation::new(SimulationConfig::tiny(), NetworkConfig::default());
        s.run();
        let n = count_event_kind(&s.log, "flow-block");
        let minter = s
            .nodes
            .iter()
            .find(|n| n.tip.is_some())
            .map(|n| n.neighbor_count())
            .unwrap_or(0);
        assert!(
            n >= minter.max(1),
            "expected flow-block count >= minter degree {minter}, got {n}"
        );
    }
}
