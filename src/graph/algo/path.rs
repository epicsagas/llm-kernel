//! Weighted shortest path via Dijkstra.
//!
//! Edge weight is relationship **strength** (higher = stronger), but shortest
//! path minimizes **distance**. We convert with `distance = -ln(weight)`: a
//! perfect edge (weight 1.0) costs 0, a weak edge costs more, and weight → 0
//! tends to infinity (the edge becomes impassable). This turns products of
//! edge strengths along a path into a sum of costs, so a path of two strong
//! edges can legitimately beat one weak edge — matching the `shortestPath`
//! semantics of Neo4j/GDS.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use rusqlite::Connection;

use crate::error::Result;

use super::csr::CsrGraph;

/// Edge weights at or below this are treated as impassable (infinite distance).
pub const SHORTEST_PATH_W_MIN: f64 = 1e-9;

/// Total-order wrapper around `f64` so it can key a min-heap. Built on
/// `total_cmp`, so it is consistent with equality and free of the usual NaN
/// pitfalls (we only ever store finite distances).
#[derive(Clone, Copy, PartialEq)]
struct OrderedFloat(f64);

impl Eq for OrderedFloat {}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// One edge's traversal cost: `-ln(weight)`. `None` if the edge is impassable
/// (weight below [`SHORTEST_PATH_W_MIN`] or non-positive).
///
/// Weight is clamped to at most `1.0`: the documented edge-weight contract is
/// `[0, 1]` (see `GraphEdge::weight`), so a contract-violating weight > 1.0 is
/// treated as maximal strength (cost 0) rather than producing a *negative*
/// cost. Without this clamp Dijkstra's non-negative-edge assumption would be
/// violated and it would silently return wrong shortest paths.
fn edge_cost(weight: f64) -> Option<f64> {
    if weight < SHORTEST_PATH_W_MIN {
        return None;
    }
    let w = weight.min(1.0);
    Some(-w.ln())
}

/// Dijkstra from `src`, returning `(node_index, distance)` for every reachable
/// node, sorted by distance ascending. `src` itself appears with distance 0.0.
///
/// Distance is the sum of `-ln(weight)` along the cheapest path. Unreachable
/// nodes (and an out-of-range `src`) are omitted.
pub fn dijkstra(g: &CsrGraph, src: u32) -> Vec<(u32, f64)> {
    let n = g.node_count();
    let mut result = Vec::new();
    if (src as usize) >= n {
        return result;
    }
    let mut dist = vec![f64::INFINITY; n];
    let mut visited = vec![false; n];
    let mut heap: BinaryHeap<(Reverse<OrderedFloat>, u32)> = BinaryHeap::new();

    dist[src as usize] = 0.0;
    heap.push((Reverse(OrderedFloat(0.0)), src));

    while let Some((Reverse(OrderedFloat(d)), u)) = heap.pop() {
        if visited[u as usize] || d > dist[u as usize] {
            continue;
        }
        visited[u as usize] = true;
        result.push((u, d));
        for (v, w) in g.out_neighbors(u) {
            let Some(cost) = edge_cost(w) else {
                continue;
            };
            let nd = d + cost;
            if nd < dist[v as usize] {
                dist[v as usize] = nd;
                heap.push((Reverse(OrderedFloat(nd)), v));
            }
        }
    }

    result.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
    result
}

/// Cheapest path from `src` to `dst` as a node-index sequence, or `None` if
/// unreachable (or either endpoint is out of range).
pub fn shortest_path(g: &CsrGraph, src: u32, dst: u32) -> Option<Vec<u32>> {
    let n = g.node_count();
    if (src as usize) >= n || (dst as usize) >= n {
        return None;
    }
    let mut dist = vec![f64::INFINITY; n];
    let mut prev: Vec<Option<u32>> = vec![None; n];
    let mut visited = vec![false; n];
    let mut heap: BinaryHeap<(Reverse<OrderedFloat>, u32)> = BinaryHeap::new();

    dist[src as usize] = 0.0;
    heap.push((Reverse(OrderedFloat(0.0)), src));

    while let Some((Reverse(OrderedFloat(d)), u)) = heap.pop() {
        if u == dst {
            break;
        }
        if visited[u as usize] {
            continue;
        }
        visited[u as usize] = true;
        for (v, w) in g.out_neighbors(u) {
            let Some(cost) = edge_cost(w) else {
                continue;
            };
            let nd = d + cost;
            if nd < dist[v as usize] {
                dist[v as usize] = nd;
                prev[v as usize] = Some(u);
                heap.push((Reverse(OrderedFloat(nd)), v));
            }
        }
    }

    if dist[dst as usize] == f64::INFINITY {
        return None;
    }

    let mut path = vec![dst];
    let mut cur = dst;
    while let Some(p) = prev[cur as usize] {
        path.push(p);
        cur = p;
    }
    path.reverse();
    Some(path)
}

/// Build a CSR snapshot from `conn` and resolve a path between two node IDs.
pub fn shortest_path_ids(conn: &Connection, src: &str, dst: &str) -> Result<Option<Vec<String>>> {
    let g = CsrGraph::build_csr(conn)?;
    let (Some(s), Some(d)) = (g.node_index(src), g.node_index(dst)) else {
        return Ok(None);
    };
    Ok(
        shortest_path(&g, s, d)
            .map(|path| path.iter().map(|&i| g.node_id(i).to_string()).collect()),
    )
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
    fn strong_two_hop_beats_weak_direct() {
        // A→B (0.5), B→C (0.5): cost = -ln(0.5) - ln(0.5) = 1.386
        // A→C (0.1):           cost = -ln(0.1)           = 2.303
        // The two-hop path through strong edges is shorter.
        let g = graph(
            &["A", "B", "C"],
            &[
                edge("e1", "A", "B", 0.5),
                edge("e2", "B", "C", 0.5),
                edge("e3", "A", "C", 0.1),
            ],
        );
        let a = g.node_index("A").unwrap();
        let b = g.node_index("B").unwrap();
        let c = g.node_index("C").unwrap();
        let path = shortest_path(&g, a, c).expect("path exists");
        assert_eq!(path, vec![a, b, c]);
    }

    #[test]
    fn dijkstra_returns_sorted_reachable() {
        let g = graph(
            &["A", "B", "C"],
            &[edge("e1", "A", "B", 1.0), edge("e2", "B", "C", 1.0)],
        );
        let a = g.node_index("A").unwrap();
        let res = dijkstra(&g, a);
        // Source first (dist 0), then B (dist 0), then C (dist 0) — all weight 1.0 → cost 0.
        // A cost 0, B cost -ln(1.0)=0, C cost 0. Reachable: all three.
        let ids: Vec<u32> = res.iter().map(|(i, _)| *i).collect();
        assert!(ids.contains(&a));
        assert_eq!(res.len(), 3);
        // Sorted ascending by distance.
        for w in res.windows(2) {
            assert!(w[0].1 <= w[1].1 + 1e-12);
        }
    }

    #[test]
    fn unreachable_returns_none() {
        // Two disconnected edges.
        let g = graph(
            &["A", "B", "C", "D"],
            &[edge("e1", "A", "B", 1.0), edge("e2", "C", "D", 1.0)],
        );
        let a = g.node_index("A").unwrap();
        let d = g.node_index("D").unwrap();
        assert!(shortest_path(&g, a, d).is_none());
    }

    #[test]
    fn self_path_is_singleton() {
        let g = graph(&["A", "B"], &[edge("e1", "A", "B", 1.0)]);
        let a = g.node_index("A").unwrap();
        assert_eq!(shortest_path(&g, a, a), Some(vec![a]));
    }

    #[test]
    fn impassable_weight_skipped() {
        // A→B is a near-zero-weight (impassable) edge; A→C→B is the only route.
        let g = graph(
            &["A", "B", "C"],
            &[
                edge("e1", "A", "B", 1e-12),
                edge("e2", "A", "C", 0.9),
                edge("e3", "C", "B", 0.9),
            ],
        );
        let a = g.node_index("A").unwrap();
        let b = g.node_index("B").unwrap();
        let c = g.node_index("C").unwrap();
        let path = shortest_path(&g, a, b).expect("path via C exists");
        assert_eq!(path, vec![a, c, b]);
    }

    #[test]
    fn out_of_range_returns_none() {
        let g = graph(&["A"], &[]);
        // dst index 5 is out of range for a 1-node graph.
        assert!(shortest_path(&g, 0, 5).is_none());
        assert!(dijkstra(&g, 5).is_empty());
    }

    #[test]
    fn weight_above_one_clamped_non_negative() {
        // A weight of 2.0 violates the [0, 1] contract. Without clamping,
        // -ln(2.0) ≈ -0.693 would be a negative Dijkstra cost. Clamped to 1.0
        // it costs -ln(1.0) = 0, and no node is ever assigned a negative
        // distance.
        let g = graph(&["A", "B"], &[edge("e1", "A", "B", 2.0)]);
        let a = g.node_index("A").unwrap();
        let b = g.node_index("B").unwrap();
        let res = dijkstra(&g, a);
        let b_dist = res.iter().find(|(i, _)| *i == b).unwrap().1;
        assert!(b_dist.abs() < 1e-12, "weight > 1 clamps to zero cost");
        assert!(
            res.iter().all(|(_, d)| *d >= -1e-12),
            "no negative distances despite contract-violating weight"
        );
    }
}
