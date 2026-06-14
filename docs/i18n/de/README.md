<!-- Translated from README.md @ commit edd4827 (2026-06-06) -->
<!-- If English README has changed since then, this translation may be outdated -->

[English](../../README.md) | [한국어](../ko/README.md) | [日本語](../ja/README.md) | [简体中文](../zh-Hans/README.md) | [繁體中文](../zh-Hant/README.md) | [Español](../es/README.md) | [Français](../fr/README.md) | **Deutsch** | [Português](../pt/README.md) | [Русский](../ru/README.md) | [Italiano](../it/README.md)

> Dieses Dokument ist eine Übersetzung von [README.md](../../README.md).
> Die englische Version ist maßgeblich und kann aktueller sein.

<div align="center">

# llm-kernel

> Grundlagenbibliothek für KI-native Rust-Anwendungen — Provider-Katalog, LLM-Client, MCP-Server, Suche, Telemetrie und Sicherheit

[![CI](https://img.shields.io/github/actions/workflow/status/epicsagas/llm-kernel/ci.yml?style=for-the-badge&labelColor=0d1117&color=2ecc71&logo=github-actions&logoColor=white)](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-kernel?style=for-the-badge&labelColor=0d1117&color=fc8d62&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)
[![docs.rs](https://img.shields.io/docsrs/llm-kernel?style=for-the-badge&labelColor=0d1117&color=58a6ff&logo=docs.rs&logoColor=white)](https://docs.rs/llm-kernel)
[![License](https://img.shields.io/badge/license-Apache--2.0-3fb950?style=for-the-badge&labelColor=0d1117)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/llm-kernel?style=for-the-badge&labelColor=0d1117&color=bc8cff&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)

</div>

## Übersicht

llm-kernel stellt die Grundschicht für den Aufbau von LLM-basierten Werkzeugen, Agenten und Servern in Rust bereit:

- **Provider-Katalog** — 16 integrierte Provider, 114 Modelle mit Metadaten, Preisen und Fähigkeiten
- **Async-Client** — Trait-basierter Client für OpenAI und Anthropic mit SSE-Streaming
- **Modellsuche** — dynamische Modellermittlung über models.dev, Ollama, OpenAI-kompatible Endpunkte
- **Anmeldeinformations-Tresor** — dotenv-artige API-Schlüsselverwaltung mit atomaren Schreibvorgängen
- **Konfigurationslader** — TOML-Konfiguration mit automatischer Erstellung aus Vorlage
- **Wissensgraph** — SQLite-basierter Graph mit FTS5-Suche, intelligentem Recall, BFS-Traversierung, Async-Wrappern
- **MCP-Server** — JSON-RPC-2.0-Server-Framework mit Stdio-Transport und Bearer-Auth
- **Embedding** — Provider-Trait + Kosinusähnlichkeit, lokales ONNX (44 Modelle), Qwen3 Candle, Nomic V2 MoE Candle, OpenAI remote ([vollständige Modellliste →](EMBEDDING_MODELS.md))
- **Suche** — Reciprocal Rank Fusion zum Zusammenführen hybrider Suchergebnisse
- **Token-Schätzung** — null-Abhängigkeits-Unicode-Skript-heuristische Token-Zählung
- **Telemetrie** — Enum-gesteuerte Ereignisse ohne PII, Console- und Noop-Sinks
- **Sicherheit** — Geheimnismaskierung, Fehlerklassifizierung, Ausgabebereinigung
- **Installationsassistent** — MCP-Konfigurationserstellung für Claude Desktop, Cursor, Copilot, OpenCode, Cline

## Feature-Flags

Jedes Modul wird durch ein Feature-Flag gesteuert, sodass Sie nur bezahlen, was Sie verwenden.

| Feature | Beschreibung | Standard |
|---------|-------------|----------|
| `provider` | Provider-Katalog, Modellbeschreibungen, Preise | ✅ |
| `client-async` | Async LLM-Client (reqwest) mit Streaming | |
| `discovery` | Dynamische Modellermittlung (models.dev, Ollama, OpenAI-kompatibel) | |
| `discovery-async` | Asynchrone Modellermittlung — `DiscoverySource`-Trait über reqwest | |
| `secrets` | SecretVault-Anmeldeinformationsverwaltung | |
| `store` | SQLite-Initialisierungshilfen (WAL, FTS5, Schema-Versionierung) | |
| `config` | TOML-Konfigurationslader | |
| `graph` | Wissensgraph — SQLite, FTS5, intelligenter Recall, BFS-Traversierung | |
| `graph-async` | Async-Graph-Wrapper (erfordert tokio) | |
| `graph-pool` | Multi-Verbindungs-Async-Graph-Pool (`AsyncPoolGraph`, WAL-Konkurrenz) | |
| `graph-cjk` | CJK-aware graph search via Rust-side segmentation (no schema change) | |
| `mcp` | MCP-Server — JSON-RPC 2.0, Stdio-Transport, Bearer-Auth | |
| `mcp-http` | MCP remote transport — HTTP/SSE (axum + tokio) | |
| `cache` | LLM response cache — `CacheClient` over `KvStore` | |
| `tokens` | Token-Schätzung, Budgetierung und satzgrenzenbasiertes Dokument-Chunking | |
| `install` | KI-Werkzeug-Installationsassistent | |
| `search` | Hybride Suche — `SearchProvider`-Trait, RRF / gewichtete Summe / CombMNZ-Fusion | |
| `embedding` | Embedding-Provider-Trait + Kosinusähnlichkeit | |
| `embedding-openai` | OpenAI-Text-Embedding-Client (sync HTTP) | |
| `embedding-fastembed` | Lokales ONNX-Embedding über fastembed-rs (44 Modelle) | |
| `embedding-fastembed-qwen3` | Qwen3-Embedding über Candle-Backend | |
| `embedding-fastembed-nomic-moe` | Nomic V2 MoE-Embedding über Candle-Backend | |
| `vector-index` | TurboQuant-komprimierter Vektorindex — 2-Bit/4-Bit, SIMD-ANN-Suche | |
| `telemetry` | Enum-gesteuerte Telemetrie-Ereignisse, keine PII | |
| `safety` | Geheimnismaskierung, Fehlerklassifizierung, Ausgabebereinigung, Prompt-Injection-Erkennung | |
| `eval` | Qualitätsbewertungs-CLI — Tokens, Sicherheit, Embedding, Suche | |
| `eval-full` | Alle Evaluationsmodule einschließlich Graph | |
| `full` | Alle Features | |

## Schnellstart

Zu Ihrer `Cargo.toml` hinzufügen:

```toml
[dependencies]
llm-kernel = "0.7.0"
```

Das `provider`-Feature ist standardmäßig aktiviert. Für den Async-Client:

```toml
[dependencies]
llm-kernel = { version = "0.7.0", features = ["client-async"] }
```

Für den Wissensgraphen mit Async-Wrappern:

```toml
[dependencies]
llm-kernel = { version = "0.7.0", features = ["graph", "graph-async"] }
```

Für lokales Embedding (ONNX, kein API-Schlüssel):

```toml
[dependencies]
llm-kernel = { version = "0.7.0", features = ["embedding-fastembed"] }
```

## Verwendung

### Provider-Katalog

Der eingebettete Katalog enthält 16 Provider mit 114 Modellen gemäß dem [models.dev](https://github.com/anomalyco/models.dev)-Schema.

```rust
use llm_kernel::prelude::*;

let catalog = ProviderIndex::embedded();

// Alle Provider auflisten
for id in catalog.ids() {
    let provider = catalog.get(&id).unwrap();
    println!("{}", provider.display_name);
}

// Modelle für einen Provider abfragen
for model in catalog.models_for("openai") {
    println!("  {} — ${:.2}/1M in", model.id, model.cost.unwrap().input);
}

// Ein bestimmtes Modell finden
if let Some(model) = catalog.find_model("claude-sonnet-4-20250514") {
    println!("Context: {} tokens", model.limit.unwrap().context);
}
```

### Asynchrone Chat-Vervollständigung

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

// Stream liefert Delta-, Usage- und Done-Ereignisse
```

### Modellermittlung

```rust
use llm_kernel::discovery::{fetch_and_cache, load_cache, fetch_ollama_models};

// Von models.dev abrufen (wird auf Festplatte zwischengespeichert)
let payload = fetch_and_cache("~/.cache/llm-kernel/models-dev.json")?;
for model in &payload.models {
    println!("{} — {} (ctx: {:?})", model.id, model.provider_id, model.limits);
}

// Aus Zwischenspeicher laden (kein Netzwerk)
if let Some(cached) = load_cache("~/.cache/llm-kernel/models-dev.json")? {
    println!("{} models cached", cached.models.len());
}

// Lokale Ollama-Modelle ermitteln
let ollama_models = fetch_ollama_models("http://localhost:11434")?;
for name in &ollama_models {
    println!("Ollama: {}", name);
}
```

### Anmeldeinformations-Tresor

```rust
use llm_kernel::prelude::*;

let vault = SecretVault::load_from("~/.config/myapp/.env")?;
vault.set("OPENAI_API_KEY", "sk-...");
vault.save_to("~/.config/myapp/.env")?;

// Anmeldeinformationen für Protokollierung redigieren
println!("{}", redact_credential("sk-abcdef1234567890"));
// → "sk-abcd...7890"
```

### TOML-Konfiguration

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

### SQLite-Speicher

```rust
use llm_kernel::store::init_schema;

let ddl = "CREATE TABLE items (id TEXT PRIMARY KEY, content TEXT);";
let conn = init_schema(&db_path, ddl, 1)?;
// WAL-Modus, Busy-Timeout und Schema-Versionierung werden automatisch angewendet
```

### Wissensgraph

```rust
use llm_kernel::prelude::*;
use rusqlite::Connection;

let conn = Connection::open_in_memory().unwrap();
init_graph_schema(&conn).unwrap();

// Knoten erstellen
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

// Mit Kanten verbinden
append_edge(&conn, &GraphEdge {
    id: "e1".into(),
    source: "rust-ownership".into(),
    target: "borrow-checker".into(),
    relation: "related".into(),
    weight: 1.5,
    ts: "2026-01-01T00:00:00Z".into(),
}).unwrap();

// Intelligenter Recall mit zusammengesetzter Bewertung
let results = smart_recall(&conn, Some("my-project"), Some("ownership"), 5).unwrap();
for scored in &results {
    println!("{:.2} — {}", scored.score, scored.node.title);
}

// Lebenszyklusverwaltung
decay_importance(&conn, 30, 0.9, 0.05).unwrap();
tag_stale_nodes(&conn, 90).unwrap();
let stats = compute_stats(&conn).unwrap();
println!("{} nodes, {} edges", stats.total_nodes, stats.total_edges);
```

### MCP-Server

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

// Führt JSON-RPC 2.0 über Stdio mit Bearer-Auth aus
server.run_stdio().await?;
```

### Token-Schätzung

```rust
use llm_kernel::tokens::estimate_tokens;

let tokens = estimate_tokens("Hello, world! こんにちは世界 🌍");
println!("Estimated tokens: {}", tokens);
```

### Embedding + Suche

```rust
use llm_kernel::embedding::{EmbeddingProvider, cosine_similarity};
use llm_kernel::search::{SearchResult, rrf_fuse};

// Kosinusähnlichkeit zwischen Vektoren
let sim = cosine_similarity(&[0.1, 0.2, 0.3], &[0.4, 0.5, 0.6]);

// Reciprocal Rank Fusion für hybride Suche
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

#### Lokales ONNX-Embedding (fastembed-rs)

44 Modelle über ONNX Runtime — kein API-Schlüssel, kein Netzwerk nach dem ersten Download.

```rust
use llm_kernel::embedding::{EmbeddingModel, FastembedProvider, EmbeddingProvider};

let provider = FastembedProvider::new(EmbeddingModel::BGESmallENV15, None)?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 384);
```

#### Qwen3-Embedding (Candle)

Reines Rust GPU/CPU-Inferenz über candle-nn — keine ONNX Runtime.

```rust
use llm_kernel::embedding::{Qwen3Provider, EmbeddingProvider};

let provider = Qwen3Provider::new("Qwen/Qwen3-Embedding-0.6B")?;
let result = provider.embed("hello world")?;
```

#### Nomic V2 MoE-Embedding (Candle)

Leichtgewichtiges MoE-Modell — 8 Experten, Top-2-Routing, 305M aktive Parameter.

```rust
use llm_kernel::embedding::{NomicMoeProvider, EmbeddingProvider};

let provider = NomicMoeProvider::new()?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 768);
```

### Sicherheitswerkzeuge

```rust
use llm_kernel::safety::{mask_secrets, classify_failure, sanitize_output};

// Geheimnisse in Protokollen maskieren
let safe = mask_secrets("Authorization: Bearer sk-abcdef123456");
// → "Authorization: Bearer [REDACTED]"

// Fehler klassifizieren
let category = classify_failure("connection timed out after 30s");
// → ErrorCategory::Timeout

// Nicht vertrauenswürdige Ausgabe bereinigen
let clean = sanitize_output(user_input)?;
```

## Modellmetadaten

Jedes Modell im Katalog enthält:

| Feld | Beschreibung |
|------|-------------|
| `cost` | Preis pro Million Token (Eingabe, Ausgabe, cache_read, cache_write) |
| `limit` | Kontext- und Ausgabe-Token-Limits |
| `modalities` | Eingabe-/Ausgabemodalitäten (Text, Bild, Audio) |
| `capabilities` | Flags: Anhang, Schlussfolgerung, Temperatur, Werkzeugaufruf, Streaming |
| `knowledge` | Trainingsdaten-Stichtag |

## Warum llm-kernel?

| | llm-kernel | [rig] | [langchain-rust] |
|--|-----------|-------|-------------------|
| Provider-Katalog | ✅ 16 Provider, 114 Modelle integriert | Manuelle Konfiguration | Manuelle Konfiguration |
| Feature-Gates | ✅ Unabhängige Module | Monolithisch | Monolithisch |
| Lokales Embedding | ✅ 44 ONNX + Qwen3 + Nomic MoE | ❌ | ❌ |
| Qualitätsbewertung | ✅ 5 Module, Baseline-Regression, CI | ❌ | ❌ |
| MCP-Server | ✅ JSON-RPC 2.0 | ❌ | ❌ |
| Wissensgraph | ✅ SQLite + FTS5 + intelligenter Recall | ❌ | ❌ |
| Pflichtabhängigkeiten | Nur `serde` | `reqwest`, `tokio`, … | Viele |
| Ketten / Agenten | ❌ | ✅ | ✅ |
| RAG-Pipelines | ❌ | ✅ | ✅ |

[rig]: https://github.com/0xPlaygrounds/rig
[langchain-rust]: https://github.com/Abraxas-365/langchain-rust

llm-kernel ist eine **leichtgewichtige Grundlagenschicht** — kombinieren Sie es mit rig oder langchain-rust, wenn Sie Ketten, Agenten oder RAG benötigen.

## Architektur

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

- **`LLMClient`-Trait** — einheitliche Schnittstelle für `OpenAIClient` und `AnthropicClient`
- **`EmbeddingProvider`-Trait** — einheitliche Schnittstelle für `FastembedProvider` (ONNX), `Qwen3Provider` (Candle), `NomicMoeProvider` (Candle), `OpenAIEmbeddingClient` (remote)
- **`ProviderIndex`** — Zero-Copy-Zugriff auf eingebetteten Katalog, abfragbar nach Provider oder Modell
- **`McpServer`** — JSON-RPC-2.0-Server mit Stdio-Transport, Bearer-Auth, Werkzeugregistrierung
- **`SecretVault`** — `HashMap<String, String>` mit dotenv-Laden/Speichern und Symlink-Schutz
- **`graph`** — SQLite-Wissensgraph mit FTS5-Suche, zusammengesetzter Bewertungs-Recall, BFS-Traversierung, Wichtigkeitsverfall
- **`TelemetryEvent`** — Enum-gesteuerte Varianten für strukturierte Observabilität (keine PII)
- **`safety`** — Geheimnismaskierung, Fehlerklassifizierung, bidi/ANSI/null-Bereinigung

## Qualitätsbewertung

Die integrierte Evaluations-CLI misst die Modulqualität anhand kuratierter Testdatensätze:

```bash
# Alle Evaluationen ausführen (Tokens, Sicherheit, Embedding, Suche)
cargo run --bin llm-kernel-eval --features eval -- all

# Graph-Evaluation einschließen
cargo run --bin llm-kernel-eval --features eval-full -- all

# Regressionsprüfung gegen Baseline-Snapshot (Exit-Code 1 bei Regression)
cargo run --bin llm-kernel-eval --features eval-full -- --baseline eval/baseline.json all

# JSON-Ausgabe für Werkzeuge
cargo run --bin llm-kernel-eval --features eval -- --format json all
```

| Modul | Metriken |
|-------|----------|
| tokens | MAE, max_error, %±3, %±10%, Aufschlüsselung nach Kategorie |
| safety | exact_match_rate, precision, recall, F1, missed_secrets |
| embedding | identity_accuracy, orthogonality, symmetry, bounds |
| search | precision@5, recall@5, MRR |
| graph | precision, recall, F1 nach Abfragetyp |

Übergeben Sie `--baseline eval/baseline.json`, um mit einem Golden-Snapshot zu vergleichen — die CLI beendet sich mit Code 1 bei jeder Metrik-Regression. CI führt dies automatisch bei jedem Push und PR über den `eval`-Job aus.

## Benchmarks

Criterion-Benchmarks unter `benches/`:

```bash
cargo bench                          # Alle Benchmarks ausführen
cargo bench -- graph_bench           # Graph: smart_recall, BFS, Nachbarn
cargo bench -- compute_bench         # Token-Schätzung, RRF-Fusion
```

## Beispiele

```bash
# Alle Provider und Modelle auflisten (kein API-Schlüssel erforderlich)
cargo run --example provider_list

# OpenAI-Chat (erfordert OPENAI_API_KEY)
cargo run --example chat_openai --features client-async

# Anthropic-Streaming (erfordert ANTHROPIC_API_KEY)
cargo run --example stream_anthropic --features client-async
```

## Voraussetzungen

- Rust 1.92+ (Edition 2024)

## Mitwirken

Siehe [CONTRIBUTING.md](CONTRIBUTING.md). PRs willkommen.

## Lizenz

[Apache-2.0](LICENSE) © 2026 EpicCounty
