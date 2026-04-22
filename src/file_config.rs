//! Optional TOML file to select a [`NetworkPreset`] and override [`SimulationConfig`] fields.

use crate::config::{NetworkConfig, NetworkPreset, SimulationConfig};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct FileConfig {
    #[serde(default)]
    pub network: NetworkPreset,
    #[serde(default)]
    pub simulation: SimulationPartial,
}

/// All fields optional; omitted keys keep the passed-in [`SimulationConfig`] values.
#[derive(Debug, Default, Deserialize)]
pub struct SimulationPartial {
    pub num_nodes: Option<u32>,
    pub target_block_interval_ms: Option<u64>,
    pub average_mining_power: Option<u64>,
    pub stdev_mining_power: Option<u64>,
    pub end_block_height: Option<u32>,
    pub block_size_bytes: Option<u64>,
    pub compact_block_size_bytes: Option<u64>,
    pub cbr_usage_rate: Option<f64>,
    pub churn_node_rate: Option<f64>,
    pub cbr_failure_rate_control: Option<f64>,
    pub cbr_failure_rate_churn: Option<f64>,
    pub rng_seed: Option<u64>,
    pub message_overhead_ms: Option<u64>,
    pub processing_time_ms: Option<u64>,
}

impl SimulationPartial {
    pub fn merge(self, mut base: SimulationConfig) -> SimulationConfig {
        if let Some(v) = self.num_nodes {
            base.num_nodes = v;
        }
        if let Some(v) = self.target_block_interval_ms {
            base.target_block_interval_ms = v;
        }
        if let Some(v) = self.average_mining_power {
            base.average_mining_power = v;
        }
        if let Some(v) = self.stdev_mining_power {
            base.stdev_mining_power = v;
        }
        if let Some(v) = self.end_block_height {
            base.end_block_height = v;
        }
        if let Some(v) = self.block_size_bytes {
            base.block_size_bytes = v;
        }
        if let Some(v) = self.compact_block_size_bytes {
            base.compact_block_size_bytes = v;
        }
        if let Some(v) = self.cbr_usage_rate {
            base.cbr_usage_rate = v;
        }
        if let Some(v) = self.churn_node_rate {
            base.churn_node_rate = v;
        }
        if let Some(v) = self.cbr_failure_rate_control {
            base.cbr_failure_rate_control = v;
        }
        if let Some(v) = self.cbr_failure_rate_churn {
            base.cbr_failure_rate_churn = v;
        }
        if let Some(v) = self.rng_seed {
            base.rng_seed = v;
        }
        if let Some(v) = self.message_overhead_ms {
            base.message_overhead_ms = v;
        }
        if let Some(v) = self.processing_time_ms {
            base.processing_time_ms = v;
        }
        base
    }
}

impl FileConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| format!("{path:?}: {e}"))?;
        toml::from_str(&raw).map_err(|e| format!("{path:?}: {e}"))
    }

    pub fn build(self, base_sim: SimulationConfig) -> (SimulationConfig, NetworkConfig) {
        let net = self.network.to_network_config();
        let sim = self.simulation.merge(base_sim);
        (sim, net)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NetworkPreset;

    #[test]
    fn parses_toml_network_and_simulation() {
        let raw = r#"
network = "litecoin"

[simulation]
num_nodes = 12
end_block_height = 5
rng_seed = 99
"#;
        let f: FileConfig = toml::from_str(raw).unwrap();
        assert_eq!(f.network, NetworkPreset::Litecoin);
        let (sim, net) = f.build(SimulationConfig::default());
        assert_eq!(sim.num_nodes, 12);
        assert_eq!(sim.end_block_height, 5);
        assert_eq!(sim.rng_seed, 99);
        assert!(net.outdegree_cdf.is_some());
    }
}
