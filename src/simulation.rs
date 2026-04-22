use crate::block::{BlockId, BlockRecord, BlockStore};
use crate::cbr_failure_fractions::{CBR_FAILURE_FRACTIONS_CHURN, CBR_FAILURE_FRACTIONS_CONTROL};
use crate::config::{DegreeBucket, NetworkConfig, RegionSpec, SimulationConfig};
use crate::consensus::{
    extend_total_difficulty, sample_mining_delay_ms, validate_pow_received, work_target_for_genesis,
};
use crate::event::{BlockPayloadKind, SimEvent};
use crate::network::{block_transfer_ms, sample_latency_ms};
use crate::output::{
    ev_add_block, ev_add_link, ev_add_node, ev_flow_block, ev_simulation_end, EventLog,
};
use crate::routing::{init_outbound_links, neighbor_ids};
use crate::types::NodeId;
use num_bigint::BigUint;
use rand::seq::SliceRandom;
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Normal};
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// Maximum number of blocks kept for propagation export; oldest is evicted when exceeded.
const OBSERVED_PROPAGATION_BLOCK_LIMIT: usize = 10;

/// Queued outbound block response after rec or `getblocktxn` (per-peer send queue).
#[derive(Debug, Clone)]
enum BlockSendItem {
    Rec { requester: NodeId, block: BlockId },
    GetBlockTxn { requester: NodeId, block: BlockId },
}

#[derive(Debug)]
struct NodeState {
    id: NodeId,
    region: usize,
    mining_power: u64,
    use_cbr: bool,
    is_churn: bool,
    max_outbound: usize,
    outbound: Vec<NodeId>,
    inbound: Vec<NodeId>,
    tip: Option<BlockId>,
    /// `(at_ms, seq)` of the single scheduled [`SimEvent::MiningFinished`] for this node, if any.
    pending_mining: Option<(u64, u64)>,
    orphans: Vec<BlockId>,
    /// Serialized block / compact sends to peers (`sending_block` plus the queue above).
    block_send_queue: Vec<BlockSendItem>,
    sending_block: bool,
    /// Blocks for which this node has sent `Rec` and not finished download.
    downloading: HashSet<BlockId>,
}

impl NodeState {
    fn neighbors(&self) -> Vec<NodeId> {
        neighbor_ids(&self.outbound, &self.inbound)
    }
}

#[derive(Debug, Eq, PartialEq)]
struct TimedEvent {
    key: (Reverse<u64>, Reverse<u64>),
    inner: SimEvent,
}

impl Ord for TimedEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

impl PartialOrd for TimedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Owns clock, event queue, nodes, blocks, RNG, and the JSON event log.
pub struct Simulation {
    pub sim: SimulationConfig,
    pub net: NetworkConfig,
    now_ms: u64,
    queue: BinaryHeap<TimedEvent>,
    seq: u64,
    rng: ChaCha8Rng,
    store: BlockStore,
    nodes: Vec<NodeState>,
    propagation: HashMap<BlockId, HashMap<NodeId, u64>>,
    /// Blocks tracked for propagation export, in observation order (FIFO cap).
    propagation_observed_blocks: Vec<BlockId>,
    /// Set mirror of [`Self::propagation_observed_blocks`] for O(1) lookup.
    propagation_observed_set: HashSet<BlockId>,
    /// Per block, node ids in first-seen order (updates do not reorder keys).
    propagation_node_order: HashMap<BlockId, Vec<NodeId>>,
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

/// Per-node draws fixed at construction (region, degree cap, CBR/churn flags).
#[derive(Debug)]
struct NodeInit {
    region: usize,
    max_outbound: usize,
    use_cbr: bool,
    is_churn: bool,
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
                block_send_queue: Vec::new(),
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
            queue: BinaryHeap::new(),
            seq: 0,
            rng,
            store: BlockStore::default(),
            nodes,
            propagation: HashMap::new(),
            propagation_observed_blocks: Vec::new(),
            propagation_observed_set: HashSet::new(),
            propagation_node_order: HashMap::new(),
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
            for &nbr in node.neighbors().iter() {
                if node.id < nbr {
                    self.log.push(ev_add_link(ts, node.id, nbr));
                }
            }
        }
    }

    fn idx(&self, id: NodeId) -> usize {
        id as usize - 1
    }

    fn enqueue_at(&mut self, at_ms: u64, ev: SimEvent) -> (u64, u64) {
        self.seq += 1;
        let seq = self.seq;
        self.queue.push(TimedEvent {
            key: (Reverse(at_ms), Reverse(seq)),
            inner: ev,
        });
        (at_ms, seq)
    }

    fn schedule(&mut self, at_ms: u64, ev: SimEvent) {
        self.enqueue_at(at_ms, ev);
    }

    /// Drop the node's pending mining event from the future event list (stale mining).
    fn cancel_mining_for_node(&mut self, node: NodeId) {
        let i = self.idx(node);
        let Some((at_ms, seq)) = self.nodes[i].pending_mining.take() else {
            return;
        };
        let old = std::mem::take(&mut self.queue);
        for te in old {
            if te.key.0 .0 == at_ms && te.key.1 .0 == seq {
                continue;
            }
            self.queue.push(te);
        }
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
        if self.propagation_observed_set.contains(&block) {
            let delays = self.propagation.entry(block).or_default();
            if let Some(d) = delays.get_mut(&node) {
                *d = delay;
            } else {
                delays.insert(node, delay);
                self.propagation_node_order
                    .entry(block)
                    .or_default()
                    .push(node);
            }
            return;
        }

        if self.propagation_observed_blocks.len() > OBSERVED_PROPAGATION_BLOCK_LIMIT {
            self.evict_oldest_propagation_block();
        }
        self.propagation_observed_blocks.push(block);
        self.propagation_observed_set.insert(block);
        self.propagation
            .insert(block, HashMap::from([(node, delay)]));
        self.propagation_node_order.insert(block, vec![node]);
    }

    fn evict_oldest_propagation_block(&mut self) {
        let Some(evicted) = self.propagation_observed_blocks.first().copied() else {
            return;
        };
        self.propagation_observed_blocks.remove(0);
        self.propagation_observed_set.remove(&evicted);
        self.propagation.remove(&evicted);
        self.propagation_node_order.remove(&evicted);
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
        while let Some(te) = self.queue.pop() {
            let t = te.key.0 .0;
            self.now_ms = t;
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

    pub fn write_json_events(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        self.log.write_array(w)
    }

    /// Writes `static.json`: region id/name list for downstream visualization.
    pub fn write_static_json(&self, path: &Path) -> std::io::Result<()> {
        let mut w = BufWriter::new(File::create(path)?);
        let regions: Vec<serde_json::Value> = self
            .net
            .topology
            .regions
            .iter()
            .enumerate()
            .map(|(id, r)| {
                serde_json::json!({
                    "id": id,
                    "name": r.name,
                })
            })
            .collect();
        serde_json::to_writer(&mut w, &serde_json::json!({ "region": regions }))?;
        writeln!(&mut w)?;
        w.flush()?;
        Ok(())
    }

    /// Prints propagation matrices to `w`: one block as `block_id:height`, then lines
    /// `node_id,delay_ms`, then a blank line.
    pub fn write_propagation_human(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        for &block in &self.propagation_observed_blocks {
            let Some(rec) = self.store.get(block) else {
                continue;
            };
            writeln!(w, "{}:{}", block.0, rec.height)?;
            let Some(delays) = self.propagation.get(&block) else {
                continue;
            };
            let order = self
                .propagation_node_order
                .get(&block)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            for &node in order {
                if let Some(&d) = delays.get(&node) {
                    writeln!(w, "{node},{d}")?;
                }
            }
            writeln!(w)?;
        }
        Ok(())
    }

    /// CSV with header `block_id,height,node_id,delay_ms` (same ordering as [`Self::write_propagation_human`]).
    pub fn write_propagation_csv(&self, path: &Path) -> std::io::Result<()> {
        let mut w = BufWriter::new(File::create(path)?);
        writeln!(w, "block_id,height,node_id,delay_ms")?;
        for &block in &self.propagation_observed_blocks {
            let Some(rec) = self.store.get(block) else {
                continue;
            };
            let Some(delays) = self.propagation.get(&block) else {
                continue;
            };
            if let Some(order) = self.propagation_node_order.get(&block) {
                for &node in order {
                    if let Some(&d) = delays.get(&node) {
                        writeln!(w, "{},{},{},{}", block.0, rec.height, node, d)?;
                    }
                }
            }
        }
        w.flush()?;
        Ok(())
    }

    /// Directed edges `from to` per line (one line per adjacency).
    fn graph_topology_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        for node in &self.nodes {
            for &nbr in node.neighbors().iter() {
                writeln!(&mut buf, "{} {}", node.id, nbr)?;
            }
        }
        Ok(buf)
    }

    /// Writes `graph/<n>.txt` for each recorded snapshot label (typically `2` and multiples of `100`).
    ///
    /// If no snapshot was recorded (e.g. very low `end_block_height`), falls back to
    /// `0..=max_tip_height()` so at least one file exists.
    ///
    /// The routing graph is fixed after initialization, so every snapshot file contains the same
    /// edge list.
    pub fn write_graph_dir(&self, out_dir: &Path) -> std::io::Result<()> {
        let graph_dir = out_dir.join("graph");
        std::fs::create_dir_all(&graph_dir)?;
        let bytes = self.graph_topology_bytes()?;
        let labels: Vec<u32> = if self.graph_snapshot_heights.is_empty() {
            (0..=self.max_tip_height()).collect()
        } else {
            self.graph_snapshot_heights.clone()
        };
        for h in labels {
            std::fs::write(graph_dir.join(format!("{h}.txt")), &bytes)?;
        }
        Ok(())
    }

    /// Writes `blockList.txt`: blocks on node 1's main chain (excluding genesis) plus all orphans,
    /// sorted by block time then id; each line `OnChain : height : id` or `Orphan : height : id`.
    pub fn write_block_list_txt(&self, path: &Path) -> std::io::Result<()> {
        let mut orphans: HashSet<BlockId> = HashSet::new();
        for node in &self.nodes {
            for &o in &node.orphans {
                orphans.insert(o);
            }
        }

        let mut chain_blocks: HashSet<BlockId> = HashSet::new();
        let mut cur = self.nodes.first().and_then(|n| n.tip);
        while let Some(bid) = cur {
            let rec = match self.store.get(bid) {
                Some(r) => r,
                None => break,
            };
            if rec.parent.is_none() {
                break;
            }
            chain_blocks.insert(bid);
            cur = rec.parent;
        }

        let mut all: Vec<BlockId> = chain_blocks
            .into_iter()
            .chain(orphans.iter().copied())
            .collect();
        all.sort_by_key(|&bid| {
            let t = self.store.get(bid).map(|r| r.time_ms).unwrap_or(0);
            (t, bid.0)
        });
        all.dedup();

        let mut w = BufWriter::new(File::create(path)?);
        for bid in all {
            let Some(rec) = self.store.get(bid) else {
                continue;
            };
            let label = if orphans.contains(&bid) {
                "Orphan"
            } else {
                "OnChain"
            };
            writeln!(w, "{} : {} : {}", label, rec.height, bid.0)?;
        }
        w.flush()?;
        Ok(())
    }

    /// Maximum block height among all node tips (genesis is height 0).
    pub fn max_tip_height(&self) -> u32 {
        self.nodes
            .iter()
            .filter_map(|n| n.tip.and_then(|b| self.store.get(b).map(|r| r.height)))
            .max()
            .unwrap_or(0)
    }

    fn on_mining_finished(&mut self, node: NodeId, parent: BlockId, difficulty: BigUint) {
        let i = self.idx(node);
        self.nodes[i].pending_mining = None;
        let n = &self.nodes[i];
        if n.tip != Some(parent) {
            return;
        }
        let parent_rec = self.store.get(parent).expect("parent").clone();

        let ph = parent_rec.height;
        if ph == self.mint_stop_layer {
            self.mint_stop_layer += 1;
        }
        if self.mint_stop_layer > self.sim.end_block_height {
            self.finished = true;
            self.queue.clear();
            return;
        }
        let c = self.mint_stop_layer;
        if (c == 2 || c.is_multiple_of(100))
            && self.graph_snapshot_heights.last().copied() != Some(c)
        {
            self.graph_snapshot_heights.push(c);
        }

        let rec = BlockRecord {
            parent: Some(parent),
            height: parent_rec.height + 1,
            time_ms: self.now_ms,
            minter: node,
            difficulty: difficulty.clone(),
            total_difficulty: extend_total_difficulty(&parent_rec, &difficulty),
            next_difficulty: parent_rec.next_difficulty.clone(),
        };
        let bid = self.store.insert(rec);
        self.apply_new_block_to_node(node, bid, true);
    }

    fn apply_new_block_to_node(&mut self, node: NodeId, block: BlockId, _mined_locally: bool) {
        let rec = match self.store.get(block) {
            Some(r) => r.clone(),
            None => return,
        };
        let i = self.idx(node);
        if self.nodes[i].tip == Some(block) {
            return;
        }
        let tip_rec = self.nodes[i].tip.and_then(|t| self.store.get(t).cloned());
        if !validate_pow_received(&rec, tip_rec.as_ref(), &self.store) {
            return;
        }
        if let Some(ref cur) = tip_rec {
            if rec.total_difficulty <= cur.total_difficulty {
                return;
            }
        }
        if let Some(old) = self.nodes[i].tip {
            if old != block && !block_extends_from(&self.store, block, old) {
                self.nodes[i].orphans.push(old);
            }
        }
        self.cancel_mining_for_node(node);
        self.nodes[i].tip = Some(block);
        self.nodes[i].downloading.remove(&block);
        let delay = self.now_ms.saturating_sub(rec.time_ms);
        self.record_propagation(block, node, delay);
        self.log.push(ev_add_block(self.now_ms, node, block.0));
        if !self.finished {
            self.schedule_mining(node, block);
            self.send_inv(node, block);
        }
    }

    fn schedule_mining(&mut self, node: NodeId, parent: BlockId) {
        let parent_rec = match self.store.get(parent) {
            Some(r) => r,
            None => return,
        };
        let i = self.idx(node);
        let power = self.nodes[i].mining_power;
        let delay = sample_mining_delay_ms(&mut self.rng, &parent_rec.next_difficulty, power);
        let at_ms = self.now_ms + delay;
        let key = self.enqueue_at(
            at_ms,
            SimEvent::MiningFinished {
                node,
                parent,
                difficulty: parent_rec.next_difficulty.clone(),
            },
        );
        self.nodes[i].pending_mining = Some(key);
    }

    /// Flood `inv` to each neighbor; one small-message delay per hop.
    fn send_inv(&mut self, from: NodeId, block: BlockId) {
        let from_region = self.nodes[self.idx(from)].region;
        let nbrs = self.nodes[self.idx(from)].neighbors();
        for &to in &nbrs {
            let to_region = self.nodes[self.idx(to)].region;
            let d = small_msg_delay_ms(
                &mut self.rng,
                &self.net,
                from_region,
                to_region,
                self.sim.message_overhead_ms,
            );
            self.schedule(self.now_ms + d, SimEvent::InvArrive { to, from, block });
        }
    }

    fn on_inv_arrive(&mut self, recipient: NodeId, inv_peer: NodeId, block: BlockId) {
        let i = self.idx(recipient);
        if self.nodes[i].orphans.contains(&block) || self.nodes[i].downloading.contains(&block) {
            return;
        }
        let Some(rec_ref) = self.store.get(block) else {
            return;
        };
        let tip = self.nodes[i].tip;
        let valid =
            validate_pow_received(rec_ref, tip.and_then(|t| self.store.get(t)), &self.store);
        let same_chain = tip
            .map(|t| is_on_same_chain(&self.store, block, t))
            .unwrap_or(false);
        if !valid && (tip.is_none() || same_chain) {
            return;
        }
        let from_reg = self.nodes[i].region;
        let to_reg = self.nodes[self.idx(inv_peer)].region;
        let d = small_msg_delay_ms(
            &mut self.rng,
            &self.net,
            from_reg,
            to_reg,
            self.sim.message_overhead_ms,
        );
        self.nodes[i].downloading.insert(block);
        self.schedule(
            self.now_ms + d,
            SimEvent::RecArrive {
                to: inv_peer,
                from: recipient,
                block,
            },
        );
    }

    fn on_rec_arrive(&mut self, recipient: NodeId, rec_peer: NodeId, block: BlockId) {
        let i = self.idx(recipient);
        self.nodes[i].block_send_queue.push(BlockSendItem::Rec {
            requester: rec_peer,
            block,
        });
        self.try_start_next_block_send(recipient);
    }

    fn on_get_block_txn_arrive(&mut self, recipient: NodeId, peer: NodeId, block: BlockId) {
        let i = self.idx(recipient);
        self.nodes[i]
            .block_send_queue
            .push(BlockSendItem::GetBlockTxn {
                requester: peer,
                block,
            });
        self.try_start_next_block_send(recipient);
    }

    /// If not already sending, dequeue the next block payload and schedule its arrival.
    fn try_start_next_block_send(&mut self, sender: NodeId) {
        let si = self.idx(sender);
        if self.nodes[si].sending_block {
            return;
        }
        let job = match self.nodes[si].block_send_queue.first() {
            Some(j) => j.clone(),
            None => return,
        };
        let requester = match &job {
            BlockSendItem::Rec { requester, .. } | BlockSendItem::GetBlockTxn { requester, .. } => {
                *requester
            }
        };
        let block = match &job {
            BlockSendItem::Rec { block, .. } | BlockSendItem::GetBlockTxn { block, .. } => *block,
        };
        let sender_reg = self.nodes[si].region;
        let req_reg = self.nodes[self.idx(requester)].region;
        let (kind, payload_bytes) = match &job {
            BlockSendItem::Rec { .. } => {
                let use_compact = self.nodes[si].use_cbr && self.nodes[self.idx(requester)].use_cbr;
                if use_compact {
                    (BlockPayloadKind::Compact, self.sim.compact_block_size_bytes)
                } else {
                    (BlockPayloadKind::Full, self.sim.block_size_bytes)
                }
            }
            BlockSendItem::GetBlockTxn { .. } => {
                let b = sample_failed_block_bytes(
                    &mut self.rng,
                    self.sim.block_size_bytes,
                    self.nodes[si].is_churn,
                );
                (BlockPayloadKind::Full, b)
            }
        };
        let interval = sample_latency_ms(&mut self.rng, &self.net, sender_reg, req_reg)
            + block_transfer_ms(
                &self.net,
                sender_reg,
                req_reg,
                payload_bytes,
                self.sim.processing_time_ms,
            );
        let at = self.now_ms + interval;
        let depart_ms = at.saturating_sub(interval);
        self.nodes[si].block_send_queue.remove(0);
        self.nodes[si].sending_block = true;
        self.schedule(
            at,
            SimEvent::BlockPayloadArrive {
                recipient: requester,
                sender,
                block,
                depart_ms,
                kind,
            },
        );
    }

    /// When a payload has been delivered, clear `sending_block` and start the next queued send if any.
    fn continue_sender_block_queue(&mut self, sender: NodeId) {
        let si = self.idx(sender);
        self.nodes[si].sending_block = false;
        self.try_start_next_block_send(sender);
    }

    fn on_block_payload_arrive(
        &mut self,
        recipient: NodeId,
        sender: NodeId,
        block: BlockId,
        depart_ms: u64,
        kind: BlockPayloadKind,
    ) {
        self.log.push(ev_flow_block(
            depart_ms,
            self.now_ms,
            sender,
            recipient,
            block.0,
        ));
        self.continue_sender_block_queue(sender);
        let ri = self.idx(recipient);
        match kind {
            BlockPayloadKind::Full => {
                self.nodes[ri].downloading.remove(&block);
                self.apply_new_block_to_node(recipient, block, false);
            }
            BlockPayloadKind::Compact => {
                let fail_p = if self.nodes[ri].is_churn {
                    self.sim.cbr_failure_rate_churn
                } else {
                    self.sim.cbr_failure_rate_control
                };
                if self.rng.gen_bool(fail_p) {
                    let from_reg = self.nodes[ri].region;
                    let to_reg = self.nodes[self.idx(sender)].region;
                    let d = small_msg_delay_ms(
                        &mut self.rng,
                        &self.net,
                        from_reg,
                        to_reg,
                        self.sim.message_overhead_ms,
                    );
                    self.schedule(
                        self.now_ms + d,
                        SimEvent::GetBlockTxnArrive {
                            to: sender,
                            from: recipient,
                            block,
                        },
                    );
                } else {
                    self.nodes[ri].downloading.remove(&block);
                    self.apply_new_block_to_node(recipient, block, false);
                }
            }
        }
    }
}

fn small_msg_delay_ms<R: Rng>(
    rng: &mut R,
    net: &NetworkConfig,
    from_reg: usize,
    to_reg: usize,
    overhead_ms: u64,
) -> u64 {
    sample_latency_ms(rng, net, from_reg, to_reg) + overhead_ms
}

/// Whether `block` and `tip` are on the same chain (compare [`BlockId`] at the aligned height).
fn is_on_same_chain(store: &BlockStore, block: BlockId, tip: BlockId) -> bool {
    let Some(rb) = store.get(block) else {
        return false;
    };
    let Some(rt) = store.get(tip) else {
        return false;
    };
    if rb.height <= rt.height {
        let mut cur = Some(tip);
        for _ in 0..(rt.height - rb.height) {
            cur = cur.and_then(|id| store.get(id).and_then(|r| r.parent));
        }
        cur == Some(block)
    } else {
        let mut cur = Some(block);
        for _ in 0..(rb.height - rt.height) {
            cur = cur.and_then(|id| store.get(id).and_then(|r| r.parent));
        }
        cur == Some(tip)
    }
}

/// Sample full-block byte size after compact decode failure (`block_size_bytes` times a table entry).
fn sample_failed_block_bytes<R: Rng>(
    rng: &mut R,
    block_size_bytes: u64,
    sender_is_churn: bool,
) -> u64 {
    let dist = if sender_is_churn {
        CBR_FAILURE_FRACTIONS_CHURN
    } else {
        CBR_FAILURE_FRACTIONS_CONTROL
    };
    let idx = rng.gen_range(0..dist.len());
    ((block_size_bytes as f64) * f64::from(dist[idx])).round() as u64
}

fn block_extends_from(store: &BlockStore, tip: BlockId, ancestor: BlockId) -> bool {
    let mut cur = Some(tip);
    while let Some(id) = cur {
        if id == ancestor {
            return true;
        }
        cur = store.get(id).and_then(|r| r.parent);
    }
    false
}

fn pick_genesis_minter<R: Rng + ?Sized>(rng: &mut R, nodes: &[NodeState]) -> NodeId {
    let total: u128 = nodes.iter().map(|n| n.mining_power as u128).sum();
    if total == 0 {
        return nodes[0].id;
    }
    let r = rng.gen_range(0u128..total);
    let mut c = 0u128;
    for n in nodes {
        c += n.mining_power as u128;
        if r < c {
            return n.id;
        }
    }
    nodes.last().unwrap().id
}

fn sample_mining_power<R: Rng + ?Sized>(rng: &mut R, mean: u64, stdev: u64) -> u64 {
    let n = Normal::new(mean as f64, stdev as f64).expect("mining power mean/stdev");
    let v = n.sample(rng).round() as i64;
    v.max(1) as u64
}

fn sample_node_inits<R: Rng + ?Sized>(
    rng: &mut R,
    n: usize,
    net: &NetworkConfig,
    sim: &SimulationConfig,
) -> Vec<NodeInit> {
    let regions = region_assign_list(rng, n, net.topology.regions);
    let max_out = if let Some(cdf) = net.outdegree_cdf {
        sample_outdegrees_from_cdf(rng, n, cdf)
    } else {
        degree_bucket_assign_list(rng, n, net.degree_buckets)
    };
    let use_cbr = bool_rate_list(rng, n, sim.cbr_usage_rate);
    let churn = bool_rate_list(rng, n, sim.churn_node_rate);
    debug_assert_eq!(regions.len(), n);
    debug_assert_eq!(max_out.len(), n);
    (0..n)
        .map(|i| NodeInit {
            region: regions[i],
            max_outbound: max_out[i],
            use_cbr: use_cbr[i],
            is_churn: churn[i],
        })
        .collect()
}

fn region_assign_list<R: Rng + ?Sized>(
    rng: &mut R,
    num: usize,
    regions: &[RegionSpec],
) -> Vec<usize> {
    let mut list = Vec::new();
    let mut ac = 0.0f64;
    for (idx, r) in regions.iter().enumerate() {
        ac += r.node_fraction;
        let target = ((num as f64) * ac).floor() as usize;
        while list.len() < target {
            list.push(idx);
        }
    }
    while list.len() < num {
        list.push(regions.len().saturating_sub(1));
    }
    list.shuffle(rng);
    list
}

fn degree_bucket_assign_list<R: Rng + ?Sized>(
    rng: &mut R,
    num: usize,
    buckets: &[DegreeBucket],
) -> Vec<usize> {
    let mut list = Vec::new();
    for b in buckets {
        let target = ((num as f64) * b.cumulative).floor() as usize;
        while list.len() < target {
            list.push(b.max_outbound);
        }
    }
    let fallback = buckets.last().map(|b| b.max_outbound).unwrap_or(1);
    while list.len() < num {
        list.push(fallback);
    }
    list.shuffle(rng);
    list
}

/// Build a multiset of outdegree caps from a cumulative CDF (length may exceed `n`).
pub(crate) fn expand_outdegree_list_from_cdf(n: usize, cdf: &'static [f64]) -> Vec<usize> {
    if cdf.is_empty() {
        return vec![1; n];
    }
    let mut list = Vec::new();
    let mut index = 0usize;
    while index < cdf.len() {
        let thresh = n as f64 * cdf[index];
        while (list.len() as f64) <= thresh {
            list.push(index + 1);
        }
        index += 1;
    }
    while list.len() < n {
        list.push(cdf.len() + 1);
    }
    list
}

/// Shuffle then keep the first `n` entries (extra draws are dropped).
fn sample_outdegrees_from_cdf<R: Rng + ?Sized>(
    rng: &mut R,
    n: usize,
    cdf: &'static [f64],
) -> Vec<usize> {
    let mut list = expand_outdegree_list_from_cdf(n, cdf);
    list.shuffle(rng);
    list.truncate(n);
    list
}

fn bool_rate_list<R: Rng + ?Sized>(rng: &mut R, num: usize, rate: f64) -> Vec<bool> {
    let mut v: Vec<bool> = (0..num).map(|i| (i as f64) < (num as f64) * rate).collect();
    v.shuffle(rng);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(h.contains(":0\n") || h.contains(":1\n")); // height line
        assert!(h.contains(',')); // node,delay

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

    fn count_event_kind(log: &EventLog, kind: &str) -> usize {
        log.events
            .iter()
            .filter(|e| e.get("kind").and_then(|k| k.as_str()) == Some(kind))
            .count()
    }

    #[test]
    fn is_on_same_chain_same_block() {
        let mut store = BlockStore::default();
        let g = store.insert(BlockRecord {
            parent: None,
            height: 0,
            time_ms: 0,
            minter: 1,
            difficulty: BigUint::from(0u32),
            total_difficulty: BigUint::from(0u32),
            next_difficulty: BigUint::from(1u32),
        });
        assert!(is_on_same_chain(&store, g, g));
    }

    #[test]
    fn is_on_same_chain_ancestor_and_descendant() {
        let mut store = BlockStore::default();
        let g = store.insert(BlockRecord {
            parent: None,
            height: 0,
            time_ms: 0,
            minter: 1,
            difficulty: BigUint::from(0u32),
            total_difficulty: BigUint::from(0u32),
            next_difficulty: BigUint::from(10u32),
        });
        let b1 = store.insert(BlockRecord {
            parent: Some(g),
            height: 1,
            time_ms: 1,
            minter: 2,
            difficulty: BigUint::from(10u32),
            total_difficulty: BigUint::from(10u32),
            next_difficulty: BigUint::from(10u32),
        });
        let b2 = store.insert(BlockRecord {
            parent: Some(b1),
            height: 2,
            time_ms: 2,
            minter: 2,
            difficulty: BigUint::from(10u32),
            total_difficulty: BigUint::from(20u32),
            next_difficulty: BigUint::from(10u32),
        });
        assert!(is_on_same_chain(&store, g, b2));
        assert!(is_on_same_chain(&store, b1, b2));
        assert!(is_on_same_chain(&store, b2, g));
    }

    #[test]
    fn is_on_same_chain_false_for_sibling_forks() {
        let mut store = BlockStore::default();
        let g = store.insert(BlockRecord {
            parent: None,
            height: 0,
            time_ms: 0,
            minter: 1,
            difficulty: BigUint::from(0u32),
            total_difficulty: BigUint::from(0u32),
            next_difficulty: BigUint::from(10u32),
        });
        let left = store.insert(BlockRecord {
            parent: Some(g),
            height: 1,
            time_ms: 1,
            minter: 2,
            difficulty: BigUint::from(10u32),
            total_difficulty: BigUint::from(10u32),
            next_difficulty: BigUint::from(10u32),
        });
        let right = store.insert(BlockRecord {
            parent: Some(g),
            height: 1,
            time_ms: 2,
            minter: 3,
            difficulty: BigUint::from(10u32),
            total_difficulty: BigUint::from(10u32),
            next_difficulty: BigUint::from(10u32),
        });
        assert!(!is_on_same_chain(&store, left, right));
    }

    #[test]
    fn sample_failed_block_bytes_within_block_size() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        for _ in 0..50 {
            let b = sample_failed_block_bytes(&mut rng, 535_000, false);
            assert!(b > 0 && b <= 535_000);
            let b2 = sample_failed_block_bytes(&mut rng, 535_000, true);
            assert!(b2 > 0 && b2 <= 535_000);
        }
    }

    #[test]
    fn dogecoin_cdf_oversized_list_len_and_counts() {
        use crate::config::NetworkConfig;
        let cdf = NetworkConfig::dogecoin()
            .outdegree_cdf
            .expect("doge preset uses cdf");
        let v = expand_outdegree_list_from_cdf(300, cdf);
        assert_eq!(v.len(), 301);
        assert_eq!(v.iter().filter(|&&c| c == 1).count(), 1);
        assert_eq!(v.iter().filter(|&&c| c == 8).count(), 300);
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
            .map(|n| n.neighbors().len())
            .unwrap_or(0);
        assert!(
            n >= minter.max(1),
            "expected flow-block count >= minter degree {minter}, got {n}"
        );
    }
}
