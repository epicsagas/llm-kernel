# Graph Performance Maximization Strategy

> Post-v0.9.0 strategy for pushing the `graph` stack to production-grade performance, and for unblocking consumers (notably `secondbrain-agent`) that currently under-use it.
>
> **Status:** Strategy (2026-06-26) · **Phase context:** v0.9.0 complete ✅ → v1.0.0 Production Readiness
> **Related:** [Roadmap Evaluation](roadmap_evaluation.md) · [FTS5 CJK Alternatives](fts5_cjk_alternatives.md) · [Future Milestones](future_roadmap_evaluation.md)

## 1. Problem Framing

The graph stack is **feature-complete** but not yet **performance-characterized**. The four levers a consumer needs are already shipped:

| Lever | Feature gate | Shipped | Module |
|---|---|---|---|
| CJK lexical search | `graph-cjk` | v0.7.0 | `src/graph/cjk.rs` (app-side inverted index, supersedes `trigram`) |
| Vector ANN | `vector-index` | v0.3.5 | TurboVec (TurboQuant 2/4-bit SIMD) |
| Concurrency | `graph-pool` / `graph-async` | v0.8.0 | `src/graph/async_pool.rs` (WAL multi-connection) |
| Multi-backend | `graph-pg` / `qdrant` / `elastic` / `federation` | v0.8.0–v0.9.0 | `pg.rs`, `src/search/federation.rs` |

So "maximize graph performance" is **not** "add graph-cjk / ANN" (done) — it is: (a) quantify what we have, (b) close the remaining algorithmic/concurrency gaps vs. native graph engines, (c) make the win legible to consumers so they migrate off bespoke implementations.

## 2. The Consumer Gap (why this matters now)

`secondbrain-agent` declares `llm-kernel = "0.3"` but **imports nothing from it** (`grep llm_kernel src-tauri/src` → empty). It carries its own graph/search stack with four diagnosed bottlenecks — each is a concrete case where llm-kernel already wins on paper but must win **measurably**:

1. **Brute-force vector search** (`vector_search`, O(n) scan, self-admitted "~10K node ceiling") → must beat hand-rolled cosine with `vector-index` at scale.
2. **Single `Mutex<Connection>`** (serialized R/W despite WAL) → `graph-pool` must demonstrate real reader/writer concurrency.
3. **`split_whitespace` FTS** (Korean effectively unsearchable) → `graph-cjk` must show recall parity with alcove's ngram.
4. **Remote embeddings only** → local `embedding-fastembed`/Qwen3 must show cost/latency + Korean-quality wins.

These are the **acceptance tests** for the strategy below. A consumer should be able to migrate and see numbers move.

## 3. Maximization Axes

### Axis A — Benchmarks & baselines (gating)
No published numbers exist for the graph stack today. Before optimizing, establish:
- Recall@k / latency / p99 curves for FTS (`graph-cjk` vs `trigram`), ANN (`vector-index` vs brute-force), hybrid (`search` RRF/CombMNZ).
- Korean corpus specifically (the differentiator vs. generic embeddings).
- Cross-backend federation overhead (`qdrant` + `elastic` + TurboVec).
- The `benches/` harness already exists — wire graph benchmarks in as the v1.0.0 exit gate.

### Axis B — Scale ceiling
`graph-pool` (WAL pool) and `PgGraph` exist but aren't characterized past ~10K nodes. Produce a curve to 100K–1M nodes so consumers know when SQLite→PostgreSQL cutover (`migrate.rs`) pays off. This is the direct answer to secondbrain-agent's "~10K revisit ANN" comment.

### Axis C — Algorithmic depth (the gap vs. Neo4j)
Today: BFS traversal, smart recall, dedup. Native graph engines (Neo4j/GDS) offer PageRank, community detection, shortest-path at scale. For v1.0.0+, evaluate whether a small algorithm set (e.g., connected components, node similarity) belongs in `graph` behind a feature gate — this is the one axis where the embedded-Rust positioning can credibly challenge a server graph DB for agent-memory workloads.

### Axis D — Recall quality, not just speed
`graph-cjk` replaced `trigram` to kill false positives, but recall quality on agglomerated Korean (compound nouns, no spaces) needs an eval harness — reuse `eval`/`eval-full` features. This is the quality dimension the competitive comparison turns on.

### Axis E — Concurrency proof
`async_pool.rs` WAL pool needs a contention benchmark (concurrent importer writes + search reads) to prove the "serialized Mutex" failure mode is actually solved. secondbrain-agent's watcher-vs-search contention is the canonical test case.

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

**Exit criteria for v1.0.0 graph readiness:** published benchmark numbers on a Korean corpus; a consumer (secondbrain-agent) can migrate from bespoke search/vector to the llm-kernel graph stack and show measurable improvement on all four diagnosed bottlenecks.

## 6. Consumer Migration Note (out of scope here, tracked in consumer)

Migrating secondbrain-agent onto llm-kernel is its own project — the real cost is **schema reconciliation** (its `graph_nodes` / `graph_nodes_fts` / `graph_edges` vs llm-kernel `src/graph/schema.rs`). Tracked as ADR-007 / DEBT-008–011 in the secondbrain-agent repo. llm-kernel's job is to make the migration obviously worth it.
