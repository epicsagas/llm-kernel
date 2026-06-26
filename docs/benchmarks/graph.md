# Graph Algorithm Benchmarks

Performance baseline for the graph algorithm module (`src/graph/algo/`):
CSR build, weighted PageRank, connected components, label propagation,
Dijkstra shortest path, and Jaccard similarity.

> **Measured:** 2026-06-26 · **Toolchain:** rustc 1.95.0 (edition 2024) ·
> **Harness:** criterion 0.8 · **Backend:** in-memory SQLite (`mem_db()`),
> Apple Silicon (darwin 25.x)
>
> Related: [Graph Performance Strategy](../research/graph_performance_strategy.md) ·
> issue #45 (Axis A — benchmarks & baselines) · ROADMAP v1.0.0 #3

## Methodology

Benchmarks live in `benches/graph_bench.rs`. Each fixture populates an
in-memory graph with `populate(node_count, edges_per_node)` — nodes seeded
with varied importance/tags, edges of weight `1.0`. For the iterative
algorithms the CSR snapshot is built **once per size** (setup), so the
measurement is the pure algorithm cost, not edge-load.

```bash
cargo bench --bench graph_bench --features graph
```

## Results (median, criterion point estimate)

| Algorithm | 100 nodes | 500 nodes | 1000 nodes |
|---|---:|---:|---:|
| **CSR build** (DB load + sort + index) | 75.0 µs | 517 µs | 727 µs |
| **PageRank** (damping 0.85, ≤100 iters) | 435 ns | 2.69 µs | **4.87 µs** |
| **Connected components** (union-find) | 956 ns | 5.53 µs | 9.50 µs |
| **Label propagation** (≤10 iters) | 8.37 µs | 56.2 µs | 109 µs |
| **Dijkstra** (single-source) | 3.17 µs | 19.9 µs | 41.0 µs |
| **Jaccard** (one pair) | 954 ns | — | — |

(Jaccard is benchmarked at 100 and 200 nodes; it is an `O(degree)` pair query,
not a full-graph sweep, so it is essentially constant in node count.)

## Interpretation

- **PageRank is sub-microsecond to ~5 µs** even at 1000 nodes. `smart_recall`
  recomputes PageRank over the top-100 candidate induced subgraph on every
  call (the "recompute" strategy) — at this cost that is a non-issue, which is
  why no result cache was needed.
- **The "10K node ceiling" is a DB-side concern, not an algorithm one.** The
  algorithms themselves scale near-linearly; the one-shot CSR build (727 µs at
  1000 nodes including a full SQLite edge read) is the only part that grows
  with graph size, and it pays the load cost once per snapshot, not per query.
- **Connected components is effectively linear** (`O((V+E)·α(V))`) — 10× node
  count gives ~10× time, as expected.
- **Label propagation** is the most expensive per call (iterative, neighbor
  aggregation) but still 109 µs at 1000 nodes — fine for offline community
  analysis, not a per-recall operation.

These numbers are the **v1.0.0 baseline**. Future regressions should be caught
by wiring `benches/graph_*` into CI as a perf-regression gate (ROADMAP v1.0.0
#3).

## Reproduce

```bash
cargo bench --bench graph_bench --features graph -- \
  "graph_csr|graph_pagerank|graph_connected|graph_label|graph_dijkstra|graph_jaccard"
```

Per-iteration HTML reports are written under `target/criterion/`.
