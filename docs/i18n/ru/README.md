<!-- Translated from README.md @ commit edd4827 (2026-06-06) -->
<!-- If English README has changed since then, this translation may be outdated -->

[English](../../README.md) | [한국어](../ko/README.md) | [日本語](../ja/README.md) | [简体中文](../zh-Hans/README.md) | [繁體中文](../zh-Hant/README.md) | [Español](../es/README.md) | [Français](../fr/README.md) | [Deutsch](../de/README.md) | [Português](../pt/README.md) | **Русский** | [Italiano](../it/README.md)

> Этот документ является переводом [README.md](../../README.md).
> Английская версия является авторитетным источником и может быть более актуальной.

<div align="center">

# llm-kernel

> Базовая библиотека для AI-приложений на Rust — каталог провайдеров, LLM-клиент, MCP-сервер, поиск, телеметрия и безопасность

[![CI](https://img.shields.io/github/actions/workflow/status/epicsagas/llm-kernel/ci.yml?style=for-the-badge&labelColor=0d1117&color=2ecc71&logo=github-actions&logoColor=white)](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-kernel?style=for-the-badge&labelColor=0d1117&color=fc8d62&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)
[![docs.rs](https://img.shields.io/docsrs/llm-kernel?style=for-the-badge&labelColor=0d1117&color=58a6ff&logo=docs.rs&logoColor=white)](https://docs.rs/llm-kernel)
[![License](https://img.shields.io/badge/license-Apache--2.0-3fb950?style=for-the-badge&labelColor=0d1117)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/llm-kernel?style=for-the-badge&labelColor=0d1117&color=bc8cff&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)

</div>

## Обзор

llm-kernel предоставляет базовый слой для создания инструментов, агентов и серверов на основе LLM в Rust:

- **Каталог провайдеров** — 16 встроенных провайдеров, 114 моделей с метаданными, ценами и возможностями
- **Асинхронный клиент** — клиент на основе типажей для OpenAI и Anthropic с SSE-стримингом
- **Обнаружение моделей** — динамическое обнаружение моделей из models.dev, Ollama, OpenAI-совместимых эндпоинтов
- **Хранилище учётных данных** — управление API-ключами в стиле dotenv с атомарной записью
- **Загрузчик конфигурации** — конфигурация TOML с автоматическим созданием из шаблона
- **Граф знаний** — граф на SQLite с поиском FTS5, умным извлечением, обходом BFS, асинхронными обёртками
- **MCP-сервер** — серверный фреймворк JSON-RPC 2.0 с транспортом stdio и аутентификацией Bearer
- **Эмбеддинг** — типаж провайдера + косинусное сходство, локальный ONNX (44 модели), Qwen3 candle, Nomic V2 MoE candle, OpenAI удалённо ([полный список моделей →](EMBEDDING_MODELS.md))
- **Поиск** — Reciprocal Rank Fusion для слияния результатов гибридного поиска
- **Оценка токенов** — эвристический подсчёт токенов по Unicode-скриптам без внешних зависимостей
- **Телеметрия** — события с перечислимыми типами без PII, консольный и noop-приёмники
- **Безопасность** — маскирование секретов, классификация ошибок, санитизация вывода
- **Мастер установки** — генерация конфигурации MCP для Claude Desktop, Cursor, Copilot, OpenCode, Cline

## Флаги функций

Каждый модуль управляется флагом функции, поэтому вы платите только за то, что используете.

| Функция | Описание | По умолчанию |
|---------|----------|--------------|
| `provider` | Каталог провайдеров, дескрипторы моделей, цены | ✅ |
| `client-async` | Асинхронный LLM-клиент (reqwest) со стримингом | |
| `discovery` | Динамическое обнаружение моделей (models.dev, Ollama, OpenAI-compat) | |
| `discovery-async` | Асинхронное обнаружение моделей — трейт `DiscoverySource` поверх reqwest | |
| `secrets` | Управление учётными данными SecretVault | |
| `store` | Вспомогательные функции инициализации SQLite (WAL, FTS5, версионирование схемы) | |
| `config` | Загрузчик конфигурации TOML | |
| `graph` | Граф знаний — SQLite, FTS5, умное извлечение, обход BFS | |
| `graph-async` | Асинхронные обёртки графа (требует tokio) | |
| `graph-pool` | Пул асинхронных подключений графа (`AsyncPoolGraph`, конкурентность WAL) | |
| `graph-cjk` | CJK-aware graph search via Rust-side segmentation (no schema change) | |
| `graph-pg` | GraphBackend на PostgreSQL (PgGraph) + CLI миграции SQLite ↔ PostgreSQL | |
| `mcp` | MCP-сервер — JSON-RPC 2.0, транспорт stdio, аутентификация Bearer | |
| `mcp-http` | MCP remote transport — HTTP/SSE (axum + tokio) | |
| `cache` | LLM response cache — `CacheClient` over `KvStore` | |
| `tokens` | Оценка токенов, бюджетирование и разбиение документов по границам предложений | |
| `install` | Мастер установки AI-инструментов | |
| `search` | Гибридный поиск — трейт `SearchProvider`, слияние RRF / взвешенная сумма / CombMNZ | |
| `embedding` | Типаж провайдера эмбеддингов + косинусное сходство + типаж AsyncVectorIndex (асинхронный аналог VectorIndex) | |
| `embedding-openai` | Клиент OpenAI text-embedding (синхронный HTTP) | |
| `embedding-fastembed` | Локальный ONNX-эмбеддинг через fastembed-rs (44 модели) | |
| `embedding-fastembed-qwen3` | Эмбеддинг Qwen3 через бэкенд candle | |
| `embedding-fastembed-nomic-moe` | Эмбеддинг Nomic V2 MoE через бэкенд candle | |
| `vector-index` | Сжатый векторный индекс TurboQuant — 2-бит/4-бит, ANN-поиск с SIMD | |
| `qdrant` | AsyncVectorIndex на Qdrant (QdrantVectorIndex) для удалённого векторного поиска | |
| `telemetry` | События телеметрии с перечислимыми типами, без PII | |
| `safety` | Маскирование секретов, классификация ошибок, санитизация вывода, обнаружение prompt-injection | |
| `eval` | CLI оценки качества — токены, безопасность, эмбеддинг, поиск | |
| `eval-full` | Все модули оценки, включая граф | |
| `full` | Все функции | |

## Быстрый старт

Добавьте в ваш `Cargo.toml`:

```toml
[dependencies]
llm-kernel = "0.8.0"
```

Функция `provider` включена по умолчанию. Для асинхронного клиента:

```toml
[dependencies]
llm-kernel = { version = "0.8.0", features = ["client-async"] }
```

Для графа знаний с асинхронными обёртками:

```toml
[dependencies]
llm-kernel = { version = "0.8.0", features = ["graph", "graph-async"] }
```

Для локальных эмбеддингов (ONNX, без API-ключа):

```toml
[dependencies]
llm-kernel = { version = "0.8.0", features = ["embedding-fastembed"] }
```

## Использование

### Каталог провайдеров

Встроенный каталог содержит 16 провайдеров с 114 моделями, соответствующими схеме [models.dev](https://github.com/anomalyco/models.dev).

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

### Асинхронное завершение чата

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

### Стриминг

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

### Обнаружение моделей

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

### Хранилище учётных данных

```rust
use llm_kernel::prelude::*;

let vault = SecretVault::load_from("~/.config/myapp/.env")?;
vault.set("OPENAI_API_KEY", "sk-...");
vault.save_to("~/.config/myapp/.env")?;

// Redact credentials for logging
println!("{}", redact_credential("sk-abcdef1234567890"));
// → "sk-abcd...7890"
```

### Конфигурация TOML

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

### Хранилище SQLite

```rust
use llm_kernel::store::init_schema;

let ddl = "CREATE TABLE items (id TEXT PRIMARY KEY, content TEXT);";
let conn = init_schema(&db_path, ddl, 1)?;
// WAL mode, busy timeout, and schema versioning applied automatically
```

### Граф знаний

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

### MCP-сервер

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

### Оценка токенов

```rust
use llm_kernel::tokens::estimate_tokens;

let tokens = estimate_tokens("Hello, world! こんにちは世界 🌍");
println!("Estimated tokens: {}", tokens);
```

### Эмбеддинг + поиск

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

#### Локальный ONNX-эмбеддинг (fastembed-rs)

44 модели через ONNX Runtime — не требуется API-ключ и сеть после первого скачивания.

```rust
use llm_kernel::embedding::{EmbeddingModel, FastembedProvider, EmbeddingProvider};

let provider = FastembedProvider::new(EmbeddingModel::BGESmallENV15, None)?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 384);
```

#### Эмбеддинг Qwen3 (candle)

Чистый GPU/CPU-вывод на Rust через candle-nn — без ONNX Runtime.

```rust
use llm_kernel::embedding::{Qwen3Provider, EmbeddingProvider};

let provider = Qwen3Provider::new("Qwen/Qwen3-Embedding-0.6B")?;
let result = provider.embed("hello world")?;
```

#### Эмбеддинг Nomic V2 MoE (candle)

Лёгкая MoE-модель — 8 экспертов, top-2 маршрутизация, 305M активных параметров.

```rust
use llm_kernel::embedding::{NomicMoeProvider, EmbeddingProvider};

let provider = NomicMoeProvider::new()?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 768);
```

### Утилиты безопасности

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

## Метаданные моделей

Каждая модель в каталоге содержит:

| Поле | Описание |
|------|----------|
| `cost` | Цены за миллион токенов (ввод, вывод, чтение кэша, запись кэша) |
| `limit` | Лимиты токенов контекста и вывода |
| `modalities` | Модальности ввода/вывода (текст, изображение, аудио) |
| `capabilities` | Флаги: attachment, reasoning, temperature, tool_call, streaming |
| `knowledge` | Дата актуальности обучающих данных |

## Почему llm-kernel?

| | llm-kernel | [rig] | [langchain-rust] |
|--|-----------|-------|-------------------|
| Каталог провайдеров | ✅ 16 провайдеров, 114 моделей встроено | Ручная настройка | Ручная настройка |
| Флаги функций | ✅ Независимые модули | Монолитная | Монолитная |
| Локальный эмбеддинг | ✅ 44 ONNX + Qwen3 + Nomic MoE | ❌ | ❌ |
| Оценка качества | ✅ 5 модулей, регрессия baseline, CI | ❌ | ❌ |
| MCP-сервер | ✅ JSON-RPC 2.0 | ❌ | ❌ |
| Граф знаний | ✅ SQLite + FTS5 + умное извлечение | ❌ | ❌ |
| Обязательные зависимости | только `serde` | `reqwest`, `tokio`, … | Много |
| Цепочки / агенты | ❌ | ✅ | ✅ |
| RAG-конвейеры | ❌ | ✅ | ✅ |

[rig]: https://github.com/0xPlaygrounds/rig
[langchain-rust]: https://github.com/Abraxas-365/langchain-rust

llm-kernel — это **лёгкий базовый слой** — комбинируйте его с rig или langchain-rust, когда нужны цепочки, агенты или RAG.

## Архитектура

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

- **`LLMClient` trait** — унифицированный интерфейс для `OpenAIClient` и `AnthropicClient`
- **`EmbeddingProvider` trait** — унифицированный интерфейс для `FastembedProvider` (ONNX), `Qwen3Provider` (candle), `NomicMoeProvider` (candle), `OpenAIEmbeddingClient` (удалённый)
- **`ProviderIndex`** — доступ с нулевым копированием к встроенному каталогу, запросы по провайдеру или модели
- **`McpServer`** — сервер JSON-RPC 2.0 с транспортом stdio, аутентификацией Bearer, регистрацией инструментов
- **`SecretVault`** — `HashMap<String, String>` с загрузкой/сохранением dotenv и защитой от символьных ссылок
- **`graph`** — граф знаний SQLite с поиском FTS5, извлечением с составным скорингом, обходом BFS, затуханием важности
- **`TelemetryEvent`** — перечислимые варианты для структурной наблюдаемости (без PII)
- **`safety`** — маскирование секретов, классификация ошибок, двунаправленная/ANSI/null-санитизация

## Оценка качества

Встроенный CLI оценки измеряет качество модулей на курированных тестовых наборах данных:

```bash
# Запустить все оценки (токены, безопасность, эмбеддинг, поиск)
cargo run --bin llm-kernel-eval --features eval -- all

# Включить оценку графа
cargo run --bin llm-kernel-eval --features eval-full -- all

# Проверка регрессии относительно базового снапшота (код выхода 1 при регрессии)
cargo run --bin llm-kernel-eval --features eval-full -- --baseline eval/baseline.json all

# JSON-вывод для инструментов
cargo run --bin llm-kernel-eval --features eval -- --format json all
```

| Модуль | Метрики |
|--------|---------|
| tokens | MAE, max_error, %±3, %±10%, разбивка по категориям |
| safety | exact_match_rate, precision, recall, F1, missed_secrets |
| embedding | identity_accuracy, orthogonality, symmetry, bounds |
| search | precision@5, recall@5, MRR |
| graph | precision, recall, F1 по типу запроса |

Передайте `--baseline eval/baseline.json` для сравнения с эталонным снапшотом — CLI завершается с кодом 1 при любой регрессии метрики. CI автоматически запускает это при каждом push и PR через задачу `eval`.

## Бенчмарки

Бенчмарки Criterion в каталоге `benches/`:

```bash
cargo bench                          # Run all benchmarks
cargo bench -- graph_bench           # Graph: smart_recall, BFS, neighbors
cargo bench -- compute_bench         # Token estimation, RRF fusion
```

## Примеры

```bash
# List all providers and models (no API key needed)
cargo run --example provider_list

# OpenAI chat (requires OPENAI_API_KEY)
cargo run --example chat_openai --features client-async

# Anthropic streaming (requires ANTHROPIC_API_KEY)
cargo run --example stream_anthropic --features client-async
```

## Требования

- Rust 1.92+ (edition 2024)

## Участие в разработке

См. [CONTRIBUTING.md](CONTRIBUTING.md). PR приветствуются.

## Лицензия

[Apache-2.0](LICENSE) © 2026 EpicCounty
