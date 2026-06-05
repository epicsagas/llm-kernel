# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2025-06-05

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

#### Benchmarks
- Criterion benchmarks for graph recall, BFS traversal, token estimation, and RRF fusion

#### CI/CD
- Feature matrix testing (9 combinations)
- `cargo audit` and CycloneDX SBOM generation
- Doc lint with `-D warnings`
- Dependabot weekly updates

## [0.1.0] - 2025-06-05

### Added
- LLM client trait with OpenAI and Anthropic implementations
- SQLite store helpers with WAL mode, FTS5, and schema versioning
- TOML configuration loader with auto-create
- Prompt template rendering with `{{variable}}` substitution
- Common error types via thiserror
