# AGENTS.md

## Project Overview

`llm-kernel` is a Rust foundation library for AI-native applications. It provides a provider catalog, async LLM client, MCP server framework, knowledge graph, search, telemetry, and safety utilities — all behind feature gates with zero mandatory external deps beyond `serde`.

## Commands

| Command | Description |
|---------|-------------|
| `cargo test --all-features` | Run all tests (483 passed, 12 ignored) |
| `cargo clippy --all-features -- -D warnings` | Lint |
| `cargo fmt --all -- --check` | Format check |
| `cargo bench` | Run criterion benchmarks |
| `cargo doc --all-features --no-deps` | Build docs |
| `cargo run --bin llm-kernel-eval --features eval -- all` | Quality eval (tokens, safety, injection, embedding, search) |
| `cargo run --bin llm-kernel-eval --features eval-full -- --baseline eval/baseline.json all` | Regression check vs baseline |

## Architecture

Hexagonal architecture with feature-gated modules under `src/`:

```
src/
  lib.rs, error.rs, prelude.rs     — crate root
  provider/    — catalog.json, capability profiles  (feature: provider)
  llm/         — async client, SSE streaming, JSON extraction, prompt templates, response cache  (features: client-async, cache)
  discovery/   — models.dev, Ollama, OpenAI-compat; async DiscoverySource  (features: discovery, discovery-async)
  secrets/     — dotenv vault, atomic writes  (feature: secrets)
  store/       — SQLite init helpers, KvStore  (feature: store)
  config/      — TOML loader  (feature: config)
  graph/       — GraphBackend trait, FTS5, smart recall, BFS, CJK search, migrations  (features: graph, graph-async, graph-pool, graph-cjk)
  mcp/         — JSON-RPC 2.0 server, stdio + HTTP/SSE transport, async handlers  (features: mcp, mcp-http)
  tokens/      — Unicode token estimation, budgeting, sentence-aware chunking  (feature: tokens)
  install/     — AI tool config wizard  (feature: install)
  search/      — SearchProvider trait, RRF + weighted-sum + CombMNZ fusion; cross-engine FederatedSearch  (features: search, federation)
  embedding/   — provider trait + OpenAI client, ElasticsearchVectorIndex  (features: embedding, embedding-openai, elastic)
  telemetry/   — enum-gated events  (feature: telemetry)
  safety/      — secret masking, error classification, prompt-injection detection  (feature: safety)
```

Additional binary targets:
```
  src/bin/eval.rs                          — quality evaluation CLI  (features: eval, eval-full)
  src/bin/migrate.rs                       — SQLite↔PostgreSQL graph migration CLI  (feature: graph-pg)
  eval/baseline.json                       — golden baseline snapshot for regression detection
```

Optional backend features (drivers compiled only when enabled; included in `full`):
```
  graph-pg  — PostgreSQL GraphBackend (PgGraph)            src/graph/pg.rs        (driver: postgres)
  qdrant    — Qdrant AsyncVectorIndex (QdrantVectorIndex)  src/embedding/qdrant.rs (driver: qdrant-client)
  elastic   — Elasticsearch AsyncVectorIndex (ElasticsearchVectorIndex)  src/embedding/elastic.rs (hand-rolled reqwest; official elasticsearch crate is alpha-only)
```
Live integration tests for all three gate on `LLMKERNEL_PG_URL` / `LLMKERNEL_QDRANT_URL` / `LLMKERNEL_ELASTIC_URL` and self-skip when unset.

## Key Conventions

- Domain types have **zero external dependencies** — `provider`, `tokens`, `embedding` (base) are no-dep
- Every module has inline `#[cfg(test)] mod tests` — no separate test files except `tests/feature_gates.rs`
- SQLite graph tests use `mem_db()` helper pattern (in-memory + `init_graph_schema`)
- Errors: `KernelError` enum via thiserror, `Result<T>` alias
- Eval features: `eval` (tokens+safety+injection+embedding+search), `eval-full` (eval+graph) — gated behind `clap` optional dep
- Feature gate: `default = ["provider"]`, `full` enables everything
- Edition 2024, MSRV 1.92

## Benchmarks

Two benchmark suites under `benches/`:
- `graph_bench.rs` — smart_recall, BFS traversal, neighbor lookup
- `compute_bench.rs` — token estimation, RRF fusion

## Version Bump Checklist

| Step | Action |
|------|--------|
| 1. Update version | Edit `Cargo.toml` version |
| 2. Update CHANGELOG | Add entry with date |
| 3. Regenerate lockfile | `cargo generate-lockfile` |
| 4. Verify | `cargo test --all-features && cargo clippy --all-features -- -D warnings` |
| 5. Commit | `git add Cargo.toml Cargo.lock CHANGELOG.md && git commit -m "chore: bump v{version}"` |
| 6. Tag | `git tag v{version}` |
| 7. Push | `git push && git push --tags` |
