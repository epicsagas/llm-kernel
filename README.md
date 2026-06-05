<div align="center">

# llm-kernel

> LLM provider catalog, async client, and model discovery for Rust applications

[![CI](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml/badge.svg)](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-kernel)](https://crates.io/crates/llm-kernel)
[![docs.rs](https://docs.rs/llm-kernel/badge.svg)](https://docs.rs/llm-kernel)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

</div>

## Overview

llm-kernel provides the foundational layer for working with LLM providers in Rust:

- **Provider catalog** — 16 built-in providers with model metadata, pricing, and capabilities
- **Async client** — trait-based client for OpenAI and Anthropic with SSE streaming
- **Model discovery** — dynamic model discovery from models.dev, Ollama, OpenAI-compatible endpoints
- **Credential vault** — dotenv-style API key management with atomic writes
- **Config loader** — TOML config with auto-create from template

## Feature flags

Each module is gated behind a feature flag so you only pay for what you use.

| Feature | Description | Default |
|---------|-------------|---------|
| `provider` | Provider catalog, model descriptors, pricing | ✅ |
| `client-async` | Async LLM client (reqwest) with streaming | |
| `discovery` | Dynamic model discovery | |
| `secrets` | SecretVault credential management | |
| `store` | SQLite init helpers (WAL, FTS5, schema versioning) | |
| `config` | TOML config loader | |
| `full` | All features | |

## Quick start

Add to your `Cargo.toml`:

```toml
[dependencies]
llm-kernel = "0.3"
```

The `provider` feature is enabled by default. For the async client:

```toml
[dependencies]
llm-kernel = { version = "0.3", features = ["client-async"] }
```

## Usage

### Provider catalog

The embedded catalog contains 16 providers with model metadata aligned to the [models.dev](https://github.com/anomalyco/models.dev) schema.

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

## Model metadata

Each model in the catalog includes:

| Field | Description |
|-------|-------------|
| `cost` | Per-million-token pricing (input, output, cache_read, cache_write) |
| `limit` | Context and output token limits |
| `modalities` | Input/output modalities (text, image, audio) |
| `capabilities` | Flags: attachment, reasoning, temperature, tool_call, streaming |
| `knowledge` | Training data cutoff date |

## Architecture

```
┌─────────────┐
│  Your app   │
├─────────────┤
│   prelude   │  ← use llm_kernel::prelude::*;
├──────┬──────┤
│provider│client│  ← trait LLMClient { complete, stream_complete }
│catalog │async │
├──────┴──────┤
│  secrets │ config │ store │  ← infrastructure modules
└─────────────┘
```

- **`LLMClient` trait** — unified interface for `OpenAIClient` and `AnthropicClient`
- **`ProviderIndex`** — zero-copy access to embedded catalog, queryable by provider or model
- **`SecretVault`** — `HashMap<String, String>` with dotenv load/save and symlink guards

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

[Apache-2.0](LICENSE) © 2025 EpicCounty
