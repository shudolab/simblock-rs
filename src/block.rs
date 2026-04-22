//! Block store and proof-of-work header fields.

use crate::types::NodeId;
use num_bigint::BigUint;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u64);

#[derive(Debug, Clone)]
pub struct BlockRecord {
    pub parent: Option<BlockId>,
    pub height: u32,
    pub time_ms: u64,
    pub minter: NodeId,
    pub difficulty: BigUint,
    pub total_difficulty: BigUint,
    /// Minimum work required for the next child block (no retargeting in this model).
    pub next_difficulty: BigUint,
}

#[derive(Debug, Default)]
pub struct BlockStore {
    blocks: HashMap<BlockId, BlockRecord>,
    next_id: u64,
}

impl BlockStore {
    pub fn insert(&mut self, rec: BlockRecord) -> BlockId {
        let id = BlockId(self.next_id);
        self.next_id += 1;
        self.blocks.insert(id, rec);
        id
    }

    pub fn get(&self, id: BlockId) -> Option<&BlockRecord> {
        self.blocks.get(&id)
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (BlockId, &BlockRecord)> + '_ {
        self.blocks.iter().map(|(&id, rec)| (id, rec))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigUint;

    #[test]
    fn block_store_insert_assigns_sequential_ids() {
        let mut s = BlockStore::default();
        let rec = BlockRecord {
            parent: None,
            height: 0,
            time_ms: 0,
            minter: 1,
            difficulty: BigUint::from(0u32),
            total_difficulty: BigUint::from(0u32),
            next_difficulty: BigUint::from(100u32),
        };
        let a = s.insert(rec.clone());
        let b = s.insert(rec);
        assert_eq!(a.0, 0);
        assert_eq!(b.0, 1);
        assert_eq!(s.len(), 2);
        assert!(!s.is_empty());
        assert_eq!(s.get(a).unwrap().minter, 1);
        assert!(s.get(BlockId(99)).is_none());
    }
}
