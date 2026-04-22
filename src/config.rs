//! Simulation and network parameters passed into [`crate::Simulation::new`].

use serde::Deserialize;

/// Static parameters for one geographic region (bandwidth, population share).
#[derive(Debug, Clone, Copy)]
pub struct RegionSpec {
    pub name: &'static str,
    pub download_bps: u64,
    pub upload_bps: u64,
    /// Fraction of nodes in this region; should sum to `1.0` over [`RegionTopology::regions`].
    pub node_fraction: f64,
}

/// Regional node parameters and the symmetric mean latency matrix (one axis per region).
#[derive(Debug, Clone)]
pub struct RegionTopology {
    pub regions: &'static [RegionSpec],
    /// Mean RTT-style latency between regions `i` and `j` in milliseconds.
    /// Must be square with side length equal to [`Self::regions`].len().
    pub latency_ms: &'static [&'static [u64]],
}

/// One bucket in a **cumulative** outbound-degree distribution.
#[derive(Debug, Clone, Copy)]
pub struct DegreeBucket {
    /// Maximum outbound connections for nodes assigned to this bucket.
    pub max_outbound: usize,
    /// Cumulative fraction of the population with at most this outbound cap (non-decreasing along the slice).
    pub cumulative: f64,
}

/// Network model: geography/latency and how outbound caps are drawn per node.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub topology: RegionTopology,
    pub degree_buckets: &'static [DegreeBucket],
    /// Optional cumulative outdegree distribution: bucket index `k` (0-based) maps to cap `k + 1`.
    /// When `Some`, caps are built from this CDF (list may exceed `N` before shuffle/truncate); when
    /// `None`, [`Self::degree_buckets`] is used.
    pub outdegree_cdf: Option<&'static [f64]>,
}

const BITCOIN_2019_REGIONS: &[RegionSpec] = &[
    RegionSpec {
        name: "NORTH_AMERICA",
        download_bps: 52_000_000,
        upload_bps: 19_200_000,
        node_fraction: 0.3316,
    },
    RegionSpec {
        name: "EUROPE",
        download_bps: 40_000_000,
        upload_bps: 20_700_000,
        node_fraction: 0.4998,
    },
    RegionSpec {
        name: "SOUTH_AMERICA",
        download_bps: 18_000_000,
        upload_bps: 5_800_000,
        node_fraction: 0.0090,
    },
    RegionSpec {
        name: "ASIA_PACIFIC",
        download_bps: 22_800_000,
        upload_bps: 15_700_000,
        node_fraction: 0.1177,
    },
    RegionSpec {
        name: "JAPAN",
        download_bps: 22_800_000,
        upload_bps: 10_200_000,
        node_fraction: 0.0224,
    },
    RegionSpec {
        name: "AUSTRALIA",
        download_bps: 29_900_000,
        upload_bps: 11_300_000,
        node_fraction: 0.0195,
    },
];

const BITCOIN_2019_LATENCY_MS: &[&[u64]] = &[
    &[32, 124, 184, 198, 151, 189],
    &[124, 11, 227, 237, 252, 294],
    &[184, 227, 88, 325, 301, 322],
    &[198, 237, 325, 85, 58, 198],
    &[151, 252, 301, 58, 12, 126],
    &[189, 294, 322, 198, 126, 16],
];

/// Cumulative outbound-degree buckets for the default 2019 preset.
const BITCOIN_2019_DEGREE_BUCKETS: &[DegreeBucket] = &[
    DegreeBucket {
        max_outbound: 1,
        cumulative: 0.025,
    },
    DegreeBucket {
        max_outbound: 2,
        cumulative: 0.050,
    },
    DegreeBucket {
        max_outbound: 3,
        cumulative: 0.075,
    },
    DegreeBucket {
        max_outbound: 4,
        cumulative: 0.10,
    },
    DegreeBucket {
        max_outbound: 5,
        cumulative: 0.20,
    },
    DegreeBucket {
        max_outbound: 6,
        cumulative: 0.30,
    },
    DegreeBucket {
        max_outbound: 7,
        cumulative: 0.40,
    },
    DegreeBucket {
        max_outbound: 8,
        cumulative: 0.50,
    },
    DegreeBucket {
        max_outbound: 9,
        cumulative: 0.60,
    },
    DegreeBucket {
        max_outbound: 10,
        cumulative: 0.70,
    },
    DegreeBucket {
        max_outbound: 11,
        cumulative: 0.80,
    },
    DegreeBucket {
        max_outbound: 12,
        cumulative: 0.85,
    },
    DegreeBucket {
        max_outbound: 13,
        cumulative: 0.90,
    },
    DegreeBucket {
        max_outbound: 14,
        cumulative: 0.95,
    },
    DegreeBucket {
        max_outbound: 15,
        cumulative: 0.97,
    },
    DegreeBucket {
        max_outbound: 16,
        cumulative: 0.97,
    },
    DegreeBucket {
        max_outbound: 17,
        cumulative: 0.98,
    },
    DegreeBucket {
        max_outbound: 18,
        cumulative: 0.99,
    },
    DegreeBucket {
        max_outbound: 19,
        cumulative: 0.995,
    },
    DegreeBucket {
        max_outbound: 20,
        cumulative: 1.0,
    },
];

impl Default for NetworkConfig {
    fn default() -> Self {
        Self::bitcoin_2019()
    }
}

/// Named geography / degree presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPreset {
    /// 2019 mean latency, bandwidth, region mix, and fixed degree buckets.
    #[default]
    Bitcoin2019,
    /// 2015 latency, bandwidth, and region mix; outdegrees from a fixed cumulative CDF slice.
    Bitcoin2015,
    Litecoin,
    Dogecoin,
}

impl NetworkPreset {
    pub fn to_network_config(self) -> NetworkConfig {
        match self {
            NetworkPreset::Bitcoin2019 => NetworkConfig::bitcoin_2019(),
            NetworkPreset::Bitcoin2015 => NetworkConfig::bitcoin_2015(),
            NetworkPreset::Litecoin => NetworkConfig::litecoin(),
            NetworkPreset::Dogecoin => NetworkConfig::dogecoin(),
        }
    }
}

impl NetworkConfig {
    pub fn bitcoin_2019() -> Self {
        Self {
            topology: RegionTopology {
                regions: BITCOIN_2019_REGIONS,
                latency_ms: BITCOIN_2019_LATENCY_MS,
            },
            degree_buckets: BITCOIN_2019_DEGREE_BUCKETS,
            outdegree_cdf: None,
        }
    }

    pub fn bitcoin_2015() -> Self {
        Self {
            topology: RegionTopology {
                regions: BITCOIN_2015_REGIONS,
                latency_ms: BITCOIN_2015_LATENCY_MS,
            },
            degree_buckets: &[],
            outdegree_cdf: Some(DEGREE_CDF_BITCOIN_2015),
        }
    }

    pub fn litecoin() -> Self {
        Self {
            topology: RegionTopology {
                regions: LITECOIN_REGIONS,
                latency_ms: BITCOIN_2019_LATENCY_MS,
            },
            degree_buckets: &[],
            outdegree_cdf: Some(DEGREE_CDF_LITECOIN),
        }
    }

    pub fn dogecoin() -> Self {
        Self {
            topology: RegionTopology {
                regions: DOGECOIN_REGIONS,
                latency_ms: BITCOIN_2019_LATENCY_MS,
            },
            degree_buckets: &[],
            outdegree_cdf: Some(DEGREE_CDF_DOGECOIN),
        }
    }
}

// --- Bitcoin 2015 geography ---

const BITCOIN_2015_REGIONS: &[RegionSpec] = &[
    RegionSpec {
        name: "NORTH_AMERICA",
        download_bps: 25_000_000,
        upload_bps: 4_700_000,
        node_fraction: 0.3869,
    },
    RegionSpec {
        name: "EUROPE",
        download_bps: 24_000_000,
        upload_bps: 8_100_000,
        node_fraction: 0.5159,
    },
    RegionSpec {
        name: "SOUTH_AMERICA",
        download_bps: 6_500_000,
        upload_bps: 1_800_000,
        node_fraction: 0.0113,
    },
    RegionSpec {
        name: "ASIA_PACIFIC",
        download_bps: 10_000_000,
        upload_bps: 5_300_000,
        node_fraction: 0.0574,
    },
    RegionSpec {
        name: "JAPAN",
        download_bps: 17_500_000,
        upload_bps: 3_400_000,
        node_fraction: 0.0119,
    },
    RegionSpec {
        name: "AUSTRALIA",
        download_bps: 14_000_000,
        upload_bps: 5_200_000,
        node_fraction: 0.0166,
    },
];

const BITCOIN_2015_LATENCY_MS: &[&[u64]] = &[
    &[36, 119, 255, 310, 154, 208],
    &[119, 12, 221, 242, 266, 350],
    &[255, 221, 137, 347, 256, 269],
    &[310, 242, 347, 99, 172, 278],
    &[154, 266, 256, 172, 9, 163],
    &[208, 350, 269, 278, 163, 22],
];

const LITECOIN_REGIONS: &[RegionSpec] = &[
    RegionSpec {
        name: "NORTH_AMERICA",
        download_bps: 52_000_000,
        upload_bps: 19_200_000,
        node_fraction: 0.3661,
    },
    RegionSpec {
        name: "EUROPE",
        download_bps: 40_000_000,
        upload_bps: 20_700_000,
        node_fraction: 0.4791,
    },
    RegionSpec {
        name: "SOUTH_AMERICA",
        download_bps: 18_000_000,
        upload_bps: 5_800_000,
        node_fraction: 0.0149,
    },
    RegionSpec {
        name: "ASIA_PACIFIC",
        download_bps: 22_800_000,
        upload_bps: 15_700_000,
        node_fraction: 0.1022,
    },
    RegionSpec {
        name: "JAPAN",
        download_bps: 22_800_000,
        upload_bps: 10_200_000,
        node_fraction: 0.0238,
    },
    RegionSpec {
        name: "AUSTRALIA",
        download_bps: 29_900_000,
        upload_bps: 11_300_000,
        node_fraction: 0.0139,
    },
];

const DOGECOIN_REGIONS: &[RegionSpec] = &[
    RegionSpec {
        name: "NORTH_AMERICA",
        download_bps: 52_000_000,
        upload_bps: 19_200_000,
        node_fraction: 0.3924,
    },
    RegionSpec {
        name: "EUROPE",
        download_bps: 40_000_000,
        upload_bps: 20_700_000,
        node_fraction: 0.4879,
    },
    RegionSpec {
        name: "SOUTH_AMERICA",
        download_bps: 18_000_000,
        upload_bps: 5_800_000,
        node_fraction: 0.0212,
    },
    RegionSpec {
        name: "ASIA_PACIFIC",
        download_bps: 22_800_000,
        upload_bps: 15_700_000,
        node_fraction: 0.0697,
    },
    RegionSpec {
        name: "JAPAN",
        download_bps: 22_800_000,
        upload_bps: 10_200_000,
        node_fraction: 0.0106,
    },
    RegionSpec {
        name: "AUSTRALIA",
        download_bps: 29_900_000,
        upload_bps: 11_300_000,
        node_fraction: 0.0182,
    },
];

/// Cumulative outdegree CDF (Bitcoin 2015 style).
const DEGREE_CDF_BITCOIN_2015: &[f64] = &[
    0.025, 0.050, 0.075, 0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 0.85, 0.90, 0.95, 0.97,
    0.97, 0.98, 0.99, 0.995, 1.0,
];

/// Cumulative outdegree CDF (Litecoin-style table).
const DEGREE_CDF_LITECOIN: &[f64] = &[
    0.01, 0.02, 0.04, 0.07, 0.09, 0.14, 0.20, 0.28, 0.39, 0.5, 0.6, 0.69, 0.76, 0.81, 0.85, 0.87,
    0.89, 0.92, 0.93, 1.0,
];

/// Cumulative outdegree CDF (Dogecoin-style table).
const DEGREE_CDF_DOGECOIN: &[f64] = &[
    0.00, 0.00, 0.00, 0.00, 0.00, 0.00, 0.00, 1.0, 1.00, 1.00, 1.00, 1.00, 1.00, 1.00, 1.00, 1.00,
    1.00, 1.00, 1.00, 1.0,
];

#[derive(Debug, Clone)]
pub struct SimulationConfig {
    pub num_nodes: u32,
    /// Target mean block interval; work target scales as total mining power times this value (ms).
    pub target_block_interval_ms: u64,
    pub average_mining_power: u64,
    pub stdev_mining_power: u64,
    pub end_block_height: u32,
    pub block_size_bytes: u64,
    pub compact_block_size_bytes: u64,
    pub cbr_usage_rate: f64,
    pub churn_node_rate: f64,
    pub cbr_failure_rate_control: f64,
    pub cbr_failure_rate_churn: f64,
    pub rng_seed: u64,
    /// Fixed overhead added to each block delivery (ms).
    pub message_overhead_ms: u64,
    /// Per-hop processing time when sending a block (ms).
    pub processing_time_ms: u64,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            num_nodes: 300,
            target_block_interval_ms: 1000 * 60 * 10,
            average_mining_power: 400_000,
            stdev_mining_power: 100_000,
            end_block_height: 10,
            block_size_bytes: 535_000,
            compact_block_size_bytes: 18_000,
            cbr_usage_rate: 0.964,
            churn_node_rate: 0.976,
            cbr_failure_rate_control: 0.13,
            cbr_failure_rate_churn: 0.27,
            rng_seed: 10,
            message_overhead_ms: 10,
            processing_time_ms: 2,
        }
    }
}

impl SimulationConfig {
    /// Small network for fast unit tests.
    pub fn tiny() -> Self {
        Self {
            num_nodes: 8,
            target_block_interval_ms: 50_000,
            average_mining_power: 10_000,
            stdev_mining_power: 1000,
            end_block_height: 4,
            block_size_bytes: 50_000,
            compact_block_size_bytes: 2000,
            cbr_usage_rate: 0.5,
            churn_node_rate: 0.5,
            cbr_failure_rate_control: 0.1,
            cbr_failure_rate_churn: 0.2,
            rng_seed: 42,
            message_overhead_ms: 10,
            processing_time_ms: 2,
        }
    }
}
