<div align="center">

# ec-kernel

> Shared infrastructure for Rust knowledge tools

[![CI](https://github.com/epicsagas/ec-kernel/actions/workflows/ci.yml/badge.svg)](https://github.com/epicsagas/ec-kernel/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

</div>

## What is this?

ec-kernel is a shared library crate providing common infrastructure for the [EpicCounty](https://github.com/epicsagas) Knowledge Creation Platform product suite:

- **fmemory** — Founder decision tracker
- **research-agent** — Personal research assistant
- **velith-engine** — Knowledge → output engine
- **knowledge-forge** — Unified knowledge platform

## Features

| | Feature | Why it matters |
|--|---------|----------------|
| 🤖 | LLM clients (OpenAI + Anthropic) | One trait, two providers — swap without rewriting |
| 🗄️ | SQLite helpers | WAL mode, FTS5, schema versioning out of the box |
| ⚙️ | TOML config loader | Auto-creates default config if missing |
| 🔌 | Prompt templates | Simple `{{variable}}` substitution |
| 🛡️ | Error types | thiserror-based, no stringly-typed errors |

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
ec-kernel = "0.1"
```

### LLM Client

```rust
use ec_kernel::llm::{LLMClient, OpenAIClient, ModelConfig, LLMRequest, ChatMessage};

let config = ModelConfig::default();
let client = OpenAIClient::new(&config)?;
let response = client.complete(LLMRequest {
    system: Some("You are a helpful assistant.".into()),
    messages: vec![ChatMessage::user("Hello!")],
    temperature: 0.7,
    max_tokens: Some(1024),
    model: None,
}).await?;
println!("{}", response.content);
```

### SQLite Store

```rust
use ec_kernel::store::init_schema;

let ddl = "CREATE TABLE items (id TEXT PRIMARY KEY);";
let conn = init_schema(&path, ddl, 1)?;
```

### Config Loader

```rust
use ec_kernel::config::load_toml_config;

let config: MyConfig = load_toml_config(path, Some("default content"))?;
```

## Requirements

- Rust 1.92+
- No runtime dependencies (statically linked)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). PRs welcome.

## License

[Apache-2.0](LICENSE) © 2025 EpicCounty
