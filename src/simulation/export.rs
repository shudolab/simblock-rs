//! File-format exports produced by [`super::Simulation`]: JSON event log, static regions,
//! propagation delays, graph snapshots, and the block list.

use super::Simulation;
use crate::block::BlockId;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

impl Simulation {
    pub fn write_json_events(&self, w: &mut impl Write) -> std::io::Result<()> {
        self.log.write_array(w)
    }

    /// Writes `static.json`: region id/name list for downstream visualization.
    pub fn write_static_json(&self, path: &Path) -> std::io::Result<()> {
        let mut w = BufWriter::new(File::create(path)?);
        let regions: Vec<serde_json::Value> = self
            .net
            .topology
            .regions
            .iter()
            .enumerate()
            .map(|(id, r)| {
                serde_json::json!({
                    "id": id,
                    "name": r.name,
                })
            })
            .collect();
        serde_json::to_writer(&mut w, &serde_json::json!({ "region": regions }))?;
        writeln!(&mut w)?;
        w.flush()?;
        Ok(())
    }

    /// Prints propagation matrices to `w`: one block as `block_id:height`, then lines
    /// `node_id,delay_ms`, then a blank line.
    pub fn write_propagation_human(&self, w: &mut impl Write) -> std::io::Result<()> {
        for block in self.propagation.blocks() {
            let Some(rec) = self.store.get(block) else {
                continue;
            };
            writeln!(w, "{}:{}", block.0, rec.height)?;
            for (node, d) in self.propagation.observations(block) {
                writeln!(w, "{node},{d}")?;
            }
            writeln!(w)?;
        }
        Ok(())
    }

    /// CSV with header `block_id,height,node_id,delay_ms` (same ordering as [`Self::write_propagation_human`]).
    pub fn write_propagation_csv(&self, path: &Path) -> std::io::Result<()> {
        let mut w = BufWriter::new(File::create(path)?);
        writeln!(w, "block_id,height,node_id,delay_ms")?;
        for block in self.propagation.blocks() {
            let Some(rec) = self.store.get(block) else {
                continue;
            };
            for (node, d) in self.propagation.observations(block) {
                writeln!(w, "{},{},{},{}", block.0, rec.height, node, d)?;
            }
        }
        w.flush()?;
        Ok(())
    }

    /// Directed edges `from to` per line (one line per adjacency).
    fn graph_topology_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        for node in &self.nodes {
            for nbr in node.neighbors_sorted() {
                writeln!(&mut buf, "{} {}", node.id, nbr)?;
            }
        }
        Ok(buf)
    }

    /// Writes `graph/<n>.txt` for each recorded snapshot label (typically `2` and multiples of `100`).
    ///
    /// If no snapshot was recorded (e.g. very low `end_block_height`), falls back to
    /// `0..=max_tip_height()` so at least one file exists.
    ///
    /// The routing graph is fixed after initialization, so every snapshot file contains the same
    /// edge list.
    pub fn write_graph_dir(&self, out_dir: &Path) -> std::io::Result<()> {
        let graph_dir = out_dir.join("graph");
        std::fs::create_dir_all(&graph_dir)?;
        let bytes = self.graph_topology_bytes()?;
        let labels: Vec<u32> = if self.graph_snapshot_heights.is_empty() {
            (0..=self.max_tip_height()).collect()
        } else {
            self.graph_snapshot_heights.clone()
        };
        for h in labels {
            std::fs::write(graph_dir.join(format!("{h}.txt")), &bytes)?;
        }
        Ok(())
    }

    /// Writes `blockList.txt`: blocks on node 1's main chain (excluding genesis) plus all orphans,
    /// sorted by block time then id; each line `OnChain : height : id` or `Orphan : height : id`.
    pub fn write_block_list_txt(&self, path: &Path) -> std::io::Result<()> {
        let mut orphans: HashSet<BlockId> = HashSet::new();
        for node in &self.nodes {
            for &o in &node.orphans {
                orphans.insert(o);
            }
        }

        let mut chain_blocks: HashSet<BlockId> = HashSet::new();
        let mut cur = self.nodes.first().and_then(|n| n.tip);
        while let Some(bid) = cur {
            let rec = match self.store.get(bid) {
                Some(r) => r,
                None => break,
            };
            if rec.parent.is_none() {
                break;
            }
            chain_blocks.insert(bid);
            cur = rec.parent;
        }

        let mut all: Vec<BlockId> = chain_blocks
            .into_iter()
            .chain(orphans.iter().copied())
            .collect();
        all.sort_by_key(|&bid| {
            let t = self.store.get(bid).map(|r| r.time_ms).unwrap_or(0);
            (t, bid.0)
        });
        all.dedup();

        let mut w = BufWriter::new(File::create(path)?);
        for bid in all {
            let Some(rec) = self.store.get(bid) else {
                continue;
            };
            let label = if orphans.contains(&bid) {
                "Orphan"
            } else {
                "OnChain"
            };
            writeln!(w, "{} : {} : {}", label, rec.height, bid.0)?;
        }
        w.flush()?;
        Ok(())
    }
}
