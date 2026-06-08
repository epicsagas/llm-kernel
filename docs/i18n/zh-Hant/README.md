<!-- Translated from README.md @ commit edd4827 (2026-06-06) -->
<!-- If English README has changed since then, this translation may be outdated -->

[English](../../README.md) | [한국어](../ko/README.md) | [日本語](../ja/README.md) | [简体中文](../zh-Hans/README.md) | **繁體中文** | [Español](../es/README.md) | [Français](../fr/README.md) | [Deutsch](../de/README.md) | [Português](../pt/README.md) | [Русский](../ru/README.md) | [Italiano](../it/README.md)

> 本文件是 [README.md](../../README.md) 的繁體中文翻譯。
> 英文版本為權威來源，可能包含更新的內容。

<div align="center">

# llm-kernel

> Rust AI 原生應用的基礎函式庫 — 供應商目錄、LLM 客戶端、MCP 伺服器、搜尋、遙測與安全性

[![CI](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml/badge.svg)](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-kernel)](https://crates.io/crates/llm-kernel)
[![docs.rs](https://docs.rs/llm-kernel/badge.svg)](https://docs.rs/llm-kernel)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

</div>

## 概覽

llm-kernel 提供了使用 Rust 建構 LLM 驅動工具、代理與伺服器的基礎層：

- **供應商目錄** — 16 個內建供應商、114 個模型，附帶中繼資料、定價與能力資訊
- **非同步客戶端** — 基於 trait 的 OpenAI 與 Anthropic 客戶端，支援 SSE 串流
- **模型探索** — 從 models.dev、Ollama、OpenAI 相容端點動態探索模型
- **憑證保存庫** — dotenv 風格的 API 金鑰管理，支援原子寫入
- **設定載入器** — TOML 設定檔載入，可從範本自動建立
- **知識圖譜** — 以 SQLite 為後端的圖譜，支援 FTS5 搜尋、智慧回憶、BFS 遍歷與非同步封裝
- **MCP 伺服器** — JSON-RPC 2.0 伺服器框架，支援 stdio 傳輸與 Bearer 認證
- **嵌入** — 供應商 trait + 餘弦相似度，本地 ONNX（44 個模型）、Qwen3 candle、Nomic V2 MoE candle、OpenAI 遠端（[完整模型列表 →](../../EMBEDDING_MODELS.md)）
- **搜尋** — Reciprocal Rank Fusion 混合搜尋結果合併
- **Token 估算** — 零依賴的 Unicode 腳本啟發式 token 計數
- **遙測** — 列舉閘控事件，不含 PII，提供主控台與空操作接收器
- **安全性** — 密鑰遮罩、錯誤分類、輸出淨化
- **安裝精靈** — 為 Claude Desktop、Cursor、Copilot、OpenCode、Cline 產生 MCP 設定

## 功能旗標

每個模組都由功能旗標控制，讓您只需為所使用的功能付出代價。

| 功能 | 說明 | 預設 |
|---------|-------------|---------|
| `provider` | 供應商目錄、模型描述、定價 | ✅ |
| `client-async` | 非同步 LLM 客戶端（reqwest），支援串流 | |
| `discovery` | 動態模型探索（models.dev、Ollama、OpenAI-compat） | |
| `secrets` | SecretVault 憑證管理 | |
| `store` | SQLite 初始化輔助（WAL、FTS5、Schema 版本控制） | |
| `config` | TOML 設定載入器 | |
| `graph` | 知識圖譜 — SQLite、FTS5、智慧回憶、BFS 遍歷 | |
| `graph-async` | 非同步圖譜封裝（需要 tokio） | |
| `graph-pool` | 多連線非同步圖譜連線池（`AsyncPoolGraph`、WAL 並行） | |
| `mcp` | MCP 伺服器 — JSON-RPC 2.0、stdio 傳輸、Bearer 認證 | |
| `tokens` | 使用 Unicode 腳本啟發式的 Token 估算 | |
| `install` | AI 工具安裝精靈 | |
| `search` | 使用 Reciprocal Rank Fusion 的混合搜尋 | |
| `embedding` | 嵌入供應商 trait + 餘弦相似度 | |
| `embedding-openai` | OpenAI text-embedding 客戶端（同步 HTTP） | |
| `embedding-fastembed` | 透過 fastembed-rs 的本地 ONNX 嵌入（44 個模型） | |
| `embedding-fastembed-qwen3` | 透過 candle 後端的 Qwen3 嵌入 | |
| `embedding-fastembed-nomic-moe` | 透過 candle 後端的 Nomic V2 MoE 嵌入 | |
| `telemetry` | 列舉閘控遙測事件，不含 PII | |
| `safety` | 密鑰遮罩、錯誤分類、輸出淨化 | |
| `eval` | 品質評估 CLI — tokens、安全性、嵌入、搜尋 | |
| `eval-full` | 所有評估模組（含圖譜） | |
| `full` | 所有功能 | |

## 快速開始

在您的 `Cargo.toml` 中加入：

```toml
[dependencies]
llm-kernel = "0.1.0"
```

`provider` 功能預設啟用。如需非同步客戶端：

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["client-async"] }
```

如需附帶非同步封裝的知識圖譜：

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["graph", "graph-async"] }
```

如需本地嵌入（ONNX，無需 API 金鑰）：

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["embedding-fastembed"] }
```

## 使用方式

### 供應商目錄

內嵌目錄包含 16 個供應商與 114 個模型，符合 [models.dev](https://github.com/anomalyco/models.dev) 結構描述。

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

### 非同步聊天補全

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

### 串流

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

### 模型探索

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

### 憑證保存庫

```rust
use llm_kernel::prelude::*;

let vault = SecretVault::load_from("~/.config/myapp/.env")?;
vault.set("OPENAI_API_KEY", "sk-...");
vault.save_to("~/.config/myapp/.env")?;

// Redact credentials for logging
println!("{}", redact_credential("sk-abcdef1234567890"));
// → "sk-abcd...7890"
```

### TOML 設定

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

### SQLite 儲存

```rust
use llm_kernel::store::init_schema;

let ddl = "CREATE TABLE items (id TEXT PRIMARY KEY, content TEXT);";
let conn = init_schema(&db_path, ddl, 1)?;
// WAL mode, busy timeout, and schema versioning applied automatically
```

### 知識圖譜

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

### MCP 伺服器

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

### Token 估算

```rust
use llm_kernel::tokens::estimate_tokens;

let tokens = estimate_tokens("Hello, world! こんにちは世界 🌍");
println!("Estimated tokens: {}", tokens);
```

### 嵌入與搜尋

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

#### 本地 ONNX 嵌入（fastembed-rs）

透過 ONNX Runtime 提供 44 個模型 — 無需 API 金鑰，首次下載後無需網路。

```rust
use llm_kernel::embedding::{EmbeddingModel, FastembedProvider, EmbeddingProvider};

let provider = FastembedProvider::new(EmbeddingModel::BGESmallENV15, None)?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 384);
```

#### Qwen3 嵌入（candle）

透過 candle-nn 實現純 Rust GPU/CPU 推論 — 無需 ONNX Runtime。

```rust
use llm_kernel::embedding::{Qwen3Provider, EmbeddingProvider};

let provider = Qwen3Provider::new("Qwen/Qwen3-Embedding-0.6B")?;
let result = provider.embed("hello world")?;
```

#### Nomic V2 MoE 嵌入（candle）

輕量級 MoE 模型 — 8 個專家、top-2 路由、305M 活躍參數。

```rust
use llm_kernel::embedding::{NomicMoeProvider, EmbeddingProvider};

let provider = NomicMoeProvider::new()?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 768);
```

### 安全性工具

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

## 模型中繼資料

目錄中每個模型包含：

| 欄位 | 說明 |
|-------|-------------|
| `cost` | 每百萬 token 定價（輸入、輸出、快取讀取、快取寫入） |
| `limit` | 上下文與輸出 token 限制 |
| `modalities` | 輸入/輸出模態（文字、影像、音訊） |
| `capabilities` | 旗標：附件、推理、溫度、工具呼叫、串流 |
| `knowledge` | 訓練資料截止日期 |

## 為何選擇 llm-kernel？

| | llm-kernel | [rig] | [langchain-rust] |
|--|-----------|-------|-------------------|
| 供應商目錄 | ✅ 16 個供應商，114 個模型內建 | 手動設定 | 手動設定 |
| 功能閘控 | ✅ 20 個獨立模組 | 單體式 | 單體式 |
| 本地嵌入 | ✅ 44 個 ONNX + Qwen3 + Nomic MoE | ❌ | ❌ |
| 品質評估 | ✅ 5 個模組，基線回歸，CI | ❌ | ❌ |
| MCP 伺服器 | ✅ JSON-RPC 2.0 | ❌ | ❌ |
| 知識圖譜 | ✅ SQLite + FTS5 + 智慧回憶 | ❌ | ❌ |
| 必要依賴 | 僅 `serde` | `reqwest`、`tokio`、… | 許多 |
| 鏈 / 代理 | ❌ | ✅ | ✅ |
| RAG 管線 | ❌ | ✅ | ✅ |

[rig]: https://github.com/0xPlaygrounds/rig
[langchain-rust]: https://github.com/Abraxas-365/langchain-rust

llm-kernel 是一個**輕量級基礎層** — 當您需要鏈、代理或 RAG 時，可將其與 rig 或 langchain-rust 組合使用。

## 架構

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

- **`LLMClient` trait** — `OpenAIClient` 與 `AnthropicClient` 的統一介面
- **`EmbeddingProvider` trait** — `FastembedProvider`（ONNX）、`Qwen3Provider`（candle）、`NomicMoeProvider`（candle）、`OpenAIEmbeddingClient`（遠端）的統一介面
- **`ProviderIndex`** — 對內嵌目錄的零拷貝存取，可依供應商或模型查詢
- **`McpServer`** — JSON-RPC 2.0 伺服器，支援 stdio 傳輸、Bearer 認證與工具註冊
- **`SecretVault`** — `HashMap<String, String>` 附帶 dotenv 載入/儲存與符號連結防護
- **`graph`** — SQLite 知識圖譜，支援 FTS5 搜尋、複合評分回憶、BFS 遍歷、重要性衰減
- **`TelemetryEvent`** — 列舉閘控變體，用於結構化可觀測性（不含 PII）
- **`safety`** — 密鑰遮罩、錯誤分類、雙向/ANSI/null 淨化

## 品質評估

內建評估 CLI 根據策劃的測試資料集測量模組品質：

```bash
# 執行所有評估（tokens、安全性、嵌入、搜尋）
cargo run --bin llm-kernel-eval --features eval -- all

# 包含圖譜評估
cargo run --bin llm-kernel-eval --features eval-full -- all

# 與基線快照進行回歸檢查（回歸時退出碼為 1）
cargo run --bin llm-kernel-eval --features eval-full -- --baseline eval/baseline.json all

# JSON 輸出供工具使用
cargo run --bin llm-kernel-eval --features eval -- --format json all
```

| 模組 | 指標 |
|------|------|
| tokens | MAE, max_error, %±3, %±10%, 按類別細分 |
| safety | exact_match_rate, precision, recall, F1, missed_secrets |
| embedding | identity_accuracy, orthogonality, symmetry, bounds |
| search | precision@5, recall@5, MRR |
| graph | precision, recall, F1（按查詢類型） |

傳入 `--baseline eval/baseline.json` 可與黃金快照比較 — 當任何指標發生回歸時，CLI 將以退出碼 1 結束。CI 會在每次推送和 PR 時透過 `eval` 任務自動執行此檢查。

## 基準測試

`benches/` 目錄下的 Criterion 基準測試：

```bash
cargo bench                          # Run all benchmarks
cargo bench -- graph_bench           # Graph: smart_recall, BFS, neighbors
cargo bench -- compute_bench         # Token estimation, RRF fusion
```

## 範例

```bash
# List all providers and models (no API key needed)
cargo run --example provider_list

# OpenAI chat (requires OPENAI_API_KEY)
cargo run --example chat_openai --features client-async

# Anthropic streaming (requires ANTHROPIC_API_KEY)
cargo run --example stream_anthropic --features client-async
```

## 系統需求

- Rust 1.92+（edition 2024）

## 貢獻

請參閱 [CONTRIBUTING.md](../../CONTRIBUTING.md)。歡迎提交 PR。

## 授權

[Apache-2.0](../../LICENSE) © 2026 EpicCounty
