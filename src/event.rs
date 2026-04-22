//! Event types scheduled by the simulator.

use crate::block::BlockId;
use crate::types::NodeId;
use num_bigint::BigUint;

/// Full block versus compact block relay payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockPayloadKind {
    /// Compact block relay payload; decode success is decided on **receipt** (receiver churn flag).
    Compact,
    /// Full block: legacy relay or after `GetBlockTxn`.
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimEvent {
    /// Mining finished; mint a new block extending `parent`.
    MiningFinished {
        node: NodeId,
        parent: BlockId,
        difficulty: BigUint,
    },
    /// Block inventory has arrived at `to` from neighbor `from`.
    InvArrive {
        to: NodeId,
        from: NodeId,
        block: BlockId,
    },
    /// Post-inv receive step has arrived at `to` from `from` (the peer that sent `inv`).
    RecArrive {
        to: NodeId,
        from: NodeId,
        block: BlockId,
    },
    /// `getblocktxn` has arrived at `to` from `from` (compact decode failed on receiver).
    GetBlockTxnArrive {
        to: NodeId,
        from: NodeId,
        block: BlockId,
    },
    /// Block or compact payload has finished transmission to `recipient` from `sender`.
    BlockPayloadArrive {
        recipient: NodeId,
        sender: NodeId,
        block: BlockId,
        depart_ms: u64,
        kind: BlockPayloadKind,
    },
}
