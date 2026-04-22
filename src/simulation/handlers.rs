//! Event handlers: mining completion, gossip (inv/rec/getblocktxn), and block payload arrival.

use super::chain::{block_extends_from, is_on_same_chain};
use super::init::{sample_failed_block_bytes, small_msg_delay_ms};
use super::node::{BlockSendJob, BlockSendKind};
use super::Simulation;
use crate::block::{BlockId, BlockRecord};
use crate::consensus::{extend_total_difficulty, sample_mining_delay_ms, validate_pow_received};
use crate::event::{BlockPayloadKind, SimEvent};
use crate::network::{block_transfer_ms, sample_latency_ms};
use crate::output::{ev_add_block, ev_flow_block};
use crate::types::NodeId;
use num_bigint::BigUint;
use rand::Rng;

impl Simulation {
    pub(super) fn on_mining_finished(
        &mut self,
        node: NodeId,
        parent: BlockId,
        difficulty: BigUint,
    ) {
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

    pub(super) fn apply_new_block_to_node(
        &mut self,
        node: NodeId,
        block: BlockId,
        _mined_locally: bool,
    ) {
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

    pub(super) fn schedule_mining(&mut self, node: NodeId, parent: BlockId) {
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
    pub(super) fn send_inv(&mut self, from: NodeId, block: BlockId) {
        let sender = &self.nodes[self.idx(from)];
        let from_region = sender.region;
        // Collect ids up-front to release the immutable borrow on `self.nodes` before we schedule.
        // Hot path: no sort needed, neighbor order only affects in-flight scheduling fan-out.
        let nbrs: Vec<NodeId> = sender.neighbors_iter().collect();
        for to in nbrs {
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

    pub(super) fn on_inv_arrive(&mut self, recipient: NodeId, inv_peer: NodeId, block: BlockId) {
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

    pub(super) fn on_rec_arrive(&mut self, recipient: NodeId, rec_peer: NodeId, block: BlockId) {
        let i = self.idx(recipient);
        self.nodes[i].block_send_queue.push_back(BlockSendJob {
            kind: BlockSendKind::Rec,
            requester: rec_peer,
            block,
        });
        self.try_start_next_block_send(recipient);
    }

    pub(super) fn on_get_block_txn_arrive(
        &mut self,
        recipient: NodeId,
        peer: NodeId,
        block: BlockId,
    ) {
        let i = self.idx(recipient);
        self.nodes[i].block_send_queue.push_back(BlockSendJob {
            kind: BlockSendKind::GetBlockTxn,
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
        let Some(&BlockSendJob {
            kind,
            requester,
            block,
        }) = self.nodes[si].block_send_queue.front()
        else {
            return;
        };
        let sender_reg = self.nodes[si].region;
        let req_reg = self.nodes[self.idx(requester)].region;
        let (payload_kind, payload_bytes) = match kind {
            BlockSendKind::Rec => {
                let use_compact = self.nodes[si].use_cbr && self.nodes[self.idx(requester)].use_cbr;
                if use_compact {
                    (BlockPayloadKind::Compact, self.sim.compact_block_size_bytes)
                } else {
                    (BlockPayloadKind::Full, self.sim.block_size_bytes)
                }
            }
            BlockSendKind::GetBlockTxn => {
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
        self.nodes[si].block_send_queue.pop_front();
        self.nodes[si].sending_block = true;
        self.schedule(
            at,
            SimEvent::BlockPayloadArrive {
                recipient: requester,
                sender,
                block,
                depart_ms,
                kind: payload_kind,
            },
        );
    }

    /// When a payload has been delivered, clear `sending_block` and start the next queued send if any.
    fn continue_sender_block_queue(&mut self, sender: NodeId) {
        let si = self.idx(sender);
        self.nodes[si].sending_block = false;
        self.try_start_next_block_send(sender);
    }

    pub(super) fn on_block_payload_arrive(
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
