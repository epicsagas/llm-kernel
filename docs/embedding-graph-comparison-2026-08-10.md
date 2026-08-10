# llm-kernel 임베딩·그래프 실증 비교분석

**대상**: llm-kernel(Rust, 결정론적, 0-의존 도메인 타입)의 임베딩·그래프 recall 성능을 타 시스템과 실측 비교.
**환경**: Apple M2 / 16GB / Metal 4. macOS. Python 3.12, ollama 0.31.1, podman 6.0.1.
**측정일**: 2026-08-10.

---

## 0. 핵심 요약

- **llm-kernel 임베딩**: 단일 embed **6.9ms(전 시스템 최고)**. candle-Metal(Qwen3)은 CPU 대비 **2.2× 가속**(`embedding-metal` 피처 실증). 단, 배치 throughput은 MLX에 밀림(ort에 Metal EP 없음).
- **llm-kernel 그래프 recall**: hint 있을 때 **19–45µs**, 전체 회귀(no hint) **0.93–1.12ms**. Mem0(33ms)/Zep(155–162ms) 대비 **30–100× 빠름**. 단, LLM-free 결정론적 회귀라 정확도 벤치는 별도 과제.
- **아키텍처 트레이드오프**: llm-kernel은 **LLM 0·외부 DB 0·오프라인** 결정론적 라이브러리. Zep/Mem0/Letta는 LLM-in-loop + 서버 + 그래프 DB로 정확도·기능성 우위, 단 비용·지연·의존 증가.

---

## 1. 임베딩 성능 비교

### 1.1 공정 비교 (bge-small-en-v1.5, 384-dim, 동일 모델)

| 시스템 | 경로 | throughput @batch32 (t/s) | 단일 embed (ms) | 설치 상태 |
|---|---|---:|---:|---|
| **mlx-embeddings** | MLX GPU bf16 | **996** | 10.0 | pip 0.1.0 |
| sentence-transformers | torch MPS | 667 | 31.9 | pip 3.x |
| sentence-transformers | torch CPU | 439 | 30.9 | pip 3.x |
| **llm-kernel fastembed** | ONNX CPU | **409** | **6.9** ← 단일 최고 | 빌드 성공 |
| TEI (인용) | A10 GPU | 450+ | — | 미측정(서버 GPU) |
| ollama | HTTP/JSON | 42 | 48.6 | brew 0.31.1 |

### 1.2 llm-kernel candle-Metal (Qwen3-Embedding-0.6B, 1024-dim, MTEB rank #4)

| 경로 | throughput @batch32 (t/s) | 단일 embed (ms) |
|---|---:|---:|
| CPU F32 | 11.3 | 178 |
| **Metal F16** (`embedding-metal`) | **24.8** | **77** |

→ **Metal이 CPU 대비 2.2–2.3× 일관적 가속**. 큰 모델일수록 GPU 이득 큼.

### 1.3 분석

- **단일 embed는 llm-kernel fastembed(6.9ms)가 최고** — ONNX 런타임 JIT가 단일 호출 오버헤드에서 유리.
- **배치 throughput은 mlx-embeddings(996 t/s)가 압도** — Apple GPU 네이티브 최적화. llm-kernel fastembed(409)는 MLX 대비 **2.4× 느림**.
- 원인: ONNX Runtime이 **Apple Metal EP 미제공**(AGENTS.md 명시). 가속은 candle-Metal 경로(Qwen3/Nomic)에만 존재. bge-small 가속은 **`embedding-fastembed-coreml`**(CoreML EP) 활성화가 유일 경로 — **후속 벤치 최우선 과제**.
- ollama는 HTTP/JSON 직렬화 오버헤드로 배치 비효율(42 t/s). 단일 서비스 배포/모델 교체 용이성은 장점.
- TEI는 A10 서버 GPU 수치(450+)만 인용 — M2 로컬 미측정(podman 이미지 무거움). Apple Silicon 공식 벤치 비공개.
- **발견한 환경 결함**: 기존 `torchaudio 2.9.1`이 torch 2.11과 네이티브 lib 충돌 → sentence-transformers/transformers import 전체 파괴. torchaudio 제거로 복구.

---

## 2. 그래프 recall 성능 비교

| 시스템 | search / recall 지연 | add 지연 | LLM 의존 | 정확도 벤치 |
|---|---:|---:|---|---|
| **llm-kernel smart_recall** | **19–45µs**(hint) / **0.93–1.12ms**(no hint) | (미측정, upsert 경량) | **0** | 미측정(지연만) |
| Mem0 | 33ms | **11.8s** | 필수(Ollama LLM) | — |
| Zep | 155–162ms p95 | — | 필수(OpenAI) | LoCoMo 94.7%, LongMemEval 90.2% |
| Letta / Graphiti | (미측정, 갭) | (갭) | 필수 | — |

### 2.1 llm-kernel smart_recall 상세 (SQLite, criterion 중앙값)

| 모드 | 노드 100 | 200 | 500 |
|---|---:|---:|---:|
| no_hint(전체 회귀) | 0.93ms | 1.01ms | 1.12ms |
| with_hint(FTS 선필터) | 19µs | 45µs | 40µs |

- **패턴**: hint 있으면 FTS5가 후보를 좁혀 PageRank 부스트 대상 축소 → no_hint 대비 **20–50× 단축**.
- 알고리즘: composite 점수 `recency(20%) + importance(35%) + access(15%) + FTS(20%) + PageRank 중심성(10%)`. CSR 기반 PageRank, FTS5 + CJK hybrid.

### 2.2 분석

- **llm-kernel이 Mem0 대비 ~34×(search), Zep 대비 ~100× 빠름** — 단, 결정론적 회귀라 LLM-rerank 정확도는 비교 불가(별과제).
- Mem0 add 11.8s는 Ollama LLM 호출 기반(entity 추출/요약) — LLM-free 경로(llm-kernel upsert)와 차원이 다름.
- Zep은 bi-temporal(valid_at/invalid_at) + cross-encoder rerank로 **정확도 94.7%(LoCoMo)** 달성, 단 LLM API 비용·지연·프라이버시 의존.
- **llm-kernel의 정확도 벤치 부재가 핵심 갭** — 지연 우위는 증명됐으나, Zep/Mem0의 LLM 강화 recall 대비 정확도 위치 미지수.

---

## 3. 아키텍처 철학 비교

| 측면 | llm-kernel | Mem0 | Zep | Letta/Graphiti |
|---|---|---|---|---|
| 언어 | Rust | Python | Python/TS | Python |
| LLM 의존 | **0** (결정론적) | recall + add 루프 | recall 루프(엔티티/임베딩) | 에이전트 자가 메모리 |
| 배포 형태 | **라이브러리**(단일 프로세스) | pip 패키지 | 서버 | 서버 |
| 외부 DB | **없음**(sqlite/pg 내장) | 벡터 DB | Neo4j/FalkorDB/Neptune | 그래프 DB |
| 회귀 알고리즘 | PageRank + FTS5 + recency 가중 | vector + LLM rerank | vector + BM25 + graph + RRF/MMR/cross-encoder + bi-temporal | LLM 관리 메모리 블록 |
| 결정론 | **O**(수식 고정) | X(LLM) | X | X |
| 오프라인 | **O** | X(LLM/DB) | X | X |

### 3.1 설계 철학

- **llm-kernel**: "AI 네이티브 **기반 라이브러리**" — 도메인 타입(provider/tokens/embedding base)은 **외부 의존 0**, 기능별 feature gate, hexagonal 구조. 메모리 회귀를 **LLM 없이 결정론적 수식**(PageRank 중심성 + FTS + 시간 감쇠)으로 해결. 임베디드/오프라인/저지연/비용 민감 워크로드 강점. 프라이버시(데이터 외부 미유출).
- **Zep/Mem0/Letta**: "에이전트 **메모리 서비스**" — LLM을 회귀 루프에 투입해 정확도·맥락 이해 우위. bi-temporal(Zep), 자가 편집(Letta) 등 고기능. 단, LLM API 비용·지연·프라이버시·인프라(그래프 DB+서버) 의존. 엔터프라이즈 장기 메모리에 적합.

핵심: **서로 다른 문제를 푠다.** llm-kernel은 "언제 어디서나 1ms 안에 결정론적 회귀", Zep 계열은 "LLM 비용을 치르더라도 가장 정확한 맥락 회귀". 경쟁이 아닌 목적 차등.

---

## 4. 한계 (측정 못 한 것과 이유)

1. **Letta / Graphiti 로컬 미측정** — 그래프 Agent가 doc-research 단계에서 yield. 아키텍처/정확도는 공식 문서 기반 추정, 수치는 갭.
2. **llm-kernel recall 정확도 벤치 부재** — 지연(criterion)만 측정. LoCoMo/LongMemEval 등 표준 메모리 벤치에서 llm-kernel smart_recall의 정확도 미측정 → **최우선 후속 과제**.
3. **TEI 로컬 미측정** — podman 이미지/소스 빌드가 시간 예산 초과. A10 수치(450+ t/s)만 인용. M2 TEI 수치 갭.
4. **`embedding-fastembed-coreml` 미측정** — bge-small에서 MLX(996) 대항 가능 경로. AGENTS.md 옵션. 후속 벤치 가치 최상.
5. **Nomic V2 MoE candle 미측정** — 모델 무거움. candle-Metal은 Qwen3-0.6B로만 검증.
6. **MTEB 정확도 인용** — 품질 열은 리더보드 공개 수치(비실측). bge-small ~62.9, nomic-v1 ~58.0, Qwen3-0.6B MTEB #4.
7. **열 스로틀 미모니터링** — M2 Air 패시브 쿨링. 짧은 warm run이라 스로틀 전 값. 서스테인드 로드 시 수치 변동 가능.

---

## 5. 결론 및 권고

### llm-kernel 강점
- **단일 임베딩 최고 속도**(6.9ms) + **candle-Metal 2.2× 가속** 확증.
- **그래프 recall 30–100× 빠름**(LLM-free 결정론적).
- **0 외부 의존, 오프라인, 프라이버시** — 임베디드/엣지/비용 민감 워크로드 최적.

### llm-kernel 약점 / 개선 우선순위
1. **`embedding-fastembed-coreml` 벤치** → bge-small 배치 throughput MLX 대항(현재 2.4× 열세).
2. **recall 정확도 벤치 도입**(LoCoMo/LongMemEval) → 지연 우위 + 정확도 입증으로 Zep/Mem0 대비 위치 확정.
3. **TEI Metal 로컬 벤치** → M2 TEI 수치 확보(현재 A10 인용).

### 타 시스템 대비 위치
- **Mem0/Zep/Letta**: LLM 강화 정확도·고기능(장기 메모리) 우위. 단 비용·지연·인프라 의존. 엔터프라이즈 맥락 회귀 적합.
- **llm-kernel**: 결정론적·저지연·LLM-free. 경쟁이 아닌 **보완** — llm-kernel 위에 Zep형 LLM-rerank 레이어를 얹는 하이브리드가 유효 전략.

---

*측정 스크립트: Python 3종(bench_ollama/st/mlx) + Rust 일회성 examples/bench_embed.rs(측정 후 제거). 재현 시 재생성 가능.*
