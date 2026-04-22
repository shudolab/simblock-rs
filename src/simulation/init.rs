//! Per-run sampling: region/degree assignment, mining-power draws, small-message delay, and
//! compact-block failure size sampler.

use super::node::{NodeInit, NodeState};
use crate::cbr_failure_fractions::{CBR_FAILURE_FRACTIONS_CHURN, CBR_FAILURE_FRACTIONS_CONTROL};
use crate::config::{DegreeBucket, NetworkConfig, RegionSpec, SimulationConfig};
use crate::network::sample_latency_ms;
use crate::types::NodeId;
use rand::seq::SliceRandom;
use rand::Rng;
use rand_distr::{Distribution, Normal};

pub(super) fn small_msg_delay_ms<R: Rng>(
    rng: &mut R,
    net: &NetworkConfig,
    from_reg: usize,
    to_reg: usize,
    overhead_ms: u64,
) -> u64 {
    sample_latency_ms(rng, net, from_reg, to_reg) + overhead_ms
}

/// Sample full-block byte size after compact decode failure (`block_size_bytes` times a table entry).
pub(super) fn sample_failed_block_bytes<R: Rng>(
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

pub(super) fn pick_genesis_minter<R: Rng + ?Sized>(rng: &mut R, nodes: &[NodeState]) -> NodeId {
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

pub(super) fn sample_mining_power<R: Rng + ?Sized>(rng: &mut R, mean: u64, stdev: u64) -> u64 {
    let n = Normal::new(mean as f64, stdev as f64).expect("mining power mean/stdev");
    let v = n.sample(rng).round() as i64;
    v.max(1) as u64
}

pub(super) fn sample_node_inits<R: Rng + ?Sized>(
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
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

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
}
