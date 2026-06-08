<!-- Translated from README.md @ commit edd4827 (2026-06-06) -->
<!-- If English README has changed since then, this translation may be outdated -->

[English](../../README.md) | [한국어](../ko/README.md) | [日本語](../ja/README.md) | [简体中文](../zh-Hans/README.md) | [繁體中文](../zh-Hant/README.md) | [Español](../es/README.md) | **Français** | [Deutsch](../de/README.md) | [Português](../pt/README.md) | [Русский](../ru/README.md) | [Italiano](../it/README.md)

> Ce document est une traduction de [README.md](../../README.md).
> La version anglaise fait autorité et peut être plus à jour.

<div align="center">

# llm-kernel

> Bibliotheque fondatrice pour les applications Rust natives IA -- catalogue de fournisseurs, client LLM, serveur MCP, recherche, telemetrie et securite

[![CI](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml/badge.svg)](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-kernel)](https://crates.io/crates/llm-kernel)
[![docs.rs](https://docs.rs/llm-kernel/badge.svg)](https://docs.rs/llm-kernel)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

</div>

## Apercu

llm-kernel fournit la couche fondatrice pour construire des outils, agents et serveurs propulses par des LLM en Rust :

- **Catalogue de fournisseurs** -- 16 fournisseurs integres, 114 modeles avec metadonnees, tarification et capacites
- **Client asynchrone** -- client base sur des traits pour OpenAI et Anthropic avec streaming SSE
- **Decouverte de modeles** -- decouverte dynamique depuis models.dev, Ollama et points de terminaison compatibles OpenAI
- **Coffre de credentials** -- gestion des cles API style dotenv avec ecritures atomiques
- **Chargeur de config** -- configuration TOML avec creation automatique depuis un modele
- **Graphe de connaissances** -- graphe sur SQLite avec recherche FTS5, rappel intelligent, parcours BFS et enveloppes asynchrones
- **Serveur MCP** -- framework de serveur JSON-RPC 2.0 avec transport stdio et authentification Bearer
- **Embedding** -- trait de fournisseur + similarite cosinus, ONNX local (44 modeles), Qwen3 candle, Nomic V2 MoE candle, OpenAI distant ([liste complete des modeles ->](../../EMBEDDING_MODELS.md))
- **Recherche** -- Reciprocal Rank Fusion pour la fusion de resultats de recherche hybride
- **Estimation de tokens** -- comptage heuristique de tokens par script Unicode sans dependance
- **Telemetrie** -- evenements gates par enum sans PII, puits console et noop
- **Securite** -- masquage de secrets, classification d'erreurs, nettoyage de sorties
- **Assistant d'installation** -- generation de config MCP pour Claude Desktop, Cursor, Copilot, OpenCode, Cline

## Indicateurs de fonctionnalite

Chaque module est derriere un indicateur de fonctionnalite afin que vous ne payiez que ce que vous utilisez.

| Fonctionnalite | Description | Par defaut |
|----------------|-------------|------------|
| `provider` | Catalogue de fournisseurs, descripteurs de modeles, tarification | Oui |
| `client-async` | Client LLM asynchrone (reqwest) avec streaming | |
| `discovery` | Decouverte dynamique de modeles (models.dev, Ollama, OpenAI-compat) | |
| `secrets` | Gestion de credentials SecretVault | |
| `store` | Aides d'initialisation SQLite (WAL, FTS5, versionnage de schema) | |
| `config` | Chargeur de configuration TOML | |
| `graph` | Graphe de connaissances -- SQLite, FTS5, rappel intelligent, parcours BFS | |
| `graph-async` | Enveloppes de graphe asynchrones (necessite tokio) | |
| `graph-pool` | Pool de graphe asynchrone multi-connexion (`AsyncPoolGraph`, concurrence WAL) | |
| `mcp` | Serveur MCP -- JSON-RPC 2.0, transport stdio, authentification Bearer | |
| `tokens` | Estimation de tokens par heuristiques Unicode-script | |
| `install` | Assistant d'installation d'outils IA | |
| `search` | Recherche hybride avec Reciprocal Rank Fusion | |
| `embedding` | Trait de fournisseur d'embedding + similarite cosinus | |
| `embedding-openai` | Client text-embedding OpenAI (HTTP synchrone) | |
| `embedding-fastembed` | Embedding ONNX local via fastembed-rs (44 modeles) | |
| `embedding-fastembed-qwen3` | Embedding Qwen3 via le backend candle | |
| `embedding-fastembed-nomic-moe` | Embedding Nomic V2 MoE via le backend candle | |
| `telemetry` | Evenements de telemetrie gates par enum, sans PII | |
| `safety` | Masquage de secrets, classification d'erreurs, nettoyage de sorties | |
| `eval` | CLI d'evaluation de qualite -- tokens, securite, embedding, recherche | |
| `eval-full` | Tous les modules d'evaluation, y compris le graphe | |
| `full` | Toutes les fonctionnalites | |

## Demarrage rapide

Ajoutez a votre `Cargo.toml` :

```toml
[dependencies]
llm-kernel = "0.1.0"
```

La fonctionnalite `provider` est activee par defaut. Pour le client asynchrone :

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["client-async"] }
```

Pour le graphe de connaissances avec enveloppes asynchrones :

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["graph", "graph-async"] }
```

Pour l'embedding local (ONNX, sans cle API) :

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["embedding-fastembed"] }
```

## Utilisation

### Catalogue de fournisseurs

Le catalogue embarque contient 16 fournisseurs avec 114 modeles alignes sur le schema [models.dev](https://github.com/anomalyco/models.dev).

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

### Completion de chat asynchrone

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

### Decouverte de modeles

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

### Coffre de credentials

```rust
use llm_kernel::prelude::*;

let vault = SecretVault::load_from("~/.config/myapp/.env")?;
vault.set("OPENAI_API_KEY", "sk-...");
vault.save_to("~/.config/myapp/.env")?;

// Redact credentials for logging
println!("{}", redact_credential("sk-abcdef1234567890"));
// → "sk-abcd...7890"
```

### Configuration TOML

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

### Stockage SQLite

```rust
use llm_kernel::store::init_schema;

let ddl = "CREATE TABLE items (id TEXT PRIMARY KEY, content TEXT);";
let conn = init_schema(&db_path, ddl, 1)?;
// WAL mode, busy timeout, and schema versioning applied automatically
```

### Graphe de connaissances

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

### Serveur MCP

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

### Estimation de tokens

```rust
use llm_kernel::tokens::estimate_tokens;

let tokens = estimate_tokens("Hello, world! こんにちは世界 🌍");
println!("Estimated tokens: {}", tokens);
```

### Embedding et recherche

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

#### Embedding ONNX local (fastembed-rs)

44 modeles via ONNX Runtime -- aucune cle API, aucun reseau apres le premier telechargement.

```rust
use llm_kernel::embedding::{EmbeddingModel, FastembedProvider, EmbeddingProvider};

let provider = FastembedProvider::new(EmbeddingModel::BGESmallENV15, None)?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 384);
```

#### Embedding Qwen3 (candle)

Inference pure Rust GPU/CPU via candle-nn -- sans ONNX Runtime.

```rust
use llm_kernel::embedding::{Qwen3Provider, EmbeddingProvider};

let provider = Qwen3Provider::new("Qwen/Qwen3-Embedding-0.6B")?;
let result = provider.embed("hello world")?;
```

#### Embedding Nomic V2 MoE (candle)

Modele MoE leger -- 8 experts, routage top-2, 305M parametres actifs.

```rust
use llm_kernel::embedding::{NomicMoeProvider, EmbeddingProvider};

let provider = NomicMoeProvider::new()?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 768);
```

### Utilitaires de securite

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

## Metadonnees des modeles

Chaque modele du catalogue inclut :

| Champ | Description |
|-------|-------------|
| `cost` | Tarification par million de tokens (entree, sortie, cache_read, cache_write) |
| `limit` | Limites de tokens de contexte et de sortie |
| `modalities` | Modalites d'entree/sortie (texte, image, audio) |
| `capabilities` | Indicateurs : attachment, reasoning, temperature, tool_call, streaming |
| `knowledge` | Date de coupure des donnees d'entrainement |

## Pourquoi llm-kernel ?

| | llm-kernel | [rig] | [langchain-rust] |
|--|-----------|-------|-------------------|
| Catalogue de fournisseurs | Oui 16 fournisseurs, 114 modeles integres | Config manuelle | Config manuelle |
| Indicateurs de fonctionnalite | Oui 20 modules independants | Monolithique | Monolithique |
| Embedding local | Oui 44 ONNX + Qwen3 + Nomic MoE | Non | Non |
| Evaluation qualite | Oui 5 modules, regression de base, CI | Non | Non |
| Serveur MCP | Oui JSON-RPC 2.0 | Non | Non |
| Graphe de connaissances | Oui SQLite + FTS5 + rappel intelligent | Non | Non |
| Dependances obligatoires | `serde` uniquement | `reqwest`, `tokio`, ... | Nombreuses |
| Chaines / agents | Non | Oui | Oui |
| Pipelines RAG | Non | Oui | Oui |

[rig]: https://github.com/0xPlaygrounds/rig
[langchain-rust]: https://github.com/Abraxas-365/langchain-rust

llm-kernel est une **couche fondatrice legere** -- composez-le avec rig ou langchain-rust lorsque vous avez besoin de chaines, d'agents ou de RAG.

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

- **Trait `LLMClient`** -- interface unifiee pour `OpenAIClient` et `AnthropicClient`
- **Trait `EmbeddingProvider`** -- interface unifiee pour `FastembedProvider` (ONNX), `Qwen3Provider` (candle), `NomicMoeProvider` (candle), `OpenAIEmbeddingClient` (distant)
- **`ProviderIndex`** -- acces zero-copy au catalogue embarque, interrogeable par fournisseur ou modele
- **`McpServer`** -- serveur JSON-RPC 2.0 avec transport stdio, authentification Bearer et enregistrement d'outils
- **`SecretVault`** -- `HashMap<String, String>` avec chargement/sauvegarde dotenv et gardes de liens symboliques
- **`graph`** -- graphe de connaissances SQLite avec recherche FTS5, rappel a score composite, parcours BFS et decroissance d'importance
- **`TelemetryEvent`** -- variantes gates par enum pour l'observabilite structuree (sans PII)
- **`safety`** -- masquage de secrets, classification d'erreurs, nettoyage bidi/ANSI/null

## Evaluation de qualite

Le CLI d'evaluation integre mesure la qualite des modules par rapport a des jeux de donnees de test selectionnes :

```bash
# Executer toutes les evaluations (tokens, securite, embedding, recherche)
cargo run --bin llm-kernel-eval --features eval -- all

# Inclure l'evaluation du graphe
cargo run --bin llm-kernel-eval --features eval-full -- all

# Verification de regression par rapport a un snapshot de reference (code de sortie 1 en cas de regression)
cargo run --bin llm-kernel-eval --features eval-full -- --baseline eval/baseline.json all

# Sortie JSON pour les outils
cargo run --bin llm-kernel-eval --features eval -- --format json all
```

| Module | Metriques |
|--------|-----------|
| tokens | MAE, max_error, %±3, %±10%, ventilation par categorie |
| safety | exact_match_rate, precision, recall, F1, missed_secrets |
| embedding | identity_accuracy, orthogonality, symmetry, bounds |
| search | precision@5, recall@5, MRR |
| graph | precision, recall, F1 par type de requete |

Passez `--baseline eval/baseline.json` pour comparer avec un snapshot de reference -- le CLI quitte avec le code 1 en cas de regression de toute metrique. Le CI execute cette verification automatiquement a chaque push et PR via le job `eval`.

## Benchmarks

Benchmarks Criterion dans `benches/` :

```bash
cargo bench                          # Run all benchmarks
cargo bench -- graph_bench           # Graph: smart_recall, BFS, neighbors
cargo bench -- compute_bench         # Token estimation, RRF fusion
```

## Exemples

```bash
# List all providers and models (no API key needed)
cargo run --example provider_list

# OpenAI chat (requires OPENAI_API_KEY)
cargo run --example chat_openai --features client-async

# Anthropic streaming (requires ANTHROPIC_API_KEY)
cargo run --example stream_anthropic --features client-async
```

## Prerequis

- Rust 1.92+ (edition 2024)

## Contribuer

Voir [CONTRIBUTING.md](../../CONTRIBUTING.md). Les PR sont les bienvenues.

## Licence

[Apache-2.0](../../LICENSE) (c) 2026 EpicCounty
