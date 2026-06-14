# Roadmap

llm-kernel development roadmap from v0.3.2 to v1.0.0.

### 📑 Research & Feasibility Documents
* **[Roadmap Evaluation Report](docs/research/roadmap_evaluation.md)**
* **[FTS5 CJK Alternatives Study](docs/research/fts5_cjk_alternatives.md)**
* **[Future Milestones Feasibility Study](docs/research/future_roadmap_evaluation.md)**

> **Current phase: v0.8.0 complete ✅ — Next: v0.9.0 Search Integrations**

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

**Patch release criteria:** All existing tests pass, no API breakage, `cargo doc --all-features` warning-free after v0.3.4.

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

### v0.9.0 — Search Integrations

Elasticsearch and cross-engine search federation.

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | `elastic` feature — Elasticsearch `AsyncVectorIndex` implementation | L | `src/embedding/elastic.rs` (`elastic` feature) |
| 2 | Search federation — query multiple backends, merge results | M | `src/search/` |

**Exit criteria:** Elasticsearch passes VectorSearch conformance tests, federation merges Qdrant + Elasticsearch + TurboVec results.

---

### v1.0.0 — Production Readiness

API stability guarantee. Once shipped, all public types and signatures are locked under semver.

| # | Deliverable | Scope | Key Files |
|---|-------------|-------|-----------|
| 1 | Audit public API surface; reduce `pub` → `pub(crate)` where appropriate | L | All modules |
| 2 | Comprehensive doc comments with `# Example` sections on every public item | L | All modules |
| 3 | Performance baseline + CI regression detection (`--perf-baseline`) | M | `src/bin/eval.rs`, `benches/` |
| 4 | `cargo-semver-checks` in CI as blocking job | M | `.github/workflows/ci.yml` |
| 5 | Security audit + `SECURITY.md` | M | New `SECURITY.md`, `src/safety/`, `src/secrets/` |
| 6 | Document `full` feature set and platform compatibility matrix | S | `README.md` |

**Exit criteria:** `cargo-semver-checks` passes, every public item documented with examples, perf baselines in CI, security review complete, at least one external project integrated successfully.

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
  ├── v0.9.0  Search Integrations
  │            Elasticsearch, federation
  │
  └── v1.0.0  Production Readiness
               API audit, semver lock, perf baselines, security audit
```

Key dependency chains:
- `MessageRole` + `ContentPart` (v0.4.0) → all downstream type work
- `TokenBudget` (v0.4.0) → history management (v0.5.0) → document chunking (v0.6.0)
- `ToolDefinition` (v0.4.0) → `CapabilityProfile.supports_tool_calling()` (v0.3.4)
- `GraphBackend` trait (v0.7.0) → PostgreSQL impl (v0.8.0)
- `KvStore` trait (v0.7.0) → LLM cache (v0.7.0)
- `VectorSearch` trait → Qdrant (v0.8.0) → Elasticsearch (v0.9.0)

Within a phase, deliverables are independent and can be parallelized.

## Out of Scope

- **RAG pipeline** — application concern; compose with rig or langchain-rust
- **Agent framework / chains** — llm-kernel provides primitives; agents are built on top
- **PostHog / Sentry telemetry adapters** — belong in downstream crates
- **Python / WASM bindings** — FFI wrappers as a separate project
- **Streaming embedding** — no current use case
