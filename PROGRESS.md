# Progress

> Auto-generated status snapshot. Last updated: 2026-06-13

## Current Version: v0.6.0

| Metric | Value |
|--------|-------|
| Version | `0.6.0` |
| Edition | Rust 2024, MSRV 1.92 |
| Lines of code | ~15,200 |
| Total tests | 459 (447 passed, 12 ignored, 0 failed) |
| Open PRs | 1 |
| Open branches | 1 |
| Last commit | `c9501d7` — feat: v0.6.0 search and intelligence |

---

## Recent Releases

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
| **v0.7.0** — Transport & Backend | 🔜 Next | CJK FTS5, MCP HTTP, `GraphBackend` trait, `KvStore`, LLM cache |

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
graph        → knowledge graph (FTS5, BFS, smart recall)
mcp          → JSON-RPC 2.0 server, stdio transport
tokens       → Unicode token estimation, budgeting, sentence-aware chunking
install      → AI tool config wizard
search       → SearchProvider trait, RRF + weighted-sum + CombMNZ fusion
embedding    → provider trait + OpenAI + turbovec index
telemetry    → enum-gated events
safety       → secret masking, error classification, prompt-injection detection
```

---

## Health Dashboard

| Check | Status |
|-------|--------|
| All tests pass | ✅ 459/459 (447 passed, 12 ignored) |
| Clippy clean | ✅ (verified before each release) |
| CI passing | ✅ Linux + macOS dual runner |
| Crate structure | ✅ Monolithic (subcrate removed) |
| Roadmap on track | ✅ v0.6.0 complete, v0.7.0 ready to start |
