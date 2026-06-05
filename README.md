<div align="center">

# llm-kernel

> Foundation library for Rust AI-native apps — provider catalog, LLM client, MCP server, search, telemetry, and safety

[![CI](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml/badge.svg)](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-kernel)](https://crates.io/crates/llm-kernel)
[![docs.rs](https://docs.rs/llm-kernel/badge.svg)](https://docs.rs/llm-kernel)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

</div>

## Overview

llm-kernel provides the foundational layer for building LLM-powered tools, agents, and servers in Rust:

- **Provider catalog** — 16 built-in providers, 114 models with metadata, pricing, and capabilities
- **Async client** — trait-based client for OpenAI and Anthropic with SSE streaming
- **Model discovery** — dynamic model discovery from models.dev, Ollama, OpenAI-compatible endpoints
- **Credential vault** — dotenv-style API key management with atomic writes
- **Config loader** — TOML config with auto-create from template
- **Knowledge graph** — SQLite-backed graph with FTS5 search, smart recall, BFS traversal, async wrappers
- **MCP server** — JSON-RPC 2.0 server framework with stdio transport and Bearer auth
- **Embedding** — provider trait + cosine similarity, local ONNX (44 models), Qwen3 candle, Nomic V2 MoE candle, OpenAI remote ([full model list →](EMBEDDING_MODELS.md))
- **Search** — Reciprocal Rank Fusion for hybrid search result merging
- **Token estimation** — zero-dependency Unicode-script heuristic token counting
- **Telemetry** — enum-gated events with no PII, console and noop sinks
- **Safety** — secret masking, error classification, output sanitization
- **Install wizard** — MCP config generation for Claude Desktop, Cursor, Copilot, OpenCode, Cline

## Feature flags

Each module is gated behind a feature flag so you only pay for what you use.

| Feature | Description | Default |
|---------|-------------|---------|
| `provider` | Provider catalog, model descriptors, pricing | ✅ |
| `client-async` | Async LLM client (reqwest) with streaming | |
| `discovery` | Dynamic model discovery (models.dev, Ollama, OpenAI-compat) | |
| `secrets` | SecretVault credential management | |
| `store` | SQLite init helpers (WAL, FTS5, schema versioning) | |
| `config` | TOML config loader | |
| `graph` | Knowledge graph — SQLite, FTS5, smart recall, BFS traversal | |
| `graph-async` | Async graph wrappers (requires tokio) | |
| `graph-pool` | Multi-connection async graph pool (WAL concurrency) | |
| `mcp` | MCP server — JSON-RPC 2.0, stdio transport, Bearer auth | |
| `tokens` | Token estimation with Unicode-script heuristics | |
| `install` | AI tool installation wizard | |
| `search` | Hybrid search with Reciprocal Rank Fusion | |
| `embedding` | Embedding provider trait + cosine similarity | |
| `embedding-openai` | OpenAI text-embedding client (sync HTTP) | |
| `embedding-fastembed` | Local ONNX embedding via fastembed-rs (44 models) | |
| `embedding-fastembed-qwen3` | Qwen3 embedding via candle backend | |
| `embedding-fastembed-nomic-moe` | Nomic V2 MoE embedding via candle backend | |
| `telemetry` | Enum-gated telemetry events, no PII | |
| `safety` | Secret masking, error classification, output sanitization | |
| `full` | All features | |

## Quick start

Add to your `Cargo.toml`:

```toml
[dependencies]
llm-kernel = "0.0.1"
```

The `provider` feature is enabled by default. For the async client:

```toml
[dependencies]
llm-kernel = { version = "0.0.1", features = ["client-async"] }
```

For the knowledge graph with async wrappers:

```toml
[dependencies]
llm-kernel = { version = "0.0.1", features = ["graph", "graph-async"] }
```

For local embedding (ONNX, no API key):

```toml
[dependencies]
llm-kernel = { version = "0.0.1", features = ["embedding-fastembed"] }
```

## Usage

### Provider catalog

The embedded catalog contains 16 providers with 114 models aligned to the [models.dev](https://github.com/anomalyco/models.dev) schema.

```rust
use llm_kernel::prelude::*;

let catalog = ProviderIndex::embedded();

// List all providers
for id in catalog.ids() {
    let provider = catalog.get(&id).unwrap();
    println!("{}", provider.display_name);
}

// Query models for a provider
for model in catalog.models_for("openai") {
    println!("  {} — ${:.2}/1M in", model.id, model.cost.unwrap().input);
}

// Find a specific model
if let Some(model) = catalog.find_model("claude-sonnet-4-20250514") {
    println!("Context: {} tokens", model.limit.unwrap().context);
}
```

### Async chat completion

```rust
use llm_kernel::prelude::*;

let config = ModelConfig {
    provider: "openai".into(),
    model: "gpt-4o".into(),
    api_key_env: "OPENAI_API_KEY".into(),
    base_url: None,
    temperature: 0.7,
    max_tokens: Some(1024),
};

let client = OpenAIClient::new(&config)?;

let response = client.complete(LLMRequest {
    system: Some("You are a helpful assistant.".into()),
    messages: vec![ChatMessage::user("Hello!")],
    temperature: 0.7,
    max_tokens: Some(1024),
    model: None,
}).await?;

println!("{}", response.content);
println!("{} tokens used", response.usage.total_tokens);
```

### Streaming

```rust
use llm_kernel::prelude::*;

let config = ModelConfig {
    provider: "anthropic".into(),
    model: "claude-haiku-4-5-20251001".into(),
    api_key_env: "ANTHROPIC_API_KEY".into(),
    base_url: None,
    temperature: 0.7,
    max_tokens: Some(256),
};

let client = AnthropicClient::new(&config)?;
let stream = client.stream_complete(LLMRequest {
    system: Some("Reply concisely.".into()),
    messages: vec![ChatMessage::user("Explain Rust in one paragraph.")],
    temperature: 0.7,
    max_tokens: Some(256),
    model: None,
}).await?;

// Stream yields Delta, Usage, and Done events
```

### Model discovery

```rust
use llm_kernel::discovery::{fetch_and_cache, load_cache, fetch_ollama_models};

// Fetch from models.dev (caches to disk)
let payload = fetch_and_cache("~/.cache/llm-kernel/models-dev.json")?;
for model in &payload.models {
    println!("{} — {} (ctx: {:?})", model.id, model.provider_id, model.limits);
}

// Load from cache (no network)
if let Some(cached) = load_cache("~/.cache/llm-kernel/models-dev.json")? {
    println!("{} models cached", cached.models.len());
}

// Discover local Ollama models
let ollama_models = fetch_ollama_models("http://localhost:11434")?;
for name in &ollama_models {
    println!("Ollama: {}", name);
}
```

### Credential vault

```rust
use llm_kernel::prelude::*;

let vault = SecretVault::load_from("~/.config/myapp/.env")?;
vault.set("OPENAI_API_KEY", "sk-...");
vault.save_to("~/.config/myapp/.env")?;

// Redact credentials for logging
println!("{}", redact_credential("sk-abcdef1234567890"));
// → "sk-abcd...7890"
```

### TOML config

```rust
use llm_kernel::config::load_toml_config;
use serde::Deserialize;

#[derive(Deserialize)]
struct AppConfig {
    model: String,
    temperature: f32,
}

let config: AppConfig = load_toml_config(
    &path,
    Some(&llm_kernel::config::default_config_template("myapp")),
)?;
```

### SQLite store

```rust
use llm_kernel::store::init_schema;

let ddl = "CREATE TABLE items (id TEXT PRIMARY KEY, content TEXT);";
let conn = init_schema(&db_path, ddl, 1)?;
// WAL mode, busy timeout, and schema versioning applied automatically
```

### Knowledge graph

```rust
use llm_kernel::prelude::*;
use rusqlite::Connection;

let conn = Connection::open_in_memory().unwrap();
init_graph_schema(&conn).unwrap();

// Create nodes
upsert_node(&conn, &GraphNode {
    id: "rust-ownership".into(),
    node_type: "concept".into(),
    title: "Rust Ownership Model".into(),
    body: "Ownership, borrowing, and lifetimes...".into(),
    tags: vec!["rust".into(), "memory-safety".into()],
    projects: vec!["my-project".into()],
    agents: vec![],
    created: "2026-01-01T00:00:00Z".into(),
    updated: "2026-01-01T00:00:00Z".into(),
    importance: 0.8,
    access_count: 0,
    accessed_at: String::new(),
}).unwrap();

// Connect with edges
append_edge(&conn, &GraphEdge {
    id: "e1".into(),
    source: "rust-ownership".into(),
    target: "borrow-checker".into(),
    relation: "related".into(),
    weight: 1.5,
    ts: "2026-01-01T00:00:00Z".into(),
}).unwrap();

// Smart recall with composite scoring
let results = smart_recall(&conn, Some("my-project"), Some("ownership"), 5).unwrap();
for scored in &results {
    println!("{:.2} — {}", scored.score, scored.node.title);
}

// Lifecycle management
decay_importance(&conn, 30, 0.9, 0.05).unwrap();
tag_stale_nodes(&conn, 90).unwrap();
let stats = compute_stats(&conn).unwrap();
println!("{} nodes, {} edges", stats.total_nodes, stats.total_edges);
```

### MCP server

```rust
use llm_kernel::mcp::{McpServer, Tool, JsonRpcRequest};
use serde_json::json;

let mut server = McpServer::new("my-server", "1.0.0");
server.register_tool(Tool {
    name: "greet".into(),
    description: "Say hello".into(),
    input_schema: json!({
        "type": "object",
        "properties": { "name": { "type": "string" } },
        "required": ["name"]
    }),
});

// Runs JSON-RPC 2.0 over stdio with Bearer auth
server.run_stdio().await?;
```

### Token estimation

```rust
use llm_kernel::tokens::estimate_tokens;

let tokens = estimate_tokens("Hello, world! こんにちは世界 🌍");
println!("Estimated tokens: {}", tokens);
```

### Embedding + search

```rust
use llm_kernel::embedding::{EmbeddingProvider, cosine_similarity};
use llm_kernel::search::rrf_fuse;

// Cosine similarity between vectors
let sim = cosine_similarity(&[0.1, 0.2, 0.3], &[0.4, 0.5, 0.6]);

// Reciprocal Rank Fusion for hybrid search
let merged = rrf_fuse(&[
    vec!["doc-a".into(), "doc-b".into()],
    vec!["doc-b".into(), "doc-c".into()],
], 60);
```

#### Local ONNX embedding (fastembed-rs)

44 models via ONNX Runtime — no API key, no network after first download.

```rust
use llm_kernel::embedding::{EmbeddingModel, FastembedProvider, EmbeddingProvider};

let provider = FastembedProvider::new(EmbeddingModel::BGESmallENV15, None)?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 384);
```

#### Qwen3 embedding (candle)

Pure Rust GPU/CPU inference via candle-nn — no ONNX Runtime.

```rust
use llm_kernel::embedding::{Qwen3Provider, EmbeddingProvider};

let provider = Qwen3Provider::new("Qwen/Qwen3-Embedding-0.6B")?;
let result = provider.embed("hello world")?;
```

#### Nomic V2 MoE embedding (candle)

Lightweight MoE model — 8 experts, top-2 routing, 305M active params.

```rust
use llm_kernel::embedding::{NomicMoeProvider, EmbeddingProvider};

let provider = NomicMoeProvider::new()?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 768);
```

### Safety utilities

```rust
use llm_kernel::safety::{mask_secrets, classify_failure, sanitize_output};

// Mask secrets in logs
let safe = mask_secrets("Authorization: Bearer sk-abcdef123456");
// → "Authorization: Bearer [REDACTED]"

// Classify errors
let category = classify_failure("connection timed out after 30s");
// → ErrorCategory::Timeout

// Sanitize untrusted output
let clean = sanitize_output(user_input)?;
```

## Model metadata

Each model in the catalog includes:

| Field | Description |
|-------|-------------|
| `cost` | Per-million-token pricing (input, output, cache_read, cache_write) |
| `limit` | Context and output token limits |
| `modalities` | Input/output modalities (text, image, audio) |
| `capabilities` | Flags: attachment, reasoning, temperature, tool_call, streaming |
| `knowledge` | Training data cutoff date |

## Why llm-kernel?

| | llm-kernel | [rig] | [langchain-rust] |
|--|-----------|-------|-------------------|
| Provider catalog | ✅ 16 providers, 114 models built-in | Manual config | Manual config |
| Feature gates | ✅ 21 independent modules | Monolithic | Monolithic |
| Local embedding | ✅ 44 ONNX + Qwen3 + Nomic MoE | ❌ | ❌ |
| MCP server | ✅ JSON-RPC 2.0 | ❌ | ❌ |
| Knowledge graph | ✅ SQLite + FTS5 + smart recall | ❌ | ❌ |
| Mandatory deps | `serde` only | `reqwest`, `tokio`, … | Many |
| Chains / agents | ❌ | ✅ | ✅ |
| RAG pipelines | ❌ | ✅ | ✅ |

[rig]: https://github.com/0xPlaygrounds/rig
[langchain-rust]: https://github.com/Abraxas-365/langchain-rust

llm-kernel is a **lightweight foundation layer** — compose it with rig or langchain-rust when you need chains, agents, or RAG.

## Architecture

```
┌──────────────────────────────────────────┐
│              Your app                    │
├──────────────────────────────────────────┤
│               prelude                    │  ← use llm_kernel::prelude::*;
├───────────────┬──────────┬───────────────┤
│   provider    │  client  │   discovery   │  ← catalog, async LLM, model discovery
│   catalog     │  async   │               │
├───────────────┴──────────┴───────────────┤
│  graph  │  mcp  │  embedding  │  search  │  ← graph, MCP, ONNX/Qwen3/Nomic embed, RRF
├──────────────────────────────────────────┤
│ tokens │ telemetry │ safety │ install    │  ← token est., events, masking, wizard
├──────────────────────────────────────────┤
│    secrets    │   config   │   store     │  ← vault, TOML, SQLite infra
└──────────────────────────────────────────┘
```

- **`LLMClient` trait** — unified interface for `OpenAIClient` and `AnthropicClient`
- **`EmbeddingProvider` trait** — unified interface for `FastembedProvider` (ONNX), `Qwen3Provider` (candle), `NomicMoeProvider` (candle), `OpenAIEmbeddingClient` (remote)
- **`ProviderIndex`** — zero-copy access to embedded catalog, queryable by provider or model
- **`McpServer`** — JSON-RPC 2.0 server with stdio transport, Bearer auth, tool registration
- **`SecretVault`** — `HashMap<String, String>` with dotenv load/save and symlink guards
- **`graph`** — SQLite knowledge graph with FTS5 search, composite scoring recall, BFS traversal, importance decay
- **`TelemetryEvent`** — enum-gated variants for structured observability (no PII)
- **`safety`** — secret masking, error classification, bidi/ANSI/null sanitization

## Benchmarks

Criterion benchmarks under `benches/`:

```bash
cargo bench                          # Run all benchmarks
cargo bench -- graph_bench           # Graph: smart_recall, BFS, neighbors
cargo bench -- compute_bench         # Token estimation, RRF fusion
```

## Examples

```bash
# List all providers and models (no API key needed)
cargo run --example provider_list

# OpenAI chat (requires OPENAI_API_KEY)
cargo run --example chat_openai --features client-async

# Anthropic streaming (requires ANTHROPIC_API_KEY)
cargo run --example stream_anthropic --features client-async
```

## Requirements

- Rust 1.92+ (edition 2024)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). PRs welcome.

## License

[Apache-2.0](LICENSE) © 2026 EpicCounty
