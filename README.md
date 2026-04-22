# simblock-rs

Rust simulator for blockchain P2P networks, inspired by [SimBlock](https://dsg-titech.github.io/simblock/).

## Prerequisites

- Install Rust (e.g. via [rustup](https://rustup.rs/)).

## Build and test

```bash
cargo build --release
cargo test
```

## Run

```bash
cargo run --release -- [output_directory]
```

If you omit the directory, outputs go under `./output/run-<YYYYMMDD_HHMMSS>/` (local wall-clock time, a new folder each run).

### Simulation configuration

You can override the network preset and simulation fields without recompiling, by passing a [TOML](https://toml.io/) file:

```bash
cargo run --release -- path/to/output --config path/to/settings.toml
```

The output directory is a positional argument; `--config <path>` may appear before or after it.

The settings file may set an optional top-level `network` string and an optional `[simulation]` table. Omitted keys keep the built-in defaults.

**`network` (optional)** — one of:

| Value | Meaning |
| --- | --- |
| `bitcoin2019` | Default: 2019-style regions, latency matrix, fixed outbound-degree buckets. |
| `bitcoin2015` | 2015-style regions/latency; CDF-based outdegrees. |
| `litecoin` | Litecoin-style region mix; CDF outdegrees; 2019 latency matrix. |
| `dogecoin` | Dogecoin-style region mix; CDF outdegrees; 2019 latency matrix. |

If `network` is omitted, it defaults to `bitcoin2019`.

**`[simulation]` (optional)** — each field overrides the default when present.

| Key | Type | Meaning |
| --- | --- | --- |
| `num_nodes` | integer | Number of P2P nodes. |
| `target_block_interval_ms` | integer | Target mean block interval (ms); mining difficulty scales with total hash power and this interval. |
| `average_mining_power` | integer | Mean mining power per node (work per ms in this model). |
| `stdev_mining_power` | integer | Standard deviation of mining power (normal draw per node). |
| `end_block_height` | integer | Stop the run after the chain reaches this height. |
| `block_size_bytes` | integer | Full block payload size (bytes). |
| `compact_block_size_bytes` | integer | Compact block payload size (bytes). |
| `cbr_usage_rate` | float | Fraction of nodes that use compact block relay when propagating. |
| `churn_node_rate` | float | Fraction of nodes treated as “churn” for CBR failure behaviour. |
| `cbr_failure_rate_control` | float | CBR failure probability for non-churn (control) nodes. |
| `cbr_failure_rate_churn` | float | CBR failure probability for churn nodes. |
| `rng_seed` | integer | Seed for the simulation RNG. |
| `message_overhead_ms` | integer | Fixed latency overhead per message (ms). |
| `processing_time_ms` | integer | Per-hop processing delay when forwarding a block (ms). |

See [`sample.toml`](sample.toml) in the repository root for an example.

## Output files

Everything below is written under the chosen output directory (or the default `output/run-<YYYYMMDD_HHMMSS>/` when you omit `DIR`).

| Path | Description |
| --- | --- |
| `static.json` | Region metadata for visualization: a JSON object with a `region` array of `{ "id", "name" }` entries matching the network preset. Emitted once after initialization, before the main simulation loop. |
| `output.json` | A single JSON **array** of discrete events in time order (SimBlock-style traces). Each element has a `kind` string (`add-node`, `add-link`, `add-block`, `flow-block`, `simulation-end`) and a `content` object with timestamps (simulation ms), node ids, and block ids as in the original SimBlock viewer format. |
| `graph/<h>.txt` | Directed P2P graph as one edge per line: `from_node_id to_node_id`. The neighbor set is fixed after startup, so every snapshot file contains the **same** edge list; multiple files exist so tools can pick a path per snapshot label `h`. Labels are normally recorded at height **2** and then every **100** layers when those milestones occur. If none were recorded (very short runs), the exporter writes `0.txt` … `<max_tip_height>.txt` instead so at least one file is present. |
| `blockList.txt` | Human-readable block inventory for the **first** node’s best chain (genesis omitted) plus every orphan block referenced by any node, sorted by block time then id. Lines look like `OnChain : <height> : <id>` or `Orphan : <height> : <id>`. |
| `propagation.csv` | Machine-readable propagation matrix: header `block_id,height,node_id,delay_ms`, then one row per (block, node) when that node adopted the block; `delay_ms` is simulated time from block creation until adoption on the node’s best chain. |

