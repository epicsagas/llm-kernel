//! Benchmarks for the knowledge graph module — smart_recall, BFS traversal, neighbor lookup,
//! CSR build, and PageRank centrality.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use rusqlite::Connection;

use llm_kernel::graph::algo::{
    CsrGraph, connected_components, dijkstra, jaccard_similarity, label_propagation_default,
    pagerank_default,
};
use llm_kernel::graph::recall::smart_recall;
use llm_kernel::graph::schema::init_graph_schema;
use llm_kernel::graph::store::{append_edge, upsert_node};
use llm_kernel::graph::traversal::{graph_neighbors, related_nodes};
use llm_kernel::graph::types::{GraphEdge, GraphNode};

// ── Fixtures ─────────────────────────────────────────────

fn mem_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_graph_schema(&conn).unwrap();
    conn
}

fn make_node(id: usize, importance: f64, tags: Vec<&str>) -> GraphNode {
    GraphNode {
        id: format!("n{id}"),
        node_type: if id.is_multiple_of(3) {
            "decision"
        } else {
            "concept"
        }
        .to_string(),
        title: format!("Node {id} — some descriptive text about topic {id}"),
        body: format!("Body content for node {id} with keywords rust, async, graph"),
        tags: tags.into_iter().map(|s| s.to_string()).collect(),
        projects: if id.is_multiple_of(5) {
            vec!["bench-proj".to_string()]
        } else {
            vec![]
        },
        agents: vec![],
        created: "2026-01-01T00:00:00Z".to_string(),
        updated: format!("2026-0{}-01T00:00:00Z", (id % 6) + 1),
        importance,
        access_count: (id % 10) as i64,
        accessed_at: String::new(),
    }
}

fn make_edge(id: usize, src: usize, tgt: usize, weight: f64) -> GraphEdge {
    GraphEdge {
        id: format!("e{id}"),
        source: format!("n{src}"),
        target: format!("n{tgt}"),
        relation: "related".to_string(),
        weight,
        ts: "2026-01-01T00:00:00Z".to_string(),
    }
}

/// Populate an in-memory DB with `nodes` nodes and `edges_per_node` edges per node.
fn populate(conn: &Connection, node_count: usize, edges_per_node: usize) {
    for i in 0..node_count {
        let importance = 0.1 + (i as f64 % 10.0) / 10.0 * 0.8;
        let tags: Vec<&str> = if i % 7 == 0 {
            vec!["benchmark", "test"]
        } else if i % 11 == 0 {
            vec!["stale"]
        } else {
            vec!["benchmark"]
        };
        upsert_node(conn, &make_node(i, importance, tags)).unwrap();
    }

    let mut eid = 0;
    for i in 0..node_count {
        for j in 1..=edges_per_node {
            let tgt = (i + j) % node_count;
            if tgt != i {
                append_edge(conn, &make_edge(eid, i, tgt, 1.0)).unwrap();
                eid += 1;
            }
        }
    }
}

// ── smart_recall ─────────────────────────────────────────

fn bench_smart_recall(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_recall");

    for &(nodes, epn) in &[(100, 2), (200, 3), (500, 2)] {
        let conn = mem_db();
        populate(&conn, nodes, epn);

        group.bench_with_input(BenchmarkId::new("no_hint", nodes), &(nodes, epn), |b, _| {
            b.iter(|| {
                black_box(smart_recall(&conn, None, None, 20).unwrap());
            });
        });

        group.bench_with_input(
            BenchmarkId::new("with_hint", nodes),
            &(nodes, epn),
            |b, _| {
                b.iter(|| {
                    black_box(smart_recall(&conn, None, Some("rust async"), 20).unwrap());
                });
            },
        );
    }

    group.finish();
}

// ── related_nodes (BFS) ──────────────────────────────────

fn bench_related_nodes(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_bfs");

    let conn = mem_db();
    populate(&conn, 200, 3);

    for depth in [1, 3, 5] {
        group.bench_with_input(BenchmarkId::new("depth", depth), &depth, |b, &d| {
            b.iter(|| {
                black_box(related_nodes(&conn, "n0", d));
            });
        });
    }

    group.finish();
}

// ── graph_neighbors ──────────────────────────────────────

fn bench_graph_neighbors(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_neighbors");

    let conn = mem_db();
    populate(&conn, 200, 3);

    for seed_count in [1, 5, 20] {
        let seeds: Vec<String> = (0..seed_count).map(|i| format!("n{i}")).collect();

        group.bench_with_input(BenchmarkId::new("seeds", seed_count), &seeds, |b, seeds| {
            b.iter(|| {
                black_box(graph_neighbors(&conn, seeds));
            });
        });
    }

    group.finish();
}

// ── CSR build ───────────────────────────────────────────

fn bench_csr_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_csr_build");

    for &(nodes, epn) in &[(100, 2), (500, 3), (1000, 2)] {
        let conn = mem_db();
        populate(&conn, nodes, epn);

        group.bench_with_input(BenchmarkId::new("nodes", nodes), &conn, |b, conn| {
            b.iter(|| {
                black_box(CsrGraph::build_csr(conn).unwrap());
            });
        });
    }

    group.finish();
}

// ── PageRank ────────────────────────────────────────────

fn bench_pagerank(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_pagerank");

    // CSR is built once per size (setup), so this measures the pure
    // power-iteration cost, not the edge-load + snapshot build.
    for &(nodes, epn) in &[(100, 2), (500, 3), (1000, 2)] {
        let conn = mem_db();
        populate(&conn, nodes, epn);
        let g = CsrGraph::build_csr(&conn).unwrap();

        group.bench_with_input(BenchmarkId::new("nodes", nodes), &g, |b, g| {
            b.iter(|| {
                black_box(pagerank_default(g));
            });
        });
    }

    group.finish();
}

// ── Connected components (union-find) ───────────────────

fn bench_connected_components(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_connected_components");

    for &(nodes, epn) in &[(100, 2), (500, 3), (1000, 2)] {
        let conn = mem_db();
        populate(&conn, nodes, epn);
        let g = CsrGraph::build_csr(&conn).unwrap();

        group.bench_with_input(BenchmarkId::new("nodes", nodes), &g, |b, g| {
            b.iter(|| {
                black_box(connected_components(g));
            });
        });
    }

    group.finish();
}

// ── Label propagation ───────────────────────────────────

fn bench_label_propagation(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_label_propagation");

    for &(nodes, epn) in &[(100, 2), (500, 3), (1000, 2)] {
        let conn = mem_db();
        populate(&conn, nodes, epn);
        let g = CsrGraph::build_csr(&conn).unwrap();

        group.bench_with_input(BenchmarkId::new("nodes", nodes), &g, |b, g| {
            b.iter(|| {
                black_box(label_propagation_default(g));
            });
        });
    }

    group.finish();
}

// ── Dijkstra shortest path ──────────────────────────────

fn bench_dijkstra(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_dijkstra");

    for &(nodes, epn) in &[(100, 2), (500, 3), (1000, 2)] {
        let conn = mem_db();
        populate(&conn, nodes, epn);
        let g = CsrGraph::build_csr(&conn).unwrap();

        group.bench_with_input(BenchmarkId::new("nodes", nodes), &g, |b, g| {
            b.iter(|| {
                black_box(dijkstra(g, 0));
            });
        });
    }

    group.finish();
}

// ── Jaccard similarity ──────────────────────────────────

fn bench_jaccard(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_jaccard");

    for &(nodes, epn) in &[(100, 5), (200, 5)] {
        let conn = mem_db();
        populate(&conn, nodes, epn);
        let g = CsrGraph::build_csr(&conn).unwrap();

        group.bench_with_input(BenchmarkId::new("nodes", nodes), &g, |b, g| {
            b.iter(|| {
                black_box(jaccard_similarity(g, 0, 1));
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_smart_recall,
    bench_related_nodes,
    bench_graph_neighbors,
    bench_csr_build,
    bench_pagerank,
    bench_connected_components,
    bench_label_propagation,
    bench_dijkstra,
    bench_jaccard
);
criterion_main!(benches);
