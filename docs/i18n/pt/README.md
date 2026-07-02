<!-- Translated from README.md @ commit edd4827 (2026-06-06) -->
<!-- If English README has changed since then, this translation may be outdated -->

[English](../../README.md) | [한국어](../ko/README.md) | [日本語](../ja/README.md) | [简体中文](../zh-Hans/README.md) | [繁體中文](../zh-Hant/README.md) | [Español](../es/README.md) | [Français](../fr/README.md) | [Deutsch](../de/README.md) | **Português** | [Русский](../ru/README.md) | [Italiano](../it/README.md)

> Este documento é uma tradução de [README.md](../../README.md).
> A versão em inglês é a fonte autorizada e pode estar mais atualizada.

<div align="center">

# llm-kernel

> Biblioteca fundamental para aplicações AI-nativas em Rust — catálogo de providers, cliente LLM, servidor MCP, busca, telemetria e segurança

[![CI](https://img.shields.io/github/actions/workflow/status/epicsagas/llm-kernel/ci.yml?style=for-the-badge&labelColor=0d1117&color=2ecc71&logo=github-actions&logoColor=white)](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-kernel?style=for-the-badge&labelColor=0d1117&color=fc8d62&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)
[![docs.rs](https://img.shields.io/docsrs/llm-kernel?style=for-the-badge&labelColor=0d1117&color=58a6ff&logo=docs.rs&logoColor=white)](https://docs.rs/llm-kernel)
[![License](https://img.shields.io/badge/license-Apache--2.0-3fb950?style=for-the-badge&labelColor=0d1117)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/llm-kernel?style=for-the-badge&labelColor=0d1117&color=bc8cff&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)

</div>

## Visão geral

llm-kernel fornece a camada fundamental para construir ferramentas, agentes e servidores baseados em LLM em Rust:

- **Catálogo de providers** — 20 providers integrados, 351 modelos com metadados, preços e capacidades
- **Cliente assíncrono** — cliente baseado em traits para OpenAI e Anthropic com streaming SSE
- **Descoberta de modelos** — descoberta dinâmica de modelos via models.dev, Ollama e endpoints compatíveis com OpenAI
- **Cofre de credenciais** — gerenciamento de chaves de API estilo dotenv com escritas atômicas
- **Carregador de config** — configuração TOML com criação automática a partir de template
- **Grafo de conhecimento** — grafo baseado em SQLite com busca FTS5, recall inteligente, travessia BFS, wrappers assíncronos
- **Servidor MCP** — framework de servidor JSON-RPC 2.0 com transporte stdio e autenticação Bearer
- **Embedding** — trait de provider + similaridade por cosseno, ONNX local (44 modelos), Qwen3 candle, Nomic V2 MoE candle, OpenAI remoto ([lista completa de modelos →](../../EMBEDDING_MODELS.md))
- **Busca** — Reciprocal Rank Fusion para fusão de resultados de busca híbrida
- **Estimativa de tokens** — contagem heurística de tokens por script Unicode sem dependências
- **Telemetria** — eventos com gate por enum sem PII, sinks de console e noop
- **Segurança** — mascaramento de segredos, classificação de erros, sanitização de saída
- **Assistente de instalação** — geração de configuração MCP para Claude Desktop, Cursor, Copilot, OpenCode, Cline

## Feature flags

Cada módulo é controlado por uma feature flag para que você só pague pelo que utiliza.

| Feature | Descrição | Padrão |
|---------|-----------|--------|
| `provider` | Catálogo de providers, descritores de modelos, preços | ✅ |
| `client-async` | Cliente LLM assíncrono (reqwest) com streaming | |
| `discovery` | Descoberta dinâmica de modelos (models.dev, Ollama, OpenAI-compat) | |
| `discovery-async` | Descoberta assíncrona de modelos — trait `DiscoverySource` sobre reqwest | |
| `secrets` | Gerenciamento de credenciais SecretVault | |
| `store` | Helpers de inicialização SQLite (WAL, FTS5, versionamento de schema) | |
| `config` | Carregador de configuração TOML | |
| `graph` | Grafo de conhecimento — SQLite, FTS5, recall inteligente, travessia BFS |, algoritmos de grafo(PageRank/community/shortest-path/similarity) |
| `graph-async` | Wrappers assíncronos do grafo (requer tokio) | |
| `graph-pool` | Pool assíncrono multi-conexão do grafo (`AsyncPoolGraph`, concorrência WAL) | |
| `graph-cjk` | CJK-aware graph search via Rust-side segmentation (no schema change) | |
| `graph-pg` | GraphBackend PostgreSQL (PgGraph) + CLI de migração SQLite<->PostgreSQL | |
| `mcp` | Servidor MCP — JSON-RPC 2.0, transporte stdio, autenticação Bearer | |
| `mcp-http` | MCP remote transport — HTTP/SSE (axum + tokio) | |
| `cache` | LLM response cache — `CacheClient` over `KvStore` | |
| `tokens` | Estimativa de tokens, orçamento e divisão de documentos por fronteiras de frase | |
| `install` | Assistente de instalação de ferramentas AI | |
| `search` | Busca híbrida — trait `SearchProvider`, fusão RRF / soma ponderada / CombMNZ | |
| `embedding` | Trait de provider de embedding + similaridade por cosseno + trait AsyncVectorIndex (contraparte assíncrona de VectorIndex) | |
| `embedding-openai` | Cliente de text-embedding do OpenAI (HTTP síncrono) | |
| `embedding-fastembed` | Embedding local via ONNX com fastembed-rs (44 modelos) | |
| `embedding-fastembed-qwen3` | Embedding Qwen3 via backend candle | |
| `embedding-fastembed-nomic-moe` | Embedding Nomic V2 MoE via backend candle | |
| `vector-index` | Índice de vetores comprimido TurboQuant — 2 bits/4 bits, busca ANN com SIMD | |
| `qdrant` | AsyncVectorIndex Qdrant (QdrantVectorIndex) para busca vetorial remota | |
| `elastic` | AsyncVectorIndex Elasticsearch (ElasticsearchVectorIndex) sobre um cliente reqwest feito à mão | |
| `federation` | Federação entre motores — consulta simultânea em vários backends `AsyncVectorIndex` com tempo limite por backend (RRF padrão) | |
| `telemetry` | Eventos de telemetria com gate por enum, sem PII | |
| `safety` | Mascaramento de segredos, classificação de erros, sanitização de saída, detecção de prompt-injection | |
| `eval` | CLI de avaliação de qualidade — tokens, segurança, embedding, busca | |
| `eval-full` | Todos os módulos de avaliação, incluindo grafo | |
| `catalog-sync` | CLI de sincronização do catálogo — atualiza `catalog.json` a partir do models.dev | |
| `full` | Todas as features | |

## Início rápido

Adicione ao seu `Cargo.toml`:

```toml
[dependencies]
llm-kernel = "0.11.0"
```

A feature `provider` é habilitada por padrão. Para o cliente assíncrono:

```toml
[dependencies]
llm-kernel = { version = "0.11.0", features = ["client-async"] }
```

Para o grafo de conhecimento com wrappers assíncronos:

```toml
[dependencies]
llm-kernel = { version = "0.11.0", features = ["graph", "graph-async"] }
```

Para embedding local (ONNX, sem chave de API):

```toml
[dependencies]
llm-kernel = { version = "0.11.0", features = ["embedding-fastembed"] }
```

## Uso

### Catálogo de providers

O catálogo embarcado contém 20 providers com 351 modelos alinhados ao schema do [models.dev](https://github.com/anomalyco/models.dev).

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

### Chat completion assíncrono

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
    ..LLMRequest::default(),
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
    ..LLMRequest::default(),
}).await?;

// Stream yields Delta, Usage, and Done events
```

### Descoberta de modelos

```rust
use llm_kernel::discovery::{fetch_and_cache, fetch_ollama_models};

// Busca do models.dev (faz cache do payload bruto em disco, byte a byte
// idêntico ao upstream). O payload é um mapa indexado por provider; .entries()
// o achata em uma lista.
let payload = fetch_and_cache("~/.cache/llm-kernel/models-dev.json")?;
for model in payload.entries() {
    // ModelEntry agora traz metadados completos: cost, limits, modalities, capabilities.
    let ctx = model.limits.as_ref().and_then(|l| l.context);
    println!("{} (via {}) — ctx: {:?}", model.id, model.provider_id, ctx);
}

// Descobrir modelos locais do Ollama
let ollama_models = fetch_ollama_models("http://localhost:11434")?;
for name in &ollama_models {
    println!("Ollama: {}", name);
}
```

### Mantendo o catálogo atualizado

O catálogo embarcado é congelado em tempo de compilação (via `include_str!`),
então só avança quando você bumpa a dependência `llm-kernel`. Para preços
**sempre atuais**, busque o models.dev em tempo de execução e o sobreponha ao
catálogo embarcado:

```rust
use llm_kernel::prelude::*; // ProviderIndex
use llm_kernel::discovery::{DiscoverySource, ModelsDevSource}; // discovery-async

let entries = ModelsDevSource::new().discover().await?; // models.dev ao vivo
let catalog = ProviderIndex::embedded().with_discovered(&entries);

// Modelos descobertos agora participam de buscas e estimativa de custo, mesmo
// se estiverem ausentes do catálogo estaticamente embarcado:
let cost = catalog.estimate_cost("some/new-model", prompt_tokens, completion_tokens);
```

Para atualizar o próprio catálogo **embarcado** (o baseline offline compilado no
crate), mantenedores executam a ferramenta de sincronização antes de um release:

```text
cargo run --bin llm-kernel-sync-catalog --features catalog-sync -- --check   # mostrar divergências
cargo run --bin llm-kernel-sync-catalog --features catalog-sync              # escrever catalog.json
```

### Cofre de credenciais

```rust
use llm_kernel::prelude::*;

let vault = SecretVault::load_from("~/.config/myapp/.env")?;
vault.set("OPENAI_API_KEY", "sk-...");
vault.save_to("~/.config/myapp/.env")?;

// Redact credentials for logging
println!("{}", redact_credential("sk-abcdef1234567890"));
// → "sk-abcd...7890"
```

### Configuração TOML

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

### Grafo de conhecimento

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

### Servidor MCP

```rust
use llm_kernel::mcp::{McpServer, ToolDescription};
use serde_json::json;

let mut server = McpServer::new("my-server", "1.0.0");
server.register_tool(ToolDescription {
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

### Estimativa de tokens

```rust
use llm_kernel::tokens::estimate_tokens;

let tokens = estimate_tokens("Hello, world! こんにちは世界 🌍");
println!("Estimated tokens: {}", tokens);
```

### Embedding + busca

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

#### Embedding local ONNX (fastembed-rs)

44 modelos via ONNX Runtime — sem chave de API, sem rede após o primeiro download.

```rust
use llm_kernel::embedding::{EmbeddingModel, FastembedProvider, EmbeddingProvider};

let provider = FastembedProvider::new(EmbeddingModel::BGESmallENV15, None)?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 384);
```

#### Embedding Qwen3 (candle)

Inferência GPU/CPU pura em Rust via candle-nn — sem ONNX Runtime.

```rust
use llm_kernel::embedding::{Qwen3Provider, EmbeddingProvider};

let provider = Qwen3Provider::new("Qwen/Qwen3-Embedding-0.6B")?;
let result = provider.embed("hello world")?;
```

#### Embedding Nomic V2 MoE (candle)

Modelo MoE leve — 8 especialistas, roteamento top-2, 305M parâmetros ativos.

```rust
use llm_kernel::embedding::{NomicMoeProvider, EmbeddingProvider};

let provider = NomicMoeProvider::new()?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 768);
```

### Utilitários de segurança

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

## Metadados dos modelos

Cada modelo no catálogo inclui:

| Campo | Descrição |
|-------|-----------|
| `cost` | Preços por milhão de tokens (input, output, cache_read, cache_write) |
| `limit` | Limites de tokens de contexto e saída |
| `modalities` | Modalidades de entrada/saída (texto, imagem, áudio) |
| `capabilities` | Flags: attachment, reasoning, temperature, tool_call, streaming |
| `knowledge` | Data de corte dos dados de treinamento |

## Por que llm-kernel?

| | llm-kernel | [rig] | [langchain-rust] |
|--|-----------|-------|-------------------|
| Catálogo de providers | ✅ 20 providers, 351 modelos integrados | Configuração manual | Configuração manual |
| Feature gates | ✅ Módulos independentes | Monolítico | Monolítico |
| Embedding local | ✅ 44 ONNX + Qwen3 + Nomic MoE | ❌ | ❌ |
| Avaliação de qualidade | ✅ 5 módulos, regressão baseline, CI | ❌ | ❌ |
| Servidor MCP | ✅ JSON-RPC 2.0 | ❌ | ❌ |
| Grafo de conhecimento | ✅ SQLite + FTS5 + recall inteligente | ❌ | ❌ |
| Dependências obrigatórias | Apenas `serde` | `reqwest`, `tokio`, … | Muitas |
| Chains / agentes | ❌ | ✅ | ✅ |
| Pipelines RAG | ❌ | ✅ | ✅ |

[rig]: https://github.com/0xPlaygrounds/rig
[langchain-rust]: https://github.com/Abraxas-365/langchain-rust

llm-kernel é uma **camada fundamental leve** — combine com rig ou langchain-rust quando precisar de chains, agentes ou RAG.

## Arquitetura

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

- **Trait `LLMClient`** — interface unificada para `OpenAIClient` e `AnthropicClient`
- **Trait `EmbeddingProvider`** — interface unificada para `FastembedProvider` (ONNX), `Qwen3Provider` (candle), `NomicMoeProvider` (candle), `OpenAIEmbeddingClient` (remoto)
- **`ProviderIndex`** — acesso zero-copy ao catálogo embarcado, consultável por provider ou modelo
- **`McpServer`** — servidor JSON-RPC 2.0 com transporte stdio, autenticação Bearer, registro de tools
- **`SecretVault`** — `HashMap<String, String>` com carregamento/salvamento dotenv e proteção contra symlinks
- **`graph`** — grafo de conhecimento SQLite com busca FTS5, recall por scoring composto, travessia BFS, decaimento de importância
- **`TelemetryEvent`** — variantes com gate por enum para observabilidade estruturada (sem PII)
- **`safety`** — mascaramento de segredos, classificação de erros, sanitização bidi/ANSI/null

## Avaliação de qualidade

O CLI de avaliação integrado mede a qualidade dos módulos contra conjuntos de dados de teste selecionados:

```bash
# Executar todas as avaliações (tokens, segurança, embedding, busca)
cargo run --bin llm-kernel-eval --features eval -- all

# Incluir avaliação do grafo
cargo run --bin llm-kernel-eval --features eval-full -- all

# Verificação de regressão contra snapshot baseline (código de saída 1 em caso de regressão)
cargo run --bin llm-kernel-eval --features eval-full -- --baseline eval/baseline.json all

# Saída JSON para ferramentas
cargo run --bin llm-kernel-eval --features eval -- --format json all
```

| Módulo | Métricas |
|--------|----------|
| tokens | MAE, max_error, %±3, %±10%, detalhamento por categoria |
| safety | exact_match_rate, precision, recall, F1, missed_secrets |
| embedding | identity_accuracy, orthogonality, symmetry, bounds |
| search | precision@5, recall@5, MRR |
| graph | precision, recall, F1 por tipo de consulta |

Passe `--baseline eval/baseline.json` para comparar com um snapshot de referência — o CLI encerra com código 1 em qualquer regressão de métrica. O CI executa isso automaticamente a cada push e PR via o job `eval`.

## Benchmarks

Benchmarks Criterion no diretório `benches/`:

```bash
cargo bench                          # Run all benchmarks
cargo bench -- graph_bench           # Graph: smart_recall, BFS, neighbors
cargo bench -- compute_bench         # Token estimation, RRF fusion
```

## Exemplos

```bash
# List all providers and models (no API key needed)
cargo run --example provider_list

# OpenAI chat (requires OPENAI_API_KEY)
cargo run --example chat_openai --features client-async

# Anthropic streaming (requires ANTHROPIC_API_KEY)
cargo run --example stream_anthropic --features client-async
```

## Requisitos

- Rust 1.92+ (edition 2024)

## Contribuindo

Consulte [CONTRIBUTING.md](../../CONTRIBUTING.md). PRs são bem-vindos.

## Licença

[Apache-2.0](../../LICENSE) © 2026 EpicCounty
