//! Weighted PageRank centrality.
//!
//! Standard power iteration with dangling-node redistribution. Edges carry
//! weights (relationship strength), so rank flows along an out-edge in
//! proportion to that edge's weight divided by the source's total out-weight —
//! stronger relations pass more rank, matching the `smart_recall` boost
//! semantics.
//!
//! The iterative math is pure-Rust over a [`CsrGraph`] and backend-agnostic,
//! so the SQLite and PostgreSQL recall paths can import the same
//! [`pagerank_default`] and stay drift-free (see `recall.rs`'s `compute_recency`
//! for the same principle).

use rusqlite::Connection;

use crate::error::Result;

use super::csr::CsrGraph;

/// damping factor (rank a node keeps vs. teleports); the canonical 0.85.
pub const PAGERANK_DAMPING: f64 = 0.85;
/// Maximum power-iteration count before giving up on further convergence.
pub const PAGERANK_ITERS: usize = 100;
/// L1-norm convergence threshold; iteration stops once the rank vector stops
/// moving more than this between steps.
pub const PAGERANK_EPS: f64 = 1e-6;

/// Weighted PageRank over `g`, returning one score per node index.
///
/// Scores sum to 1.0 (normalized to absorb floating-point drift). Dangling
/// nodes — those with no out-edges, or all-zero out-weights — have their rank
/// mass redistributed uniformly each iteration so rank does not leak.
pub fn pagerank(g: &CsrGraph, damping: f64, iters: usize) -> Vec<f64> {
    let n = g.node_count();
    if n == 0 {
        return Vec::new();
    }
    let base = (1.0 - damping) / n as f64;
    let mut r = vec![1.0 / n as f64; n];
    let mut next = vec![0.0_f64; n];

    for _ in 0..iters {
        for slot in next.iter_mut() {
            *slot = 0.0;
        }

        // Distribute rank from non-dangling nodes along weighted out-edges,
        // accumulating dangling mass for uniform redistribution.
        let mut dangling_mass = 0.0_f64;
        for i in g.node_indices() {
            let out_sum = g.out_weight_sum(i);
            if out_sum <= 0.0 {
                dangling_mass += r[i as usize];
                continue;
            }
            let share = damping * r[i as usize] / out_sum;
            for (j, w) in g.out_neighbors(i) {
                next[j as usize] += share * w;
            }
        }

        // Teleport base + dangling redistribution is uniform across all nodes.
        let uniform = base + damping * dangling_mass / n as f64;
        for slot in next.iter_mut() {
            *slot += uniform;
        }

        // L1 convergence check.
        let mut delta = 0.0_f64;
        for j in 0..n {
            delta += (next[j] - r[j]).abs();
        }
        std::mem::swap(&mut r, &mut next);
        if delta < PAGERANK_EPS {
            break;
        }
    }

    // Normalize so Σ == 1.0 exactly (absorbs accumulated float drift).
    let sum: f64 = r.iter().sum();
    if sum > 0.0 {
        for v in &mut r {
            *v /= sum;
        }
    }
    r
}

/// PageRank with the canonical defaults (`PAGERANK_DAMPING`, `PAGERANK_ITERS`).
pub fn pagerank_default(g: &CsrGraph) -> Vec<f64> {
    pagerank(g, PAGERANK_DAMPING, PAGERANK_ITERS)
}

/// Build a CSR snapshot from `conn` and return `(node_id, score)` pairs in
/// node-index order.
pub fn pagerank_scores(conn: &Connection) -> Result<Vec<(String, f64)>> {
    let g = CsrGraph::build_csr(conn)?;
    let scores = pagerank_default(&g);
    Ok(g.node_indices()
        .map(|i| (g.node_id(i).to_string(), scores[i as usize]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::algo::csr::CsrGraph;
    use crate::graph::schema::init_graph_schema;
    use crate::graph::store::{append_edge, upsert_node};
    use crate::graph::types::{GraphEdge, GraphNode};
    use rusqlite::Connection;

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_graph_schema(&conn).unwrap();
        conn
    }

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

    /// Diamond: A→B, A→C, B→D, C→D. D is a sink. B, C symmetric.
    fn diamond_csr() -> CsrGraph {
        let nodes: Vec<String> = ["A", "B", "C", "D"].iter().map(|s| s.to_string()).collect();
        let edges = vec![
            edge("e1", "A", "B", 1.0),
            edge("e2", "A", "C", 1.0),
            edge("e3", "B", "D", 1.0),
            edge("e4", "C", "D", 1.0),
        ];
        CsrGraph::from_edges(&nodes, &edges)
    }

    #[test]
    fn empty_graph_returns_empty() {
        let g = CsrGraph::from_edges(&[], &[]);
        assert!(pagerank_default(&g).is_empty());
    }

    #[test]
    fn single_node_scores_one() {
        let nodes: Vec<String> = vec!["A".to_string()];
        let g = CsrGraph::from_edges(&nodes, &[]);
        let pr = pagerank_default(&g);
        assert_eq!(pr.len(), 1);
        assert!((pr[0] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn diamond_sink_dominates_and_symmetric() {
        let g = diamond_csr();
        let pr = pagerank_default(&g);
        let a = g.node_index("A").unwrap() as usize;
        let b = g.node_index("B").unwrap() as usize;
        let c = g.node_index("C").unwrap() as usize;
        let d = g.node_index("D").unwrap() as usize;

        // Normalization invariant.
        assert!(
            (pr.iter().sum::<f64>() - 1.0).abs() < 1e-9,
            "scores must sum to 1.0"
        );
        // All positive.
        assert!(pr.iter().all(|&v| v > 0.0));
        // D (sink) outranks A (pure source).
        assert!(pr[d] > pr[a], "sink D should outrank source A");
        // B and C are structurally symmetric.
        assert!(
            (pr[b] - pr[c]).abs() < 1e-9,
            "symmetric nodes B,C must match"
        );
        // D is the global maximum.
        let max_idx = (0..pr.len())
            .max_by(|&a, &b| {
                pr[a]
                    .partial_cmp(&pr[b])
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap();
        assert_eq!(max_idx, d);
    }

    #[test]
    fn dangling_mass_conserved() {
        // A→B, B is dangling. Rank must not leak: Σ == 1.0 after convergence.
        let nodes: Vec<String> = ["A", "B"].iter().map(|s| s.to_string()).collect();
        let edges = vec![edge("e1", "A", "B", 1.0)];
        let g = CsrGraph::from_edges(&nodes, &edges);
        let pr = pagerank_default(&g);
        assert!((pr.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        // B receives A's rank plus teleport — outranks A.
        let a = g.node_index("A").unwrap() as usize;
        let b = g.node_index("B").unwrap() as usize;
        assert!(pr[b] > pr[a]);
    }

    #[test]
    fn weighted_edges_flow_more_rank() {
        // Two parallel two-hop paths A→hub→sink: one via a strong edge, one weak.
        // The sink reachable through the stronger link accrues more rank.
        let nodes: Vec<String> = ["A", "P", "Q", "Sp", "Sq"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let edges = vec![
            edge("e1", "A", "P", 0.9), // strong path
            edge("e2", "A", "Q", 0.1), // weak path
            edge("e3", "P", "Sp", 1.0),
            edge("e4", "Q", "Sq", 1.0),
        ];
        let g = CsrGraph::from_edges(&nodes, &edges);
        let pr = pagerank_default(&g);
        let sp = g.node_index("Sp").unwrap() as usize;
        let sq = g.node_index("Sq").unwrap() as usize;
        assert!(
            pr[sp] > pr[sq],
            "sink on the stronger A→P edge should outrank the weaker A→Q sink"
        );
        assert!((pr.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn pagerank_scores_from_sqlite() {
        let conn = mem_db();
        for id in ["A", "B", "C"] {
            let n = GraphNode {
                id: id.into(),
                node_type: "concept".into(),
                title: id.into(),
                body: String::new(),
                tags: vec![],
                projects: vec![],
                agents: vec![],
                created: "2026-01-01T00:00:00Z".into(),
                updated: "2026-01-01T00:00:00Z".into(),
                importance: 0.5,
                access_count: 0,
                accessed_at: String::new(),
            };
            upsert_node(&conn, &n).unwrap();
        }
        append_edge(&conn, &edge("e1", "A", "B", 1.0)).unwrap();
        append_edge(&conn, &edge("e2", "A", "C", 1.0)).unwrap();

        let scores = pagerank_scores(&conn).unwrap();
        assert_eq!(scores.len(), 3);
        let map: std::collections::HashMap<String, f64> = scores.into_iter().collect();
        let sum: f64 = map.values().sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }
}
