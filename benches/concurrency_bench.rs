//! Concurrency benchmark — proves the WAL multi-connection pool delivers
//! reader/writer concurrency that a single-connection mutex wrapper cannot.
//!
//! Issue #45, Axis E ("Concurrency: implemented, unproven"). The pool's module
//! doc claims "Multiple read queries can execute concurrently in WAL mode"; a
//! background writer runs while a wave of concurrent readers completes, and the
//! wave latency is compared between `AsyncGraph` (single `Mutex<Connection>`,
//! all access serialized) and `AsyncPoolGraph` (WAL, bounded connection pool).
//!
//! Run: `cargo bench --bench concurrency_bench --features graph-pool,graph-async`

use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tempfile::TempDir;

use llm_kernel::graph::AsyncPoolGraph;
use llm_kernel::graph::async_graph::AsyncGraph;
use llm_kernel::graph::schema::init_graph_schema;
use llm_kernel::graph::store::{append_edge, upsert_node as upsert_sync};
use llm_kernel::graph::types::{GraphEdge, GraphNode};
use rusqlite::Connection;

// ── Fixtures ─────────────────────────────────────────────

fn make_node(id: usize) -> GraphNode {
    GraphNode {
        id: format!("n{id}"),
        node_type: "concept".to_string(),
        title: format!("Node {id}"),
        body: format!("Body content for node {id} with assorted keywords"),
        tags: vec!["bench".to_string()],
        projects: vec![],
        agents: vec![],
        created: "2026-01-01T00:00:00Z".to_string(),
        updated: "2026-01-01T00:00:00Z".to_string(),
        importance: 0.5,
        access_count: 0,
        accessed_at: String::new(),
    }
}

fn make_edge(id: usize, src: usize, tgt: usize) -> GraphEdge {
    GraphEdge {
        id: format!("e{id}"),
        source: format!("n{src}"),
        target: format!("n{tgt}"),
        relation: "related".to_string(),
        weight: 1.0,
        ts: "2026-01-01T00:00:00Z".to_string(),
    }
}

/// Populate a fresh file DB with `nodes` nodes + 3 edges/node under WAL. Both
/// backends open WAL files, so the benchmark variable is the *access pattern*
/// (single mutex vs multi-connection pool), not the journal mode.
fn populate_file(dir: &TempDir, label: &str, nodes: usize) -> std::path::PathBuf {
    let path = dir.path().join(format!("{label}.db"));
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
        .unwrap();
    init_graph_schema(&conn).unwrap();
    for i in 0..nodes {
        upsert_sync(&conn, &make_node(i)).unwrap();
    }
    let mut eid = 0;
    for i in 0..nodes {
        for j in 1..=3 {
            let tgt = (i + j) % nodes;
            append_edge(&conn, &make_edge(eid, i, tgt)).unwrap();
            eid += 1;
        }
    }
    path
}

// ── Backend abstraction ──────────────────────────────────

#[derive(Clone)]
enum Backend {
    Single(AsyncGraph),
    Pool(AsyncPoolGraph),
}

impl Backend {
    fn label(&self) -> &'static str {
        match self {
            Backend::Single(_) => "single-mutex",
            Backend::Pool(_) => "wal-pool-4",
        }
    }
    async fn upsert(&self, node: GraphNode) -> llm_kernel::error::Result<()> {
        match self {
            Backend::Single(g) => g.upsert_node(node).await,
            Backend::Pool(p) => p.upsert_node(node).await,
        }
    }
    async fn read(&self, id: &str) -> llm_kernel::error::Result<Option<GraphNode>> {
        match self {
            Backend::Single(g) => g.read_node(id).await,
            Backend::Pool(p) => p.read_node(id).await,
        }
    }
}

// ── Benchmark: reader wave under sustained writer ────────

const READERS: usize = 16;
const NODES: usize = 2000;

/// Background writer upserting in a tight loop until `stop` flips. Returns the
/// number of successful writes — a non-WAL / over-contended setup surfaces as
/// SQLITE_BUSY failures here (diagnostic, not asserted).
async fn writer_loop(backend: Backend, stop: Arc<AtomicBool>, ok: Arc<AtomicU64>) {
    let mut i = 0u64;
    while !stop.load(Ordering::Relaxed) {
        let node = make_node(NODES + (i as usize % 500));
        if backend.upsert(node).await.is_ok() {
            ok.fetch_add(1, Ordering::Relaxed);
        }
        i += 1;
    }
}

fn bench_read_under_write(c: &mut Criterion) {
    // criterion's `async_tokio` feature implements `AsyncExecutor` for the
    // tokio multi-thread runtime, so we pass `&rt` straight to `to_async`.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("concurrent_read_under_write");
    group.throughput(Throughput::Elements(READERS as u64)); // each iter = READERS reads
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(8));

    let dir = TempDir::new().unwrap();
    let single_path = populate_file(&dir, "single", NODES);
    let pool_path = populate_file(&dir, "pool", NODES);

    let single = rt
        .block_on(async { AsyncGraph::open(single_path.to_str().unwrap()).await })
        .expect("open single");
    let pool = rt
        .block_on(async { AsyncPoolGraph::open(&pool_path, 4).await })
        .expect("open pool");

    let backends = [Backend::Single(single), Backend::Pool(pool)];

    for backend in backends {
        let label = backend.label().to_string();
        group.bench_function(&label, |b| {
            b.to_async(&rt).iter(|| {
                let backend = backend.clone();
                let stop = Arc::new(AtomicBool::new(false));
                let writer_ok = Arc::new(AtomicU64::new(0));
                let w_backend = backend.clone();
                let w_stop = stop.clone();
                let w_ok = writer_ok.clone();
                async move {
                    // Sustained writer contention for the whole reader wave.
                    let writer = tokio::spawn(writer_loop(w_backend, w_stop, w_ok));
                    let mut handles = Vec::with_capacity(READERS);
                    for r in 0..READERS {
                        let b = backend.clone();
                        handles.push(tokio::spawn(async move {
                            let _ = black_box(b.read(&format!("n{}", r % NODES)).await);
                        }));
                    }
                    for h in handles {
                        h.await.unwrap();
                    }
                    stop.store(true, Ordering::Relaxed);
                    let _ = writer.await;
                    writer_ok.load(Ordering::Relaxed)
                }
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_read_under_write);
criterion_main!(benches);
