# llm-kernel — Quick Start

Get up and running in under 5 minutes.

## Prerequisites

- Rust **1.92+** (`rustup update stable`)
- API key — required only for the async client and remote embedding examples

## 1. Add the dependency

Add to your `Cargo.toml`. The `provider` feature is enabled by default.

```toml
[dependencies]
llm-kernel = "0.1.0"
```

For the async LLM client:

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["client-async"] }
```

To enable everything:

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["full"] }
```

## 2. Browse the provider catalog

```rust
use llm_kernel::prelude::*;

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
use llm_kernel::prelude::*;

let config = ModelConfig {
    provider: "openai".into(),
    model: "gpt-4o-mini".into(),
    api_key_env: "OPENAI_API_KEY".into(),
    base_url: None,
    temperature: 0.7,
    max_tokens: Some(256),
};

let client = OpenAIClient::new(&config)?;
let response = client.complete(LLMRequest {
    system: Some("You are a helpful assistant.".into()),
    messages: vec![ChatMessage::user("Hello!")],
    temperature: 0.7,
    max_tokens: Some(256),
    model: None,
}).await?;
println!("{}", response.content);
```

## 4. Knowledge graph

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["graph"] }
```

```rust
use llm_kernel::prelude::*;

let conn = rusqlite::Connection::open_in_memory().unwrap();
init_graph_schema(&conn).unwrap();

upsert_node(&conn, &GraphNode {
    id: "rust".into(),
    node_type: "language".into(),
    title: "Rust Programming Language".into(),
    body: "A systems programming language focused on safety...".into(),
    tags: vec!["rust".into(), "systems".into()],
    projects: vec!["my-project".into()],
    agents: vec![],
    created: "2026-01-01T00:00:00Z".into(),
    updated: "2026-01-01T00:00:00Z".into(),
    importance: 0.8,
    access_count: 0,
    accessed_at: String::new(),
}).unwrap();

// Smart recall with composite scoring
let results = smart_recall(&conn, Some("my-project"), Some("rust"), 5).unwrap();
for scored in &results {
    println!("{:.2} — {}", scored.score, scored.node.title);
}
```

## 5. Local embedding (no API key)

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["embedding-fastembed"] }
```

```rust
use llm_kernel::embedding::{EmbeddingModel, FastembedProvider, EmbeddingProvider};

let provider = FastembedProvider::new(EmbeddingModel::BGESmallENV15, None)?;
let result = provider.embed("hello world")?;
println!("Dimension: {}", result.vector.len());

// Cosine similarity
use llm_kernel::embedding::cosine_similarity;
let sim = cosine_similarity(&result.vector, &[0.1; 384]);
```

## Verify

```bash
cargo test --all-features
cargo clippy --all-features -- -D warnings
```

## Next Steps

- [README](README.md) — full feature flag reference and API overview
- [EMBEDDING_MODELS.md](EMBEDDING_MODELS.md) — 44 local embedding model catalog
- [CONTRIBUTING.md](CONTRIBUTING.md) — development environment setup
- [examples/](examples/) — provider_list, chat_openai, stream_anthropic
- [docs.rs/llm-kernel](https://docs.rs/llm-kernel) — full API documentation
