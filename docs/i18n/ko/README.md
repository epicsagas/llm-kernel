<!-- Translated from README.md @ commit edd4827 (2026-06-06) -->
<!-- If English README has changed since then, this translation may be outdated -->

[English](../../README.md) | **한국어** | [日本語](../ja/README.md) | [简体中文](../zh-Hans/README.md) | [繁體中文](../zh-Hant/README.md) | [Español](../es/README.md) | [Français](../fr/README.md) | [Deutsch](../de/README.md) | [Português](../pt/README.md) | [Русский](../ru/README.md) | [Italiano](../it/README.md)

> 이 문서는 [README.md](../../README.md)의 한국어 번역입니다.
> 영어 원문이 권위 있는 출처이며, 더 최신 내용을 담고 있을 수 있습니다.

<div align="center">

# llm-kernel

> Rust AI 네이티브 애플리케이션을 위한 기반 라이브러리 — 프로바이더 카탈로그, LLM 클라이언트, MCP 서버, 검색, 원격 측정, 안전 유틸리티

[![CI](https://img.shields.io/github/actions/workflow/status/epicsagas/llm-kernel/ci.yml?style=for-the-badge&labelColor=0d1117&color=2ecc71&logo=github-actions&logoColor=white)](https://github.com/epicsagas/llm-kernel/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-kernel?style=for-the-badge&labelColor=0d1117&color=fc8d62&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)
[![docs.rs](https://img.shields.io/docsrs/llm-kernel?style=for-the-badge&labelColor=0d1117&color=58a6ff&logo=docs.rs&logoColor=white)](https://docs.rs/llm-kernel)
[![License](https://img.shields.io/badge/license-Apache--2.0-3fb950?style=for-the-badge&labelColor=0d1117)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/llm-kernel?style=for-the-badge&labelColor=0d1117&color=bc8cff&logo=rust&logoColor=white)](https://crates.io/crates/llm-kernel)

</div>

## 개요

llm-kernel은 Rust로 LLM 기반 도구, 에이전트, 서버를 구축하기 위한 기반 계층을 제공합니다:

- **프로바이더 카탈로그** — 16개 내장 프로바이더, 114개 모델의 메타데이터, 가격, 기능 정보
- **비동기 클라이언트** — trait 기반 OpenAI/Anthropic 클라이언트, SSE 스트리밍 지원
- **모델 탐색** — models.dev, Ollama, OpenAI 호환 엔드포인트에서 동적 모델 탐색
- **자격 증명 금고** — dotenv 방식 API 키 관리, 원자적 쓰기 지원
- **설정 로더** — TOML 설정 로더, 템플릿에서 자동 생성
- **지식 그래프** — SQLite 기반 그래프, FTS5 검색, 스마트 리콜, BFS 순회, 비동기 래퍼
- **MCP 서버** — JSON-RPC 2.0 서버 프레임워크, stdio 전송 및 Bearer 인증
- **임베딩** — 프로바이더 trait + 코사인 유사도, 로컬 ONNX (44개 모델), Qwen3 candle, Nomic V2 MoE candle, OpenAI 원격 ([전체 모델 목록 →](../../EMBEDDING_MODELS.md))
- **검색** — 하이브리드 검색 결과 병합을 위한 Reciprocal Rank Fusion
- **토큰 추정** — 외부 의존성 없는 유니코드 스크립트 휴리스틱 토큰 계산
- **원격 측정** — PII 미포함 enum 게이트 이벤트, 콘솔 및 noop 싱크
- **안전** — 비밀 마스킹, 오류 분류, 출력 새니타이제이션
- **설치 마법사** — Claude Desktop, Cursor, Copilot, OpenCode, Cline용 MCP 설정 생성

## 기능 플래그

각 모듈은 기능 플래그로 제어되어 필요한 것만 사용할 수 있습니다.

| 기능 | 설명 | 기본 |
|------|------|------|
| `provider` | 프로바이더 카탈로그, 모델 설명자, 가격 | ✅ |
| `client-async` | 비동기 LLM 클라이언트 (reqwest), 스트리밍 | |
| `discovery` | 동적 모델 탐색 (models.dev, Ollama, OpenAI-compat) | |
| `discovery-async` | 비동기 모델 탐색 — reqwest 기반 `DiscoverySource` trait | |
| `secrets` | SecretVault 자격 증명 관리 | |
| `store` | SQLite 초기화 헬퍼 (WAL, FTS5, 스키마 버전 관리) | |
| `config` | TOML 설정 로더 | |
| `graph` | 지식 그래프 — SQLite, FTS5, 스마트 리콜, BFS 순회 | |
| `graph-async` | 비동기 그래프 래퍼 (tokio 필요) | |
| `graph-pool` | 다중 연결 비동기 그래프 풀 (`AsyncPoolGraph`, WAL 동시성) | |
| `graph-cjk` | CJK-aware graph search via Rust-side segmentation (no schema change) | |
| `graph-pg` | PostgreSQL GraphBackend (PgGraph) + SQLite<->PostgreSQL 마이그레이션 CLI | |
| `mcp` | MCP 서버 — JSON-RPC 2.0, stdio 전송, Bearer 인증 | |
| `mcp-http` | MCP remote transport — HTTP/SSE (axum + tokio) | |
| `cache` | LLM response cache — `CacheClient` over `KvStore` | |
| `tokens` | 토큰 추정, 예산 관리, 문장 경계 문서 청킹 | |
| `install` | AI 도구 설치 마법사 | |
| `search` | 하이브리드 검색 — `SearchProvider` trait, RRF / 가중합 / CombMNZ 퓨전 | |
| `embedding` | 임베딩 프로바이더 trait + 코사인 유사도 + AsyncVectorIndex trait (VectorIndex의 비동기 대응) | |
| `embedding-openai` | OpenAI 텍스트 임베딩 클라이언트 (동기 HTTP) | |
| `embedding-fastembed` | fastembed-rs를 통한 로컬 ONNX 임베딩 (44개 모델) | |
| `embedding-fastembed-qwen3` | candle 백엔드를 통한 Qwen3 임베딩 | |
| `embedding-fastembed-nomic-moe` | candle 백엔드를 통한 Nomic V2 MoE 임베딩 | |
| `vector-index` | TurboQuant 압축 벡터 인덱스 — 2비트/4비트, SIMD ANN 검색 | |
| `qdrant` | 원격 벡터 검색용 Qdrant AsyncVectorIndex (QdrantVectorIndex) | |
| `elastic` | 직접 구현한 reqwest 클라이언트 기반 Elasticsearch AsyncVectorIndex (ElasticsearchVectorIndex) | |
| `federation` | 크로스 엔진 연합 — 여러 `AsyncVectorIndex` 백엔드 동시 쿼리, 백엔드별 타임아웃(기본 RRF) | |
| `telemetry` | enum 게이트 원격 측정 이벤트, PII 미포함 | |
| `safety` | 비밀 마스킹, 오류 분류, 출력 새니타이제이션, 프롬프트 인젝션 탐지 | |
| `eval` | 품질 평가 CLI — 토큰, 안전, 임베딩, 검색 | |
| `eval-full` | 그래프 포함 전체 평가 모듈 | |
| `full` | 모든 기능 | |

## 빠른 시작

`Cargo.toml`에 추가:

```toml
[dependencies]
llm-kernel = "0.9.0"
```

`provider` 기능이 기본으로 활성화됩니다. 비동기 클라이언트를 사용하려면:

```toml
[dependencies]
llm-kernel = { version = "0.9.0", features = ["client-async"] }
```

비동기 래퍼가 포함된 지식 그래프를 사용하려면:

```toml
[dependencies]
llm-kernel = { version = "0.9.0", features = ["graph", "graph-async"] }
```

로컬 임베딩을 사용하려면 (ONNX, API 키 불필요):

```toml
[dependencies]
llm-kernel = { version = "0.9.0", features = ["embedding-fastembed"] }
```

## 사용법

### 프로바이더 카탈로그

내장 카탈로그는 [models.dev](https://github.com/anomalyco/models.dev) 스키마에 맞춘 16개 프로바이더와 114개 모델을 포함합니다.

```rust
use llm_kernel::prelude::*;

let catalog = ProviderIndex::embedded();

// 모든 프로바이더 나열
for id in catalog.ids() {
    let provider = catalog.get(&id).unwrap();
    println!("{}", provider.display_name);
}

// 프로바이더의 모델 조회
for model in catalog.models_for("openai") {
    println!("  {} — ${:.2}/1M in", model.id, model.cost.unwrap().input);
}

// 특정 모델 찾기
if let Some(model) = catalog.find_model("claude-sonnet-4-20250514") {
    println!("Context: {} tokens", model.limit.unwrap().context);
}
```

### 비동기 채팅 완성

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

### 스트리밍

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

// 스트림은 Delta, Usage, Done 이벤트를 생성합니다
```

### 모델 탐색

```rust
use llm_kernel::discovery::{fetch_and_cache, load_cache, fetch_ollama_models};

// models.dev에서 가져오기 (디스크에 캐시)
let payload = fetch_and_cache("~/.cache/llm-kernel/models-dev.json")?;
for model in &payload.models {
    println!("{} — {} (ctx: {:?})", model.id, model.provider_id, model.limits);
}

// 캐시에서 로드 (네트워크 없음)
if let Some(cached) = load_cache("~/.cache/llm-kernel/models-dev.json")? {
    println!("{} models cached", cached.models.len());
}

// 로컬 Ollama 모델 탐색
let ollama_models = fetch_ollama_models("http://localhost:11434")?;
for name in &ollama_models {
    println!("Ollama: {}", name);
}
```

### 자격 증명 금고

```rust
use llm_kernel::prelude::*;

let vault = SecretVault::load_from("~/.config/myapp/.env")?;
vault.set("OPENAI_API_KEY", "sk-...");
vault.save_to("~/.config/myapp/.env")?;

// 로깅용 자격 증명 마스킹
println!("{}", redact_credential("sk-abcdef1234567890"));
// → "sk-abcd...7890"
```

### TOML 설정

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

### SQLite 저장소

```rust
use llm_kernel::store::init_schema;

let ddl = "CREATE TABLE items (id TEXT PRIMARY KEY, content TEXT);";
let conn = init_schema(&db_path, ddl, 1)?;
// WAL 모드, busy 타임아웃, 스키마 버전 관리가 자동으로 적용됩니다
```

### 지식 그래프

```rust
use llm_kernel::prelude::*;
use rusqlite::Connection;

let conn = Connection::open_in_memory().unwrap();
init_graph_schema(&conn).unwrap();

// 노드 생성
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

// 엣지로 연결
append_edge(&conn, &GraphEdge {
    id: "e1".into(),
    source: "rust-ownership".into(),
    target: "borrow-checker".into(),
    relation: "related".into(),
    weight: 1.5,
    ts: "2026-01-01T00:00:00Z".into(),
}).unwrap();

// 복합 점수 기반 스마트 리콜
let results = smart_recall(&conn, Some("my-project"), Some("ownership"), 5).unwrap();
for scored in &results {
    println!("{:.2} — {}", scored.score, scored.node.title);
}

// 수명 주기 관리
decay_importance(&conn, 30, 0.9, 0.05).unwrap();
tag_stale_nodes(&conn, 90).unwrap();
let stats = compute_stats(&conn).unwrap();
println!("{} nodes, {} edges", stats.total_nodes, stats.total_edges);
```

### MCP 서버

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

// Bearer 인증과 함께 stdio를 통해 JSON-RPC 2.0 실행
server.run_stdio().await?;
```

### 토큰 추정

```rust
use llm_kernel::tokens::estimate_tokens;

let tokens = estimate_tokens("Hello, world! こんにちは世界 🌍");
println!("Estimated tokens: {}", tokens);
```

### 임베딩 + 검색

```rust
use llm_kernel::embedding::{EmbeddingProvider, cosine_similarity};
use llm_kernel::search::{SearchResult, rrf_fuse};

// 벡터 간 코사인 유사도
let sim = cosine_similarity(&[0.1, 0.2, 0.3], &[0.4, 0.5, 0.6]);

// 하이브리드 검색을 위한 Reciprocal Rank Fusion
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

#### 로컬 ONNX 임베딩 (fastembed-rs)

ONNX Runtime을 통한 44개 모델 — API 키 없이, 최초 다운로드 후 네트워크 불필요.

```rust
use llm_kernel::embedding::{EmbeddingModel, FastembedProvider, EmbeddingProvider};

let provider = FastembedProvider::new(EmbeddingModel::BGESmallENV15, None)?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 384);
```

#### Qwen3 임베딩 (candle)

candle-nn를 통한 순수 Rust GPU/CPU 추론 — ONNX Runtime 불필요.

```rust
use llm_kernel::embedding::{Qwen3Provider, EmbeddingProvider};

let provider = Qwen3Provider::new("Qwen/Qwen3-Embedding-0.6B")?;
let result = provider.embed("hello world")?;
```

#### Nomic V2 MoE 임베딩 (candle)

경량 MoE 모델 — 8개 전문가, top-2 라우팅, 305M 활성 파라미터.

```rust
use llm_kernel::embedding::{NomicMoeProvider, EmbeddingProvider};

let provider = NomicMoeProvider::new()?;
let result = provider.embed("hello world")?;
assert_eq!(result.vector.len(), 768);
```

### 안전 유틸리티

```rust
use llm_kernel::safety::{mask_secrets, classify_failure, sanitize_output};

// 로그에서 비밀 마스킹
let safe = mask_secrets("Authorization: Bearer sk-abcdef123456");
// → "Authorization: Bearer [REDACTED]"

// 오류 분류
let category = classify_failure("connection timed out after 30s");
// → ErrorCategory::Timeout

// 신뢰할 수 없는 출력 새니타이즈
let clean = sanitize_output(user_input)?;
```

## 모델 메타데이터

카탈로그의 각 모델은 다음 정보를 포함합니다:

| 필드 | 설명 |
|------|------|
| `cost` | 백만 토큰당 가격 (input, output, cache_read, cache_write) |
| `limit` | 컨텍스트 및 출력 토큰 제한 |
| `modalities` | 입력/출력 모달리티 (text, image, audio) |
| `capabilities` | 플래그: attachment, reasoning, temperature, tool_call, streaming |
| `knowledge` | 학습 데이터 기준일 |

## 왜 llm-kernel인가?

| | llm-kernel | [rig] | [langchain-rust] |
|--|-----------|-------|-------------------|
| 프로바이더 카탈로그 | ✅ 16개 프로바이더, 114개 모델 내장 | 수동 설정 | 수동 설정 |
| 기능 게이트 | ✅ 독립 모듈 | 모놀리식 | 모놀리식 |
| 로컬 임베딩 | ✅ 44개 ONNX + Qwen3 + Nomic MoE | ❌ | ❌ |
| 품질 평가 | ✅ 5개 모듈, 베이스라인 회귀 감지, CI | ❌ | ❌ |
| MCP 서버 | ✅ JSON-RPC 2.0 | ❌ | ❌ |
| 지식 그래프 | ✅ SQLite + FTS5 + 스마트 리콜 | ❌ | ❌ |
| 필수 의존성 | `serde`만 | `reqwest`, `tokio`, … | 다수 |
| 체인 / 에이전트 | ❌ | ✅ | ✅ |
| RAG 파이프라인 | ❌ | ✅ | ✅ |

[rig]: https://github.com/0xPlaygrounds/rig
[langchain-rust]: https://github.com/Abraxas-365/langchain-rust

llm-kernel은 **경량 기반 계층**입니다 — 체인, 에이전트 또는 RAG이 필요할 때는 rig이나 langchain-rust와 조합하여 사용하세요.

## 아키텍처

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

- **`LLMClient` trait** — `OpenAIClient`과 `AnthropicClient`의 통합 인터페이스
- **`EmbeddingProvider` trait** — `FastembedProvider` (ONNX), `Qwen3Provider` (candle), `NomicMoeProvider` (candle), `OpenAIEmbeddingClient` (원격)의 통합 인터페이스
- **`ProviderIndex`** — 내장 카탈로그에 대한 제로 카피 접근, 프로바이더 또는 모델별 조회
- **`McpServer`** — stdio 전송, Bearer 인증, 도구 등록을 갖춘 JSON-RPC 2.0 서버
- **`SecretVault`** — dotenv 로드/저장과 심볼릭 링크 가드가 있는 `HashMap<String, String>`
- **`graph`** — FTS5 검색, 복합 점수 리콜, BFS 순회, 중요도 감쇠를 갖춘 SQLite 지식 그래프
- **`TelemetryEvent`** — 구조화된 관측 가능성을 위한 enum 게이트 변형 (PII 미포함)
- **`safety`** — 비밀 마스킹, 오류 분류, 양방향/ANSI/null 새니타이제이션

## 품질 평가

내장 평가 CLI이 큐레이션된 테스트 데이터셋으로 모듈 품질을 측정합니다:

```bash
# 모든 평가 실행 (토큰, 안전, 임베딩, 검색)
cargo run --bin llm-kernel-eval --features eval -- all

# 그래프 평가 포함
cargo run --bin llm-kernel-eval --features eval-full -- all

# 베이스라인 스냅샷과 회귀 확인 (회귀 시 exit 1)
cargo run --bin llm-kernel-eval --features eval-full -- --baseline eval/baseline.json all

# 도구용 JSON 출력
cargo run --bin llm-kernel-eval --features eval -- --format json all
```

| 모듈 | 메트릭 |
|------|--------|
| tokens | MAE, max_error, %±3, %±10%, 카테고리별 분석 |
| safety | exact_match_rate, precision, recall, F1, missed_secrets |
| embedding | identity_accuracy, orthogonality, symmetry, bounds |
| search | precision@5, recall@5, MRR |
| graph | precision, recall, F1 (쿼리 유형별) |

`--baseline eval/baseline.json`을 전달하면 골든 스냅샷과 비교합니다 — 메트릭 회귀 발생 시 exit code 1로 종료합니다. CI는 모든 push와 PR에서 `eval` 잡으로 자동 실행합니다.

## 벤치마크

`benches/` 디렉토리에 Criterion 벤치마크가 있습니다:

```bash
cargo bench                          # 모든 벤치마크 실행
cargo bench -- graph_bench           # 그래프: smart_recall, BFS, 이웃 조회
cargo bench -- compute_bench         # 토큰 추정, RRF 퓨전
```

## 예제

```bash
# 모든 프로바이더와 모델 나열 (API 키 불필요)
cargo run --example provider_list

# OpenAI 채팅 (OPENAI_API_KEY 필요)
cargo run --example chat_openai --features client-async

# Anthropic 스트리밍 (ANTHROPIC_API_KEY 필요)
cargo run --example stream_anthropic --features client-async
```

## 요구 사항

- Rust 1.92+ (edition 2024)

## 기여

[CONTRIBUTING.md](../../CONTRIBUTING.md)를 참조하세요. PR을 환영합니다.

## 라이선스

[Apache-2.0](../../LICENSE) © 2026 EpicCounty
