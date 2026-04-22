//! Per-block propagation delay tracking with a FIFO observation window.
//!
//! Replaces the four separate fields (`propagation`, `propagation_observed_blocks`,
//! `propagation_observed_set`, `propagation_node_order`) that used to live on `Simulation`.
//! Iteration order matches the first-seen order of blocks and, within each block, of nodes.

use crate::block::BlockId;
use crate::types::NodeId;
use std::collections::{HashMap, HashSet, VecDeque};

/// Tracks receive-delays for a bounded, FIFO-evicted window of recent blocks.
#[derive(Debug)]
pub(super) struct PropagationTracker {
    capacity: usize,
    /// Blocks currently tracked, oldest first.
    order: VecDeque<BlockId>,
    /// `order` mirrored as a set for O(1) membership checks.
    present: HashSet<BlockId>,
    /// Per-block map `node_id -> delay_ms` plus first-seen node ordering.
    entries: HashMap<BlockId, BlockEntry>,
}

#[derive(Debug, Default)]
struct BlockEntry {
    delays: HashMap<NodeId, u64>,
    node_order: Vec<NodeId>,
}

impl PropagationTracker {
    /// `capacity` is the maximum number of distinct blocks retained before FIFO eviction kicks in;
    /// the tracker is allowed to temporarily hold `capacity + 1` blocks, matching the original
    /// eviction semantics (`> capacity` triggers a trim).
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::with_capacity(capacity + 1),
            present: HashSet::with_capacity(capacity + 1),
            entries: HashMap::with_capacity(capacity + 1),
        }
    }

    /// Insert or update a `(block, node) -> delay` observation. A brand-new block may evict the
    /// oldest tracked block to keep within `capacity`.
    pub(super) fn record(&mut self, block: BlockId, node: NodeId, delay: u64) {
        if self.present.contains(&block) {
            let entry = self.entries.entry(block).or_default();
            if let Some(d) = entry.delays.get_mut(&node) {
                *d = delay;
            } else {
                entry.delays.insert(node, delay);
                entry.node_order.push(node);
            }
            return;
        }

        if self.order.len() > self.capacity {
            self.evict_oldest();
        }
        self.order.push_back(block);
        self.present.insert(block);
        let mut entry = BlockEntry::default();
        entry.delays.insert(node, delay);
        entry.node_order.push(node);
        self.entries.insert(block, entry);
    }

    fn evict_oldest(&mut self) {
        if let Some(evicted) = self.order.pop_front() {
            self.present.remove(&evicted);
            self.entries.remove(&evicted);
        }
    }

    /// Iterate tracked blocks in observation order (oldest first).
    pub(super) fn blocks(&self) -> impl Iterator<Item = BlockId> + '_ {
        self.order.iter().copied()
    }

    /// `(node, delay_ms)` pairs for `block`, in first-seen node order.
    pub(super) fn observations(&self, block: BlockId) -> impl Iterator<Item = (NodeId, u64)> + '_ {
        let entry = self.entries.get(&block);
        entry
            .into_iter()
            .flat_map(|e| e.node_order.iter().copied().map(move |n| (n, e.delays[&n])))
    }
}
