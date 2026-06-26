//! Compressed Sparse Row (CSR) representation of the knowledge graph.
//!
//! [`CsrGraph`] is a backend-agnostic in-memory snapshot of the directed,
//! weighted edge set, built once and reused by all graph algorithms. Storing
//! the graph as CSR — rather than querying SQL per algorithm step — lets the
//! iterative algorithms (PageRank, label propagation) run in cache-friendly
//! contiguous sweeps over flat `Vec`s.
//!
//! `relation` is intentionally collapsed: every algorithm treats the graph as
//! a single directed weighted graph, matching the existing `smart_recall`
//! graph-boost semantics (`recall.rs`). Multi-relational PageRank is out of
//! scope for the agent-memory workload.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::error::Result;
use crate::graph::store::{list_node_ids, read_edges};
use crate::graph::types::GraphEdge;

/// Edges-per-node cap passed to `read_edges` in [`CsrGraph::build_csr`].
///
/// `read_edges` hardcodes `LIMIT ?`, so we pass a node-count-proportional
/// bound instead of the fixed 2000-row snapshot cap used by `build_graph`.
const CSR_EDGE_CAP_PER_NODE: usize = 64;
/// Lower bound on the `build_csr` edge cap so tiny graphs still load fully.
const CSR_EDGE_CAP_MIN: usize = 2000;

/// A directed, weighted graph in Compressed Sparse Row format.
///
/// Built once from the edge set and reused by every graph algorithm. Both
/// forward (out-edges) and reverse (in-edges) CSR arrays are stored: PageRank
/// and Dijkstra consume out-edges; community detection and similarity use the
/// undirected view (both directions).
pub struct CsrGraph {
    node_count: usize,
    edge_count: usize,
    // Out-edges grouped by source. `out_offsets.len() == node_count + 1`;
    // node `i`'s out-edges occupy `out_offsets[i]..out_offsets[i+1]`.
    out_offsets: Vec<u32>,
    out_targets: Vec<u32>,
    out_weights: Vec<f64>,
    // In-edges grouped by target (reverse CSR).
    in_offsets: Vec<u32>,
    in_sources: Vec<u32>,
    in_weights: Vec<f64>,
    // Σ out-weights per node. `<= 0.0` marks a dangling node (PageRank leak).
    out_weight_sum: Vec<f64>,
    // ID <-> index mapping, built once and immutable.
    id_to_idx: HashMap<String, u32>,
    idx_to_id: Vec<String>,
}

impl CsrGraph {
    /// Build a CSR graph from an explicit node-id list and edge list.
    ///
    /// Edges whose `source` or `target` is absent from `node_ids` are dropped
    /// (foreign-reference integrity). Duplicate node IDs collapse to the first
    /// occurrence. The construction is `O(V + E log E)`.
    pub fn from_edges(node_ids: &[String], edges: &[GraphEdge]) -> Self {
        let mut id_to_idx: HashMap<String, u32> = HashMap::with_capacity(node_ids.len());
        let mut idx_to_id: Vec<String> = Vec::with_capacity(node_ids.len());
        for id in node_ids {
            if id_to_idx.contains_key(id) {
                continue;
            }
            id_to_idx.insert(id.clone(), idx_to_id.len() as u32);
            idx_to_id.push(id.clone());
        }
        let node_count = idx_to_id.len();

        // Resolve edges to (source_idx, target_idx, weight), dropping orphans.
        let mut resolved: Vec<(u32, u32, f64)> = Vec::with_capacity(edges.len());
        for e in edges {
            let (Some(&src), Some(&dst)) = (id_to_idx.get(&e.source), id_to_idx.get(&e.target))
            else {
                continue;
            };
            resolved.push((src, dst, e.weight));
        }

        // Out CSR: stable sort by source, count, prefix-sum, append targets.
        resolved.sort_unstable_by_key(|&(s, _, _)| s);
        let edge_count = resolved.len();
        let mut out_offsets = vec![0u32; node_count + 1];
        let mut out_weight_sum = vec![0.0f64; node_count];
        for &(src, _, w) in &resolved {
            out_offsets[src as usize + 1] += 1;
            out_weight_sum[src as usize] += w;
        }
        for i in 1..=node_count {
            out_offsets[i] += out_offsets[i - 1];
        }
        let mut out_targets = Vec::with_capacity(edge_count);
        let mut out_weights = Vec::with_capacity(edge_count);
        for &(_, dst, w) in &resolved {
            out_targets.push(dst);
            out_weights.push(w);
        }

        // In CSR: reorder to (target, source, weight), sort by target.
        let mut by_target: Vec<(u32, u32, f64)> =
            resolved.iter().map(|&(s, t, w)| (t, s, w)).collect();
        by_target.sort_unstable_by_key(|&(t, _, _)| t);
        let mut in_offsets = vec![0u32; node_count + 1];
        for &(tgt, _, _) in &by_target {
            in_offsets[tgt as usize + 1] += 1;
        }
        for i in 1..=node_count {
            in_offsets[i] += in_offsets[i - 1];
        }
        let mut in_sources = Vec::with_capacity(edge_count);
        let mut in_weights = Vec::with_capacity(edge_count);
        for &(_, src, w) in &by_target {
            in_sources.push(src);
            in_weights.push(w);
        }

        Self {
            node_count,
            edge_count,
            out_offsets,
            out_targets,
            out_weights,
            in_offsets,
            in_sources,
            in_weights,
            out_weight_sum,
            id_to_idx,
            idx_to_id,
        }
    }

    /// Build a CSR snapshot from a SQLite connection.
    ///
    /// Loads all node IDs (including edge-less dangling nodes) and a
    /// node-count-proportional edge cap.
    pub fn build_csr(conn: &Connection) -> Result<Self> {
        let node_ids = list_node_ids(conn)?;
        let cap = node_ids
            .len()
            .saturating_mul(CSR_EDGE_CAP_PER_NODE)
            .max(CSR_EDGE_CAP_MIN);
        let edges = read_edges(conn, cap)?;
        Ok(Self::from_edges(&node_ids, &edges))
    }

    /// Number of nodes.
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// Number of resolved edges (orphans excluded).
    pub fn edge_count(&self) -> usize {
        self.edge_count
    }

    /// Resolve a node ID to its index, or `None` if absent.
    pub fn node_index(&self, id: &str) -> Option<u32> {
        self.id_to_idx.get(id).copied()
    }

    /// Resolve an index back to its node ID. Panics on out-of-range index.
    pub fn node_id(&self, idx: u32) -> &str {
        &self.idx_to_id[idx as usize]
    }

    /// Iterate `(target_idx, weight)` over the out-edges of `src`.
    pub fn out_neighbors(&self, src: u32) -> impl Iterator<Item = (u32, f64)> + '_ {
        let start = self.out_offsets[src as usize] as usize;
        let end = self.out_offsets[src as usize + 1] as usize;
        self.out_targets[start..end]
            .iter()
            .zip(self.out_weights[start..end].iter())
            .map(|(&t, &w)| (t, w))
    }

    /// Iterate `(source_idx, weight)` over the in-edges of `dst`.
    pub fn in_neighbors(&self, dst: u32) -> impl Iterator<Item = (u32, f64)> + '_ {
        let start = self.in_offsets[dst as usize] as usize;
        let end = self.in_offsets[dst as usize + 1] as usize;
        self.in_sources[start..end]
            .iter()
            .zip(self.in_weights[start..end].iter())
            .map(|(&s, &w)| (s, w))
    }

    /// Out-degree (number of out-edges) of `idx`.
    pub fn out_degree(&self, idx: u32) -> u32 {
        self.out_offsets[idx as usize + 1] - self.out_offsets[idx as usize]
    }

    /// Sum of out-edge weights for `idx`. `<= 0.0` marks a dangling node.
    pub fn out_weight_sum(&self, idx: u32) -> f64 {
        self.out_weight_sum[idx as usize]
    }

    /// Whether `idx` is a dangling node — no out-edges, or all-zero weights.
    ///
    /// PageRank redistributes a dangling node's rank mass globally each
    /// iteration to prevent rank leakage and convergence failure.
    pub fn is_dangling(&self, idx: u32) -> bool {
        self.out_weight_sum[idx as usize] <= 0.0
    }

    /// Iterate all node indices in `[0, node_count)`.
    pub fn node_indices(&self) -> std::ops::Range<u32> {
        0..self.node_count as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::schema::init_graph_schema;
    use crate::graph::store::{append_edge, upsert_node};
    use crate::graph::types::GraphNode;
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

    fn node(id: &str) -> GraphNode {
        GraphNode {
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
        }
    }

    /// Diamond: A→B, A→C, B→D, C→D (all weight 1.0). D is a sink.
    fn diamond() -> (Vec<String>, Vec<GraphEdge>) {
        let nodes: Vec<String> = ["A", "B", "C", "D"].iter().map(|s| s.to_string()).collect();
        let edges = vec![
            edge("e1", "A", "B", 1.0),
            edge("e2", "A", "C", 1.0),
            edge("e3", "B", "D", 1.0),
            edge("e4", "C", "D", 1.0),
        ];
        (nodes, edges)
    }

    #[test]
    fn diamond_out_neighbors_and_sink() {
        let (nodes, edges) = diamond();
        let g = CsrGraph::from_edges(&nodes, &edges);
        let a = g.node_index("A").unwrap();
        let b = g.node_index("B").unwrap();
        let c = g.node_index("C").unwrap();
        let d = g.node_index("D").unwrap();

        let mut nbrs: Vec<u32> = g.out_neighbors(a).map(|(t, _)| t).collect();
        nbrs.sort_unstable();
        assert_eq!(nbrs, vec![b, c]);

        // D is a sink — no out-edges → dangling.
        assert_eq!(g.out_degree(d), 0);
        assert!(g.is_dangling(d));
        assert!(!g.is_dangling(a));
    }

    #[test]
    fn reverse_csr_in_neighbors() {
        let (nodes, edges) = diamond();
        let g = CsrGraph::from_edges(&nodes, &edges);
        let b = g.node_index("B").unwrap();
        let c = g.node_index("C").unwrap();
        let d = g.node_index("D").unwrap();

        let mut srcs: Vec<u32> = g.in_neighbors(d).map(|(s, _)| s).collect();
        srcs.sort_unstable();
        assert_eq!(srcs, vec![b, c]);
    }

    #[test]
    fn out_weight_sum_accumulates_parallel_edges() {
        let nodes: Vec<String> = ["A", "B"].iter().map(|s| s.to_string()).collect();
        let edges = vec![
            edge("e1", "A", "B", 0.3),
            edge("e2", "A", "B", 0.7), // parallel edge A→B
        ];
        let g = CsrGraph::from_edges(&nodes, &edges);
        let a = g.node_index("A").unwrap();
        assert!((g.out_weight_sum(a) - 1.0).abs() < 1e-12);
        assert_eq!(g.out_degree(a), 2);
    }

    #[test]
    fn orphan_edges_dropped() {
        let nodes: Vec<String> = ["A", "B"].iter().map(|s| s.to_string()).collect();
        let edges = vec![
            edge("e1", "A", "B", 1.0),
            edge("e2", "A", "Z", 1.0), // target Z absent
            edge("e3", "X", "B", 1.0), // source X absent
        ];
        let g = CsrGraph::from_edges(&nodes, &edges);
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn isolated_node_is_dangling() {
        let nodes: Vec<String> = ["A", "B", "E"].iter().map(|s| s.to_string()).collect();
        let edges = vec![edge("e1", "A", "B", 1.0)];
        let g = CsrGraph::from_edges(&nodes, &edges);
        let e = g.node_index("E").unwrap();
        assert!(g.is_dangling(e));
        assert_eq!(g.node_count(), 3);
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn build_csr_from_sqlite() {
        let conn = mem_db();
        for id in ["A", "B", "C", "D"] {
            upsert_node(&conn, &node(id)).unwrap();
        }
        append_edge(&conn, &edge("e1", "A", "B", 1.0)).unwrap();
        append_edge(&conn, &edge("e2", "B", "C", 1.0)).unwrap();

        let g = CsrGraph::build_csr(&conn).unwrap();
        assert_eq!(g.node_count(), 4);
        assert_eq!(g.edge_count(), 2);
        let a = g.node_index("A").unwrap();
        assert_eq!(g.out_neighbors(a).count(), 1);
        // D has no edges in the DB → dangling.
        assert!(g.is_dangling(g.node_index("D").unwrap()));
    }

    #[test]
    fn self_loop_kept() {
        let nodes: Vec<String> = ["A"].iter().map(|s| s.to_string()).collect();
        let edges = vec![edge("e1", "A", "A", 1.0)];
        let g = CsrGraph::from_edges(&nodes, &edges);
        let a = g.node_index("A").unwrap();
        assert_eq!(g.out_degree(a), 1);
        assert_eq!(g.out_neighbors(a).next().unwrap().0, a);
    }

    #[test]
    fn node_id_roundtrip() {
        let (nodes, edges) = diamond();
        let g = CsrGraph::from_edges(&nodes, &edges);
        for id in &["A", "B", "C", "D"] {
            let idx = g.node_index(id).unwrap();
            assert_eq!(g.node_id(idx), *id);
        }
        assert!(g.node_index("missing").is_none());
    }
}
