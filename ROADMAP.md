# Roadmap

llm-kernel development roadmap from v0.3.2 to v1.0.0.

### 📑 Research & Feasibility Documents
* **[Roadmap Evaluation Report](docs/research/roadmap_evaluation.md)**
* **[FTS5 CJK Alternatives Study](docs/research/fts5_cjk_alternatives.md)**
* **[Future Milestones Feasibility Study](docs/research/future_roadmap_evaluation.md)**
* **[Graph Performance Maximization Strategy](docs/research/graph_performance_strategy.md)**

> **Current phase: v0.18.0 ✅ released → v0.19.0 staged (general directed-graph backend, PR #66) — Next: v1.0.0 (axis B + external integration: klr citation graph + alcove backlinks)**
>
> v1.0.0 prerequisites (issue #45): **#1 API audit ✅, #2 examples (primary surface) ✅, #3 perf baselines + CI gates ✅, #4 semver ✅, #5 security ✅, #6 feature/platform docs ✅**; axes **D ✅ measured, E ✅ measured + WAL fix, A ✅**. Remaining: axis B scale characterization (10K–1M), external integration (v1.0.0 exit criterion).

Each phase has a clear theme, concrete deliverables, and exit criteria.
The library's core philosophy — zero-mandatory-dep composability with feature gates — is preserved throughout.

---

## Patch Releases — v0.3.x

Non-breaking fixes, doc corrections, internal refactors, and additive utilities.
No public API changes. No new types that break existing signatures.

### v0.3.3 — Fixes & Cleanup

| # | Deliverable | Type | Key Files |
|---|-------------|------|-----------|
| 1 | Fix stale version `0.1.0` → `0.3.2` in README and 11 i18n translations | doc fix | `README.md`, `docs/i18n/*/README.md` |
| 2 | Fix Anthropic temperature silently dropped in serialization | bug fix | `src/llm/client.rs:401` |
| 3 | Remove non-existent PostHog/Sentry references from telemetry docstring | doc fix | `src/telemetry/mod.rs` |
| 4 | Deduplicate `text_preview` helper into `embedding/types.rs` | refactor | `src/embedding/fastembed.rs`, `openai.rs` |
| 5 | Deduplicate 429/error handling across OpenAI + Anthropic clients | refactor | `src/llm/client.rs` (4 locations) |
| 6 | Add macOS CI runner | CI | `.github/workflows/ci.yml` |

### v0.3.4 — Lint & Additive Utilities

| # | Deliverable | Type | Key Files |
|---|-------------|------|-----------|
| 1 | Enforce `#![deny(missing_docs)]` + fill missing doc comments | lint | `src/lib.rs`, all modules |
| 2 | Optimize `mask_secrets` from multi-pass to single-pass regex | perf | `src/safety/sanitize.rs` |
| 3 | Add `finish_reason`, `id`, `created` to `LLMResponse` (Optional fields) | additive | `src/llm/types.rs` |
| 4 | `normalize(&mut [f32])` vector normalization utility | additive | `src/embedding/types.rs` |
| 5 | `estimate_cost(model, prompt_tokens, completion_tokens)` utility | additive | `src/provider/catalog.rs` |
| 6 | `extract_xml_tag(text, tag)` utility for Claude-style output | additive | `src/llm/json_extract.rs` |
| 7 | Expand `CapabilityProfile` with default trait methods (tool_calling, vision, streaming, context_limit) | additive | `src/provider/capability.rs` |

**Patch release criteria:** All existing tests pass, no API breakage, `cargo doc --features full` warning-free after v0.3.4.

---

## Minor Releases — v0.4.0+

New types, traits, and features. May include breaking API changes within 0.x semver.

### v0.4.0 — Core Type Upgrades ✅

Strengthen the foundational types that every downstream consumer depends on.
This is the only phase with intentional breaking changes — do it once, lock it down.

**Shipped in PR [#34](https://github.com/epicsagas/llm-kernel/pull/34).**

| # | Deliverable | Scope | Breaking | Key Files |
|---|-------------|-------|----------|-----------|
| 1 | `MessageRole` enum replacing `String` role on `ChatMessage` | S | **Yes** | `src/llm/types.rs` |
| 2 | `ToolDefinition`, `ToolCall`, `ToolResult` — tool/function calling types | M | No | new `src/llm/tool.rs` |
| 3 | `ContentPart` enum — multimodal content (Text, ImageUrl, ImageBase64) | M | **Yes** | `src/llm/types.rs` |
| 4 | `ResponseFormat` enum (Text, Json, JsonSchema) + JSON mode support | S | No | `src/llm/types.rs`, `client.rs` |
| 5 | `TokenBudget` type (total, used, remaining, try_reserve, release) | S | No | new `src/tokens/budget.rs` |
| 6 | `LLMRequest` builder pattern (`.system().user_message().temperature().build()`) | S | No | `src/llm/types.rs` |

**Exit criteria:** Tool calling round-trips through `LLMClient`, multimodal messages serialize for OpenAI+Anthropic, `TokenBudget` enforces context limits, all v0.3.x tests still pass.

---

### v0.5.0 — Client Resilience & Completion ✅

Make the LLM client production-ready. Close gaps in developing modules.

**Shipped in PR [#35](https://github.com/epicsagas/llm-kernel/pull/35).**

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | `with_retry(client, max_retries, base_delay)` — exponential backoff wrapper | M | new `src/llm/retry.rs` |
| 2 | `LLMClientMiddleware` trait (on_request, on_response, on_error hooks) | S | `src/llm/client.rs` |
| 3 | `embed_batch` on `LazyFastembedProvider` (cache + batch merge) | M | `src/embedding/lazy.rs` |
| 4 | Batch chunking utility — split `embed_batch` by provider limits | S | `src/embedding/types.rs` |
| 5 | Conversation history management (truncate to token budget, role validation) | M | new `src/llm/history.rs` |
| 6 | Config schema validation with field-level errors | S | `src/config/loader.rs` |
| 7 | Expand install wizard (Windsurf, Aider, RooCode) | S | `src/install/wizard.rs` |

**Exit criteria:** Retry wrapper handles 429/500 automatically, middleware hooks fire on every request/response, `LazyFastembedProvider::embed_batch` performs true batching, history truncation respects `TokenBudget`.

---

### v0.6.0 — Search & Intelligence ✅

Unified search abstractions, safety enhancements, and content processing utilities.

**Shipped in PR [#37](https://github.com/epicsagas/llm-kernel/pull/37).**

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | `SearchProvider` trait — unified interface for BM25, vector, API search | M | `src/search/mod.rs` |
| 2 | Score normalization (min-max) + alternative fusion (weighted sum, CombMNZ) | M | `src/search/fusion.rs` |
| 3 | Prompt injection detection (`detect_injection → InjectionScore`) | M | `src/safety/injection.rs` |
| 4 | `DiscoverySource` trait + async discovery (`discovery-async` feature) | M | `src/discovery/mod.rs` |
| 5 | Document chunking (sentence-boundary, token-budget, overlap) | M | new `src/tokens/chunk.rs` |
| 6 | Prompt templates (variable substitution, few-shot examples) | M | new `src/llm/template.rs` |

**Exit criteria:** Multiple search backends composable via `SearchProvider`, injection detection eval integrated, document chunking handles CJK + Latin, prompt templates round-trip through serialization.

---

### v0.7.0 — Transport & Backend ✅

Remote MCP, CJK graph search, backend abstraction, and caching.

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | Application-side CJK N-gram index (`graph-cjk` feature gate) | L | `src/graph/schema.rs`, `src/tokens/tokenizer.rs` |
| 2 | MCP HTTP/SSE remote transport (`mcp-http` feature gate) | L | `src/mcp/transport.rs`, new `http.rs` |
| 3 | Async MCP handlers alongside existing sync handlers | L | `src/mcp/server.rs` |
| 4 | `GraphBackend` trait (internal refactor, SQLite impl) | L | `src/graph/*.rs` |
| 5 | Graph schema migration framework on `GraphBackend` | M | `src/graph/schema.rs` |
| 6 | `KvStore` trait + SQLite implementation | M | `src/store/`, new `kv.rs` |
| 7 | LLM response cache on `KvStore` (prompt → response) | M | `src/llm/client.rs`, new `cache.rs` |
| 8 | Unpin `ort` from `=2.0.0-rc.12` when stable releases | S | `Cargo.toml` |

**Why application-side CJK index instead of SQLite FTS5 extension:** Integrating a custom C-FFI FTS5 tokenizer in Rust introduces major compile-time complexity (linker issues, platform compatibility). By implementing N-gram tokenization in safe Rust and storing the postings index in standard relational tables, we achieve 100% database portability (enabling PostgreSQL migration in v0.8.0) and zero native compile dependencies.

**Why trait before migration:** Migration runs SQL against a backend. Building on `GraphBackend` means the same migration logic works for every backend.

**Why KV trait before LLM cache:** The LLM response cache is a specialized use of a generic `KvStore`. The same trait serves embedding caches, session state, and rate-limit counters.

**Exit criteria:** CJK content searchable using application-side index, MCP over HTTP, `GraphBackend` trait with SQLite impl, migrations work via trait, `KvStore` powers LLM cache, `ort` unpinned.

---

### v0.8.0 — Backend Expansion ✅

Multi-DBMS and vector search backends.

**Shipped as the `graph-pg` and `qdrant` feature gates (single crate, consistent with `embedding-fastembed`/`mcp-http`).** Both backends are live-verified (PostgreSQL conformance + SQLite↔PostgreSQL migration round-trip; Qdrant add/search/filter/remove); the env-gated live tests skip in CI without services.

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | `graph-pg` — PostgreSQL `GraphBackend` (`PgGraph`) | L | `src/graph/pg.rs` (`graph-pg` feature) |
| 2 | `qdrant` — Qdrant `AsyncVectorIndex` (`QdrantVectorIndex`) | L | `src/embedding/qdrant.rs` (`qdrant` feature) |
| 3 | DBMS-to-DBMS migration CLI (SQLite ↔ PostgreSQL) | M | `src/bin/migrate.rs` (`graph-pg` feature) |

**Architecture:**

```
llm-kernel (single crate, feature-gated)
  ├── trait GraphBackend   → SQLite (built-in) / PostgreSQL (graph-pg)
  ├── trait VectorIndex    → TurboVec (vector-index, built-in)
  ├── trait AsyncVectorIndex → Qdrant (qdrant)
  ├── trait KvStore        → SQLite (built-in)
  ├── trait SearchProvider → RRF (built-in)
```

Each backend is an optional feature — drivers (`postgres`, `qdrant-client`) are only compiled when the feature is enabled, so the default build is unchanged.

**Exit criteria:** PostgreSQL passes same graph test suite as SQLite, Qdrant passes VectorSearch conformance tests, migration CLI round-trips without data loss.

---

### v0.9.0 — Search Integrations ✅

Elasticsearch and cross-engine search federation.

**Shipped as the `elastic` feature gate (`ElasticsearchVectorIndex`, a hand-rolled reqwest client — the official `elasticsearch` crate is alpha-only) plus `FederatedSearch` in `src/search/federation.rs`.** Federation defaults to rank-based RRF so heterogeneous raw scores across Qdrant / Elasticsearch / TurboVec merge correctly with no normalization; a per-backend timeout drops slow or failing backends observably rather than blocking the query.

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | `elastic` feature — Elasticsearch `AsyncVectorIndex` implementation | L | `src/embedding/elastic.rs` (`elastic` feature) |
| 2 | Search federation — query multiple backends, merge results | M | `src/search/federation.rs` (`search` feature) |

**Exit criteria:** Elasticsearch passes VectorSearch conformance tests, federation merges Qdrant + Elasticsearch + TurboVec results.

---

### v0.10.0 — Graph Algorithms ✅

Pure-Rust, zero-dependency graph algorithms closing the Neo4j/GDS gap, compiled in behind the existing `graph` feature (no `Cargo.toml` change, no `petgraph`).

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | `CsrGraph` snapshot + weighted PageRank | M | `src/graph/algo/pagerank.rs` |
| 2 | Connected components + label propagation | M | `src/graph/algo/community.rs` |
| 3 | Dijkstra weighted shortest path | S | `src/graph/algo/path.rs` |
| 4 | Jaccard / common-neighbors / Adamic-Adar / link prediction | S | `src/graph/algo/similarity.rs` |
| 5 | `smart_recall` graph boost ranks by true PageRank centrality (SQLite + PostgreSQL share one impl) | M | `src/graph/recall.rs`, `src/graph/pg.rs` |

**Exit criteria:** algorithms re-exported from `graph` as backend-agnostic free functions, PageRank eval scenario + criterion benchmarks in place, zero backend drift.

---

### v0.11.0 — PostgreSQL TLS ✅

Optional `graph-pg-tls` feature adding TLS to `PgGraph` connections
(`connect_native_tls` / `connect_tls` / `connect_config_tls`), closing #48.
Existing `NoTls` constructors are unchanged.

---

### v0.12.0 — Embedding Robustness ✅

`ModelState::Failed(String)` → `Failed { message, panicked }`; dropped the
default `ort-load-dynamic` so `embedding-fastembed` statically links ONNX
Runtime, made the model-load path panic-safe via `catch_unwind` +
`LazyFastembedProvider::reset()`, and added the opt-in
`embedding-fastembed-dynamic-linking` feature (#50).

---

### v0.13.0 — Consistency & Protocol Compliance ✅

Unify the public error surface and bring the LLM client and MCP server up to spec.

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | Unify `embedding` + `discovery` public APIs from `anyhow::Result` to `KernelError` (`Embedding` / `Discovery` variants) | L | `src/error.rs`, `src/embedding/*`, `src/discovery/*` |
| 2 | Forward `LLMRequest::tools` + `response_format` to OpenAI/Anthropic; parse tool calls into `LLMResponse::tool_calls` | M | `src/llm/client.rs`, `src/llm/types.rs` |
| 3 | MCP server: protocol 2025-06-18 negotiation, `ping`, prompts, string/number ids, `tools/call` `isError`, camelCase wire format | M | `src/mcp/*` |
| 4 | Fix `LazyFastembedProvider::embed_batch` panic on truncated provider response; offload blocking cache I/O via `spawn_blocking` | S | `src/embedding/lazy.rs`, `src/llm/cache.rs` |
| 5 | Isolated per-feature CI checks | S | `.github/workflows/ci.yml` |

**Exit criteria:** no `anyhow` in the public library surface, MCP dispatch conforms to the spec, all features build in isolation.

---

### v0.14.0 — Forward Compatibility ✅

Stop the per-minor breakage caused by adding fields/variants to public types. Several changes are breaking (see migration notes); this is the structural groundwork that lets the library add fields in any future minor without forcing downstream rewrites.

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | `Default` derived on every growable public data struct (provider, graph, mcp result types) | M | `src/provider/*`, `src/graph/*`, `src/mcp/*` |
| 2 | `KernelError` marked `#[non_exhaustive]` — new variants may arrive in any minor | S | `src/error.rs` |
| 3 | Read-mostly catalog/result types `#[non_exhaustive]` (`ServiceDescriptor`, `ModelDescriptor`, `GraphStats`, …) | M | `src/provider/*`, `src/graph/*` |
| 4 | `KernelError::Serialization` available under any feature pulling `serde_json` (not just `provider`) | S | `src/error.rs` |
| 5 | `OpenAIClient::from_key` / `AnthropicClient::from_key` now return `Result<Self>` (no silent timeout-less fallback) | S | `src/llm/client.rs` |

**Exit criteria:** downstream can future-proof with `..Default::default()`, exhaustive `match`es on `KernelError` carry a `_ =>` arm, `from_key` call sites append `?`.

---

### v0.15.0 — Embedding Robustness (Dynamic Linking Escape Hatch) ✅

Fix the `embedding-fastembed-dynamic-linking` escape hatch that never actually worked: the dynamic feature was a superset of the static one, so Cargo feature unification silently activated both `ort-load-dynamic` and `ort-download-binaries-*` on the shared `fastembed`/`ort-sys` crate, turning the static path into a no-op (#50 failure mode).

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | Make `embedding-fastembed` and `embedding-fastembed-dynamic-linking` mutually exclusive via `compile_error!` | S | `src/lib.rs` |
| 2 | `fastembed`'s ort features selected by the consuming feature (static archive vs runtime dylib) | M | `Cargo.toml` |
| 3 | Gate `FastembedProvider`/`LazyFastembedProvider`/`EmbeddingCache`/`is_model_cached`/`as_fastembed` under both features | S | `src/embedding/*` |

**Exit criteria:** the dynamic escape hatch exposes the same API as the static path; any feature conflict is a hard build error.

---

### v0.16.0 — Vector Backend Expansion & Routing ✅

Third async remote vector backend, cost-aware client routing, and an MSRV/build-stability dep downgrade.

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | `pgvector` `AsyncVectorIndex` (`PgVectorIndex`) — PostgreSQL + pgvector extension (cosine `<=>`, HNSW index) (#59) | L | `src/embedding/pgvector.rs` (`pgvector` feature) |
| 2 | `RouterClient` — cost-aware routing (`Fallback` / `LowestCost`) with cross-provider fallback; error-class aware (transient 5xx/429/408 moves on, permanent 4xx short-circuits) (#60) | M | `src/llm/router.rs` |
| 3 | `rusqlite` 0.40 → 0.37 (MSRV/build stability; drops `rsqlite-vfs` transitive dep) (#61) | S | `Cargo.toml` |

**Exit criteria:** pgvector passes VectorSearch conformance, `RouterClient` composes with `RetryClient`/`MiddlewareClient`/`CacheClient`, default build unchanged.

---

### v0.16.1 — pgvector Bind Fix ✅

Patch: `pgvector::Vector` sqlx `Type` bind conflict (surfaced in the `klr` environment) — bind the vector as a string literal (`[1,2,3]::vector`) instead of a typed `Vector` to sidestep the sqlx `Type` mismatch.

---

### v0.16.2 — CoreML Execution Provider ✅

`embedding-fastembed-coreml` feature + `new_with_coreml()` constructor (mirrors the DirectML pattern). Adds the `coreml` execution-provider feature to `ort`, accelerating `bge-m3` on macOS GPU/ANE. The static `embedding-fastembed` build now links CoreML alongside the default ONNX Runtime.

---

### v0.17.0 — pgvector Transaction Integration ✅

Make the Rust `add()` path actually insert and enable transactional integration.

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | `add()` switched from `push_values` to manual `VALUES` assembly with the `::vector` cast (was missing → type mismatch) | S | `src/embedding/pgvector.rs` |
| 2 | `pool()` getter + `remove_in_tx(&mut PgConnection, ids)` for single-transaction atomicity | M | `src/embedding/pgvector.rs` |

**Exit criteria:** Rust `add()` inserts correctly (previously a Python `COPY` bypass in `klr` masked the bug); `klr` prune runs in a single atomic transaction.

---

### v0.18.0 — v1.0.0 Readiness Gates ✅

Measured perf/quality gates, API audit, security review, and docs — the bulk of the v1.0.0 prerequisites shipped as an incubating minor release. The API audit's `pub` → `pub(crate)` reductions + dead-code removal are intentional **breaking** changes (permitted under 0.x with a minor bump, enforced by the new semver gate). Tracked in PR [#64](https://github.com/epicsagas/llm-kernel/pull/64); issue [#45](https://github.com/epicsagas/llm-kernel/issues/45).

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| E | `AsyncPoolGraph::open` enables WAL + per-connection `busy_timeout`/`synchronous=NORMAL` — the pool's "concurrent reads during writes" claim now holds (1.8× faster 16-reader wave under writer) | M | `src/graph/async_pool.rs` |
| D | `graph-korean` eval: `graph-cjk` vs FTS5 `trigram` recall (1.000 vs 0.286 @k=5) + `--strict` CI gate mode | M | `src/bin/eval.rs`, `eval/datasets/` |
| A | `bench-smoke` CI job (caught a real UTF-8 panic) + `semver.yml` (`cargo-semver-checks`; breaking requires minor bump) | M | `.github/workflows/` |
| 1 | API audit: 8 internal `pub` → `pub(crate)`, dead `importance_for_type` removed (**breaking**) | L | all modules |
| 2 | `# Example` doctests on primary entry surface (`estimate_tokens`, `mask_secrets`, `LLMRequest::builder`, `OpenAIClient::from_key`) | M | `src/tokens`, `src/safety`, `src/llm` |
| 5 | Security M2: HTTP error bodies routed through `mask_secrets` before `KernelError::Http` (prevents API-key leak via proxy-echoed error bodies) | S | `src/llm/client.rs`, `docs/security-audit-2026-07.md` |
| 6 | `docs/features.md` feature catalog + platform matrix; measured baselines in `docs/benchmarks/` | S | `docs/` |

**Exit criteria:** gates #1/#2/#3/#5/#6 closed; axis A/D/E measured. Remaining for v1.0.0: axis B (scale 10K–1M) + external integration.

---

### v0.19.0 — General Directed-Graph Backend ✅ (staged, PR #66)

Extends `GraphBackend` from an AI-memory-recall layer into a **general directed-graph backend** — the foundation for the v1.0.0 "real-world integration" exit criterion (klr citation graph + alcove backlinks). Four new trait methods ship with **default implementations**, so adding them is non-breaking for external implementors (pre-1.0 surface freeze).

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | `append_edges` — batch edge upsert (single transaction + prepared-statement reuse; scales to hundreds of thousands of edges) | M | `src/graph/store.rs`, `src/graph/pg.rs` |
| 2 | `EdgeDirection` + `edges_for_node_dir` / `neighbors_weighted` — directional (Out/In/Both) + relation-filtered lookups | M | `src/graph/types.rs`, `src/graph/traversal.rs`, `src/graph/backend.rs` |
| 3 | `related_nodes_filtered` — BFS with direction + relation filter | S | `src/graph/backend.rs` |
| 4 | Schema v3 — `idx_edges_src_rel` / `idx_edges_tgt_rel` composite indexes for relation-filtered queries | S | `src/graph/schema.rs`, `src/graph/pg.rs` |
| 5 | `PgGraph::from_client` public — external synchronous `postgres::Client` injection | S | `src/graph/pg.rs` |
| 6 | `PgGraph` optional table prefix (`connect_with_prefix`) — default `""` keeps per-service-DB behavior; a prefix lets multiple graphs coexist in one DB | M | `src/graph/pg.rs` |

**Exit criteria:** trait surface frozen pre-1.0; `SqliteGraph`/`PgGraph`/`AsyncGraph`/`AsyncPoolGraph` all covered; CI green (`semver-checks` confirms non-breaking). External integration lands in stages 2/3. The planned async `SqlxPgGraph` backend (for klr's `sqlx::PgPool`) is designed but unimplemented.

---

### v1.0.0 — Production Readiness

API stability guarantee. Once shipped, all public types and signatures are locked under semver.

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | Audit public API surface; reduce `pub` → `pub(crate)` where appropriate — **done (8 items → `pub(crate)`, dead code removed)** | L | All modules |
| 2 | `# Example` sections on every public item (`#![deny(missing_docs)]` already enforced since v0.3.4) — **primary entry surface done; long tail deferred** | L | All modules |
| 3 | Performance baseline + CI regression detection (`--perf-baseline`) — **`--strict` eval gate + `bench-smoke` done; baselines in `docs/benchmarks/` (graph/compute/korean/concurrency)** | M | `src/bin/eval.rs`, `benches/`, `docs/benchmarks/` |
| 4 | `cargo-semver-checks` in CI as blocking job — **done (`.github/workflows/semver.yml`)** | M | `.github/workflows/semver.yml` |
| 5 | Security audit (`SECURITY.md` already published; `cargo audit` + gitleaks already in CI) — **done (`docs/security-audit-2026-07.md`)** | M | `src/safety/`, `src/secrets/` |
| 6 | Document `full` feature set and platform compatibility matrix — **done (`docs/features.md`)** | S | `docs/features.md` |

**Exit criteria:** `cargo-semver-checks` passes, every public item documented with examples, perf baselines in CI, security review complete, at least one external project integrated successfully — **external integration staged**: (1) general directed-graph backend ✅ v0.19.0, (2) klr citation-graph integration, (3) alcove backlink integration.

---

## Dependency Graph

```
v0.3.2
  │
  ├── v0.3.3  Patch: Fixes & Cleanup
  │
  ├── v0.3.4  Patch: Lint & Additive Utilities
  │
  ├── v0.4.0  Core Type Upgrades ✅       ← only breaking-change release
  │            MessageRole, Tool types, ContentPart, TokenBudget
  │
  ├── v0.5.0  Client Resilience ✅
  │            Retry, Middleware, embed_batch, history management
  │
  ├── v0.6.0  Search & Intelligence ✅
  │            SearchProvider, injection detection, chunking, templates
  │
  ├── v0.7.0  Transport & Backend
  │            CJK, MCP HTTP, GraphBackend trait, KvStore, LLM cache
  │
  ├── v0.8.0  Backend Expansion ✅
  │            PostgreSQL, Qdrant, DBMS migration
  │
  ├── v0.9.0  Search Integrations ✅
  │            Elasticsearch, federation
  │
  ├── v0.10.0 Graph Algorithms ✅
  │            CSR PageRank, community, Dijkstra, similarity
  │
  ├── v0.11.0 PostgreSQL TLS ✅
  │            graph-pg-tls
  │
  ├── v0.12.0 Embedding Robustness ✅
  │            static ONNX linking, panic-safe load
  │
  ├── v0.13.0 Consistency & Protocol Compliance ✅
  │            KernelError unification, tool forwarding, MCP 2025-06-18
  │
  ├── v0.14.0 Forward Compatibility ✅
  │            non_exhaustive, Default derive, from_key → Result
  │
  ├── v0.15.0 Embedding Robustness (dynamic-linking escape hatch) ✅
  │            mutually-exclusive fastembed features, compile_error! guard
  │
  ├── v0.16.0 Vector Backend Expansion & Routing ✅
  │            pgvector, RouterClient, rusqlite 0.37
  │
  ├── v0.16.1 pgvector Bind Fix ✅
  │            Vector bind → string-literal ::vector cast
  │
  ├── v0.16.2 CoreML Execution Provider ✅
  │            embedding-fastembed-coreml, macOS GPU/ANE bge-m3
  │
  ├── v0.17.0 pgvector Transaction Integration ✅
  │            add() cast fix, pool() + remove_in_tx
  │
  ├── v0.18.0 v1.0.0 Readiness Gates ✅
  │            WAL pool, graph-korean eval, --strict gate, semver, API audit, security M2
  │
  ├── v0.19.0 General Directed-Graph Backend ✅
  │            append_edges, EdgeDirection, relation-filtered lookups, schema v3, from_client pub
  │
  └── v1.0.0  Production Readiness
               API audit, semver lock, perf baselines, security audit, external integration
```

Key dependency chains:
- `MessageRole` + `ContentPart` (v0.4.0) → all downstream type work
- `TokenBudget` (v0.4.0) → history management (v0.5.0) → document chunking (v0.6.0)
- `ToolDefinition` (v0.4.0) → `CapabilityProfile.supports_tool_calling()` (v0.3.4)
- `GraphBackend` trait (v0.7.0) → PostgreSQL impl (v0.8.0)
- `KvStore` trait (v0.7.0) → LLM cache (v0.7.0)
- `AsyncVectorIndex` trait → Qdrant (v0.8.0) → Elasticsearch (v0.9.0) → pgvector (v0.16.0)

Within a phase, deliverables are independent and can be parallelized.

## Out of Scope

- **RAG pipeline** — application concern; compose with rig or langchain-rust
- **Agent framework / chains** — llm-kernel provides primitives; agents are built on top
- **PostHog / Sentry telemetry adapters** — belong in downstream crates
- **Python / WASM bindings** — FFI wrappers as a separate project
- **Streaming embedding** — no current use case
