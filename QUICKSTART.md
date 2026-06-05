# llm-kernel — Quick Start

Get up and running in under 5 minutes.

## Prerequisites

- Rust **1.92+** (`rustup update stable`)
- OpenAI API key — required only for the async client examples

## 1. Add the dependency

Add to your `Cargo.toml`. The `provider` feature is enabled by default.

```toml
[dependencies]
llm-kernel = "0.0.1"
```

For the async LLM client:

```toml
[dependencies]
llm-kernel = { version = "0.0.1", features = ["client-async"] }
```

To enable everything:

```toml
[dependencies]
llm-kernel = { version = "0.0.1", features = ["full"] }
```

## 2. Browse the provider catalog

```rust
use llm_kernel::provider::ProviderIndex;

let catalog = ProviderIndex::embedded();
for id in catalog.ids() {
    let p = catalog.get(&id).unwrap();
    println!("{}: {} models", p.display_name, catalog.models_for(&id).len());
}
```

Run the bundled example:

```bash
cargo run --example provider_list
```

## 3. Async LLM client (OpenAI)

```bash
export OPENAI_API_KEY=sk-...
cargo run --example chat_openai --features client-async
```

```rust
use llm_kernel::llm::{ChatMessage, LLMClient, ModelConfig, OpenAIClient};

let config = ModelConfig {
    provider: "openai".into(),
    model: "gpt-4o-mini".into(),
    api_key_env: "OPENAI_API_KEY".into(),
    base_url: None,
    temperature: 0.7,
    max_tokens: Some(256),
};

let client = OpenAIClient::new(&config)?;
let response = client.complete(/* LLMRequest */).await?;
println!("{}", response.content);
```

## 4. Knowledge graph

```toml
[dependencies]
llm-kernel = { version = "0.0.1", features = ["graph"] }
```

```rust
use llm_kernel::graph::{KnowledgeGraph, init_graph_schema};
use rusqlite::Connection;

let conn = Connection::open("my.db")?;
init_graph_schema(&conn)?;
let graph = KnowledgeGraph::new(conn);
graph.insert_node("rust", "Rust programming language")?;
let results = graph.fts_search("programming")?;
```

## Verify

```bash
cargo test --all-features
cargo clippy --all-features -- -D warnings
```

## Next Steps

- [README](README.md) — full feature flag reference and API overview
- [CONTRIBUTING.md](CONTRIBUTING.md) — development environment setup
- [examples/](examples/) — provider_list, chat_openai, stream_anthropic
- [docs.rs/llm-kernel](https://docs.rs/llm-kernel) — full API documentation
