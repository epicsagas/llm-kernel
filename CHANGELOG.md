# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.4] - 2026-06-07

### Fixed

- `embedding`: `NomicEmbedTextV15` and `NomicEmbedTextV15Q` now return correct task instruction prefixes — `search_query:` / `search_document:` — matching the official Nomic v1.5 model requirements. Previously both returned `None`, producing suboptimal embeddings for search/retrieval workloads (fixes #11)

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
