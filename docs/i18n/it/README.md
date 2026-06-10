<!-- Translated from README.md @ commit edd4827 (2026-06-06) -->
<!-- If English README has changed since then, this translation may be outdated -->

[English](../../README.md) | [한국어](../ko/README.md) | [日本語](../ja/README.md) | [简体中文](../zh-Hans/README.md) | [繁體中文](../zh-Hant/README.md) | [Español](../es/README.md) | [Français](../fr/README.md) | [Deutsch](../de/README.md) | [Português](../pt/README.md) | [Русский](../ru/README.md) | **Italiano**

> Questo documento è una traduzione di [README.md](../../README.md).
> La versione inglese è la fonte autorevole e potrebbe essere più aggiornata.

<div align="center">

# llm-kernel

> Libreria di base per applicazioni Rust native AI — catalogo provider, client LLM, server MCP, ricerca, telemetria e sicurezza

[![CI](https://img.shields.io/github/actions/workflow/status/epicsagas/llm-kernel/ci.yml?style=for-the-badge&labelColor=0d1117&color=2ecc71&logo=github-actions&logoColor=white)](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-kernel?style=for-the-badge&labelColor=0d1117&color=fc8d62&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)
[![docs.rs](https://img.shields.io/docsrs/llm-kernel?style=for-the-badge&labelColor=0d1117&color=58a6ff&logo=docs.rs&logoColor=white)](https://docs.rs/llm-kernel)
[![License](https://img.shields.io/badge/license-Apache--2.0-3fb950?style=for-the-badge&labelColor=0d1117)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/llm-kernel?style=for-the-badge&labelColor=0d1117&color=bc8cff&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)

</div>

## Panoramica

llm-kernel fornisce il livello fondamentale per costruire strumenti, agent e server basati su LLM in Rust:

- **Catalogo provider** — 16 provider integrati, 114 modelli con metadati, prezzi e capacità
- **Client asincrono** — client basato su trait per OpenAI e Anthropic con streaming SSE
- **Scoperta modelli** — scoperta dinamica dei modelli da models.dev, Ollama, endpoint compatibili OpenAI
- **Vault delle credenziali** — gestione delle chiavi API in stile dotenv con scritture atomiche
- **Caricamento configurazione** — configurazione TOML con creazione automatica da template
- **Grafo di conoscenza** — grafo basato su SQLite con ricerca FTS5, richiamo intelligente, attraversamento BFS, wrapper asincroni
- **Server MCP** — framework server JSON-RPC 2.0 con trasporto stdio e autenticazione Bearer
- **Embedding** — trait provider + similarità coseno, ONNX locale (44 modelli), Qwen3 candle, Nomic V2 MoE candle, OpenAI remoto ([elenco completo modelli →](EMBEDDING_MODELS.md))
- **Ricerca** — Reciprocal Rank Fusion per la fusione ibrida dei risultati di ricerca
- **Stima token** — conteggio euristico dei token basato su script Unicode senza dipendenze
- **Telemetria** — eventi con gating enum senza PII, sink console e noop
- **Sicurezza** — mascheramento segreti, classificazione errori, sanificazione output
- **Wizard di installazione** — generazione configurazione MCP per Claude Desktop, Cursor, Copilot, OpenCode, Cline

## Flag di feature

Ogni modulo è protetto da una flag di feature, così paghi solo per ciò che utilizzi.

| Feature | Descrizione | Predefinito |
|---------|-------------|-------------|
| `provider` | Catalogo provider, descrittori modelli, prezzi | ✅ |
| `client-async` | Client LLM asincrono (reqwest) con streaming | |
| `discovery` | Scoperta dinamica modelli (models.dev, Ollama, OpenAI-compat) | |
| `secrets` | Gestione credenziali SecretVault | |
| `store` | Helper init SQLite (WAL, FTS5, versionamento schema) | |
| `config` | Caricatore configurazione TOML | |
| `graph` | Grafo di conoscenza — SQLite, FTS5, richiamo intelligente, attraversamento BFS | |
| `graph-async` | Wrapper grafo asincroni (richiede tokio) | |
| `graph-pool` | Pool grafo asincrono multi-connessione (`AsyncPoolGraph`, concorrenza WAL) | |
| `mcp` | Server MCP — JSON-RPC 2.0, trasporto stdio, autenticazione Bearer | |
| `tokens` | Stima token con euristiche Unicode-script | |
| `install` | Wizard di installazione strumenti AI | |
| `search` | Ricerca ibrida con Reciprocal Rank Fusion | |
| `embedding` | Trait provider embedding + similarità coseno | |
| `embedding-openai` | Client OpenAI text-embedding (HTTP sincrono) | |
| `embedding-fastembed` | Embedding ONNX locale via fastembed-rs (44 modelli) | |
| `embedding-fastembed-qwen3` | Embedding Qwen3 via backend candle | |
| `embedding-fastembed-nomic-moe` | Embedding Nomic V2 MoE via backend candle | |
| `vector-index` | Indice vettoriale compresso TurboQuant — 2 bit/4 bit, ricerca ANN con SIMD | |
| `telemetry` | Eventi di telemetria con gating enum, senza PII | |
| `safety` | Mascheramento segreti, classificazione errori, sanificazione output | |
| `eval` | CLI di valutazione qualità — token, sicurezza, embedding, ricerca | |
| `eval-full` | Tutti i moduli di valutazione incluso il grafo | |
| `full` | Tutte le feature | |

## Guida rapida

Aggiungi al tuo `Cargo.toml`:

```toml
[dependencies]
llm-kernel = "0.3.4"
```

La feature `provider` è abilitata per impostazione predefinita. Per il client asincrono:

```toml
[dependencies]
llm-kernel = { version = "0.3.4", features = ["client-async"] }
```

Per il grafo di conoscenza con wrapper asincroni:

```toml
[dependencies]
llm-kernel = { version = "0.3.4", features = ["graph", "graph-async"] }
```

Per l'embedding locale (ONNX, nessuna chiave API):

```toml
[dependencies]
llm-kernel = { version = "0.3.4", features = ["embedding-fastembed"] }
```

## Utilizzo

### Catalogo provider

Il catalogo integrato contiene 16 provider con 114 modelli allineati allo schema [models.dev](https://github.com/anomalyco/models.dev).

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

### Completamento chat asincrono

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

### Scoperta modelli

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

### Vault delle credenziali

```rust
use llm_kernel::prelude::*;

let vault = SecretVault::load_from("~/.config/myapp/.env")?;
vault.set("OPENAI_API_KEY", "sk-...");
vault.save_to("~/.config/myapp/.env")?;

// Redact credentials for logging
println!("{}", redact_credential("sk-abcdef1234567890"));
// → "sk-abcd...7890"
```

### Configurazione TOML

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

### Store SQLite

```rust
use llm_kernel::store::init_schema;

let ddl = "CREATE TABLE items (id TEXT PRIMARY KEY, content TEXT);";
let conn = init_schema(&db_path, ddl, 1)?;
// WAL mode, busy timeout, and schema versioning applied automatically
```

### Grafo di conoscenza

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

### Server MCP

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

### Stima dei token

```rust
use llm_kernel::tokens::estimate_tokens;

let tokens = estimate_tokens("Hello, world! こんにちは世界 🌍");
println!("Estimated tokens: {}", tokens);
```

### Embedding e ricerca

```rust
use llm_kernel::embedding::{EmbeddingProvider, cosine_similarity};
use llm_kernel::search::{SearchResult, rrf_fuse};

// Cosine similarity between vectors
let sim = cosine_similarity(&[0.1, 0.2, 0.3], &[0.4, 0.5, 0.6]);

// Reciprocal Rank Fusion for hybrid search
let bm25 = vec![
    SearchResult { id: "doc-a".into(), score: 0.9, text: "Rust guide".into() },
    SearchResult { id: "doc-b".into(), score: 0.7, text: "Python basics".into() },
];
let vector = vec![
    SearchResult { id: "doc-b".into(), score: 0.95, text: "Python basics".into() },
    SearchResult { id: "doc-c".into(), score: 0.6, text: "Go concurrency".into() },
];
let merged = rrf_fuse(&[bm25, vector], 60);
```

#### Embedding ONNX locale (fastembed-rs)

44 modelli via ONNX Runtime — nessuna chiave API, nessuna connessione di rete dopo il primo download.

```rust
use llm_kernel::embedding::{EmbeddingModel, FastembedProvider, EmbeddingProvider};

let provider = FastembedProvider::new(EmbeddingModel::BGESmallENV15, None)?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 384);
```

#### Embedding Qwen3 (candle)

Inferenza pura Rust GPU/CPU via candle-nn — senza ONNX Runtime.

```rust
use llm_kernel::embedding::{Qwen3Provider, EmbeddingProvider};

let provider = Qwen3Provider::new("Qwen/Qwen3-Embedding-0.6B")?;
let result = provider.embed("hello world")?;
```

#### Embedding Nomic V2 MoE (candle)

Modello MoE leggero — 8 esperti, routing top-2, 305M parametri attivi.

```rust
use llm_kernel::embedding::{NomicMoeProvider, EmbeddingProvider};

let provider = NomicMoeProvider::new()?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 768);
```

### Utilità di sicurezza

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

## Metadati dei modelli

Ogni modello nel catalogo include:

| Campo | Descrizione |
|-------|-------------|
| `cost` | Prezzi per milione di token (input, output, cache_read, cache_write) |
| `limit` | Limiti di token per contesto e output |
| `modalities` | Modalità di input/output (testo, immagine, audio) |
| `capabilities` | Flag: attachment, reasoning, temperature, tool_call, streaming |
| `knowledge` | Data di cutoff dei dati di addestramento |

## Perché llm-kernel?

| | llm-kernel | [rig] | [langchain-rust] |
|--|-----------|-------|-------------------|
| Catalogo provider | ✅ 16 provider, 114 modelli integrati | Configurazione manuale | Configurazione manuale |
| Feature gate | ✅ 20 moduli indipendenti | Monolitico | Monolitico |
| Embedding locale | ✅ 44 ONNX + Qwen3 + Nomic MoE | ❌ | ❌ |
| Valutazione qualità | ✅ 5 moduli, regressione baseline, CI | ❌ | ❌ |
| Server MCP | ✅ JSON-RPC 2.0 | ❌ | ❌ |
| Grafo di conoscenza | ✅ SQLite + FTS5 + richiamo intelligente | ❌ | ❌ |
| Dipendenze obbligatorie | Solo `serde` | `reqwest`, `tokio`, … | Molte |
| Catene / agent | ❌ | ✅ | ✅ |
| Pipeline RAG | ❌ | ✅ | ✅ |

[rig]: https://github.com/0xPlaygrounds/rig
[langchain-rust]: https://github.com/Abraxas-365/langchain-rust

llm-kernel è uno **strato fondamentale leggero** — componilo con rig o langchain-rust quando hai bisogno di catene, agent o RAG.

## Architettura

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

- **Trait `LLMClient`** — interfaccia unificata per `OpenAIClient` e `AnthropicClient`
- **Trait `EmbeddingProvider`** — interfaccia unificata per `FastembedProvider` (ONNX), `Qwen3Provider` (candle), `NomicMoeProvider` (candle), `OpenAIEmbeddingClient` (remoto)
- **`ProviderIndex`** — accesso zero-copy al catalogo integrato, interrogabile per provider o modello
- **`McpServer`** — server JSON-RPC 2.0 con trasporto stdio, autenticazione Bearer, registrazione tool
- **`SecretVault`** — `HashMap<String, String>` con caricamento/salvataggio dotenv e guardie symlink
- **`graph`** — grafo di conoscenza SQLite con ricerca FTS5, richiamo con punteggio composito, attraversamento BFS, decadimento dell'importanza
- **`TelemetryEvent`** — varianti con gating enum per osservabilità strutturata (senza PII)
- **`safety`** — mascheramento segreti, classificazione errori, sanificazione bidi/ANSI/null

## Valutazione qualità

Il CLI di valutazione integrato misura la qualità dei moduli rispetto a dataset di test curati:

```bash
# Eseguire tutte le valutazioni (token, sicurezza, embedding, ricerca)
cargo run --bin llm-kernel-eval --features eval -- all

# Includere la valutazione del grafo
cargo run --bin llm-kernel-eval --features eval-full -- all

# Verifica di regressione rispetto allo snapshot baseline (uscita con codice 1 in caso di regressione)
cargo run --bin llm-kernel-eval --features eval-full -- --baseline eval/baseline.json all

# Output JSON per strumenti
cargo run --bin llm-kernel-eval --features eval -- --format json all
```

| Modulo | Metriche |
|--------|----------|
| tokens | MAE, max_error, %±3, %±10%, analisi per categoria |
| safety | exact_match_rate, precision, recall, F1, missed_secrets |
| embedding | identity_accuracy, orthogonality, symmetry, bounds |
| search | precision@5, recall@5, MRR |
| graph | precision, recall, F1 per tipo di query |

Passare `--baseline eval/baseline.json` per confrontare con uno snapshot di riferimento — il CLI esce con codice 1 in caso di regressione di qualsiasi metrica. Il CI esegue questo automaticamente ad ogni push e PR tramite il job `eval`.

## Benchmark

Benchmark Criterion nella directory `benches/`:

```bash
cargo bench                          # Run all benchmarks
cargo bench -- graph_bench           # Graph: smart_recall, BFS, neighbors
cargo bench -- compute_bench         # Token estimation, RRF fusion
```

## Esempi

```bash
# List all providers and models (no API key needed)
cargo run --example provider_list

# OpenAI chat (requires OPENAI_API_KEY)
cargo run --example chat_openai --features client-async

# Anthropic streaming (requires ANTHROPIC_API_KEY)
cargo run --example stream_anthropic --features client-async
```

## Requisiti

- Rust 1.92+ (edition 2024)

## Contribuire

Consulta [CONTRIBUTING.md](CONTRIBUTING.md). Le PR sono benvenute.

## Licenza

[Apache-2.0](LICENSE) © 2026 EpicCounty
