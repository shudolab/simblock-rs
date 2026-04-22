//! Chain topology queries that run over [`BlockStore`].

use crate::block::{BlockId, BlockStore};

/// Whether `block` and `tip` are on the same chain (compare [`BlockId`] at the aligned height).
pub(super) fn is_on_same_chain(store: &BlockStore, block: BlockId, tip: BlockId) -> bool {
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

/// True when walking parents from `tip` passes through `ancestor`.
pub(super) fn block_extends_from(store: &BlockStore, tip: BlockId, ancestor: BlockId) -> bool {
    let mut cur = Some(tip);
    while let Some(id) = cur {
        if id == ancestor {
            return true;
        }
        cur = store.get(id).and_then(|r| r.parent);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockRecord;
    use num_bigint::BigUint;

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
}
