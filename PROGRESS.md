# Progress

> Auto-generated status snapshot. Last updated: 2026-07-10

## Current Version: v0.18.0

| Metric | Value |
|--------|-------|
| Version | `0.18.0` |
| Edition | Rust 2024, MSRV 1.92 |
| Lines of code | ~25,400 |
| Total tests | `--features full`: 627 passed, 14 ignored, 0 failed |
| Backend features | `graph-pg` (PostgreSQL), `graph-pg-tls` (TLS), `qdrant` (vector search), `elastic` (Elasticsearch vector search), `pgvector` (PostgreSQL vector search), `mcp-http` (remote MCP) |
| Last commit | `feat(v1-readiness): issue #45 + v1.0.0 readiness #1/#2/#3/#5/#6 (#64)` |

---

## Recent Releases

### v0.18.0 (2026-07-10)

- **graph** (`graph-pool`, issue #45 axis E): `AsyncPoolGraph::open` enables WAL on the file + applies `busy_timeout`/`synchronous=NORMAL` per connection — the pool's "concurrent reads during writes" claim previously did not hold (DELETE journal, no busy timeout). Measured 1.8× faster 16-reader wave under writer.
- **eval** (#45 axis D): `graph-korean` scenario quantifying `graph-cjk` vs FTS5 `trigram` Korean recall (1.000 vs 0.286 @k=5) + `--strict` CI gate mode (exits non-zero on module failure/error/disappearance vs baseline).
- **ci** (#45 axis A): `bench-smoke` job (caught a real UTF-8 panic) + `.github/workflows/semver.yml` (`cargo-semver-checks`; 0.x breaking requires a minor bump).
- **api** (#1, breaking): 8 internal-only `pub` items → `pub(crate)`, dead `importance_for_type` removed; `list_node_ids`/`read_nodes_limited` stay `pub` (migrate binary / cross-feature).
- **llm** (#5, security M2): HTTP error response bodies routed through `mask_secrets` before `KernelError::Http` (prevents API-key leak via proxy-echoed error bodies).
- **docs** (#2/#3/#6): `# Example` doctests on primary entry surface; measured baselines in `docs/benchmarks/`; `docs/features.md` feature catalog + platform matrix; `docs/security-audit-2026-07.md`.

### v0.17.0 (2026-07-08)

- **embedding** (`pgvector`): `add()` switched from `push_values` to manual `VALUES` assembly with the `::vector` cast (was missing → type mismatch); Rust `add` now actually inserts (previously a Python `COPY` bypass in `klr` masked the bug)
- **embedding** (`pgvector`): `pool()` getter + `remove_in_tx(&mut PgConnection, ids)` for single-transaction atomicity (`klr` prune premise)

### v0.16.2 (2026-07-08)

- **embedding**: `embedding-fastembed-coreml` feature + `new_with_coreml()` constructor (mirrors DirectML pattern); adds the `coreml` execution-provider feature to `ort`, accelerating `bge-m3` on macOS GPU/ANE

### v0.16.1 (2026-07-08)

- **embedding** (`pgvector`): `pgvector::Vector` sqlx `Type` bind conflict (surfaced in `korean-law-rag`) — bind as a string literal (`[1,2,3]::vector`) instead of a typed `Vector`

### v0.16.0 (2026-07-08)

- **embedding** (#59): `pgvector` `AsyncVectorIndex` (`PgVectorIndex`) — PostgreSQL + the `pgvector` extension as a third async remote vector backend (cosine `<=>`, HNSW index), alongside qdrant/elastic
- **llm** (#60): `RouterClient` — cost-aware routing (`Fallback` / `LowestCost`) with cross-provider fallback; transient errors (5xx, rate-limit `429`, timeout `408`) move on, permanent 4xx short-circuits. Composes with `RetryClient` / `MiddlewareClient` / `CacheClient`
- **deps** (#61): `rusqlite` 0.40 → 0.37 (MSRV/build stability; drops the `rsqlite-vfs` transitive dependency)

### v0.15.0 (2026-07-06)

- **embedding** (#55): `embedding-fastembed-dynamic-linking` no longer pulls in `embedding-fastembed` (static ONNX download). The two features are now mutually exclusive; `fastembed`'s ort features are selected by the consuming feature, and a `compile_error!` in `src/lib.rs` makes any conflict a hard build error. The dynamic escape hatch now exposes the same API as the static path (`FastembedProvider`, `LazyFastembedProvider`, `EmbeddingCache`, `is_model_cached`, `as_fastembed`)

### v0.14.0 (2026-07-03)

A forward-compatibility release: stops the per-minor breakage caused by adding fields/variants to public types (**several changes are breaking**).
- **stability**: `Default` derived on every growable public data struct (provider, graph, mcp result types) — downstream future-proofs with `..Default::default()`
- **error** (breaking): `KernelError` is now `#[non_exhaustive]`; exhaustive `match`es need a `_ =>` arm. `KernelError::Serialization` available under any feature pulling `serde_json`
- **catalog/graph/mcp** (breaking): read-mostly catalog/result types are `#[non_exhaustive]` (`ServiceDescriptor`, `ModelDescriptor`, `GraphStats`, …) — construct via `Default::default()` + field assignment, or read from the catalog/query APIs
- **llm** (breaking): `OpenAIClient::from_key` / `AnthropicClient::from_key` now return `Result<Self>` (no silent timeout-less fallback) — append `?` at call sites

### v0.13.0 (2026-07-03)

- **error**: unified `embedding` + `discovery` public APIs onto `KernelError` (new `Embedding` / `Discovery` variants) — no more `anyhow::Result` in the library's public surface (**breaking**)
- **llm**: `LLMRequest::tools` and `response_format` are now forwarded to OpenAI and Anthropic; tool calls are parsed back into `LLMResponse::tool_calls`
- **mcp**: protocol version negotiation (2025-06-18), `ping`, prompts (`prompts/list` / `prompts/get`), string/number JSON-RPC ids, `tools/call` in-band `isError`, and camelCase wire format (`inputSchema` / `mimeType`)
- **embedding**: fixed a `LazyFastembedProvider::embed_batch` panic on a truncated provider response; `CacheClient` now offloads blocking store I/O via `spawn_blocking`
- **ci**: isolated per-feature build/test matrix entries (`mcp`, `tokens`, `safety`, `search`, `cache`, …)

### v0.12.0 (2026-07-02)

- **embedding** (breaking): `ModelState::Failed(String)` → `Failed { message, panicked }`; dropped default `ort-load-dynamic` so `embedding-fastembed` statically links ONNX Runtime, made the model-load path panic-safe via `catch_unwind`, and added `LazyFastembedProvider::reset()` + opt-in `embedding-fastembed-dynamic-linking` (#50)

### v0.11.0 (2026-07-01)

- **graph** (`graph-pg-tls`): TLS support for `PgGraph` connections — `connect_native_tls` / `connect_tls` / `connect_config_tls` (#48)

### v0.10.0 (2026-06-29)

- **graph**: pure-Rust CSR graph algorithms (`algo/`) — weighted PageRank, connected components, label propagation, Dijkstra, Jaccard/Adamic-Adar similarity; `smart_recall`'s graph boost now ranks by true PageRank centrality (SQLite + PostgreSQL share one impl)

### v0.9.0 (2026-06-15)

- **embedding** (`elastic`): `ElasticsearchVectorIndex` — `AsyncVectorIndex` over Elasticsearch 8.x (dense_vector cosine, bulk upsert/delete, knn `_search`, `_count`). 공식 `elasticsearch` 크레이트가 alpha-only라 **직접 구현한 reqwest 클라이언트** 사용 (v1.0.0 semver lock 안전)
- **search**: `FederatedSearch` — 여러 `AsyncVectorIndex` 백엔드 동시 쿼리, 백엔드별 타임아웃, 실패한 백엔드는 `tracing::warn!`으로 관찰 가능하게 drop, 기본 RRF 퓨전 (`src/search/federation.rs`)
- **search**: `FusionStrategy` enum + 순수 `federate_results` (동기 `TurbovecIndex`도 federation 참여 가능)
- **features**: 신규 `elastic` 피처 (reqwest는 `client-async` 재사용, 신규 전이 의존성 없음), `full`에 포함. 메인 크레이트 0.8.0 → 0.9.0
- **infra**: `docker-compose.yml`에 Elasticsearch 서비스 추가 (local-dev 전용, CI는 self-skip)

### v0.8.0 (2026-06-14)

- **embedding**: `AsyncVectorIndex` async trait (`VectorIndex`의 async 대응, 원격/공유 백엔드용)
- **graph-pg**: PostgreSQL `GraphBackend` (`PgGraph`, 동기 `postgres` 드라이버, ILIKE 검색, 동일 smart_recall 스코어링, 재귀 CTE BFS) — 메인 크레이트 `graph-pg` 피처
- **graph-pg**: SQLite↔PostgreSQL 마이그레이션 CLI (`llm-kernel-migrate-graph`, `--dry-run`)
- **qdrant**: `QdrantVectorIndex` (`AsyncVectorIndex` 구현, universal Query API) — 메인 크레이트 `qdrant` 피처
- **infra**: `docker-compose.yml` (docker/podman 호환), 신규 `crates` CI 잡 추가
- **graph**: `compute_recency` 공개(백엔드 간 스코어링 일치), 양 백엔드 라이브 검증 완료

### v0.7.0 (2026-06-14)

- **graph**: `GraphBackend` 동기 trait + `SqliteGraph` 구현 (백엔드 교체 가능, rusqlite 미노출)
- **graph**: trait 기반 스키마 마이그레이션 프레임워크(트랜잭션 롤백), 스키마 v1→v2
- **graph**: CJK 분할 기반 검색(`graph-cjk`, 스키마 변경 없음)
- **store**: `KvStore` trait + `SqliteKvStore`
- **llm**: `CacheClient` 응답 캐시(`KvStore` 기반, `cache` 피처)
- **mcp**: async 핸들러 + HTTP/SSE 원격 트랜스포트(`mcp-http` 피처, Bearer 인증)
- **deps**: ort 핀 유지(주석 보강, stable 미출시)

### v0.6.0 (2026-06-13)

- **search**: `SearchProvider` trait + `KeywordIndex` 참조 구현, 정규화/퓨전(normalize_minmax, weighted_sum, 정통 CombMNZ) 추가
- **safety**: `detect_injection → InjectionScore` 프롬프트 인젝션 탐지(가중 regex 규칙, 어휘적 휴리스틱)
- **discovery**: async `DiscoverySource` trait + `ModelsDevSource` (`discovery-async` 피처), 응답 크기 제한·리다이렉트 차단
- **tokens**: 문장 경계 + 토큰 예산 + overlap 기반 `chunk_text` (CJK + Latin)
- **llm**: `PromptTemplate` 변수 치환 + few-shot + serde 왕복
- **eval**: `injection` 서브커맨드 + baseline 회귀 게이트에 injection 항목 추가
- `KernelError::Search` 추가

### v0.5.0 (2026-06-13)

- `RetryClient`/`RetryConfig` 지수 백오프 래퍼
- `LLMClientMiddleware` trait + `MiddlewareClient`
- `ConversationHistory` 토큰 예산 기반 히스토리 관리
- `embed_batch` 배치 청킹 + `LazyFastembedProvider::embed_batch`
- `validate_config` 필드 수준 검증, install 마법사 확장

### v0.4.0 (2026-06-12)

- `MessageRole` enum, `ContentPart` 멀티모달 (breaking)
- `ToolDefinition`/`ToolCall`/`ToolResult` 도구 호출 타입
- `ResponseFormat` (Text/Json/JsonSchema) + JSON 모드
- `TokenBudget` 타입, `LLMRequest` 빌더 패턴

### v0.3.5 (2026-06-10)

- **vector-index 통합**: `llm-kernel-vector-index` 서브크레이트를 `vector-index` 피처 게이트로 흡수
- `TurbovecIndex` → `llm_kernel::embedding::TurbovecIndex` 리익스포트
- `VectorIndex` trait에서 `load` 제거 → 완전 object-safe (`dyn VectorIndex` 사용 가능)
- atomic save 패턴 적용 (temp file → fsync → rename)
- `SearchHit`에 `Copy + PartialEq` + `PartialOrd` 정렬 추가
- meta validation: invalid `bit_width`, zero `dim` 거부

### v0.3.4 (2026-06-09)

- `#![deny(missing_docs)]` 적용 + 누락된 doc comment 채움
- `mask_secrets` multi-pass → single-pass regex 최적화
- `LLMResponse`에 `finish_reason`, `id`, `created` optional 필드 추가
- `normalize(&mut [f32])`, `estimate_cost`, `extract_xml_tag` 유틸리티 추가
- `CapabilityProfile` 기본 trait 메서드 확장

### v0.3.3 (2026-06-09)

- README 12개 언어 버전 stale version `0.1.0` → `0.3.2` 수정
- Anthropic temperature 직렬화 누락 버그 수정
- `text_preview` 헬퍼 중복 제거
- 429/error handling 중복 제거
- macOS CI 러너 추가

### v0.3.2 (2026-06-09)

- reqwest Client에 connect/total timeout 추가 (#21)
- `mask_secrets` 패턴 확장 (#22)
- SQLite migration 트랜잭션 래핑 (#23)
- `vault.rs` anyhow → `KernelError::Vault` 통일 (#24)
- 메시지 빌더 중복 제거 (#25)
- 키릴/그리스/히브리 토큰 추정 확장 (#26)

---

## Roadmap Status

| Phase | Status | Notes |
|-------|--------|-------|
| **v0.3.2** — Stability Audit | ✅ Complete | Issues #21–#26 resolved |
| **v0.3.3** — Bug Fixes | ✅ Complete | README versions, Anthropic temp |
| **v0.3.4** — Lint & Utilities | ✅ Complete | `missing_docs`, mask perf, additive utils |
| **v0.3.5** — vector-index Integration | ✅ Complete | Subcrate → feature gate absorption |
| **v0.4.0** — Core Type Upgrades | ✅ Complete | `MessageRole`, `ContentPart`, `ToolDefinition`, `TokenBudget`, `LLMRequest` builder |
| **v0.5.0** — Client Resilience | ✅ Complete | Retry, middleware, embed_batch, history management |
| **v0.6.0** — Search & Intelligence | ✅ Complete | `SearchProvider`, injection detection, chunking, templates, async discovery |
| **v0.7.0** — Transport & Backend | ✅ Complete | `GraphBackend` trait, migration framework, CJK search, `KvStore`, LLM cache, MCP HTTP/SSE, async handlers |
| **v0.8.0** — Backend Expansion | ✅ Complete | PostgreSQL `GraphBackend`, Qdrant `AsyncVectorIndex`, DBMS migration CLI |
| **v0.9.0** — Search Integrations | ✅ Complete | Elasticsearch `AsyncVectorIndex`, `FederatedSearch` (RRF default, per-backend timeout) |
| **v0.10.0** — Graph Algorithms | ✅ Complete | CSR PageRank, community, Dijkstra, similarity; `smart_recall` PageRank boost |
| **v0.11.0** — PostgreSQL TLS | ✅ Complete | `graph-pg-tls` — `connect_native_tls` / `connect_tls` / `connect_config_tls` (#48) |
| **v0.12.0** — Embedding Robustness | ✅ Complete | Static ONNX linking, panic-safe load, `embedding-fastembed-dynamic-linking` (#50) |
| **v0.13.0** — Consistency & Protocol | ✅ Complete | `KernelError` unification, tool forwarding, MCP 2025-06-18 |
| **v0.14.0** — Forward Compatibility | ✅ Complete | `non_exhaustive`, `Default` derive, `from_key` → `Result` |
| **v0.15.0** — Embedding Robustness II | ✅ Complete | Mutually-exclusive fastembed features, `compile_error!` guard (#55) |
| **v0.16.0** — Vector Backend & Routing | ✅ Complete | pgvector `PgVectorIndex` (#59), `RouterClient` (#60), rusqlite 0.37 (#61) |
| **v0.16.1** — pgvector Bind Fix | ✅ Complete | Vector bind → string-literal `::vector` cast (sqlx `Type` conflict) |
| **v0.16.2** — CoreML EP | ✅ Complete | `embedding-fastembed-coreml` + `new_with_coreml()` (macOS GPU/ANE bge-m3) |
| **v0.17.0** — pgvector Transaction Integration | ✅ Complete | `add()` cast fix, `pool()` + `remove_in_tx` |

---

## Architecture Summary

16 feature-gated modules under hexagonal architecture:

```
provider     → catalog.json, capability profiles
llm          → async client, SSE streaming, JSON extraction, prompt templates
discovery    → models.dev, Ollama, OpenAI-compat; async DiscoverySource
secrets      → dotenv vault, atomic writes
store        → SQLite init helpers
config       → TOML loader
graph        → knowledge graph (GraphBackend, FTS5/CJK search, BFS, CSR algorithms, smart recall)
mcp          → JSON-RPC 2.0 server, stdio + HTTP/SSE transport
tokens       → Unicode token estimation, budgeting, sentence-aware chunking
install      → AI tool config wizard
search       → SearchProvider trait, RRF + weighted-sum + CombMNZ fusion
embedding    → provider trait + OpenAI + turbovec + qdrant/elastic/pgvector backends + fastembed (CoreML EP)
telemetry    → enum-gated events
safety       → secret masking, error classification, prompt-injection detection
```

---

## Health Dashboard

| Check | Status |
|-------|--------|
| All tests pass | ✅ 627 passed, 14 ignored, 0 failed (`--features full`) |
| Clippy clean | ✅ (verified before each release) |
| CI passing | ✅ Linux + macOS dual runner |
| Crate structure | ✅ Monolithic (subcrate removed) |
| Roadmap on track | ✅ v0.17.0 complete, v1.0.0 next |
