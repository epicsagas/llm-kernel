<!-- Translated from README.md @ commit edd4827 (2026-06-06) -->
<!-- If English README has changed since then, this translation may be outdated -->

[English](../../README.md) | [한국어](../ko/README.md) | **日本語** | [简体中文](../zh-Hans/README.md) | [繁體中文](../zh-Hant/README.md) | [Español](../es/README.md) | [Français](../fr/README.md) | [Deutsch](../de/README.md) | [Português](../pt/README.md) | [Русский](../ru/README.md) | [Italiano](../it/README.md)

> この文書は [README.md](../../README.md) の日本語翻訳です。
> 英語版が権威ある情報源であり、より最新の情報が含まれている場合があります。

<div align="center">

# llm-kernel

> Rust AIネイティブアプリ向けの基盤ライブラリ — プロバイダーカタログ、LLMクライアント、MCPサーバー、検索、テレメトリ、セーフティ

[![CI](https://img.shields.io/github/actions/workflow/status/epicsagas/llm-kernel/ci.yml?style=for-the-badge&labelColor=0d1117&color=2ecc71&logo=github-actions&logoColor=white)](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-kernel?style=for-the-badge&labelColor=0d1117&color=fc8d62&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)
[![docs.rs](https://img.shields.io/docsrs/llm-kernel?style=for-the-badge&labelColor=0d1117&color=58a6ff&logo=docs.rs&logoColor=white)](https://docs.rs/llm-kernel)
[![License](https://img.shields.io/badge/license-Apache--2.0-3fb950?style=for-the-badge&labelColor=0d1117)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/llm-kernel?style=for-the-badge&labelColor=0d1117&color=bc8cff&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)

</div>

## 概要

llm-kernelは、RustでLLM搭載ツール、エージェント、サーバーを構築するための基盤レイヤーを提供します：

- **プロバイダーカタログ** — 16の組み込みプロバイダー、114モデルのメタデータ、価格情報、機能プロファイル
- **非同期クライアント** — トレイトベースのOpenAI/Anthropicクライアント、SSEストリーミング対応
- **モデルディスカバリー** — models.dev、Ollama、OpenAI互換エンドポイントからの動的モデル検出
- **クレデンシャル保管庫** — dotenv形式のAPIキー管理、アトミック書き込み対応
- **設定ローダー** — テンプレートからの自動生成付きTOML設定
- **ナレッジグラフ** — SQLiteベースのグラフ、FTS5検索、スマートリコール、BFSトラバーサル、非同期ラッパー
- **MCPサーバー** — JSON-RPC 2.0サーバーフレームワーク、stdioトランスポート、Bearer認証
- **エンベディング** — プロバイダートレイト + コサイン類似度、ローカルONNX（44モデル）、Qwen3 candle、Nomic V2 MoE candle、OpenAIリモート（[全モデル一覧 →](../../EMBEDDING_MODELS.md)）
- **検索** — ハイブリッド検索結果のマージにReciprocal Rank Fusionを使用
- **トークン推定** — 依存関係ゼロのUnicodeスクリプトヒューリスティックによるトークンカウント
- **テレメトリ** — PIIを含まないenumゲート方式のイベント、コンソールおよびnoopシンク
- **セーフティ** — シークレットマスキング、エラー分類、出力サニタイズ
- **インストールウィザード** — Claude Desktop、Cursor、Copilot、OpenCode、Cline向けMCP設定生成

## フィーチャーフラグ

各モジュールはフィーチャーフラグで制御されており、必要なものだけを利用できます。

| フィーチャー | 説明 | デフォルト |
|---------|-------------|---------|
| `provider` | プロバイダーカタログ、モデル記述子、価格情報 | ✅ |
| `client-async` | 非同期LLMクライアント（reqwest）、ストリーミング対応 | |
| `discovery` | 動的モデル検出（models.dev、Ollama、OpenAI互換） | |
| `secrets` | SecretVaultクレデンシャル管理 | |
| `store` | SQLite初期化ヘルパー（WAL、FTS5、スキーマバージョニング） | |
| `config` | TOML設定ローダー | |
| `graph` | ナレッジグラフ — SQLite、FTS5、スマートリコール、BFSトラバーサル | |
| `graph-async` | 非同期グラフラッパー（tokioが必要） | |
| `graph-pool` | マルチ接続非同期グラフプール（`AsyncPoolGraph`、WAL同時実行） | |
| `mcp` | MCPサーバー — JSON-RPC 2.0、stdioトランスポート、Bearer認証 | |
| `tokens` | Unicodeスクリプトヒューリスティックによるトークン推定 | |
| `install` | AIツールインストールウィザード | |
| `search` | Reciprocal Rank Fusionによるハイブリッド検索 | |
| `embedding` | エンベディングプロバイダートレイト + コサイン類似度 | |
| `embedding-openai` | OpenAI text-embeddingクライアント（同期HTTP） | |
| `embedding-fastembed` | fastembed-rsによるローカルONNXエンベディング（44モデル） | |
| `embedding-fastembed-qwen3` | candleバックエンドによるQwen3エンベディング | |
| `embedding-fastembed-nomic-moe` | candleバックエンドによるNomic V2 MoEエンベディング | |
| `vector-index` | TurboQuant圧縮ベクトルインデックス — 2ビット/4ビット、SIMD ANN検索 | |
| `telemetry` | enumゲート方式のテレメトリイベント、PIIなし | |
| `safety` | シークレットマスキング、エラー分類、出力サニタイズ | |
| `eval` | 品質評価CLI — トークン、セーフティ、エンベディング、検索 | |
| `eval-full` | グラフを含む全評価モジュール | |
| `full` | 全フィーチャー | |

## クイックスタート

`Cargo.toml`に追加：

```toml
[dependencies]
llm-kernel = "0.3.4"
```

`provider`フィーチャーはデフォルトで有効です。非同期クライアントを使用する場合：

```toml
[dependencies]
llm-kernel = { version = "0.3.4", features = ["client-async"] }
```

非同期ラッパー付きナレッジグラフを使用する場合：

```toml
[dependencies]
llm-kernel = { version = "0.3.4", features = ["graph", "graph-async"] }
```

ローカルエンベディング（ONNX、APIキー不要）を使用する場合：

```toml
[dependencies]
llm-kernel = { version = "0.3.4", features = ["embedding-fastembed"] }
```

## 使用方法

### プロバイダーカタログ

組み込みカタログには16のプロバイダーと114のモデルが含まれており、[models.dev](https://github.com/anomalyco/models.dev)スキーマに準拠しています。

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

### 非同期チャット補完

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

### ストリーミング

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

### モデルディスカバリー

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

### クレデンシャル保管庫

```rust
use llm_kernel::prelude::*;

let vault = SecretVault::load_from("~/.config/myapp/.env")?;
vault.set("OPENAI_API_KEY", "sk-...");
vault.save_to("~/.config/myapp/.env")?;

// Redact credentials for logging
println!("{}", redact_credential("sk-abcdef1234567890"));
// → "sk-abcd...7890"
```

### TOML設定

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

### SQLiteストア

```rust
use llm_kernel::store::init_schema;

let ddl = "CREATE TABLE items (id TEXT PRIMARY KEY, content TEXT);";
let conn = init_schema(&db_path, ddl, 1)?;
// WAL mode, busy timeout, and schema versioning applied automatically
```

### ナレッジグラフ

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

### MCPサーバー

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

### トークン推定

```rust
use llm_kernel::tokens::estimate_tokens;

let tokens = estimate_tokens("Hello, world! こんにちは世界 🌍");
println!("Estimated tokens: {}", tokens);
```

### エンベディング + 検索

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

#### ローカルONNXエンベディング（fastembed-rs）

ONNX Runtime経由で44モデルを利用可能 — APIキー不要、初回ダウンロード後はネットワーク不要。

```rust
use llm_kernel::embedding::{EmbeddingModel, FastembedProvider, EmbeddingProvider};

let provider = FastembedProvider::new(EmbeddingModel::BGESmallENV15, None)?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 384);
```

#### Qwen3エンベディング（candle）

candle-nnによるPure Rust GPU/CPU推論 — ONNX Runtime不要。

```rust
use llm_kernel::embedding::{Qwen3Provider, EmbeddingProvider};

let provider = Qwen3Provider::new("Qwen/Qwen3-Embedding-0.6B")?;
let result = provider.embed("hello world")?;
```

#### Nomic V2 MoEエンベディング（candle）

軽量MoEモデル — 8エキスパート、top-2ルーティング、305Mアクティブパラメータ。

```rust
use llm_kernel::embedding::{NomicMoeProvider, EmbeddingProvider};

let provider = NomicMoeProvider::new()?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 768);
```

### セーフティユーティリティ

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

## モデルメタデータ

カタログ内の各モデルには以下が含まれます：

| フィールド | 説明 |
|-------|-------------|
| `cost` | 100万トークンあたりの価格（入力、出力、cache_read、cache_write） |
| `limit` | コンテキストおよび出力トークン制限 |
| `modalities` | 入力/出力モダリティ（テキスト、画像、音声） |
| `capabilities` | フラグ：attachment、reasoning、temperature、tool_call、streaming |
| `knowledge` | 学習データのカットオフ日 |

## なぜllm-kernel？

| | llm-kernel | [rig] | [langchain-rust] |
|--|-----------|-------|-------------------|
| プロバイダーカタログ | ✅ 16プロバイダー、114モデル組み込み | 手動設定 | 手動設定 |
| フィーチャーゲート | ✅ 20の独立モジュール | モノリシック | モノリシック |
| ローカルエンベディング | ✅ 44 ONNX + Qwen3 + Nomic MoE | ❌ | ❌ |
| 品質評価 | ✅ 5モジュール、ベースライン回帰検出、CI | ❌ | ❌ |
| MCPサーバー | ✅ JSON-RPC 2.0 | ❌ | ❌ |
| ナレッジグラフ | ✅ SQLite + FTS5 + スマートリコール | ❌ | ❌ |
| 必須依存関係 | `serde`のみ | `reqwest`、`tokio`、… | 多数 |
| チェーン / エージェント | ❌ | ✅ | ✅ |
| RAGパイプライン | ❌ | ✅ | ✅ |

[rig]: https://github.com/0xPlaygrounds/rig
[langchain-rust]: https://github.com/Abraxas-365/langchain-rust

llm-kernelは**軽量な基盤レイヤー**です — チェーン、エージェント、RAGが必要な場合はrigやlangchain-rustと組み合わせてご利用ください。

## アーキテクチャ

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

- **`LLMClient`トレイト** — `OpenAIClient`と`AnthropicClient`の統一インターフェース
- **`EmbeddingProvider`トレイト** — `FastembedProvider`（ONNX）、`Qwen3Provider`（candle）、`NomicMoeProvider`（candle）、`OpenAIEmbeddingClient`（リモート）の統一インターフェース
- **`ProviderIndex`** — 組み込みカタログへのゼロコピーアクセス、プロバイダーまたはモデルでクエリ可能
- **`McpServer`** — JSON-RPC 2.0サーバー、stdioトランスポート、Bearer認証、ツール登録
- **`SecretVault`** — `HashMap<String, String>`ベース、dotenvロード/セーブ、シンボリックリンクガード付き
- **`graph`** — SQLiteナレッジグラフ、FTS5検索、複合スコアリングリコール、BFSトラバーサル、重要度減衰
- **`TelemetryEvent`** — 構造化オブザーバビリティのためのenumゲートバリアント（PIIなし）
- **`safety`** — シークレットマスキング、エラー分類、双方向/ANSI/nullサニタイズ

## 品質評価

組み込み評価CLIにより、キュレーションされたテストデータセットでモジュール品質を測定します:

```bash
# 全評価の実行（トークン、セーフティ、エンベディング、検索）
cargo run --bin llm-kernel-eval --features eval -- all

# グラフ評価を含む
cargo run --bin llm-kernel-eval --features eval-full -- all

# ベースラインスナップショットとの回帰チェック（回帰検出時にexit 1）
cargo run --bin llm-kernel-eval --features eval-full -- --baseline eval/baseline.json all

# ツール連携用JSON出力
cargo run --bin llm-kernel-eval --features eval -- --format json all
```

| モジュール | メトリクス |
|------|--------|
| tokens | MAE, max_error, %±3, %±10%, カテゴリ別内訳 |
| safety | exact_match_rate, precision, recall, F1, missed_secrets |
| embedding | identity_accuracy, orthogonality, symmetry, bounds |
| search | precision@5, recall@5, MRR |
| graph | precision, recall, F1（クエリタイプ別） |

`--baseline eval/baseline.json`を渡すとゴールデンスナップショットと比較し、メトリクスの回帰を検出した場合はexit code 1で終了します。CIは全pushとPRで`eval`ジョブとして自動実行されます。

## ベンチマーク

`benches/`ディレクトリにCriterionベンチマークが含まれています：

```bash
cargo bench                          # Run all benchmarks
cargo bench -- graph_bench           # Graph: smart_recall, BFS, neighbors
cargo bench -- compute_bench         # Token estimation, RRF fusion
```

## 例

```bash
# List all providers and models (no API key needed)
cargo run --example provider_list

# OpenAI chat (requires OPENAI_API_KEY)
cargo run --example chat_openai --features client-async

# Anthropic streaming (requires ANTHROPIC_API_KEY)
cargo run --example stream_anthropic --features client-async
```

## 要件

- Rust 1.92以上（edition 2024）

## コントリビューション

[CONTRIBUTING.md](../../CONTRIBUTING.md)をご覧ください。PRを歓迎します。

## ライセンス

[Apache-2.0](LICENSE) © 2026 EpicCounty
