//! Discrete-event blockchain P2P network simulator.
//!
//! All simulation state is owned by [`Simulation`]. Block propagation uses inv → rec →
//! per-sender queues with serialized block sends, then compact relay with optional `GetBlockTxn`
//! and full block retry.

pub mod block;
mod cbr_failure_fractions;
pub mod config;
pub mod consensus;
pub mod event;
pub mod file_config;
pub mod network;
pub mod output;
pub mod routing;
pub mod simulation;
pub mod types;

pub use config::{
    DegreeBucket, NetworkConfig, NetworkPreset, RegionSpec, RegionTopology, SimulationConfig,
};
pub use consensus::validate_pow_received;
pub use file_config::FileConfig;
pub use simulation::{RunStats, Simulation};
pub use types::NodeId;
