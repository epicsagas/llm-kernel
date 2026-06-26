//! Node similarity and link prediction.
//!
//! All measures use the **undirected** neighborhood (out-edges + in-edges),
//! ignoring edge direction and self-loops. These are the classic
//! graph-theoretic link-prediction heuristics (Jaccard, common neighbors,
//! Adamic-Adar) used to surface "these two nodes are likely related even
//! though no edge exists yet" — useful for suggesting edges to a knowledge
//! graph, or for finding near-duplicate nodes to merge.

use std::cmp::Ordering;
use std::collections::HashSet;

use super::csr::CsrGraph;

/// Undirected neighbor set of `x` — out-neighbors plus in-neighbors, minus
/// `x` itself (self-loops excluded).
fn neighbor_set(g: &CsrGraph, x: u32) -> HashSet<u32> {
    let mut s: HashSet<u32> = g.out_neighbors(x).map(|(t, _)| t).collect();
    for (src, _) in g.in_neighbors(x) {
        s.insert(src);
    }
    s.remove(&x);
    s
}

/// Jaccard similarity of two nodes' neighborhoods:
/// `|N(a) ∩ N(b)| / |N(a) ∪ N(b)|`.
///
/// Returns 0.0 if both neighborhoods are empty.
pub fn jaccard_similarity(g: &CsrGraph, a: u32, b: u32) -> f64 {
    let na = neighbor_set(g, a);
    let nb = neighbor_set(g, b);
    if na.is_empty() && nb.is_empty() {
        return 0.0;
    }
    let inter = na.intersection(&nb).count() as f64;
    let union = na.union(&nb).count() as f64;
    if union == 0.0 { 0.0 } else { inter / union }
}

/// Number of shared neighbors: `|N(a) ∩ N(b)|`.
pub fn common_neighbors(g: &CsrGraph, a: u32, b: u32) -> u32 {
    let na = neighbor_set(g, a);
    let nb = neighbor_set(g, b);
    na.intersection(&nb).count() as u32
}

/// Adamic-Adar index: `Σ 1/ln(degree(z))` over shared neighbors `z`.
///
/// Rarer shared neighbors (low degree) contribute more than popular hubs — a
/// shared leaf node is stronger evidence of a relationship than a shared hub.
/// Shared neighbors of degree ≤ 1 contribute nothing (avoiding division by
/// zero / `1/ln(1) = ∞`).
pub fn adamic_adar(g: &CsrGraph, a: u32, b: u32) -> f64 {
    let na = neighbor_set(g, a);
    let nb = neighbor_set(g, b);
    let mut score = 0.0_f64;
    for &z in na.intersection(&nb) {
        let deg = neighbor_set(g, z).len();
        if deg > 1 {
            score += 1.0 / (deg as f64).ln();
        }
    }
    score
}

/// Rank currently-unconnected node pairs by predicted connection strength.
///
/// For every pair `(a, b)` that shares at least one neighbor but has no direct
/// edge, computes the Adamic-Adar score and returns the `top_k` strongest,
/// sorted descending. Restricting to pairs with a common neighbor prunes the
/// `O(V²)` candidate space down to `O(V · d²)` (d = average degree) — pairs
/// with no common neighbor score 0 and carry no signal.
///
/// Each pair is emitted once with `a < b`.
pub fn link_prediction(g: &CsrGraph, top_k: usize) -> Vec<(u32, u32, f64)> {
    let n = g.node_count();
    if n < 2 {
        return Vec::new();
    }
    let neighbor_sets: Vec<HashSet<u32>> = (0..n as u32).map(|i| neighbor_set(g, i)).collect();

    let mut candidates: Vec<(u32, u32, f64)> = Vec::new();
    for a in 0..n as u32 {
        for b in (a + 1)..n as u32 {
            // Skip pairs already directly connected (undirected).
            if neighbor_sets[a as usize].contains(&b) {
                continue;
            }
            let shared: Vec<u32> = neighbor_sets[a as usize]
                .intersection(&neighbor_sets[b as usize])
                .copied()
                .collect();
            if shared.is_empty() {
                continue;
            }
            let mut score = 0.0_f64;
            for z in shared {
                let deg = neighbor_sets[z as usize].len();
                if deg > 1 {
                    score += 1.0 / (deg as f64).ln();
                }
            }
            candidates.push((a, b, score));
        }
    }

    candidates.sort_by(|x, y| y.2.partial_cmp(&x.2).unwrap_or(Ordering::Equal));
    candidates.truncate(top_k);
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::algo::csr::CsrGraph;
    use crate::graph::types::GraphEdge;

    fn edge(id: &str, s: &str, t: &str, w: f64) -> GraphEdge {
        GraphEdge {
            id: id.into(),
            source: s.into(),
            target: t.into(),
            relation: "related".into(),
            weight: w,
            ts: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn graph(nodes: &[&str], edges: &[GraphEdge]) -> CsrGraph {
        let node_ids: Vec<String> = nodes.iter().map(|s| s.to_string()).collect();
        CsrGraph::from_edges(&node_ids, edges)
    }

    #[test]
    fn jaccard_one_third() {
        // A→B, A→C, B→C: N(A)={B,C}, N(B)={A,C} → inter {C}, union {A,B,C}.
        let g = graph(
            &["A", "B", "C"],
            &[
                edge("e1", "A", "B", 1.0),
                edge("e2", "A", "C", 1.0),
                edge("e3", "B", "C", 1.0),
            ],
        );
        let a = g.node_index("A").unwrap();
        let b = g.node_index("B").unwrap();
        assert!((jaccard_similarity(&g, a, b) - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn jaccard_identical_neighborhoods_is_one() {
        // B and C both link only to A (and A to both): N(B)={A}, N(C)={A}.
        let g = graph(
            &["A", "B", "C"],
            &[edge("e1", "A", "B", 1.0), edge("e2", "A", "C", 1.0)],
        );
        let b = g.node_index("B").unwrap();
        let c = g.node_index("C").unwrap();
        assert!((jaccard_similarity(&g, b, c) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn common_neighbors_counts_shared() {
        let g = graph(
            &["A", "B", "C"],
            &[edge("e1", "A", "C", 1.0), edge("e2", "B", "C", 1.0)],
        );
        let a = g.node_index("A").unwrap();
        let b = g.node_index("B").unwrap();
        assert_eq!(common_neighbors(&g, a, b), 1); // shared neighbor C
    }

    #[test]
    fn adamic_adar_shared_hub() {
        // Shared neighbor C with degree 2 → contributes 1/ln(2).
        let g = graph(
            &["A", "B", "C"],
            &[edge("e1", "A", "C", 1.0), edge("e2", "B", "C", 1.0)],
        );
        let a = g.node_index("A").unwrap();
        let b = g.node_index("B").unwrap();
        assert!((adamic_adar(&g, a, b) - 1.0 / 2.0_f64.ln()).abs() < 1e-9);
    }

    #[test]
    fn link_prediction_finds_unconnected_pair() {
        // A is a hub: A-B, A-C. B and C are unconnected but share neighbor A.
        let g = graph(
            &["A", "B", "C"],
            &[edge("e1", "A", "B", 1.0), edge("e2", "A", "C", 1.0)],
        );
        let b = g.node_index("B").unwrap();
        let c = g.node_index("C").unwrap();
        let predictions = link_prediction(&g, 5);
        assert!(!predictions.is_empty(), "should predict the B-C link");
        // Top prediction is the B-C pair.
        let (pa, pb, score) = predictions[0];
        assert!(((pa, pb) == (b, c)) || ((pa, pb) == (c, b)));
        assert!(score > 0.0);
    }

    #[test]
    fn link_prediction_empty_when_fully_connected() {
        // Triangle: every pair already connected → no predictions.
        let g = graph(
            &["A", "B", "C"],
            &[
                edge("e1", "A", "B", 1.0),
                edge("e2", "B", "C", 1.0),
                edge("e3", "C", "A", 1.0),
            ],
        );
        assert!(link_prediction(&g, 5).is_empty());
    }

    #[test]
    fn similarity_disjoint_nodes_zero() {
        let g = graph(&["A", "B"], &[edge("e1", "A", "B", 1.0)]);
        let a = g.node_index("A").unwrap();
        let b = g.node_index("B").unwrap();
        // N(A)={B}, N(B)={A} → no shared neighbor.
        assert_eq!(common_neighbors(&g, a, b), 0);
        assert!((adamic_adar(&g, a, b)).abs() < 1e-12);
    }
}
