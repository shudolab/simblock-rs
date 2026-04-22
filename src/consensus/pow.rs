//! Proof-of-work: work target, mining delay, validation. Difficulty retargeting is not implemented.

use crate::block::{BlockRecord, BlockStore};
use num_bigint::BigUint;
use rand::Rng;

/// Work required for the first block after genesis: total mining power times target interval.
pub fn work_target_for_genesis(total_mining_power: u128, target_interval_ms: u64) -> BigUint {
    BigUint::from(total_mining_power) * BigUint::from(target_interval_ms)
}

fn difficulty_as_f64(d: &BigUint) -> f64 {
    d.to_string().parse().unwrap_or(f64::MAX)
}

/// Mining delay in milliseconds from exponential work model: `-ln(1-u) * difficulty / mining_power`,
/// truncated toward zero (`u ~ Uniform(0,1)`).
pub fn sample_mining_delay_ms<R: Rng + ?Sized>(
    rng: &mut R,
    difficulty: &BigUint,
    mining_power: u64,
) -> u64 {
    let mut u = rng.gen::<f64>();
    if u >= 1.0 {
        u = 1.0 - f64::EPSILON;
    }
    let d = difficulty_as_f64(difficulty);
    let p = mining_power as f64;
    if p <= 0.0 {
        return 0;
    }
    let ms = -(1.0 - u).ln() * d / p;
    if !ms.is_finite() || ms <= 0.0 {
        return 0;
    }
    ms.trunc() as u64
}

/// Whether `received` is a valid heavier extension than `current_tip`.
pub fn validate_pow_received(
    received: &BlockRecord,
    current_tip: Option<&BlockRecord>,
    store: &BlockStore,
) -> bool {
    if received.height == 0 {
        return true;
    }
    let Some(pid) = received.parent else {
        return false;
    };
    let Some(parent) = store.get(pid) else {
        return false;
    };
    if received.difficulty < parent.next_difficulty {
        return false;
    }
    match current_tip {
        None => true,
        Some(cur) => received.total_difficulty > cur.total_difficulty,
    }
}

pub fn extend_total_difficulty(parent: &BlockRecord, block_difficulty: &BigUint) -> BigUint {
    parent.total_difficulty.clone() + block_difficulty
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockId, BlockRecord, BlockStore};
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    #[test]
    fn work_target_for_genesis_scales_linearly() {
        let a = work_target_for_genesis(100, 1000);
        let b = work_target_for_genesis(200, 1000);
        assert_eq!(b, a.clone() + a);
    }

    #[test]
    fn extend_total_difficulty_adds_work() {
        let parent = BlockRecord {
            parent: None,
            height: 0,
            time_ms: 0,
            minter: 1,
            difficulty: BigUint::from(0u32),
            total_difficulty: BigUint::from(10u32),
            next_difficulty: BigUint::from(5u32),
        };
        let d = BigUint::from(7u32);
        assert_eq!(extend_total_difficulty(&parent, &d), BigUint::from(17u32));
    }

    #[test]
    fn validate_pow_height_zero_always_ok() {
        let store = BlockStore::default();
        let gen = BlockRecord {
            parent: None,
            height: 0,
            time_ms: 0,
            minter: 1,
            difficulty: BigUint::from(0u32),
            total_difficulty: BigUint::from(0u32),
            next_difficulty: BigUint::from(1u32),
        };
        assert!(validate_pow_received(&gen, None, &store));
    }

    #[test]
    fn validate_pow_rejects_unknown_parent() {
        let store = BlockStore::default();
        let bad_parent = BlockId(0);
        let child = BlockRecord {
            parent: Some(bad_parent),
            height: 1,
            time_ms: 1,
            minter: 1,
            difficulty: BigUint::from(1u32),
            total_difficulty: BigUint::from(1u32),
            next_difficulty: BigUint::from(1u32),
        };
        assert!(!validate_pow_received(&child, None, &store));
    }

    #[test]
    fn validate_pow_accepts_heavier_chain_extension() {
        let mut store = BlockStore::default();
        let next_d = BigUint::from(100u32);
        let gen = BlockRecord {
            parent: None,
            height: 0,
            time_ms: 0,
            minter: 1,
            difficulty: BigUint::from(0u32),
            total_difficulty: BigUint::from(0u32),
            next_difficulty: next_d.clone(),
        };
        let gid = store.insert(gen);
        let b1 = BlockRecord {
            parent: Some(gid),
            height: 1,
            time_ms: 1,
            minter: 2,
            difficulty: next_d.clone(),
            total_difficulty: BigUint::from(50u32),
            next_difficulty: next_d.clone(),
        };
        let id1 = store.insert(b1);
        let tip = store.get(id1).unwrap();
        let b2 = BlockRecord {
            parent: Some(gid),
            height: 1,
            time_ms: 2,
            minter: 3,
            difficulty: next_d.clone(),
            total_difficulty: BigUint::from(80u32),
            next_difficulty: next_d,
        };
        assert!(validate_pow_received(&b2, Some(tip), &store));
    }

    #[test]
    fn validate_pow_rejects_insufficient_difficulty_vs_parent_next() {
        let mut store = BlockStore::default();
        let next_d = BigUint::from(10u32);
        let gen = BlockRecord {
            parent: None,
            height: 0,
            time_ms: 0,
            minter: 1,
            difficulty: BigUint::from(0u32),
            total_difficulty: BigUint::from(0u32),
            next_difficulty: next_d,
        };
        let gid = store.insert(gen);
        let child = BlockRecord {
            parent: Some(gid),
            height: 1,
            time_ms: 1,
            minter: 2,
            difficulty: BigUint::from(5u32),
            total_difficulty: BigUint::from(5u32),
            next_difficulty: BigUint::from(1u32),
        };
        assert!(!validate_pow_received(&child, None, &store));
    }

    #[test]
    fn sample_mining_delay_ms_is_deterministic_for_seed() {
        let diff = BigUint::from(1_000_000u32);
        let mut r1 = ChaCha8Rng::seed_from_u64(7);
        let mut r2 = ChaCha8Rng::seed_from_u64(7);
        assert_eq!(
            sample_mining_delay_ms(&mut r1, &diff, 5000),
            sample_mining_delay_ms(&mut r2, &diff, 5000)
        );
    }
}
