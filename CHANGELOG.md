# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.1] - 2026-06-16

### Added

- **provider** (`catalog-sync` feature): `llm-kernel-sync-catalog` binary — refreshes `catalog.json` from the live models.dev catalog. `--check` reports drift without writing; the default writes atomically. Drives field-precedence merge: provider service fields (auth, base URL, tiers, setup) are kept from the catalog, model data (cost, limits, modalities, capabilities) comes from models.dev, and empty `api_base_url`/`npm_package`/`doc_url` are filled from upstream. New `src/provider/sync.rs` (`merge_catalog`, `CatalogDiff`, `PriceDelta`) + `src/bin/sync-catalog.rs`.
- **provider**: `provider::mapping` — `Mapping` enum + `resolve()` mapping each catalog provider id to its models.dev counterpart (8 exact, 7 aliased, 5 manual). New `src/provider/mapping.rs`.
- **provider**: `ProviderIndex::from_providers(Vec<ServiceDescriptor>)` public constructor and `ProviderIndex::with_discovered(&[ModelEntry])` (gated on `discovery`) — overlays runtime-discovered models onto the embedded catalog so `find_model`/`estimate_cost` see them. Resolves the catalog↔discovery gap.
- **provider**: catalog value types (`ModelCost`, `ModelLimit`, `ModelModalities`, `ModelCapabilities`, `ModelDescriptor`, `ServiceDescriptor`, `ModelChoice`) now derive `Serialize` and `PartialEq`.
- **discovery**: `fetch()` / `fetch_from(url)` no-cache fetch helpers; `ModelsDevPayload::entries()`, `provider_models(key)`, `provider_api_base`/`provider_npm`/`provider_doc` accessors.
- **discovery**: `ModelEntry` enriched with optional `cost`, `modalities`, `capabilities`, `family`, `release_date`, `knowledge` (mirroring `ModelDescriptor`) and `Default`; `From<ModelEntry> for ModelDescriptor`.

### Changed

- **discovery** (*breaking*): `ModelsDevPayload` now mirrors the real models.dev API — a provider-keyed map (`HashMap<provider_id, provider>`) — instead of the previous `{ models: Vec<ModelEntry> }` shape, which never parsed the live `https://models.dev/api.json`. The on-disk cache written by `fetch_and_cache` is now byte-identical to upstream.
- **catalog**: `catalog.json` refreshed from models.dev — 20 providers, 351 models (was ~57). Pricing/limits/modalities/capabilities now track models.dev (e.g. `glm-5` input 0.5→1.0, output 0.5→3.2). `glm-5` and `ZAI_API_KEY` preserved (catalog-wins for connection fields). Provider-doc comment corrected (16→20).
- **docs**: README "Model discovery" example updated for the new payload shape; new "Keeping the catalog fresh" section documents the runtime `with_discovered` path (always-current) versus the `sync-catalog` tool (offline baseline at release time).

### Notes

- The embedded catalog is frozen at compile time (`include_str!`), so the `sync-catalog` tool refreshes the **offline baseline** that ships with each crate release. For always-current data at runtime, fetch models.dev via `discovery` and merge with `ProviderIndex::with_discovered` — the library provides the fetch + merge; the application drives timing/caching.

## [0.9.0] - 2026-06-15

### Added

- **embedding** (`elastic` feature): `ElasticsearchVectorIndex` — `AsyncVectorIndex` over Elasticsearch 8.x (dense_vector cosine mapping, bulk upsert/delete, knn `_search`, `_count`), implemented with a **hand-rolled reqwest client** rather than the official `elasticsearch` crate (which is alpha-only — no stable release) so the dependency stays safe ahead of the v1.0.0 semver lock (new `src/embedding/elastic.rs`)
- **federation** (`federation` feature): `FederatedSearch` — concurrent cross-engine federation over multiple `AsyncVectorIndex` backends with a per-backend timeout, observable failure handling, and rank-based RRF fusion as the default (new `src/search/federation.rs`). The feature composes `search` + `embedding` and owns the `tokio` + `futures-util` deps so search-only and single-backend users compile no federation runtime.
- **search**: `FusionStrategy` enum + pure `federate_results` merge so a synchronous `TurbovecIndex` can participate in federation alongside the async backends

### Changed

- **search**: the pure fusion functions (`rrf_fuse`, `normalize_minmax`, `weighted_sum_fuse`, `combmnz_fuse`) are unchanged; the `search` feature remains light (serde_json only). Async cross-engine federation moved to a dedicated `federation` feature gate that owns `tokio` (+ `time`) and `futures-util`.
- **features**: new `elastic` feature gate — the reqwest driver is reused from `client-async` (no new transitive deps); `elastic` is included in `full`. Single crate, single publish. Main crate version 0.8.0 → 0.9.0.
- **infra**: `docker-compose.yml` gained an Elasticsearch service for the live integration test (local-dev only; CI self-skips)
- **elastic** (hardening, pre-v1.0.0 stabilization): the reqwest client now sets a 5 s connect timeout + 30 s request timeout so direct (non-federated) callers cannot hang on an unresponsive node; `redact_credentials` now redacts userinfo up to the **last** `@` in the authority (a password containing `@` no longer leaks its tail); bulk upsert/delete errors surface the first failing item's redacted JSON; index names are validated against the ES 8.x rules (lowercase, `[a-z0-9_.-]`, no leading `_`/`-`/`+`, ≤255 bytes) before any network call; `_count` no longer sends a no-op `track_total`; `FederatedSearch` collects per-backend weights only under `WeightedSum`.
- **elastic** (review hardening): the knn `num_candidates` is now computed by a shared `knn_num_candidates(k)` helper that caps candidates at `MAX_KNN_CANDIDATES = 1_000` (so a large `k` cannot ask ES to score thousands of candidates) while preserving the ES invariant `num_candidates >= k`; error response bodies embedded in `anyhow` errors are capped to `ERROR_BODY_MAX_CHARS = 1024` characters at a UTF-8 boundary (with a `... [truncated]` marker) so a verbose ES error cannot bloat logs, applied after `redact_credentials` so a credential past the cap stays masked; the `SearchHit.score` semantics (`(1 + cosine) / 2`, not comparable across backends) and the WeightedSum caveat are now documented in the module and `search` method docs.
- **federation** (review hardening): `FederatedSearch::search` now over-fetches each backend (`fetch_k = 2 * k`) before RRF/WeightedSum fusion and truncates the merged list to the requested `k`, so a document ranking just below `k` in one backend but near the top in another keeps its cross-backend rank-credit instead of being silently dropped.

### Notes

- Federation defaults to **RRF** (rank-based, scale-invariant) so heterogeneous raw scores across backends — Qdrant cosine `[0,1]`, Elasticsearch `_score = (1+cos)/2 ∈ [0,1]`, TurboVec raw cosine `[-1,1]` — fuse correctly with no normalization. `FusionStrategy::WeightedSum` is opt-in and applies per-list min-max normalization first.
- Elasticsearch connection-string credentials (`https://user:pass@host`) are used for the request but **never** leaked in errors — all error messages route through `redact_credentials`, which strips userinfo up to the last `@` in the authority (handles passwords that themselves contain `@`).
- The live Elasticsearch conformance test mirrors the Qdrant conformance body and self-skips without `LLMKERNEL_ELASTIC_URL`; it deletes its throwaway index on every exit path.

## [0.8.0] - 2026-06-14

### Added

- **graph** (`graph-pg` feature): `PgGraph` — a PostgreSQL `GraphBackend` over the synchronous `postgres` driver (ILIKE substring search, no extension required; identical `smart_recall` scoring; recursive-CTE BFS traversal; schema versioning via the trait)
- **graph** (`graph-pg`): `llm-kernel-migrate-graph` binary — a SQLite↔PostgreSQL migration CLI with a `--dry-run` planning mode
- **embedding** (`qdrant` feature): `QdrantVectorIndex` — `AsyncVectorIndex` over `qdrant-client` (upsert / remove / search / filtered search / count via the universal Query API)
- **embedding**: `AsyncVectorIndex` trait — the async, object-safe counterpart to `VectorIndex` for remote/shared backends whose clients are async-only (new `src/embedding/async_vector_index.rs`)
- **infra**: `docker-compose.yml` for opt-in local PostgreSQL + Qdrant to run the live integration tests (works with `docker compose` or `podman compose`)

### Changed

- **features**: new `graph-pg` and `qdrant` feature gates — drivers are optional and not in `default`; both are included in `full`. Single crate, single publish (no separate workspace crates). Main crate version 0.7.0 → 0.8.0.
- **embedding**: the `embedding` feature now pulls `async-trait` (for the `AsyncVectorIndex` trait); the existing synchronous `VectorIndex` is unchanged
- **ci**: `graph-pg` and `qdrant` added to the test matrix (live integration tests self-skip without `LLMKERNEL_PG_URL` / `LLMKERNEL_QDRANT_URL`, so CI without services stays green)
- **graph**: `compute_recency` is now `pub` so the PostgreSQL backend reuses the exact recency math — no scoring drift across backends

### Notes

- Both new backends are live-verified: `PgGraph` passes the full `GraphBackend` conformance and a SQLite→PostgreSQL migration round-trip; `QdrantVectorIndex` passes add / search / filter / remove against a live Qdrant. These live tests are env-gated and skip in CI.
- Driver dependencies (`postgres`, `qdrant-client`) are optional and only compiled when `graph-pg` / `qdrant` are enabled — the default (and `provider`-only) build is unchanged.

## [0.7.0] - 2026-06-14

### Added

- **graph**: `GraphBackend` trait — sync, object-safe, backend-agnostic interface for graph storage with **no `rusqlite` types in its surface**, ready for non-SQLite backends; includes the composite `smart_recall` and `related_nodes` operations (new `src/graph/backend.rs`)
- **graph**: `SqliteGraph` — bundled `GraphBackend` implementation wrapping the existing graph free-function API behind a mutex-guarded connection
- **graph**: schema migration framework expressed through `GraphBackend` (`current_version`, `migrate`) — version-to-version steps with transactional rollback; graph schema bumped to v2 (new `idx_nodes_created` index)
- **graph**: CJK-aware search via contiguous substring matching (`segment_cjk` utility + `search_nodes_cjk`) behind the new `graph-cjk` feature — **no FTS5 schema change**, so the feature toggles safely on any existing database (new `src/graph/cjk.rs`)
- **store**: `KvStore` trait (sync, object-safe) + `SqliteKvStore` implementation (new `src/store/kv.rs`)
- **llm**: `CacheClient` — response-cache wrapper for any `LLMClient`, backed by `KvStore`; client-namespaced key (no cross-provider collision on a shared store), optional TTL (`with_ttl`), `complete` cached, `stream_complete` pass-through (new `src/llm/cache.rs`, new `cache` feature)
- **mcp**: async tool handlers (`AsyncToolHandler`, `set_async_handler`, `call_tool_async`) alongside the existing synchronous handlers
- **mcp**: HTTP/SSE remote transport (`HttpTransport`, `serve`) behind the new `mcp-http` feature — JSON-RPC over `POST /mcp` (incl. `resources/read`) and SSE streaming via `POST /mcp/sse`, reusing the server's Bearer auth (new `src/mcp/http.rs`)

### Changed

- **graph**: schema version bumped 1 → 2; `init_graph_schema` is backward compatible and `SqliteGraph::open` migrates older databases transparently
- **features**: new `cache`, `graph-cjk`, and `mcp-http` feature gates; `mcp` now pulls `async-trait`; all three are included in the `full` feature set
- **deps**: `ort` remains pinned to `=2.0.0-rc.12` (no 2.0.0 stable yet); the pin now carries an explicit lockstep-with-fastembed comment
- **deps**: dev-dependency `tokio` for async tests

### Notes

- The existing sync graph free-function API (`upsert_node(&conn, …)`, `search_nodes(&conn, …)`, …) is unchanged. `GraphBackend` / `SqliteGraph` are additive and may be used alongside it.
- The LLM cache is a dedicated `LLMClient` wrapper rather than an `LLMClientMiddleware`, because the middleware trait is observe-only by design and cannot short-circuit a request with a cached response.

## [0.6.0] - 2026-06-13

### Added

- **search**: `SearchProvider` trait — unified sync interface for ranking backends; `KeywordIndex` term-frequency reference implementation (new `src/search/provider.rs`)
- **search**: `normalize_minmax`, `weighted_sum_fuse`, `combmnz_fuse` — min-max score normalization and alternative fusion strategies complementing existing RRF (new `src/search/fusion.rs`)
- **safety**: `detect_injection(text) → InjectionScore` — weighted regex rules over instruction-override, role-hijack, delimiter-escape, jailbreak, and payload-drop signals; aggregate score saturated to `[0.0, 1.0]` (new `src/safety/injection.rs`)
- **discovery**: async `DiscoverySource` trait + `ModelsDevSource` reqwest implementation behind the new `discovery-async` feature (new `src/discovery/source.rs`)
- **tokens**: `chunk_text(text, opts)` — sentence-boundary, token-budgeted chunking with overlap and CJK + Latin terminator awareness; `ChunkOptions` builder (new `src/tokens/chunk.rs`)
- **llm**: `PromptTemplate` — `{{variable}}` substitution, few-shot example support, and serde round-trip; reuses `render_prompt` (new `src/llm/template.rs`)
- **eval**: `injection` subcommand — measures detection accuracy, recall, and specificity over benign and injection corpora

### Changed

- **errors**: `KernelError` gains a `Search(String)` variant for search-backend failures
- **features**: new `discovery-async` feature gate (adds `discovery`, `reqwest`, `async-trait`, `tokio`); included in the `full` feature set
- **search**, **safety**, **tokens**, **llm**: new public items re-exported from their module roots

## [0.5.0] - 2026-06-13

### Added

- **llm**: `RetryClient` and `RetryConfig` — exponential backoff wrapper around any `LLMClient`, auto-retries 429 and 5xx with jitter (new `src/llm/retry.rs`)
- **llm**: `LLMClientMiddleware` trait with `on_request`/`on_response`/`on_error` async hooks and composable `MiddlewareClient` wrapper (new `src/llm/middleware.rs`)
- **llm**: `ConversationHistory` — ordered message list with role-alternation validation and token-budget-aware truncation that preserves the system message (new `src/llm/history.rs`, `tokens` feature)
- **embedding**: `chunk_batch` utility — splits a batch into provider-limit-sized chunks
- **embedding**: `LazyFastembedProvider::embed_batch` override — LRU cache lookup + batch merge of misses for true batching
- **config**: `FieldError` struct and `validate_config` — structured field-level TOML validation errors (path/expected/value) instead of raw serde strings
- **install**: `AgentKind` expanded with `Windsurf` and `RooCode` variants

### Changed

- **embedding**: `chunk_batch` and `validate_config` re-exported from their module roots
- **llm**: non-success HTTP responses now surface as `KernelError::Http { status, message }` instead of an `LlmApi` string; `RetryClient` retries on the structured 5xx status

### Fixed

- **llm**: `ConversationHistory::truncate_to_budget` now actually removes messages in place (was `&self`, left history untouched); signature is `&mut self`
- **llm**: `ConversationHistory::push` allows consecutive `Tool` messages (parallel tool results)
- **llm**: `RetryClient` jitter mixes `SystemTime` entropy so concurrent retriers desynchronize (real thundering-herd avoidance, no RNG dependency)
- **install**: removed the `Aider` variant — its config path wrote `mcpServers` JSON to `.aider.conf.yml`, which Aider does not consume

## [0.4.0] - 2026-06-12

### Added

- **llm**: `MessageRole` enum replacing stringly-typed role on `ChatMessage`
- **llm**: `ToolDefinition`, `ToolCall`, `ToolResult` — tool/function calling types (new `src/llm/tool.rs`)
- **llm**: `ContentPart` enum — multimodal content (Text, ImageUrl, ImageBase64)
- **llm**: `ResponseFormat` enum (Text, Json, JsonSchema) + JSON mode support
- **llm**: `LLMRequest` builder pattern (`.system().user_message().temperature().build()`) and `tools` field
- **tokens**: `TokenBudget` type (total, used, remaining, `try_reserve`, `release`) (new `src/tokens/budget.rs`)

### Changed

- **llm**: `ChatMessage` role now `MessageRole` instead of `String` (**breaking**)
- **llm**: `LLMRequest` content now `ContentPart`-based for multimodal support (**breaking**)

## [0.3.6] - 2026-06-12

### Added

- **embedding**: `normalize(&mut [f32])` — in-place L2 vector normalization utility
- **provider**: `ProviderIndex::estimate_cost(model_id, prompt_tokens, completion_tokens)` — USD cost estimator using catalog pricing data
- **llm**: `extract_xml_tag(text, tag)` — extract content from Claude-style `<tag>...</tag>` output
- **provider**: `CapabilityProfile` trait extended with default methods: `supports_tool_calling`, `supports_vision`, `supports_streaming`, `context_limit`; `ServiceDescriptor` implements all four from catalog data
- **llm**: `LLMResponse` gains optional fields `finish_reason`, `id`, `created`

### Changed

- **safety**: `mask_secrets` rewritten as single-pass `Regex::replace_all` (eliminates 3 separate loop passes over the input)
- **docs**: `#![deny(missing_docs)]` enforced crate-wide; all 187 previously undocumented public items now have doc comments

### Fixed

- **llm**: `OpenAIClient` and `AnthropicClient` struct literal initializers updated for new `LLMResponse` optional fields

## [0.3.5] - 2026-06-10

### Changed

- **vector-index**: absorb `llm-kernel-vector-index` subcrate into `llm-kernel` as `vector-index` feature gate — no separate crate needed; use `features = ["vector-index"]`
- **vector-index**: `TurbovecIndex` now re-exported as `llm_kernel::embedding::TurbovecIndex`
- **vector-index**: remove `load` from `VectorIndex` trait — trait is now fully object-safe (`dyn VectorIndex` usable); `TurbovecIndex::load` becomes an inherent method
- **vector-index**: atomic save pattern in `TurbovecIndex::save` (temp file → fsync → rename) for crash safety
- **vector-index**: `SearchHit` derives `Copy + PartialEq`; `PartialOrd` impl sorts descending by score, ascending by id on ties
- **full**: `vector-index` feature included in the `full` feature set

### Fixed

- **vector-index**: meta validation on `load` — rejects invalid `bit_width` (must be 2 or 4) and zero `dim`
- **vector-index**: cross-validate loaded index dim/bit_width against sidecar `.meta.json` on load
- **vector-index**: eliminate duplicate `validate_dim` calls in `add → add_with_ids` path

## [0.3.2] - 2026-06-09

### Fixed

- **llm**: add connect and total timeouts to reqwest Client to prevent indefinite hangs (#21)
- **safety**: expand `mask_secrets` patterns — `api_key`, `access_token`, `private_key`, `Basic` auth, AWS `AKIA`, GitHub tokens (#22)
- **store**: wrap SQLite migration in a transaction for atomicity (#23)

### Changed

- **errors**: unify `vault.rs` from `anyhow` to `KernelError::Vault`, add `discovery`/`store`/`config` prelude exports (#24)
- **llm**: extract `into_openai_messages`/`into_anthropic_messages` methods on `LLMRequest`, deduplicate 4 message builder blocks (#25)

### Added

- **tokens**: extend token estimation for Cyrillic, Greek, and Hebrew scripts; count whitespace at 0.25 token weight; add doc comments with `#![deny(missing_docs)]` (#26)

### Docs

- Update badge styles in README files (all 12 languages) for better visibility

## [0.3.0] - 2026-06-08

### Added

- `eval` feature gate: quality evaluation CLI (`llm-kernel-eval`) measuring token estimation accuracy, secret masking completeness, embedding correctness, search quality, and graph query precision
- `eval-full` feature gate: includes graph evaluation module on top of `eval`
- `--baseline <path>` flag for regression detection — compares current metrics against a golden JSON snapshot and exits 1 on any regression
- `eval/baseline.json` — golden baseline snapshot for CI regression checks
- CI `eval` job runs quality regression check on every push and PR
- `llm-kernel-vector-index` eval CLI (`llm-kernel-vector-index-eval`) measuring ANN recall, quantization impact, filtered search accuracy, and persistence round-trip integrity
- `llm-kernel-vector-index` `--baseline` flag for vector-index regression detection

## [0.3.0] - 2026-06-08

### Added

- `embedding`: `VectorIndex` trait — abstract interface for compressed vector indexes, zero dependencies. Concrete implementation: `crates/llm-kernel-vector-index` (TurboQuant)
- `embedding`: `SearchHit` type (`{ id: u64, score: f32 }`) for vector index search results
- `embedding`: `SearchHit::partial_cmp` — sorts by descending score with ascending ID tiebreak
- `embedding`: `VectorIndex::remove(&mut self, ids: &[u64])` — delete vectors by external ID (O(1) per ID)
- `llm-kernel-vector-index`: cross-validation of index dim/bit_width vs sidecar meta.json on load
- `llm-kernel-vector-index`: criterion benchmarks for add (1k/10k), search, filtered search, save/load (2-bit vs 4-bit)

## [0.2.6] - 2026-06-08

### Fixed

- `embedding`: `ort-load-dynamic` restricted to Windows only — Unix targets use static linking for reliable cross-platform builds
- `embedding`: switched ONNX Runtime backend from native-tls to rustls with dynamic loading for cross-platform compatibility
- Restored `Cargo.lock` to version control for reproducible builds

### Changed

- `embedding`: `NomicEmbedTextV15` and `NomicEmbedTextV15Q` now return correct task instruction prefixes (`search_query:` / `search_document:`) matching the official Nomic v1.5 model requirements

## [0.2.5] - 2026-06-08

### Fixed

- `embedding`: use `ort-load-dynamic` for all linux targets to avoid glibc 2.38 dependency (`__isoc23_strtol` etc on ubuntu-22.04)

### Added

- `embedding`: re-export `ort` for DirectML execution provider configuration
- **docs**: add `cargo generate-lockfile` to version bump checklist

## [0.2.4] - 2026-06-07

### Fixed

- `embedding`: `NomicEmbedTextV15` and `NomicEmbedTextV15Q` now return correct task instruction prefixes — `search_query:` / `search_document:` — matching the official Nomic v1.5 model requirements. Previously both returned `None`, producing suboptimal embeddings for search/retrieval workloads (fixes #11)

## [0.2.3] - 2026-06-07

### Fixed

- `embedding`: `ort-load-dynamic` now enabled for `aarch64-linux` targets, fixing cross-compile builds on ARM64

## [0.2.2] - 2026-06-07

### Fixed

- `embedding`: `ort-load-dynamic` restricted to Windows only — Unix targets use static linking for reliable cross-platform builds

## [0.2.1] - 2026-06-07

### Fixed

- `embedding`: switched ONNX Runtime backend from native-tls to rustls with dynamic loading (`ort-load-dynamic`) for cross-platform compatibility
- Restored `Cargo.lock` to version control for reproducible builds

## [0.2.0] - 2026-06-07

### Added

- `embedding-fastembed`: `EmbeddingModel` now exposes `size_mb()`, `model_id()`, `max_seq_length()` const methods for all 44 variants
- `embedding-fastembed`: `LazyFastembedProvider` — instant constructor with lazy model loading, `Condvar`-based concurrent access, and configurable idle eviction
- `embedding-fastembed`: `EmbeddingCache` — zero-dep LRU cache backed by `IndexMap` for query deduplication
- `embedding-fastembed`: `is_model_cached(model, cache_dir)` utility for checking HuggingFace cache
- `embedding-fastembed`: `ModelState` enum for introspecting provider lifecycle (`NotLoaded`, `Loading`, `Cached`, `Ready`, `Disabled`, `Failed`)
- `embedding-fastembed`: `LazyOpts` struct for configuring idle timeout, load timeout, and cache capacity

### Fixed

- `embedding-fastembed`: `ensure_model()` now transitions to `Failed` on load timeout, preventing permanent `Loading` state deadlock

## [0.1.1] - 2026-06-06

### Fixed

- `embedding`: `cosine_similarity` now accumulates in `f64` and returns `f64`, preventing precision loss in high-dimensional spaces (384–1024 dims) where `f32` rounding can flip ranking order between near-identical candidates (fixes #6)
  - **Breaking:** return type changed from `f32` → `f64` for both the free function and `EmbeddingResult::cosine_similarity`
- `embedding-openai`: `embed_batch` now sorts by `index` before mapping to input texts — OpenAI API does not guarantee response ordering, so the previous `zip` could silently corrupt text↔vector associations
- `embedding-openai`, `embedding-fastembed`: `&text[..64]` byte-slice replaced with char-boundary-safe `text_preview` helper — previously panicked on Korean/emoji/CJK input
- `embedding-fastembed`: removed unnecessary `prepared.clone()` in `embed_batch`

### Added

- `embedding-openai`: `OpenAIEmbeddingClient::new_with_model(api_key, model, dim)` for arbitrary model names and dimensions (closes #5)
- `embedding-fastembed-directml`: new feature gate; `FastembedProvider::new_with_directml` for DirectML GPU acceleration on Windows (closes #4)
- `embedding-fastembed`: `new_with_directml` doc warns about D3D12 initialisation latency
- `benches/compute_bench`: `cosine_similarity` criterion benchmarks for 128/384/768/1024 dims
- CI: `directml-check` job now runs `cargo clippy` on Windows in addition to `cargo check`

## [0.1.0] - 2026-06-06

### Changed

- Updated QUICKSTART and README to reflect current API (`prelude::*`, `GraphNode`, `smart_recall`, `SearchResult`, `rrf_fuse`)
- Fixed feature gate count in comparison table (20 modules)

### Note

First public-ready release. No API changes since 0.0.1 — all public types remain the same.

## [0.0.1] - 2026-06-05

### Added

#### Provider Catalog
- Embedded catalog with 16 providers and 114 models (`catalog.json`)
- `ProviderIndex` with O(1) lookup by provider name or model ID
- `CapabilityProfile` trait and `AuthStrategy` enum for auth mode logic
- Model pricing metadata (input/output cost per million tokens)

#### Knowledge Graph
- SQLite-backed graph with FTS5 (trigram tokenizer) full-text search
- `smart_recall` — composite scoring with 5 weighted signals (recency 20%, importance 35%, access 15%, FTS 20%, graph boost 10%)
- BFS traversal via recursive CTE (`related_nodes`)
- 1-hop neighbor lookup with weight aggregation (`graph_neighbors`)
- Full CRUD for nodes and edges
- Lifecycle management: importance decay, stale tagging, access tracking, stats
- `AsyncGraph` wrapper with `spawn_blocking` for tokio runtimes

#### MCP Server
- JSON-RPC 2.0 server framework with tool/resource registration
- Stdio transport loop with batch request support
- Bearer authentication with constant-time comparison
- Auto-generated auth tokens via xorshift PRNG

#### LLM Client (`client-async`)
- Async `LLMClient` trait with `complete()` and `stream_complete()`
- OpenAI and Anthropic implementations (sync + SSE streaming)
- `render_prompt()` with `{{variable}}` substitution
- `extract_json()` / `parse_json()` for structured LLM output extraction

#### Dynamic Model Discovery
- `models.dev` API fetcher with disk cache
- Ollama `/api/tags` model discovery
- OpenAI-compatible `/v1/models` discovery

#### Embedding
- `EmbeddingProvider` trait + `cosine_similarity()`
- OpenAI `text-embedding-3-small`/`large` client with batch support

#### Search
- Reciprocal Rank Fusion (`rrf_fuse`) for hybrid search result merging

#### Token Estimation
- Zero-dependency `estimate_tokens()` with Unicode-script heuristics (CJK, emoji, Arabic, Devanagari, Thai)

#### Telemetry
- Enum-gated `TelemetryEvent` variants (no free-form strings, no PII)
- `ConsoleSink` and `NoopSink` implementations

#### Safety
- `mask_secrets()` — Bearer tokens, API keys, passwords (all occurrences)
- `sanitize_output()` — bidi overrides, plane-14 tags, null bytes, C1 controls
- `classify_failure()` — regex-based error classification into 10 categories
- `strip_ansi()` — ANSI escape code removal

#### Installation Wizard
- MCP config generation for 5 agent types: Claude Desktop, Cursor, Copilot, OpenCode, Cline

#### Security
- `SecretVault` with dotenv-style load/save and symlink guards
- Atomic file writes with 0o600 permissions
- Constant-time bearer token comparison
- Regex-based secret masking across all occurrences

#### Infrastructure
- SQLite store helpers with WAL mode, FTS5, and schema versioning
- TOML configuration loader with auto-create from template
- Criterion benchmarks for graph recall, BFS traversal, token estimation, and RRF fusion

#### CI/CD
- Feature matrix testing (9 combinations)
- `cargo audit` and CycloneDX SBOM generation
- Doc lint with `-D warnings`
- Dependabot weekly updates
- Release workflow — crates.io publish + GitHub Release on tag push
