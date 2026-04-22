//! CLI: writes `output.json`, `static.json`, `propagation.csv`, `graph/<n>.txt`, and
//! `blockList.txt` under the output directory (default `./output/run-<local_datetime>/`).
//! Optional TOML config: `--config path.toml`.

use chrono::Local;
use clap::Parser;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use simblock::{FileConfig, NetworkConfig, Simulation, SimulationConfig};
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often to refresh the progress bar (discrete events processed).
const PROGRESS_TICK_EVENTS: u64 = 256;
/// Number of discrete file-export steps reported to the export progress bar.
const EXPORT_STEPS: u64 = 4;

#[derive(Parser, Debug)]
#[command(
    name = "simblock",
    version,
    about = "Discrete-event blockchain P2P network simulator"
)]
struct Cli {
    /// Directory to write output files (output.json, static.json, graph/, etc.).
    /// If omitted, uses `output/run-<YYYYMMDD_HHMMSS>/` (local time).
    #[arg(value_name = "DIR")]
    output_dir: Option<PathBuf>,

    /// Load network/simulation parameters from a TOML file
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
}

fn spinner_line_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] {wide_msg}")
        .expect("spinner template")
}

fn bar_line_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len}  {wide_msg}",
    )
    .expect("bar template")
    .progress_chars("=> ")
}

fn simulation_block_height_bar_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] block height {pos}/{len}",
    )
    .expect("sim bar template")
    .progress_chars("=> ")
}

fn default_output_run_dir() -> PathBuf {
    let stamp = Local::now().format("%Y%m%d_%H%M%S");
    PathBuf::from(format!("output/run-{stamp}"))
}

/// Load simulation + network configuration, optionally from a TOML file, falling back to built-in defaults.
fn load_configs(
    path: Option<&Path>,
    multi: &MultiProgress,
) -> io::Result<(SimulationConfig, NetworkConfig)> {
    match path {
        Some(p) => {
            let fc =
                FileConfig::load(p).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            multi.println(format!("Loaded config {}", p.display()))?;
            Ok(fc.build(SimulationConfig::default()))
        }
        None => {
            multi.println("No config file; using built-in defaults")?;
            Ok((SimulationConfig::default(), NetworkConfig::default()))
        }
    }
}

/// Paths written by [`write_all_outputs`]; only `output_json` is returned for CLI display.
struct OutputPaths {
    output_json: PathBuf,
}

/// Write `static.json` before running; writes are reported via `multi`'s spinner.
fn write_static(simulation: &Simulation, out_dir: &Path, multi: &MultiProgress) -> io::Result<()> {
    let static_path = out_dir.join("static.json");
    let pb = multi.add(ProgressBar::new_spinner());
    pb.set_style(spinner_line_style());
    pb.set_message(format!("Writing {}", static_path.display()));
    pb.enable_steady_tick(Duration::from_millis(100));
    simulation.write_static_json(&static_path)?;
    pb.finish_with_message(format!("Wrote {}", static_path.display()));
    Ok(())
}

/// Write `output.json`, `graph/`, `blockList.txt`, and `propagation.csv` (post-run).
fn write_all_outputs(
    simulation: &Simulation,
    out_dir: &Path,
    multi: &MultiProgress,
) -> io::Result<OutputPaths> {
    let pb = multi.add(ProgressBar::new(EXPORT_STEPS));
    pb.set_style(bar_line_style());
    pb.set_message("Writing outputs");

    let json_path = out_dir.join("output.json");
    pb.set_message(format!("Writing {}", json_path.display()));
    let mut w = BufWriter::new(File::create(&json_path)?);
    simulation.write_json_events(&mut w)?;
    w.flush()?;
    pb.inc(1);

    pb.set_message(format!("Writing {}", out_dir.join("graph").display()));
    simulation.write_graph_dir(out_dir)?;
    pb.inc(1);

    let block_list = out_dir.join("blockList.txt");
    pb.set_message(format!("Writing {}", block_list.display()));
    simulation.write_block_list_txt(&block_list)?;
    pb.inc(1);

    let propagation_csv = out_dir.join("propagation.csv");
    pb.set_message(format!("Writing {}", propagation_csv.display()));
    simulation.write_propagation_csv(&propagation_csv)?;
    pb.inc(1);

    pb.finish_with_message(format!("Wrote all outputs under {}", out_dir.display()));
    Ok(OutputPaths {
        output_json: json_path,
    })
}

fn main() -> io::Result<()> {
    let Cli {
        output_dir,
        config: config_path,
    } = Cli::parse();
    let out_dir = output_dir.unwrap_or_else(default_output_run_dir);
    std::fs::create_dir_all(&out_dir)?;

    let multi = MultiProgress::with_draw_target(ProgressDrawTarget::stderr());
    let (sim_cfg, net_cfg) = load_configs(config_path.as_deref(), &multi)?;

    let mut simulation = Simulation::new(sim_cfg, net_cfg);
    multi.println(format!(
        "Initialized {} nodes, {} region(s), run until block height {}",
        simulation.sim.num_nodes,
        simulation.net.topology.regions.len(),
        simulation.sim.end_block_height,
    ))?;

    write_static(&simulation, &out_dir, &multi)?;

    let end_h = simulation.sim.end_block_height.max(1);
    let sim_pb = multi.add(ProgressBar::new(end_h as u64));
    sim_pb.set_style(simulation_block_height_bar_style());
    let stats = simulation.run_observer(PROGRESS_TICK_EVENTS, |_events_done, s| {
        let tip = s.max_tip_height().min(end_h);
        sim_pb.set_position(tip as u64);
    });
    sim_pb.finish_with_message(format!(
        "done: sim time {} ms, {} blocks, {} log events",
        stats.final_time_ms, stats.blocks, stats.events_logged
    ));

    let OutputPaths { output_json } = write_all_outputs(&simulation, &out_dir, &multi)?;

    let config_note = config_path
        .as_ref()
        .map(|p| format!(" (config {})", p.display()))
        .unwrap_or_default();
    multi.println(format!(
        "Summary: {} blocks, {} ms sim time, {} log events — {}{}",
        stats.blocks,
        stats.final_time_ms,
        stats.events_logged,
        output_json.display(),
        config_note
    ))?;

    Ok(())
}
