//! Per-node mutable state, static init draws, and the per-peer block-send queue item.

use crate::block::BlockId;
use crate::routing::neighbor_ids;
use crate::types::NodeId;
use std::collections::{HashSet, VecDeque};

/// Variant tag on a [`BlockSendJob`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockSendKind {
    /// Send a block payload in response to a received `rec` (compact if both peers are CBR-capable).
    Rec,
    /// Send a full block in response to `getblocktxn` (compact decode failed on receiver).
    GetBlockTxn,
}

/// Queued outbound block response after rec or `getblocktxn` (per-peer send queue).
#[derive(Debug, Clone, Copy)]
pub(super) struct BlockSendJob {
    pub kind: BlockSendKind,
    pub requester: NodeId,
    pub block: BlockId,
}

#[derive(Debug)]
pub(super) struct NodeState {
    pub id: NodeId,
    pub region: usize,
    pub mining_power: u64,
    pub use_cbr: bool,
    pub is_churn: bool,
    pub max_outbound: usize,
    pub outbound: Vec<NodeId>,
    pub inbound: Vec<NodeId>,
    pub tip: Option<BlockId>,
    /// `(at_ms, seq)` of the single scheduled [`crate::event::SimEvent::MiningFinished`] for this node, if any.
    pub pending_mining: Option<(u64, u64)>,
    pub orphans: Vec<BlockId>,
    /// Serialized block / compact sends to peers (`sending_block` plus the queue above).
    pub block_send_queue: VecDeque<BlockSendJob>,
    pub sending_block: bool,
    /// Blocks for which this node has sent `Rec` and not finished download.
    pub downloading: HashSet<BlockId>,
}

impl NodeState {
    /// Sorted/deduplicated neighbor list; used by output paths that need a deterministic order.
    pub(super) fn neighbors_sorted(&self) -> Vec<NodeId> {
        neighbor_ids(&self.outbound, &self.inbound)
    }

    /// Non-allocating neighbor iterator (outbound first, then inbound). `init_outbound_links`
    /// guarantees the two lists are disjoint, so no deduplication is needed.
    pub(super) fn neighbors_iter(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.outbound
            .iter()
            .copied()
            .chain(self.inbound.iter().copied())
    }

    /// Number of distinct neighbors (equivalent to `outbound.len() + inbound.len()` by disjointness).
    #[cfg(test)]
    pub(super) fn neighbor_count(&self) -> usize {
        self.outbound.len() + self.inbound.len()
    }
}

/// Per-node draws fixed at construction (region, degree cap, CBR/churn flags).
#[derive(Debug)]
pub(super) struct NodeInit {
    pub region: usize,
    pub max_outbound: usize,
    pub use_cbr: bool,
    pub is_churn: bool,
}
