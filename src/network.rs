//! Latency sampling and bandwidth-based transfer delays.

use crate::config::NetworkConfig;
use rand::Rng;

// --- Regional latency (heavy-tailed sample from the mean matrix) ------------

/// Multiplier `k` in `shape = k * mean` for the tail sampler.
const LATENCY_SHAPE_FACTOR: f64 = 0.2;
/// Subtracted from the regional mean to form the `scale` term (`mean - scale_offset`).
const LATENCY_SCALE_OFFSET_MS: f64 = 5.0;

// --- Block transfer (`bits / (bps / divisor) + processing`) -----------------

const BYTES_TO_BITS: u64 = 8;
/// Applied as `bps / BANDWIDTH_BPS_PER_MS_UNIT` when turning link rate into a per-ms throughput term.
const BANDWIDTH_BPS_PER_MS_UNIT: u64 = 1000;
const BANDWIDTH_MIN_BPS: u64 = 1;
const BANDWIDTH_MIN_BITS_PER_MS: u64 = 1;

/// Random latency sample: `round(scale / u^(1/shape))` with `shape = 0.2 * mean`, `scale = mean - 5`, `u ~ Uniform(0,1)`.
/// Non-finite positive results map to a large finite value.
pub fn sample_latency_ms<R: Rng + ?Sized>(
    rng: &mut R,
    net: &NetworkConfig,
    from: usize,
    to: usize,
) -> u64 {
    let mean = net.topology.latency_ms[from][to] as f64;
    if mean <= 0.0 {
        return 0;
    }
    let shape = LATENCY_SHAPE_FACTOR * mean;
    let scale = mean - LATENCY_SCALE_OFFSET_MS;
    if scale <= 0.0 {
        return mean.round().max(0.0) as u64;
    }
    let u = rng.gen::<f64>();
    let v = scale / u.powf(1.0 / shape);
    let rounded = v.round();
    if !rounded.is_finite() || rounded <= 0.0 {
        return u64::MAX / 4;
    }
    rounded.min(u64::MAX as f64) as u64
}

pub fn bandwidth_bps(net: &NetworkConfig, from: usize, to: usize) -> u64 {
    let up = net.topology.regions[from].upload_bps;
    let down = net.topology.regions[to].download_bps;
    up.min(down)
}

/// Transfer time in ms: `size_bytes * BYTES_TO_BITS / (bps / BANDWIDTH_BPS_PER_MS_UNIT) + processing_ms`.
pub fn block_transfer_ms(
    net: &NetworkConfig,
    from_region: usize,
    to_region: usize,
    size_bytes: u64,
    processing_ms: u64,
) -> u64 {
    let bps = bandwidth_bps(net, from_region, to_region).max(BANDWIDTH_MIN_BPS);
    let bits = size_bytes.saturating_mul(BYTES_TO_BITS);
    let bits_per_ms = bps / BANDWIDTH_BPS_PER_MS_UNIT;
    bits / bits_per_ms.max(BANDWIDTH_MIN_BITS_PER_MS) + processing_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DegreeBucket, NetworkConfig, RegionSpec, RegionTopology};
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn test_net() -> NetworkConfig {
        const REGIONS: &[RegionSpec] = &[
            RegionSpec {
                name: "R0",
                download_bps: 10_000_000,
                upload_bps: 8_000_000,
                node_fraction: 0.5,
            },
            RegionSpec {
                name: "R1",
                download_bps: 12_000_000,
                upload_bps: 6_000_000,
                node_fraction: 0.5,
            },
        ];
        const LAT: &[&[u64]] = &[&[50, 100], &[100, 50]];
        NetworkConfig {
            topology: RegionTopology {
                regions: REGIONS,
                latency_ms: LAT,
            },
            degree_buckets: &[DegreeBucket {
                max_outbound: 8,
                cumulative: 1.0,
            }],
            outdegree_cdf: None,
        }
    }

    #[test]
    fn bandwidth_bps_is_min_of_upload_and_download() {
        let net = test_net();
        assert_eq!(bandwidth_bps(&net, 0, 1), 8_000_000);
        assert_eq!(bandwidth_bps(&net, 1, 0), 6_000_000);
    }

    #[test]
    fn block_transfer_ms_includes_processing() {
        let net = test_net();
        let t = block_transfer_ms(&net, 0, 1, 6_000_000, 5);
        assert!(t >= 5);
    }

    #[test]
    fn sample_latency_ms_finite_with_seeded_rng() {
        let net = test_net();
        let mut rng = StdRng::seed_from_u64(99);
        for _ in 0..20 {
            let ms = sample_latency_ms(&mut rng, &net, 0, 1);
            assert!(ms < u64::MAX / 8);
        }
    }

    #[test]
    fn sample_latency_ms_fallback_when_mean_below_five() {
        const REGIONS: &[RegionSpec] = &[RegionSpec {
            name: "A",
            download_bps: 10_000_000,
            upload_bps: 10_000_000,
            node_fraction: 1.0,
        }];
        const LAT: &[&[u64]] = &[&[(super::LATENCY_SCALE_OFFSET_MS as u64).saturating_sub(2)]];
        let net = NetworkConfig {
            topology: RegionTopology {
                regions: REGIONS,
                latency_ms: LAT,
            },
            degree_buckets: &[DegreeBucket {
                max_outbound: 4,
                cumulative: 1.0,
            }],
            outdegree_cdf: None,
        };
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..10 {
            assert_eq!(sample_latency_ms(&mut rng, &net, 0, 0), 3);
        }
    }
}
