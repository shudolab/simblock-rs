//! Optional TOML file to select a [`NetworkPreset`] and override [`SimulationConfig`] fields.

use crate::config::{NetworkConfig, NetworkPreset, SimulationConfig};
use serde::Deserialize;
use std::fmt;
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

/// Overwrite each named field on `$dst` with the inner value of `$src.$field` when `Some`.
///
/// Declarative alternative to manually unpacking every `Option` on [`SimulationPartial`].
macro_rules! apply_fields {
    ($dst:expr, $src:expr, [$($field:ident),* $(,)?]) => {
        $(
            if let Some(v) = $src.$field {
                $dst.$field = v;
            }
        )*
    };
}

impl SimulationPartial {
    pub fn merge(self, mut base: SimulationConfig) -> SimulationConfig {
        apply_fields!(
            base,
            self,
            [
                num_nodes,
                target_block_interval_ms,
                average_mining_power,
                stdev_mining_power,
                end_block_height,
                block_size_bytes,
                compact_block_size_bytes,
                cbr_usage_rate,
                churn_node_rate,
                cbr_failure_rate_control,
                cbr_failure_rate_churn,
                rng_seed,
                message_overhead_ms,
                processing_time_ms,
            ]
        );
        base
    }
}

#[derive(Debug)]
pub enum FileConfigError {
    Read {
        path: String,
        source: std::io::Error,
    },
    Parse {
        path: String,
        source: toml::de::Error,
    },
}

impl fmt::Display for FileConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(f, "{path}: {source}"),
            Self::Parse { path, source } => write!(f, "{path}: {source}"),
        }
    }
}

impl std::error::Error for FileConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

impl FileConfig {
    pub fn load(path: &Path) -> Result<Self, FileConfigError> {
        let path_display = path.display().to_string();
        let raw = std::fs::read_to_string(path).map_err(|source| FileConfigError::Read {
            path: path_display.clone(),
            source,
        })?;
        toml::from_str(&raw).map_err(|source| FileConfigError::Parse {
            path: path_display,
            source,
        })
    }

    pub fn build(self, base_sim: SimulationConfig) -> (SimulationConfig, NetworkConfig) {
        let net = NetworkConfig::from(self.network);
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
