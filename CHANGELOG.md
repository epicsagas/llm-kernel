# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0] - 2026-06-13

### Added

- **llm**: `RetryClient` and `RetryConfig` — exponential backoff wrapper around any `LLMClient`, auto-retries 429 and 5xx with jitter (new `src/llm/retry.rs`)
- **llm**: `LLMClientMiddleware` trait with `on_request`/`on_response`/`on_error` async hooks and composable `MiddlewareClient` wrapper (new `src/llm/middleware.rs`)
- **llm**: `ConversationHistory` — ordered message list with role-alternation validation and token-budget-aware truncation that preserves the system message (new `src/llm/history.rs`, `tokens` feature)
- **embedding**: `chunk_batch` utility — splits a batch into provider-limit-sized chunks
- **embedding**: `LazyFastembedProvider::embed_batch` override — LRU cache lookup + batch merge of misses for true batching
- **config**: `FieldError` struct and `validate_config` — structured field-level TOML validation errors (path/expected/value) instead of raw serde strings
- **install**: `AgentKind` expanded with `Windsurf`, `Aider`, `RooCode` variants

### Changed

- **embedding**: `chunk_batch` and `validate_config` re-exported from their module roots

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
