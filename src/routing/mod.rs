//! Topology: random outbound links and neighbor enumeration.

use crate::types::NodeId;
use rand::seq::SliceRandom;
use rand::Rng;

/// Each node opens outbound connections up to its cap; the peer records an inbound edge.
pub fn init_outbound_links<R: Rng + ?Sized>(
    rng: &mut R,
    max_out: &[usize],
    out: &mut [Vec<NodeId>],
    inn: &mut [Vec<NodeId>],
) {
    let n = out.len();
    debug_assert_eq!(inn.len(), n);
    let mut order: Vec<usize> = (0..n).collect();
    order.shuffle(rng);
    for &i in &order {
        let self_id = (i + 1) as NodeId;
        let mut cand: Vec<usize> = (0..n).filter(|&x| x != i).collect();
        cand.shuffle(rng);
        for &j in &cand {
            let peer = (j + 1) as NodeId;
            let _ = try_add_edge(i, peer, self_id, max_out, out, inn);
        }
    }
}

fn try_add_edge(
    i: usize,
    peer: NodeId,
    self_id: NodeId,
    max_out: &[usize],
    out: &mut [Vec<NodeId>],
    inn: &mut [Vec<NodeId>],
) -> bool {
    if peer == self_id {
        return false;
    }
    if out[i].contains(&peer) || inn[i].contains(&peer) {
        return false;
    }
    if out[i].len() >= max_out[i] {
        return false;
    }
    let j = peer as usize - 1;
    if inn[j].contains(&self_id) {
        return false;
    }
    out[i].push(peer);
    inn[j].push(self_id);
    true
}

pub fn neighbor_ids(out: &[NodeId], inn: &[NodeId]) -> Vec<NodeId> {
    let mut v: Vec<NodeId> = out.iter().chain(inn.iter()).copied().collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn neighbor_ids_merges_and_dedups() {
        let out = vec![3u32, 1];
        let inn = vec![2, 3];
        assert_eq!(neighbor_ids(&out, &inn), vec![1, 2, 3]);
    }

    #[test]
    fn init_outbound_respects_caps_and_avoids_duplicates() {
        let n = 5usize;
        let max_out = vec![2, 2, 2, 2, 2];
        let mut out: Vec<Vec<NodeId>> = vec![Vec::new(); n];
        let mut inn: Vec<Vec<NodeId>> = vec![Vec::new(); n];
        let mut rng = StdRng::seed_from_u64(123);
        init_outbound_links(&mut rng, &max_out, &mut out, &mut inn);
        for i in 0..n {
            assert!(out[i].len() <= 2);
            for &p in &out[i] {
                assert_ne!(p, (i + 1) as NodeId);
            }
            let uniq: std::collections::HashSet<_> = out[i].iter().copied().collect();
            assert_eq!(uniq.len(), out[i].len());
        }
        for j in 0..n {
            for &from in &inn[j] {
                let peer = (j + 1) as NodeId;
                assert!(out[from as usize - 1].contains(&peer));
            }
        }
    }
}
