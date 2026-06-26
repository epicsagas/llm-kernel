# Graph Performance Maximization Strategy

> Post-v0.9.0 strategy for pushing the `graph` stack to production-grade performance and measurable quality before the v1.0.0 semver lock.
>
> **Status:** Strategy (2026-06-26) · **Phase context:** v0.9.0 complete ✅ → v1.0.0 Production Readiness
> **Related:** [Roadmap Evaluation](roadmap_evaluation.md) · [FTS5 CJK Alternatives](fts5_cjk_alternatives.md) · [Future Milestones](future_roadmap_evaluation.md)

## 1. Problem Framing

The graph stack is **feature-complete** but not yet **performance-characterized**. The breadth a user needs is already shipped:

| Lever | Feature gate | Shipped | Module |
|---|---|---|---|
| CJK lexical search | `graph-cjk` | v0.7.0 | `src/graph/cjk.rs` (app-side inverted index, supersedes `trigram`) |
| Vector ANN | `vector-index` | v0.3.5 | TurboVec (TurboQuant 2/4-bit SIMD) |
| Concurrency | `graph-pool` / `graph-async` | v0.8.0 | `src/graph/async_pool.rs` (WAL multi-connection) |
| Multi-backend | `graph-pg` / `qdrant` / `elastic` / `federation` | v0.8.0–v0.9.0 | `pg.rs`, `src/search/federation.rs` |

So "maximize graph performance" is **not** "add graph-cjk / ANN" (done) — it is: (a) quantify what we have, (b) close the algorithmic/concurrency gaps vs. native graph engines, (c) make the win legible before the v1.0.0 API lock.

## 2. Why Measure Now

v1.0.0 freezes the public API. Two obligations follow before that lock:

- **Don't ship unmeasured guarantees.** The stack currently claims Korean recall (`graph-cjk`), ANN scale (`vector-index`), and reader/writer concurrency (`graph-pool`) without published numbers. Once the API is locked, these become implied promises the library has never validated.
- **Don't ship a shallow surface.** `src/graph/traversal.rs` offers only BFS. A "graph-capable" dependency that users discover — after locking in — to lack connected components, similarity, or community detection is a v1.x churn risk.

The work below is what makes the v1.0.0 lock defensible.

## 3. Maximization Axes

### Axis A — Benchmarks & baselines (gating)
No published numbers exist for the graph stack today. Before optimizing, establish:
- Recall@k / latency / p99 curves for FTS (`graph-cjk` vs `trigram`), ANN (`vector-index` vs brute-force), hybrid (`search` RRF/CombMNZ).
- Korean corpus specifically (the differentiator vs. generic embeddings).
- Cross-backend federation overhead (`qdrant` + `elastic` + TurboVec).
- The `benches/` harness already exists — wire graph benchmarks in as the v1.0.0 exit gate.

### Axis B — Scale ceiling
`graph-pool` (WAL pool) and `PgGraph` exist but aren't characterized past ~10K nodes. Produce a curve to 100K–1M nodes so users know when the SQLite→PostgreSQL cutover (`migrate.rs`) pays off.

### Axis C — Algorithmic depth (the gap vs. Neo4j)
Today: BFS traversal, smart recall, dedup. Native graph engines (Neo4j/GDS) offer PageRank, community detection, shortest-path at scale. For v1.0.0+, evaluate whether a small algorithm set (connected components, node similarity, community detection) belongs in `graph` behind a feature gate — this is the one axis where the embedded-Rust positioning can credibly challenge a server graph DB.

### Axis D — Recall quality, not just speed
`graph-cjk` replaced `trigram` to kill false positives, but recall quality on agglomerated Korean (compound nouns, no spaces) needs an eval harness — reuse `eval`/`eval-full` features. This is the quality dimension the competitive comparison turns on.

### Axis E — Concurrency proof
`async_pool.rs` WAL pool needs a contention benchmark (concurrent writes + reads) to prove the reader/writer concurrency it promises. This is what turns an architectural claim into a measured one.

## 4. Competitive Positioning (the target to defend)

| | alcove | harness-mem | Neo4j agent-memory | **llm-kernel (target)** |
|---|---|---|---|---|
| Paradigm | BM25 lexical | full-text + heuristics | native graph + vector + auto-extract | **lexical + vector + graph, RRF-fused, embedded** |
| Korean | strong (ngram) | weak | embedding-dependent | **`graph-cjk` + Qwen3 (target: parity with alcove)** |
| Vector | ✗ | ✗ | ✓ | ✓ (`vector-index`) |
| Graph | ✗ | wikilink-runtime | native (POLE+O, GDS) | **embedded (BFS → target: more algorithms)** |
| Ops cost | ~0 | 0 | high (server) | **0 (embedded SQLite)** |

llm-kernel's defensible claim: **alcove's Korean strength + Neo4j's semantic/graph depth, in a single embedded Rust dependency with zero server ops.** The benchmarks in Axis A are what make that claim true rather than aspirational.

## 5. Concrete v1.0.0 Work Items

| # | Deliverable | Axis | Key files |
|---|---|---|---|
| 1 | Graph benchmark suite (FTS / ANN / hybrid / federation) | A | `benches/graph_*` (new) |
| 2 | Korean recall eval harness | D | `eval/`, `src/graph/cjk.rs` |
| 3 | `graph-pool` contention benchmark + doc | E | `src/graph/async_pool.rs` |
| 4 | Scale curve 10K→1M (SQLite vs Pg cutover guidance) | B | `benches/`, `src/bin/migrate.rs` doc |
| 5 | Feasibility: additional graph algorithms behind feature gate | C | `src/graph/` (new module) |

**Exit criteria for v1.0.0 graph readiness:** published benchmark numbers on a Korean corpus; at least one graph algorithm beyond BFS shipped behind a feature gate; CI perf-regression gate active; real-world integration validated.
