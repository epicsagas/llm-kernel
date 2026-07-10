# Feature catalog & platform compatibility

llm-kernel is a single crate with **zero mandatory external dependencies**
beyond `serde` (default = `["provider"]`). Every additional capability is an
opt-in Cargo feature. This is the reference for the `full` surface and the
platform constraints per feature — v1.0.0 ROADMAP #6.

> Related: `Cargo.toml` `[features]` · `AGENTS.md`

## How features compose

- `default = ["provider"]` — the only thing you get without opting in.
- `full` — every feature except Windows-only / dev-only ones (see exclusions
  below). Use it for "give me everything that builds on this platform".
- Backends (PostgreSQL, Qdrant, Elasticsearch, pgvector) pull their driver
  crate **only when the feature is on**, so the default build stays light.
- `embedding-fastembed` and `embedding-fastembed-dynamic-linking` are
  **mutually exclusive** (`compile_error!` in `src/lib.rs`, #50/#55): static
  vs runtime-loaded ONNX Runtime. Pick one.

## Catalog

### Core / domain (no heavy deps)
| Feature | What it enables |
|---|---|
| `provider` *(default)* | Model/provider catalog (`catalog.json`), capability profiles, models.dev mapping |
| `tokens` | Unicode-aware token estimation, `TokenBudget`, sentence-aware chunking |
| `safety` | Secret masking (`mask_secrets`), output sanitisation (Bidi/unicode attacks), prompt-injection detection. Owns the `regex` dep. |
| `secrets` | `.env` vault with atomic write + `0o600` |
| `store` | SQLite init helpers + `KvStore` trait |
| `config` | TOML config loader |
| `telemetry` | Enum-gated events |

### LLM client & search
| Feature | What it enables |
|---|---|
| `client-async` | Async LLM client (OpenAI/Anthropic), SSE streaming, retry/middleware/cache hooks. `redact_http_body` masks error bodies when `safety` is also on. |
| `cache` | LLM response cache on `KvStore` (`cache = ["client-async","store"]`) |
| `search` | `SearchProvider` trait, RRF + weighted-sum + CombMNZ fusion |
| `federation` | Cross-engine `FederatedSearch` (qdrant + elastic + TurboVec) |
| `discovery` / `discovery-async` | models.dev / Ollama / OpenAI-compat model discovery |
| `mcp` | JSON-RPC 2.0 MCP server, stdio transport |
| `mcp-http` | MCP over HTTP/SSE (axum) |

### Knowledge graph
| Feature | What it enables |
|---|---|
| `graph` | Memory graph + general directed-graph backend: `GraphBackend` trait (batch edges via `append_edges`, directional / relation-filtered lookups via `EdgeDirection`, filtered BFS), FTS5 search, smart recall, CSR algorithms (PageRank, components, Dijkstra, similarity) |
| `graph-cjk` | CJK (Korean/Japanese/Chinese) substring search path — see [korean-recall.md](benchmarks/korean-recall.md) |
| `graph-async` | Tokio wrapper around the graph (batch/directional edge methods included) |
| `graph-pool` | WAL multi-connection async pool — see [graph_concurrency.md](benchmarks/graph_concurrency.md) |
| `graph-pg` | PostgreSQL `GraphBackend` (`PgGraph`); `from_client` adopts an external synchronous `postgres::Client` |
| `graph-pg-tls` | TLS for `PgGraph` connections |

### Embeddings & vector backends
| Feature | What it enables |
|---|---|
| `embedding` | `EmbeddingProvider` trait + base types |
| `embedding-openai` | OpenAI embeddings client |
| `embedding-fastembed` | **Static** ONNX Runtime embeddings (default fastembed path) |
| `embedding-fastembed-dynamic-linking` | **Runtime-loaded** ONNX Runtime (escape hatch; mutually exclusive with the static feature) |
| `embedding-fastembed-qwen3` / `-nomic-moe` | Specific fastembed model presets |
| `embedding-fastembed-directml` | DirectML execution provider (**Windows-only**) |
| `embedding-fastembed-coreml` | CoreML execution provider (**macOS**, GPU/ANE) |
| `vector-index` | Built-in TurboVec SIMD vector index |
| `qdrant` | Qdrant `AsyncVectorIndex` |
| `elastic` | Elasticsearch `AsyncVectorIndex` (hand-rolled reqwest) |
| `pgvector` | PostgreSQL + pgvector `AsyncVectorIndex` |

### Tooling (not part of the library surface)
| Feature | What it enables |
|---|---|
| `install` | AI-tool config wizard |
| `catalog-sync` | `llm-kernel-sync-catalog` binary (refresh catalog from models.dev) |
| `eval` / `eval-full` | `llm-kernel-eval` binary (quality regression). `eval-full` adds `graph` + `graph-cjk`. |
| `full` | Everything except the Windows-only and dev-only features below |

### Intentionally excluded from `full`
- `embedding-fastembed-directml` — Windows-only.
- `embedding-fastembed-dynamic-linking` — opt-in escape hatch, mutually
  exclusive with the static feature (#55).
- `eval` / `eval-full` — developer CLI tooling, not library surface.

## Platform compatibility

| Feature | macOS | Linux | Windows | Notes |
|---|:---:|:---:|:---:|---|
| default / core / graph | ✅ | ✅ | ✅ | Pure Rust + bundled SQLite |
| `embedding-fastembed` (static ONNX) | ✅ | ✅ glibc ≥ 2.38 | ✅ current MSVC CRT | Static archive links at build time; oldest baselines use the dynamic escape hatch (#55/#57) |
| `embedding-fastembed-dynamic-linking` | ✅ | ✅ (any glibc) | ✅ | Loads `libonnxruntime` at runtime — supply the dylib yourself |
| `embedding-fastembed-coreml` | ✅ | ❌ | ❌ | macOS GPU/ANE |
| `embedding-fastembed-directml` | ❌ | ❌ | ✅ | Windows GPU |
| `mcp-http` / `client-async` | ✅ | ✅ | ✅ | rustls (no system TLS) |
| `graph-pg` / `pgvector` | ✅ | ✅ | ✅ | Needs PostgreSQL + (pgvector) at runtime; tests self-skip without `LLMKERNEL_PG_URL` |
| `qdrant` / `elastic` | ✅ | ✅ | ✅ | Need the server at runtime; tests self-skip without `LLMKERNEL_*_URL` |

### MSRV & toolchain
- **MSRV:** 1.92 · **Edition:** 2024 · `clap` 4, `serde`, `thiserror`/`anyhow`.
- **macOS static link:** the `ort` static archive links `libclang_rt.osx.a`,
  dropped by Xcode 16+ runner images — CI pins `macos-14` and injects the
  clang_rt path (#57). Local builds on current Xcode need the same.

### CI coverage (`ci.yml`)
- `full` matrix on Ubuntu (check/test/clippy/fmt/docs).
- Isolated per-feature build/test matrix (every feature builds alone).
- `release-link-check` (Linux/Windows static, Ubuntu-22.04 dynamic escape
  hatch), `directml-check` (Windows), `macos-check`.
- `cargo audit` + gitleaks + SBOM; `cargo-semver-checks` (`semver.yml`).
