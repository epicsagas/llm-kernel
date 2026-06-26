//! Community detection: connected components and label propagation.
//!
//! Both algorithms treat the directed weighted graph as **undirected** — an
//! edge connects its endpoints into the same community regardless of
//! direction. Edges are traversed via the forward CSR; since every stored
//! edge is visited from its source, that single pass already unions both
//! endpoints, so no reverse traversal is needed for connected components.
//! Label propagation reads neighbors in both directions.

use super::csr::CsrGraph;

/// Default maximum iteration count for [`label_propagation`].
pub const LABEL_PROPAGATION_ITERS: usize = 10;

/// Connected components via union-find (union-by-rank + path compression).
///
/// Returns one component-root index per node; nodes sharing a root are in the
/// same component. `O((V + E) · α(V))` — effectively linear. The root is a
/// node index (not a sequential component id); callers can relabel if needed.
pub fn connected_components(g: &CsrGraph) -> Vec<u32> {
    let n = g.node_count();
    let mut parent: Vec<u32> = (0..n as u32).collect();
    let mut rank: Vec<u32> = vec![0; n];

    fn find(parent: &mut [u32], x: u32) -> u32 {
        // Walk to the root.
        let mut root = x;
        while parent[root as usize] != root {
            root = parent[root as usize];
        }
        // Path compression: point every node on the path straight at the root.
        let mut cur = x;
        while parent[cur as usize] != root {
            let next = parent[cur as usize];
            parent[cur as usize] = root;
            cur = next;
        }
        root
    }

    // Forward pass unions every edge's source and target — each stored edge is
    // visited once from its source, which is enough for an undirected view.
    for i in g.node_indices() {
        for (j, _) in g.out_neighbors(i) {
            let ri = find(&mut parent, i);
            let rj = find(&mut parent, j);
            if ri == rj {
                continue;
            }
            // Union by rank: smaller tree hangs under the larger.
            let (a, b) = if rank[ri as usize] < rank[rj as usize] {
                (rj, ri)
            } else {
                (ri, rj)
            };
            parent[b as usize] = a;
            if rank[ri as usize] == rank[rj as usize] {
                rank[a as usize] += 1;
            }
        }
    }

    // Final compression so every node holds its root directly.
    for i in 0..n as u32 {
        // Using a scoped helper: re-find mutably.
        let root = {
            let mut root = i;
            while parent[root as usize] != root {
                root = parent[root as usize];
            }
            root
        };
        let mut cur = i;
        while parent[cur as usize] != root {
            let next = parent[cur as usize];
            parent[cur as usize] = root;
            cur = next;
        }
    }
    parent
}

/// Label propagation community detection (asynchronous, weighted).
///
/// Each node adopts the most frequent label among its undirected neighbors,
/// breaking ties toward the smallest label for determinism. Iterates until no
/// label changes or `max_iters` is reached. Weights bias a neighbor's vote by
/// edge strength. Returns one community label per node index.
pub fn label_propagation(g: &CsrGraph, max_iters: usize) -> Vec<u32> {
    let n = g.node_count();
    let mut label: Vec<u32> = (0..n as u32).collect();

    for _ in 0..max_iters {
        let mut changed = false;
        for i in g.node_indices() {
            // Undirected neighbors: out-edges + in-edges, each vote weighted.
            let mut votes: Vec<(u32, f64)> = g
                .out_neighbors(i)
                .map(|(t, w)| (label[t as usize], w))
                .chain(g.in_neighbors(i).map(|(s, w)| (label[s as usize], w)))
                .collect();
            if votes.is_empty() {
                continue;
            }
            votes.sort_unstable_by_key(|&(l, _)| l);

            // Aggregate weights per label, tracking the heaviest (ties → smallest).
            let mut best_label = votes[0].0;
            let mut best_weight = 0.0_f64;
            let mut cur_label = votes[0].0;
            let mut cur_weight = 0.0_f64;
            for &(l, w) in &votes {
                if l == cur_label {
                    cur_weight += w;
                } else {
                    cur_label = l;
                    cur_weight = w;
                }
                // Strictly-greater keeps the first (smallest label) on ties,
                // since votes are sorted ascending by label.
                if cur_weight > best_weight {
                    best_label = cur_label;
                    best_weight = cur_weight;
                }
            }

            if best_label != label[i as usize] {
                label[i as usize] = best_label;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    label
}

/// Label propagation with the default iteration cap (`LABEL_PROPAGATION_ITERS`).
pub fn label_propagation_default(g: &CsrGraph) -> Vec<u32> {
    label_propagation(g, LABEL_PROPAGATION_ITERS)
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
    fn two_disjoint_components() {
        let g = graph(
            &["A", "B", "C", "D", "E"],
            &[
                edge("e1", "A", "B", 1.0),
                edge("e2", "B", "C", 1.0),
                edge("e3", "D", "E", 1.0),
            ],
        );
        let comp = connected_components(&g);
        let a = g.node_index("A").unwrap() as usize;
        let b = g.node_index("B").unwrap() as usize;
        let c = g.node_index("C").unwrap() as usize;
        let d = g.node_index("D").unwrap() as usize;
        let e = g.node_index("E").unwrap() as usize;
        // {A,B,C} share a root; {D,E} share a root; the two differ.
        assert_eq!(comp[a], comp[b]);
        assert_eq!(comp[b], comp[c]);
        assert_eq!(comp[d], comp[e]);
        assert_ne!(comp[a], comp[d]);
    }

    #[test]
    fn single_component_connected() {
        let g = graph(
            &["A", "B", "C"],
            &[edge("e1", "A", "B", 1.0), edge("e2", "B", "C", 1.0)],
        );
        let comp = connected_components(&g);
        let labels: std::collections::HashSet<u32> = comp.iter().copied().collect();
        assert_eq!(labels.len(), 1, "fully connected graph has one component");
    }

    #[test]
    fn direction_ignored_for_components() {
        // A→B, C→B: undirected view unites all three.
        let g = graph(
            &["A", "B", "C"],
            &[edge("e1", "A", "B", 1.0), edge("e2", "C", "B", 1.0)],
        );
        let comp = connected_components(&g);
        let labels: std::collections::HashSet<u32> = comp.iter().copied().collect();
        assert_eq!(labels.len(), 1);
    }

    #[test]
    fn label_propagation_unites_connected_graph() {
        // Path A-B-C converges to a single label.
        let g = graph(
            &["A", "B", "C"],
            &[edge("e1", "A", "B", 1.0), edge("e2", "B", "C", 1.0)],
        );
        let label = label_propagation_default(&g);
        let labels: std::collections::HashSet<u32> = label.iter().copied().collect();
        assert_eq!(
            labels.len(),
            1,
            "connected graph collapses to one community"
        );
    }

    #[test]
    fn label_propagation_splits_disjoint_components() {
        // {A-B} and {C-D}, disconnected → two communities.
        let g = graph(
            &["A", "B", "C", "D"],
            &[edge("e1", "A", "B", 1.0), edge("e2", "C", "D", 1.0)],
        );
        let label = label_propagation_default(&g);
        let a = g.node_index("A").unwrap() as usize;
        let b = g.node_index("B").unwrap() as usize;
        let c = g.node_index("C").unwrap() as usize;
        let d = g.node_index("D").unwrap() as usize;
        assert_eq!(label[a], label[b]);
        assert_eq!(label[c], label[d]);
        assert_ne!(label[a], label[c]);
        let distinct: std::collections::HashSet<u32> = label.iter().copied().collect();
        assert_eq!(distinct.len(), 2);
    }

    #[test]
    fn isolated_nodes_each_own_component() {
        // No edges → every node its own component.
        let g = graph(&["A", "B", "C"], &[]);
        let comp = connected_components(&g);
        let distinct: std::collections::HashSet<u32> = comp.iter().copied().collect();
        assert_eq!(distinct.len(), 3);
    }
}
