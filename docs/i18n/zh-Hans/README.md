<!-- Translated from README.md @ commit edd4827 (2026-06-06) -->
<!-- If English README has changed since then, this translation may be outdated -->

[English](../../README.md) | [한국어](../ko/README.md) | [日本語](../ja/README.md) | **简体中文** | [繁體中文](../zh-Hant/README.md) | [Español](../es/README.md) | [Français](../fr/README.md) | [Deutsch](../de/README.md) | [Português](../pt/README.md) | [Русский](../ru/README.md) | [Italiano](../it/README.md)

> 本文档是 [README.md](../../README.md) 的简体中文翻译。
> 英文版本为权威来源，可能包含更新的内容。

<div align="center">

# llm-kernel

> Rust AI 原生应用基础库 — 供应商目录、LLM 客户端、MCP 服务器、搜索、遥测与安全工具

[![CI](https://img.shields.io/github/actions/workflow/status/epicsagas/llm-kernel/ci.yml?style=for-the-badge&labelColor=0d1117&color=2ecc71&logo=github-actions&logoColor=white)](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-kernel?style=for-the-badge&labelColor=0d1117&color=fc8d62&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)
[![docs.rs](https://img.shields.io/docsrs/llm-kernel?style=for-the-badge&labelColor=0d1117&color=58a6ff&logo=docs.rs&logoColor=white)](https://docs.rs/llm-kernel)
[![License](https://img.shields.io/badge/license-Apache--2.0-3fb950?style=for-the-badge&labelColor=0d1117)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/llm-kernel?style=for-the-badge&labelColor=0d1117&color=bc8cff&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)

</div>

## 概述

llm-kernel 为 Rust 中构建 LLM 驱动的工具、代理和服务器提供基础层：

- **供应商目录** — 16 个内置供应商，114 个模型，包含元数据、定价和能力信息
- **异步客户端** — 基于 trait 的客户端，支持 OpenAI 和 Anthropic，含 SSE 流式传输
- **模型发现** — 从 models.dev、Ollama、OpenAI 兼容端点动态发现模型
- **凭证保险库** — dotenv 风格的 API 密钥管理，支持原子写入
- **配置加载器** — TOML 配置，支持从模板自动创建
- **知识图谱** — 基于 SQLite 的图谱，包含 FTS5 搜索、智能召回、BFS 遍历、异步封装
- **MCP 服务器** — JSON-RPC 2.0 服务器框架，支持 stdio 传输和 Bearer 认证
- **嵌入** — provider trait + 余弦相似度，本地 ONNX（44 个模型）、Qwen3 candle、Nomic V2 MoE candle、OpenAI 远程（[完整模型列表 →](EMBEDDING_MODELS.md)）
- **搜索** — Reciprocal Rank Fusion 混合搜索结果合并
- **Token 估算** — 零依赖 Unicode 脚本启发式 token 计数
- **遥测** — 枚举门控事件，不含 PII，支持控制台和空操作接收器
- **安全** — 密钥遮蔽、错误分类、输出净化
- **安装向导** — 为 Claude Desktop、Cursor、Copilot、OpenCode、Cline 生成 MCP 配置

## 特性标志

每个模块都通过特性标志进行门控，只为使用的部分付费。

| 特性 | 说明 | 默认 |
|------|------|------|
| `provider` | 供应商目录、模型描述符、定价 | ✅ |
| `client-async` | 异步 LLM 客户端（reqwest），支持流式传输 | |
| `discovery` | 动态模型发现（models.dev、Ollama、OpenAI 兼容） | |
| `secrets` | SecretVault 凭证管理 | |
| `store` | SQLite 初始化辅助（WAL、FTS5、模式版本控制） | |
| `config` | TOML 配置加载器 | |
| `graph` | 知识图谱 — SQLite、FTS5、智能召回、BFS 遍历 | |
| `graph-async` | 异步图谱封装（依赖 tokio） | |
| `graph-pool` | 多连接异步图谱连接池（`AsyncPoolGraph`，WAL 并发） | |
| `mcp` | MCP 服务器 — JSON-RPC 2.0、stdio 传输、Bearer 认证 | |
| `tokens` | Unicode 脚本启发式 token 估算 | |
| `install` | AI 工具安装向导 | |
| `search` | Reciprocal Rank Fusion 混合搜索 | |
| `embedding` | 嵌入 provider trait + 余弦相似度 | |
| `embedding-openai` | OpenAI 文本嵌入客户端（同步 HTTP） | |
| `embedding-fastembed` | 通过 fastembed-rs 的本地 ONNX 嵌入（44 个模型） | |
| `embedding-fastembed-qwen3` | 通过 candle 后端的 Qwen3 嵌入 | |
| `embedding-fastembed-nomic-moe` | 通过 candle 后端的 Nomic V2 MoE 嵌入 | |
| `telemetry` | 枚举门控遥测事件，不含 PII | |
| `safety` | 密钥遮蔽、错误分类、输出净化 | |
| `eval` | 质量评估 CLI — token、安全、嵌入、搜索 | |
| `eval-full` | 包含图谱的全部评估模块 | |
| `full` | 所有特性 | |

## 快速开始

添加到你的 `Cargo.toml`：

```toml
[dependencies]
llm-kernel = "0.1.0"
```

`provider` 特性默认启用。如需异步客户端：

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["client-async"] }
```

如需带异步封装的知识图谱：

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["graph", "graph-async"] }
```

如需本地嵌入（ONNX，无需 API 密钥）：

```toml
[dependencies]
llm-kernel = { version = "0.1.0", features = ["embedding-fastembed"] }
```

## 使用方法

### 供应商目录

内嵌目录包含 16 个供应商和 114 个模型，遵循 [models.dev](https://github.com/anomalyco/models.dev) 规范。

```rust
use llm_kernel::prelude::*;

let catalog = ProviderIndex::embedded();

// 列出所有供应商
for id in catalog.ids() {
    let provider = catalog.get(&id).unwrap();
    println!("{}", provider.display_name);
}

// 查询某个供应商的模型
for model in catalog.models_for("openai") {
    println!("  {} — ${:.2}/1M in", model.id, model.cost.unwrap().input);
}

// 查找特定模型
if let Some(model) = catalog.find_model("claude-sonnet-4-20250514") {
    println!("Context: {} tokens", model.limit.unwrap().context);
}
```

### 异步聊天补全

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

### 流式传输

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

// 流式产生 Delta、Usage 和 Done 事件
```

### 模型发现

```rust
use llm_kernel::discovery::{fetch_and_cache, load_cache, fetch_ollama_models};

// 从 models.dev 获取（缓存到磁盘）
let payload = fetch_and_cache("~/.cache/llm-kernel/models-dev.json")?;
for model in &payload.models {
    println!("{} — {} (ctx: {:?})", model.id, model.provider_id, model.limits);
}

// 从缓存加载（无需网络）
if let Some(cached) = load_cache("~/.cache/llm-kernel/models-dev.json")? {
    println!("{} models cached", cached.models.len());
}

// 发现本地 Ollama 模型
let ollama_models = fetch_ollama_models("http://localhost:11434")?;
for name in &ollama_models {
    println!("Ollama: {}", name);
}
```

### 凭证保险库

```rust
use llm_kernel::prelude::*;

let vault = SecretVault::load_from("~/.config/myapp/.env")?;
vault.set("OPENAI_API_KEY", "sk-...");
vault.save_to("~/.config/myapp/.env")?;

// 对日志中的凭证进行脱敏
println!("{}", redact_credential("sk-abcdef1234567890"));
// → "sk-abcd...7890"
```

### TOML 配置

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

### SQLite 存储

```rust
use llm_kernel::store::init_schema;

let ddl = "CREATE TABLE items (id TEXT PRIMARY KEY, content TEXT);";
let conn = init_schema(&db_path, ddl, 1)?;
// WAL 模式、忙等待超时和模式版本控制会自动应用
```

### 知识图谱

```rust
use llm_kernel::prelude::*;
use rusqlite::Connection;

let conn = Connection::open_in_memory().unwrap();
init_graph_schema(&conn).unwrap();

// 创建节点
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

// 用边连接
append_edge(&conn, &GraphEdge {
    id: "e1".into(),
    source: "rust-ownership".into(),
    target: "borrow-checker".into(),
    relation: "related".into(),
    weight: 1.5,
    ts: "2026-01-01T00:00:00Z".into(),
}).unwrap();

// 带复合评分的智能召回
let results = smart_recall(&conn, Some("my-project"), Some("ownership"), 5).unwrap();
for scored in &results {
    println!("{:.2} — {}", scored.score, scored.node.title);
}

// 生命周期管理
decay_importance(&conn, 30, 0.9, 0.05).unwrap();
tag_stale_nodes(&conn, 90).unwrap();
let stats = compute_stats(&conn).unwrap();
println!("{} nodes, {} edges", stats.total_nodes, stats.total_edges);
```

### MCP 服务器

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

// 通过 stdio 运行 JSON-RPC 2.0，支持 Bearer 认证
server.run_stdio().await?;
```

### Token 估算

```rust
use llm_kernel::tokens::estimate_tokens;

let tokens = estimate_tokens("Hello, world! こんにちは世界 🌍");
println!("Estimated tokens: {}", tokens);
```

### 嵌入与搜索

```rust
use llm_kernel::embedding::{EmbeddingProvider, cosine_similarity};
use llm_kernel::search::{SearchResult, rrf_fuse};

// 向量间的余弦相似度
let sim = cosine_similarity(&[0.1, 0.2, 0.3], &[0.4, 0.5, 0.6]);

// Reciprocal Rank Fusion 混合搜索
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

通过 ONNX Runtime 提供 44 个模型 — 无需 API 密钥，首次下载后无需网络。

```rust
use llm_kernel::embedding::{EmbeddingModel, FastembedProvider, EmbeddingProvider};

let provider = FastembedProvider::new(EmbeddingModel::BGESmallENV15, None)?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 384);
```

#### Qwen3 嵌入（candle）

通过 candle-nn 实现纯 Rust GPU/CPU 推理 — 无需 ONNX Runtime。

```rust
use llm_kernel::embedding::{Qwen3Provider, EmbeddingProvider};

let provider = Qwen3Provider::new("Qwen/Qwen3-Embedding-0.6B")?;
let result = provider.embed("hello world")?;
```

#### Nomic V2 MoE 嵌入（candle）

轻量级 MoE 模型 — 8 个专家，top-2 路由，3.05 亿活跃参数。

```rust
use llm_kernel::embedding::{NomicMoeProvider, EmbeddingProvider};

let provider = NomicMoeProvider::new()?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 768);
```

### 安全工具

```rust
use llm_kernel::safety::{mask_secrets, classify_failure, sanitize_output};

// 遮蔽日志中的密钥
let safe = mask_secrets("Authorization: Bearer sk-abcdef123456");
// → "Authorization: Bearer [REDACTED]"

// 错误分类
let category = classify_failure("connection timed out after 30s");
// → ErrorCategory::Timeout

// 净化不可信输出
let clean = sanitize_output(user_input)?;
```

## 模型元数据

目录中的每个模型包含：

| 字段 | 说明 |
|------|------|
| `cost` | 每百万 token 定价（输入、输出、cache_read、cache_write） |
| `limit` | 上下文和输出 token 限制 |
| `modalities` | 输入/输出模态（文本、图像、音频） |
| `capabilities` | 标志：attachment、reasoning、temperature、tool_call、streaming |
| `knowledge` | 训练数据截止日期 |

## 为什么选择 llm-kernel？

| | llm-kernel | [rig] | [langchain-rust] |
|--|-----------|-------|-------------------|
| 供应商目录 | ✅ 16 个供应商，114 个模型内置 | 手动配置 | 手动配置 |
| 特性门控 | ✅ 20 个独立模块 | 单体式 | 单体式 |
| 本地嵌入 | ✅ 44 个 ONNX + Qwen3 + Nomic MoE | ❌ | ❌ |
| 质量评估 | ✅ 5 个模块、基线回归检测、CI | ❌ | ❌ |
| MCP 服务器 | ✅ JSON-RPC 2.0 | ❌ | ❌ |
| 知识图谱 | ✅ SQLite + FTS5 + 智能召回 | ❌ | ❌ |
| 必需依赖 | 仅 `serde` | `reqwest`、`tokio`、… | 很多 |
| 链式调用 / 代理 | ❌ | ✅ | ✅ |
| RAG 管道 | ❌ | ✅ | ✅ |

[rig]: https://github.com/0xPlaygrounds/rig
[langchain-rust]: https://github.com/Abraxas-365/langchain-rust

llm-kernel 是一个**轻量级基础层** — 当你需要链式调用、代理或 RAG 时，可以与 rig 或 langchain-rust 组合使用。

## 架构

```
┌──────────────────────────────────────────┐
│              Your app                    │
├──────────────────────────────────────────┤
│               prelude                    │  ← use llm_kernel::prelude::*;
├───────────────┬──────────┬───────────────┤
│   provider    │  client  │   discovery   │  ← 目录、异步 LLM、模型发现
│   catalog     │  async   │               │
├───────────────┴──────────┴───────────────┤
│  graph  │  mcp  │  embedding  │  search  │  ← 图谱、MCP、ONNX/Qwen3/Nomic 嵌入、RRF
├──────────────────────────────────────────┤
│ tokens │ telemetry │ safety │ install    │  ← token 估算、事件、遮蔽、向导
├──────────────────────────────────────────┤
│    secrets    │   config   │   store     │  ← 保险库、TOML、SQLite 基础设施
└──────────────────────────────────────────┘
```

- **`LLMClient` trait** — `OpenAIClient` 和 `AnthropicClient` 的统一接口
- **`EmbeddingProvider` trait** — `FastembedProvider`（ONNX）、`Qwen3Provider`（candle）、`NomicMoeProvider`（candle）、`OpenAIEmbeddingClient`（远程）的统一接口
- **`ProviderIndex`** — 对内嵌目录的零拷贝访问，可按供应商或模型查询
- **`McpServer`** — JSON-RPC 2.0 服务器，支持 stdio 传输、Bearer 认证、工具注册
- **`SecretVault`** — `HashMap<String, String>`，支持 dotenv 加载/保存和符号链接保护
- **`graph`** — SQLite 知识图谱，包含 FTS5 搜索、复合评分召回、BFS 遍历、重要性衰减
- **`TelemetryEvent`** — 枚举门控变体，用于结构化可观测性（不含 PII）
- **`safety`** — 密钥遮蔽、错误分类、双向/ANSI/null 净化

## 质量评估

内置评估 CLI 使用精选测试数据集测量模块质量:

```bash
# 运行所有评估（token、安全、嵌入、搜索）
cargo run --bin llm-kernel-eval --features eval -- all

# 包含图谱评估
cargo run --bin llm-kernel-eval --features eval-full -- all

# 与基线快照进行回归检查（检测到回归时 exit 1）
cargo run --bin llm-kernel-eval --features eval-full -- --baseline eval/baseline.json all

# JSON 输出用于工具集成
cargo run --bin llm-kernel-eval --features eval -- --format json all
```

| 模块 | 指标 |
|------|------|
| tokens | MAE, max_error, %±3, %±10%, 按类别分析 |
| safety | exact_match_rate, precision, recall, F1, missed_secrets |
| embedding | identity_accuracy, orthogonality, symmetry, bounds |
| search | precision@5, recall@5, MRR |
| graph | precision, recall, F1（按查询类型） |

传入 `--baseline eval/baseline.json` 可与基准快照比较 — 检测到指标回归时以 exit code 1 退出。CI 在每次 push 和 PR 时自动运行 `eval` 任务。

## 基准测试

`benches/` 目录下的 Criterion 基准测试：

```bash
cargo bench                          # 运行所有基准测试
cargo bench -- graph_bench           # 图谱：smart_recall、BFS、邻居查询
cargo bench -- compute_bench         # token 估算、RRF 融合
```

## 示例

```bash
# 列出所有供应商和模型（无需 API 密钥）
cargo run --example provider_list

# OpenAI 聊天（需要 OPENAI_API_KEY）
cargo run --example chat_openai --features client-async

# Anthropic 流式传输（需要 ANTHROPIC_API_KEY）
cargo run --example stream_anthropic --features client-async
```

## 系统要求

- Rust 1.92+（edition 2024）

## 参与贡献

参见 [CONTRIBUTING.md](CONTRIBUTING.md)。欢迎提交 PR。

## 许可证

[Apache-2.0](LICENSE) © 2026 EpicCounty
