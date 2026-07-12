# Scale characterization (axis B)

How the SQLite graph backend behaves as the graph grows from 10K to 1M nodes,
and where the SQLite → PostgreSQL cutover sits. Issue #45, axis B ("Scale
ceiling: characterize `graph-pool` and `PgGraph` from ~10K to 1M nodes").

## Method

`benches/scale_bench.rs` — single-shot wall-clock per scale/operation (scale
*trends* matter more than microvariance, and 1M graphs are too expensive to
sample 100×). Run:

```
cargo bench --bench scale_bench --features graph
```

Synthetic **citation-like** topology, deterministic (no RNG): node `i` cites
`DEGREE = 8` forward-spread targets → `n` nodes and `8n` edges, sparse and
weakly clustered. SQLite/WAL (`journal_mode = WAL`, `synchronous = NORMAL`),
default feature set, release profile, local SSD.

## Results (SQLite/WAL)

| nodes | edges | node_insert | edge_ingest | search | recall | neighbors |
|------:|------:|------------:|------------:|-------:|-------:|----------:|
|  10K  |  80K  |    1.861s   |    0.277s   | 0.004s | 0.007s |   0.000s  |
| 100K  | 800K  |   23.288s   |    4.034s   | 0.060s | 0.082s |   0.000s  |
|   1M  |   8M  |  272.976s   |   62.592s   | 8.558s | 9.850s |   0.003s  |

- `node_insert` — `upsert_node` × n (per-row)
- `edge_ingest` — `append_edges` batch (one transaction; v0.19.0)
- `search` — `search_nodes` FTS5 trigram, `"ownership"`
- `recall` — `smart_recall` (composite + PageRank boost)
- `neighbors` — `graph_neighbors` 1-hop from one seed (≈instant; bounded by degree)

## Findings

1. **`node_insert` is the write bottleneck.** It scales linearly but per-row:
   1M nodes take ~4.5 min, dominated by per-upsert transaction overhead.
   `edge_ingest` — which uses the v0.19.0 batch path — is ~4× faster per row
   (1M × 8 edges in 62s ≈ 128K edges/s). **A batch `upsert_nodes` API would
   close most of this gap** (candidate for a post-1.0 addition).

2. **Read paths stay interactive through ~100K nodes** (search 60 ms, recall
   82 ms). At **1M, `search`/`recall` climb to ~8–10 s** — beyond interactive
   latency for a single query.

3. **`search`/`recall` cost at 1M is FTS5-trigram-bound.** `search_nodes` uses
   `trigram` tokenization; the recency/importance/access gating in
   `smart_recall` adds little on top of the FTS scan. The `pg.rs` perf note
   documents the same class of issue for ILIKE on PostgreSQL.

4. **`neighbors` is effectively free** at every scale (single-seed 1-hop,
   degree-bounded) — directional/relation-filtered lookups (`edges_for_node_dir`,
   `neighbors_weighted`) inherit the v3 composite indexes and stay cheap.

## SQLite → PostgreSQL cutover

| Workload | Recommendation |
|---|---|
| ≤ ~100K nodes, read-heavy | SQLite/WAL (`graph-pool`) — interactive reads, single-file ops |
| 1M nodes, read-heavy (search/recall frequent) | **PostgreSQL** — index substring search with `pg_trgm` (`CREATE EXTENSION pg_trgm; CREATE INDEX … USING gin (…)`) to bring 1M reads back toward interactive |
| Bulk ingest (build once) | SQLite is acceptable even at 1M (edges batch = ~1 min); the pain is per-row **node** upsert — prefer a batch path |
| Concurrent writer + readers | `AsyncPoolGraph` (WAL) already proven (see `graph_concurrency.md`); PostgreSQL for higher write concurrency |

Rule of thumb: **size by read frequency, not just node count.** A 1M static
graph that is rarely queried is fine on SQLite; a 1M graph with frequent
`smart_recall` per request wants PostgreSQL + `pg_trgm`.

## Migration (`migrate.rs`)

The bundled `llm-kernel-migrate-graph` CLI copies nodes/edges SQLite →
PostgreSQL through the `GraphBackend` trait. At 1M, expect the bulk copy to
take a few minutes (bounded by `append_edges` throughput on the target); the
cutover itself is a one-time offline operation.

## PostgreSQL numbers

Not yet measured on the same fixture. To capture them, port `scale_bench.rs`
to `PgGraph` (or run the SQLite bench as the SQLite column and a `LLMKERNEL_PG_URL`-gated
PG variant alongside). The expectation, given the `pg_trgm` opt-in, is that PG
with the trigram index narrows the 1M `search`/`recall` gap substantially while
SQLite remains the lighter default for small/medium graphs.

## Related

- Issue #45 axis B (this closes the "scale ceiling" characterization)
- `benches/scale_bench.rs`
- `graph_concurrency.md` (axis E — WAL reader/writer concurrency)
- `korean-recall.md` (axis D — `graph-cjk` vs FTS5 trigram recall)
- `src/graph/pg.rs` perf note (ILIKE sequential scan; `pg_trgm` opt-in)
