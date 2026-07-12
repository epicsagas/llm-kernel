//! Scale characterization benchmark — measures how the SQLite graph backend
//! behaves as node/edge count grows from 10K to 1M, and documents the
//! SQLite→PostgreSQL cutover point (issue #45, axis B).
//!
//! Unlike the criterion benches (which exist for tight regression detection),
//! this is a **single-shot characterization**: each scale/operation runs once
//! and the wall-clock is reported. Scale trends matter more here than
//! microvariance, and 1M graphs are too expensive to sample 100×.
//!
//! What is measured, per scale (10K / 100K / 1M nodes):
//! - **node_insert**: `upsert_node` × n
//! - **edge_ingest**: `append_edges` (n × DEGREE edges, one transaction)
//! - **search**: `search_nodes` (FTS5 trigram)
//! - **recall**: `smart_recall` (composite scoring + PageRank boost)
//! - **neighbors**: `graph_neighbors` (1-hop expansion from a single seed)
//!
//! Run: `cargo bench --bench scale_bench --features graph`
//! (PostgreSQL backend measured separately under `LLMKERNEL_PG_URL`.)
//!
//! The synthetic graph is deterministic (no RNG): node `i` cites `DEGREE`
//! forward-spread targets — a sparse, weakly-clustered citation-like topology.

use std::time::{Duration, Instant};

use rusqlite::Connection;
use tempfile::TempDir;

use llm_kernel::graph::recall::smart_recall;
use llm_kernel::graph::schema::init_graph_schema;
use llm_kernel::graph::search::search_nodes;
use llm_kernel::graph::store::{append_edges, upsert_node};
use llm_kernel::graph::traversal::graph_neighbors;
use llm_kernel::graph::types::{GraphEdge, GraphNode};

/// Average out-degree of the synthetic citation-like graph.
const DEGREE: usize = 8;

/// Scales under test.
const SCALES: &[usize] = &[10_000, 100_000, 1_000_000];

/// Deterministic synthetic edges: node `i` → `DEGREE` targets spread forward.
fn synth_edges(n: usize) -> Vec<GraphEdge> {
    let mut out = Vec::with_capacity(n * DEGREE);
    for i in 0..n {
        for j in 0..DEGREE {
            let tgt = (i + 1 + j * 7) % n;
            out.push(GraphEdge {
                id: format!("e-{i}-{j}"),
                source: format!("n{i}"),
                target: format!("n{tgt}"),
                relation: "cites".to_string(),
                weight: 1.0,
                ts: "2026-01-01T00:00:00Z".to_string(),
            });
        }
    }
    out
}

/// Deterministic synthetic nodes with searchable bodies.
fn synth_nodes(n: usize) -> Vec<GraphNode> {
    (0..n)
        .map(|i| GraphNode {
            id: format!("n{i}"),
            node_type: "concept".to_string(),
            title: format!("Node {i}"),
            body: format!("ownership borrowing lifetime concept {i} keyword{}", i % 16),
            tags: vec![format!("band{}", i % 4)],
            projects: vec![],
            agents: vec![],
            created: "2026-01-01T00:00:00Z".to_string(),
            updated: "2026-01-01T00:00:00Z".to_string(),
            importance: 0.5,
            access_count: 0,
            accessed_at: String::new(),
        })
        .collect()
}

/// Open a fresh WAL SQLite graph in a temp dir.
fn fresh_db() -> (TempDir, Connection) {
    let dir = tempfile::Builder::new().tempdir().unwrap();
    let conn = Connection::open(dir.path().join("scale.db")).unwrap();
    init_graph_schema(&conn).unwrap();
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
        .unwrap();
    (dir, conn)
}

fn secs(d: Duration) -> String {
    format!("{:.3}s", d.as_secs_f64())
}

fn main() {
    eprintln!("axis B scale characterization (SQLite/WAL, DEGREE={DEGREE})\n");
    println!(
        "{:>9} {:>10} {:>11} {:>11} {:>9} {:>9} {:>10}",
        "nodes", "edges", "node_insert", "edge_ingest", "search", "recall", "neighbors"
    );

    for &n in SCALES {
        let (dir, conn) = fresh_db();

        // node insert (per-row upsert)
        let nodes = synth_nodes(n);
        let t = Instant::now();
        for node in &nodes {
            upsert_node(&conn, node).unwrap();
        }
        let node_insert = t.elapsed();

        // edge batch ingest (single transaction)
        let edges = synth_edges(n);
        let t = Instant::now();
        append_edges(&conn, &edges).unwrap();
        let edge_ingest = t.elapsed();

        // FTS search (mid-band keyword present in many bodies)
        let t = Instant::now();
        let hits = search_nodes(&conn, "ownership", 10).unwrap();
        let search = t.elapsed();

        // smart recall (composite scoring + PageRank boost)
        let t = Instant::now();
        let recalled = smart_recall(&conn, None, Some("ownership"), 10).unwrap();
        let recall = t.elapsed();

        // 1-hop neighbor expansion from the middle node
        let seeds = vec![format!("n{}", n / 2)];
        let t = Instant::now();
        let nbs = graph_neighbors(&conn, &seeds);
        let neighbors = t.elapsed();

        println!(
            "{:>9} {:>10} {:>11} {:>11} {:>9} {:>9} {:>10}",
            n,
            n * DEGREE,
            secs(node_insert),
            secs(edge_ingest),
            secs(search),
            secs(recall),
            secs(neighbors),
        );
        eprintln!(
            "  n={} → search hits={}, recall hits={}, neighbor seeds-expanded={}",
            n,
            hits.len(),
            recalled.len(),
            nbs.len(),
        );

        drop(conn);
        drop(dir);
    }
}
