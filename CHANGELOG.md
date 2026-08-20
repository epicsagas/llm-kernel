# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.28.1] - 2026-08-21

### Fixed
- **tls**: the `rustls-ring` bootstrap added in 0.28.0 tripped `dead_code`
  under `-D warnings` on builds without a reqwest-backed feature (default
  `provider`-only, `embedding-fastembed-directml`,
  `embedding-fastembed-dynamic-linking`). `ensure_tls_provider` now compiles
  only when `client-async` / `discovery-async` / `elastic` is active. 0.28.0's
  release CI failed on this and was **never published to crates.io** — 0.28.1
  is the first published 0.28.x. CI lint now also clippies
  `--no-default-features`, the gap that let this through.

## [0.28.0] - 2026-08-21

### ⚠️ Changed (breaking — minor on the 0.x track)
- **features**: reqwest-backed features (`client-async`, `discovery-async`,
  `elastic`) now require an explicit TLS provider feature. `default` gained
  `rustls-aws-lc-rs`, so default-feature builds are unchanged — but
  `default-features = false` users must add `rustls-aws-lc-rs` (aws-lc-rs, the
  previous implicit provider) or `rustls-ring`. A `compile_error!` guard in
  `src/lib.rs` turns what was a silent runtime panic (reqwest 0.13
  `rustls-no-provider` panics at client-build time without an installed
  provider) into a clear build error.

### Added
- **rustls-ring** (#93): ring TLS provider for the reqwest-backed
  features — cross-compiles without cmake/nasm (aws-lc-sys needs cmake, and
  nasm on some targets; ring builds some C/assembly but a plain C compiler
  suffices). reqwest is switched to `rustls-no-provider` and
  llm-kernel installs the process-default ring provider before building any
  HTTP client, so no application code changes are needed. Mutually exclusive
  with `rustls-aws-lc-rs` (enforced by `compile_error!`) and not part of
  `full` — combine feature flags explicitly. Caveat: Cargo feature
  unification means any other dependency enabling reqwest's `rustls` feature
  pulls aws-lc-rs back into the tree.

## [0.27.0] - 2026-08-20

### ⚠️ Changed (breaking — minor on the 0.x track)
- **mcp**: `PromptArgument` gains an optional `type` field (`arg_type` in Rust,
  serialized as `type`, omitted when `None`) for typed prompt arguments. Struct
  literals must add `arg_type: None`; serialized form is unchanged for untyped
  arguments. Also on the transport surface: notification-only HTTP POSTs now
  answer `202 Accepted` (was `204 No Content`), and the nonstandard
  `POST /mcp/sse` endpoint is gone — the Streamable HTTP transport serves
  everything on `POST /mcp`.

### Added
- **mcp**: dual-era spec conformance. The server now implements the `2026-07-28`
  revision (stateless, per-request `_meta` protocol version) alongside the
  legacy `initialize`-handshake revisions (`2025-06-18` / `2025-03-26` /
  `2024-11-05`):
  - `server/discover` — supported versions, capabilities, `_meta` serverInfo,
    cacheable per `CacheableResult` (`ttlMs` + `cacheScope`).
  - Modern results are stamped `resultType: "complete"`; cacheable list/read
    results also carry `ttlMs`/`cacheScope`.
  - Version gate: an unsupported `_meta` version answers `-32022` with
    `data.supported` (HTTP: 400), so the client can retry with a mutually
    supported version.
  - Streamable HTTP request-header validation for modern requests
    (`MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` with `=?base64?…?=`
    sentinel decoding); violations answer 400 + `-32020` HeaderMismatch, and
    unknown modern methods answer 404 + `-32601`.
  - `subscriptions/listen` — acknowledgment notification + graceful-closure
    result (two lines on stdio, an SSE stream with `X-Accel-Buffering: no` on
    HTTP). This server advertises `listChanged: false`, so subscriptions close
    immediately.
  - `ping` is rejected for modern requests (removed in `2026-07-28`) and kept
    for legacy ones; a modern request inside a JSON-RPC batch rejects the batch
    (`-32600`) on both transports.

### Fixed
- **mcp**: `initialize` negotiation echoes legacy revisions only — a handshake
  client is never handed a stateless revision (new
  `LEGACY_PROTOCOL_VERSIONS` / `LEGACY_LATEST_PROTOCOL_VERSION` constants).
- **mcp**: auth/origin failures on HTTP answer at the HTTP level — 401 with
  `WWW-Authenticate: Bearer realm="mcp"` (RFC 6750) and 403 — before any body
  is produced.
- **mcp**: the stdio line cap is enforced *during* reading (bounded
  `fill_buf`/`consume` reader) instead of after the line was fully buffered, so
  a peer that never sends `\n` can no longer grow memory toward the cap.
- **mcp**: stdio enforces the initialize-first lifecycle (`-32002`), bypassed
  by stateless modern requests.
- **mcp**: unknown resource → `-32602` (was `-32603`); `Authorization` scheme
  matched case-insensitively (RFC 7235).

## [0.26.2] - 2026-08-19

### Fixed

- Docs: `SqliteGraph::with_tx` nesting footgun — `append_edges` and
  `delete_node` open their own transaction and fail inside a `with_tx`
  closure ("cannot start a transaction within a transaction"). Their
  single-item counterparts are safe; documented on both sides.

## [0.26.1] - 2026-08-18

### Added

- `SqliteGraph::with_tx(f)` — run multi-step sequences (e.g. edge
  delete-then-insert replacement) in one transaction; commits on `Ok`,
  rolls back on `Err`. Exposes the `unchecked_transaction` pattern already
  used internally by `store.rs`.

## [0.26.0] - 2026-08-18

### Added

- **Graph temporal validity** (closes #92): `GraphNode.valid_until` /
  `GraphNode.last_verified` (ISO 8601, empty string = unset / never verified).
  - Schema v4 on SQLite and both Postgres backends — existing v3 databases
    upgrade in place, no data loss (validated against a live 1k-node DB).
  - `mark_verified()` / `count_expired_nodes()` lifecycle functions, exported
    via prelude; `SqliteGraph::mark_verified` / `SqliteGraph::count_expired_nodes`
    wrappers so callers don't need their own connection.

### Breaking (semver minor)

- `GraphNode` gained two fields — exhaustive struct literals in downstream
  code must add `..Default::default()` (or the new fields). Serde is
  unaffected (`#[serde(default)]`).

## [0.25.0] - 2026-08-12

### Changed

- **⚠️ `query_prefix()` now returns the BGE instruction prefix for BGE-en-v1.5
  (small/base/large), mxbai-embed-large and Snowflake Arctic — previously
  `None`.** These are asymmetric models that expect the prefix on the query
  side, so omitting it silently degraded retrieval quality.

  **Migration — existing indexes need re-embedding.** `embed()` now prepends
  the prefix for these models, so queries land in a different region of the
  embedding space than vectors stored with ≤ 0.24.0. Mixing the two degrades
  recall *silently* — no error, no dimension mismatch, just worse results.
  Either re-embed the corpus with 0.25.0, or pin to 0.24.0 until you can.
  Unaffected: E5 and Nomic (already prefixed), and every symmetric model
  (MiniLM, paraphrase-ML, mpnet), which still return `None`.

  `cargo-semver-checks` does not flag this — the signature is unchanged and
  only the returned value differs.

### Added

- **`embedding-mlx` feature** — Rust-native MLX embedding provider
  (`MlxEmbeddingProvider`) running a BERT encoder forward pass on the Apple
  Silicon GPU via unified memory, complementing `embedding-metal` (which wins
  on single-embed latency) on the batch-throughput path. macOS/aarch64 only:
  `mlx-rs` and friends sit behind a `target.'cfg(...)'` dependency section, so
  `full` stays resolvable on Linux CI.

  Covers 13 vanilla-BERT models (21 catalog variants): BGE-en-v1.5
  small/base/large, bge-small-zh-v1.5, all-MiniLM-L6/L12,
  paraphrase-multilingual-MiniLM, multilingual-e5-small, Snowflake Arctic
  xs/s/m/l and mxbai-embed-large. Membership is not guesswork — each candidate's
  original weight repo was probed (`config.json` + safetensors header) and
  admitted only if it is `architectures: ["BertModel"]` with absolute position
  embeddings, gelu, and the standard `encoder.layer.N.*` tensor layout. See
  `EmbeddingModel::mlx_supported` for the exclusion list (NomicBert, XLM-R,
  MPNet, JinaBert, GTE `NewModel`, ModernBERT, Gemma, CLIP, plus
  `BGELargeZHV15`, which ships no safetensors).

  New catalog accessors: `mlx_supported()`, `uses_cls_pooling()`, `mlx_repo()`.
  Weights load from F32, F16 or BF16; any other dtype is an explicit error
  rather than a silent misread.

- **`embedding-metal` feature** — Metal GPU acceleration for the candle-based
  embedding providers (`Qwen3Provider`, `NomicMoeProvider`) on Apple Silicon,
  via candle-core's `metal` feature. New `new_metal()` constructors route
  inference to the Metal device (F16). This is the same path Hugging Face's
  Text Embeddings Inference uses (`-F metal`). Combined with the existing
  `embedding-fastembed-coreml` execution provider (CoreML EP for the ONNX
  models bge-small / bge-m3), both embedding backend families now have a
  native macOS GPU path. `ort` has no Metal EP, so CoreML is the only ONNX
  route. (MLX was initially rejected here because `mlx-rs` ships no embedding
  forward path; `embedding-mlx` above supersedes that by assembling the BERT
  encoder from `mlx-rs` `nn` primitives directly.)

## [0.24.0] - 2026-08-07

### Security

- **`SecretVault` no longer derives `Debug`.** The derive printed every stored
  secret verbatim, so any `{:?}` of the vault (or a struct embedding it) wrote
  the user's API keys into logs and panic messages. `Debug` now shows sorted
  key names only.
- **`BearerAuth::generate` uses the OS CSPRNG** (128 bits via `getrandom`)
  instead of a wall-clock-seeded xorshift, whose output was predictable to
  anyone who could guess the start time. `try_generate` is the fallible form;
  neither falls back to a weak token. `BearerAuth`'s `Debug` no longer prints
  the token.
- **MCP HTTP transport validates `Origin`** (MCP spec DNS-rebinding
  mitigation). Any web page could previously POST to a loopback MCP server
  and execute tools; browser requests from non-loopback origins now get 403,
  and non-browser clients (no `Origin`) are unaffected.
- **Secrets are zeroized** on vault drop and after the serialized body is
  written. Best-effort — `DerefMut`/`IntoIterator` let copies escape.

### Fixed

- **`redact_credential` panicked on multi-byte credentials** — it sliced at
  fixed byte offsets, on the very log/error paths it exists to protect.
- **MCP stdio could not execute async tool handlers.** `has_tool` accepted
  tools registered with `set_async_handler`, but dispatch resolved them
  through the sync-only `call_tool`, so every such tool was advertised in
  `tools/list` and then failed as "unknown tool". Adds `dispatch_async`,
  `dispatch_authenticated_async`, and `run_stdio_async`; `run_stdio` now
  fails fast when the server has async-only tools instead of returning
  `isError` per call, and `call_tool` distinguishes "async-only" from
  "unknown". `McpServer::async_only_tools` exposes the set.
- **Tool arguments are validated against the advertised `input_schema`**
  (object-ness and `required` fields). Previously a call missing a required
  argument reached the handler, where `params["x"].as_str().unwrap_or_default()`
  turned a protocol violation into a silently wrong result.
- **MCP HTTP dropped JSON-RPC batch requests**, answering 204 and leaving the
  client waiting; batches are now dispatched and answered as a batch.
- **`SecretVault::load_from` panicked** on a `KEY=$'` line (out-of-bounds
  slice), and corrupted non-ASCII values by reinterpreting UTF-8 bytes as
  Latin-1.
- **`SecretVault` round-trip corruption.** Values wrapped in double quotes or
  starting with `$` were written unquoted and then stripped on load.
- **`SecretVault::persist_to` silently dropped entries** whose key failed
  validation — `insert` then `persist_to` both reported success while the
  credential never reached disk. It now errors.
- **`SecretVault::load_from` silently dropped invalid-UTF-8 lines**, making
  the documented error path unreachable and letting the next `persist_to`
  erase the entry permanently.
- **`write_atomic` now fsyncs** the temp file before renaming, and takes a
  `Path` instead of a lossy string (non-UTF-8 paths wrote to a different
  file).
- **`estimate_tokens` counted newlines and tabs as zero** (the ASCII-control
  skip ran before the whitespace check) and returned 0 for short non-empty
  text.
- **Non-object JSON-RPC requests** were treated as notifications and answered
  with silence; they now get `-32600`. Stdio lines are length-capped.

## [0.23.0] - 2026-07-31

### Added
- `embedding/pgvector`: `PgVectorIndex::new_halfvec` — half-precision `halfvec`
  (float16) variant, ~half the RAM of `vector` with negligible recall loss for
  cosine similarity. Requires the `pgvector` extension ≥ 0.6 (`halfvec` type +
  `halfvec_cosine_ops`). `new` (float32 `vector`) is unchanged.
- `embedding/bgem3`: `Bgem3Provider` — BGE-M3 joint embedding, returning a dense
  vector and a learned-lexical `SparseVector` from a single pass (no second
  model and no separate BM25 index). Input is sliced into capped runs because
  fastembed's BGE-M3 graph always emits a ColBERT output and accumulates it per
  call (~2 MB per 512-token chunk); the provider drops it immediately, keeping
  bulk indexing memory flat. Optional `with_sparse_top_k` pruning for stores
  that bound non-zero counts.
- `embedding/sparse`: `SparseVector` — sparse (lexical) vector type for hybrid
  retrieval, kept index-sorted and zero-free, with `prune_top_k` to bound the
  non-zero count that pgvector will accept into an HNSW index.
- `embedding/vector_index`: `Fusion` — `Rrf { k }` / `Weighted { weights }` over
  `SearchHit` lists, the join point for dense + sparse hybrid search. RRF is
  rank-only (safe across different score scales); weighted sums raw scores.
- `embedding/pgvector`: `PgSparseVectorIndex` — `sparsevec(N)` storage with an
  inner-product HNSW index (`sparsevec_ip_ops`), mirroring `PgVectorIndex`'s
  add/search/remove/`remove_in_tx` surface. Requires pgvector ≥ 0.7.
- `embedding/pgvector`: `PgVectorOpts` + `PgVectorIndex::new_with_opts` — HNSW
  tuning. `m` / `ef_construction` are applied to `CREATE INDEX` (new indexes
  only); `hnsw.ef_search` — the main query-time recall/latency knob — is set on
  every pooled connection. `PgVectorOpts::default()` preserves current behaviour.

### Fixed
- `embedding/openai`: send the `dimensions` request parameter for
  `text-embedding-3-*` models. Previously a configured `dim` was metadata only —
  the API always returned the model's native width (1536 / 3072), so a reduced
  `dim` (e.g. 512 for Matryoshka shortening) silently disagreed with the vectors
  actually emitted and failed on insert into a `vector(512)` column.
  First-generation models (`text-embedding-ada-002`) still omit the field, which
  they reject.
- `embedding/pgvector`: dense and sparse `search`/`search_filtered` now carry an
  `ORDER BY …, id` tie-break, so equal-distance/equal-inner-product rows resolve
  to a stable order. Without it the per-branch rank order that RRF feeds on was
  index-scan dependent and could drift between the dense and sparse branches.
- `embedding/vector_index`: `Fusion` docs now call out that dense cosine and
  sparse inner-product scores disagree on scale, so `Weighted` must not combine
  them raw — normalize first, or use `Rrf` (the scale-safe default). Also notes
  the `f64 → f32` score narrowing as a further reason to normalize.
- `embedding/sparse`: documented that `SparseVector::new` drops both `+0.0` and
  `-0.0` (`-0.0 == 0.0` per IEEE-754), and added a regression test for the
  signed-zero case.
- `graph/recall`: FTS-window recovery now uses a single batched `read_nodes`
  query instead of a per-id `read_node` loop (N+1), and re-applies the
  `RecallOptions` scope filters (`project`, `node_types`, `tags_any`, `since`,
  `stale`) to recovered nodes. Previously a recovered node was read with no
  `WHERE` clause, so an FTS hit outside the requested scope (e.g. a generic node
  mentioning a symbol when `tags_any` scopes to that symbol) leaked into results.
- `embedding/pgvector`: `PgVectorIndex::add` chunks the INSERT at 16k vectors per
  round. A single INSERT binds 2 params per vector, so >~32k vectors overflowed
  PostgreSQL's `u16::MAX` bound-parameter limit (65535).
- `graph/search`: `search_nodes_hybrid` always runs the CJK substring path and
  unions it with the FTS results, de-duplicated by id (FTS BM25 order first, then
  CJK-only hits). FTS and CJK match different query shapes — trigram needs ≥3
  chars while CJK covers short and mid-word hits — so an FTS-only result can be
  non-empty yet still miss CJK-only matches. The short-CJK recovery case
  (trigram drops the query entirely) is covered.
- `search/rrf`: `rrf_fuse_weighted` uses one `HashMap` (id → (score, text))
  instead of two, removing a per-doc `remove` lookup on the build path.

### Changed
- Downgraded `rusqlite` 0.40 → 0.32. Cargo's `links = "sqlite3"` rule rejects two
  `libsqlite3-sys` versions in one graph; 0.32 pins `libsqlite3-sys` to exactly
  0.30.1, the same version `sqlx-sqlite` 0.8.6 resolves to. Consumers that also
  depend on `sqlx` (sqlite via rusqlite + postgres via sqlx, e.g. founder-os)
  could not otherwise build — `rusqlite` 0.40 pulled `libsqlite3-sys` 0.38,
  clashing with `sqlx-sqlite`'s 0.30. The graph API surface uses only stable
  primitives (`Connection`, `params`, `ToSql`, `unchecked_transaction`), so the
  downgrade is source-compatible. The previous 0.40 bump (0.21.x) is reverted.
- `graph/search` + `graph/recall`: added unit tests for `query_nodes_ex` paging /
  time-range / tag filters and for the recall FTS-recovery scope isolation
  (AC1 coverage).

## [0.22.0] - 2026-07-28

### Fixed
- **graph**: `upsert_node` (SQLite) now uses `ON CONFLICT DO UPDATE` instead of
  `INSERT OR REPLACE`, preserving `created`, `access_count`, and `accessed_at`
  on re-insert of a fixed-id node. Brings SQLite to parity with the Postgres
  backend (`sqlx_pg.rs`), which already did `ON CONFLICT`.
- **graph**: `delete_node` now removes the node's edges in the same transaction
  (`remove_edges_for_node`), preventing orphan edges.
- **graph** (`smart_recall`): a hint that matches nothing lexically now returns
  an empty result instead of the globally-most-important nodes — recall no
  longer answers every query with *something* and injects it as if relevant.
- **graph** (`smart_recall`): matching nodes are force-included in the candidate
  set even when they fall outside the importance-ordered window, so `W_FTS` is
  no longer unreachable on larger graphs.
- **graph** (`search_nodes`): the query is escaped as an FTS5 phrase literal, so
  a stray `"`, `*`, or `NEAR` in user/LLM text no longer crashes the whole
  recall. Malformed expressions degrade to empty results, not `Err`. Ranking
  now uses `bm25()` instead of discarding the FTS rank.
- **embedding**: `add_with_ids` failures are tagged (`add failed[duplicate_id]`
  vs `add failed[backend]`) so consumers can distinguish a routine re-ingest
  from a real backend failure without brittle full-string matching.

### Added
- **graph**: `search_nodes_hybrid` — FTS ∪ CJK substring union (de-duped by id),
  so short CJK queries invisible to the trigram tokenizer are found. Falls back
  to `search_nodes` without the `graph-cjk` feature.
- **graph**: `NodeQuery`/`NodeOrder`/`query_nodes_ex` — structured node query
  with paging, time range (`since`/`until`), and ordering. Exposed on
  `AsyncPoolGraph::query_nodes_ex`. The old `query_nodes` is preserved as a shim.
- **graph**: `RecallOptions`/`smart_recall_with` — structured recall with
  `node_types`, `tags_any` (symbol scope), `since`, and `touch` gating (LLM-
  context recall need not mutate `access_count`). Exposed on
  `AsyncPoolGraph::smart_recall_with`.
- **embedding**: `EmbeddingProvider::embed_document(s)` default methods.
  `FastembedProvider` overrides them to use the model's `doc_prefix` (E5
  `passage:`) for corpus text, while `embed` keeps the query prefix — fixing an
  asymmetric-model mismatch where documents were embedded with the query prefix.
- **embedding**: `TurbovecIndex::with_meta` / `meta` / `IndexMeta` — the index
  sidecar now carries `model_id`, `prefix_policy`, and `schema_version`
  (`#[serde(default)]`-compatible with old sidecars) so consumers can detect a
  rebuild need beyond `dim` (e.g. a prefix-policy change).
- **search**: `rrf_fuse_weighted` — per-source weights. `rrf_fuse` delegates to it.

## [0.21.0] - 2026-07-28

### ⚠️ Changed (breaking — minor on the 0.x track)
- **llm**: this release adds new public surface (`LLMResponse::reasoning`,
  `TokenUsage::reasoning_tokens`, `StreamEvent::ReasoningDelta`) and marks
  `StreamEvent` `#[non_exhaustive]`. Per Rust semver these are breaking for
  downstream crates: struct-literal construction of `LLMResponse`/`TokenUsage`
  must add the new fields, exhaustive `match` on `StreamEvent` must add a `_`
  arm, and `cargo-semver-checks` flags all three accordingly. Bump dependent
  crates' version requirement to `0.21`. `#[serde(default)]` +
  `skip_serializing_if` keep **serialized/cache** form backward-compatible, so
  no migration is needed for cached responses — only source-level match/literal
  sites need updating.

### Added
- **llm**: reasoning-model support — `LLMResponse::reasoning` and `TokenUsage::reasoning_tokens`
  fields, plus `StreamEvent::ReasoningDelta`. Parses `reasoning_content` (GLM-4.5+/z.ai),
  its `reasoning` alias (DeepSeek-R1), Anthropic extended-thinking (`thinking`) blocks /
  `thinking_delta` SSE, and `usage.completion_tokens_details.reasoning_tokens`. When a
  provider leaves `content` empty and returns the final answer in `reasoning_content`
  (GLM-4.7), `complete` promotes the reasoning into `content`; the original is preserved
  in `LLMResponse::reasoning`. Streaming surfaces reasoning as separate `ReasoningDelta`
  events — see that variant's docs for the consumer accumulation contract.
  `StreamEvent` is now `#[non_exhaustive]` so future variant additions stay non-breaking.

### Fixed (review follow-up)
- **llm**: extracted `promote_reasoning_into_content()` so the GLM-4.7 answer-promotion
  logic is unit-tested directly (no test-side duplication of `complete()` logic);
  documented the streaming/reasoning asymmetry on `LLMClient` and `ReasoningDelta`;
  documented the GLM (non-standard) vs o1/DeepSeek-R1 (standard) promotion caveat; and
  added `#[serde(default)]` to `OpenAIUsage` token fields so partial responses don't fail
  parsing.

## [0.20.1] - 2026-07-20

### Added
- **llm**: `OpenAiClient::from_key_with_base_url` and `AnthropicClient::from_key_with_base_url`
  constructors — custom base URL + explicit API key + shared `reqwest::Client` in a
  single call. Enables OpenAI-compatible providers (DeepSeek, Groq, Ollama, LM Studio,
  custom gateways) and Anthropic-compatible proxies without the `ModelConfig` env-var
  round-trip. Non-breaking; additive public API.

### Fixed
- **embedding** (`embedding-fastembed-qwen3`, `embedding-fastembed-nomic-moe`): CI build
  broken by `fastembed 5.17.2 → 5.17.3` (#71), which bumped its transitive
  `candle-core` to `0.11.0`. `Cargo.toml` still pinned `candle-core = "0.10"`, so
  two crate versions coexisted in the dependency graph and the `from_hf(device, dtype)`
  calls in `qwen3.rs` / `nomic_moe.rs` failed with `E0308` (expected `0.11.0`
  `Device`/`DType`, found `0.10.2`). Bumped the direct dep to `candle-core = "0.11"`
  to realign with fastembed (#74 CI failure).

## [0.20.0] - 2026-07-15

### Added
- **graph** (`graph-pg-sqlx`): async `SqlxPgGraph` backend over `sqlx::PgPool` — for consumers (e.g. klr) that own an async pool and need transaction sharing that `PgGraph`'s sync `postgres::Client` cannot provide. Inherent async methods (`append_edges`, `edges_for_node_dir`, `neighbors_weighted`, `remove_edges_for_node`, node CRUD, `search_nodes`, `related_nodes`); `pool()` getter + `append_edges_in_tx` / `remove_edges_for_node_in_tx` for atomic multi-table prune. Non-breaking (`GraphBackend` / `PgGraph` untouched). Unblocks klr citation-graph integration (klr#42).

## [0.19.0] - 2026-07-11

### Added
- **graph**: general directed-graph backend support — batch edge writes
  (`GraphBackend::append_edges`), directional / relation-filtered lookups
  (`edges_for_node_dir`, `neighbors_weighted`), filtered BFS
  (`related_nodes_filtered`), and the `EdgeDirection` enum (`Out` / `In` / `Both`).
  The new trait methods ship with **default implementations**, so adding them is
  non-breaking for external `GraphBackend` implementors; `SqliteGraph` and
  `PgGraph` override for throughput. The async SQLite wrappers (`AsyncGraph`,
  `AsyncPoolGraph`) gain matching inherent methods. This is the foundation for
  the planned klr citation-graph and alcove backlink integrations (the v1.0.0
  "real-world integration" exit criterion); klr/alcove integration lands in a
  follow-up.
- **graph** (`graph-pg`): `PgGraph::from_client` is now public — a consumer that
  already owns a synchronous `postgres::Client` can adopt `PgGraph` without
  re-opening the connection.
- **graph** (`graph-pg`): optional table prefix — `PgGraph::connect_with_prefix`
  (and `from_client_with_prefix`) namespaces the `nodes`/`edges`/`_meta` tables
  and indexes behind a caller-chosen prefix (default `""` keeps per-service-DB
  behavior unchanged). Lets multiple graphs coexist in one database.
- **graph**: schema v3 — composite `idx_edges_src_rel` / `idx_edges_tgt_rel`
  indexes serve relation-filtered directional edge queries (additive migration;
  no impact on existing graphs).

### Changed
- **deps** (#63): `rusqlite` 0.37 → 0.40 — reverses the intentional 0.40 → 0.37 downgrade from #61 (which held rusqlite at 0.37 because 0.38+ raised build requirements). The intervening dependency updates let 0.40 build cleanly again: `cargo check` and `cargo build --release --features full` both pass on MSRV 1.92. Note: re-introduces the `rsqlite-vfs` transitive dependency #61 had dropped as a side effect.
- **deps** (#62): `regex` 1.12 → 1.13.

## [0.18.0] - 2026-07-10

### Added
- **graph** (`graph-pool`, issue #45 axis E): `AsyncPoolGraph::open` now enables WAL on the file and applies `busy_timeout` + `synchronous = NORMAL` to every connection. Previously the pool ran under the default DELETE journal with no busy timeout, where a writer's lock blocked readers and concurrent writers failed immediately with `SQLITE_BUSY` — the module's "concurrent reads during writes" claim did not actually hold. Measured: a 16-reader wave under a sustained writer completes ~1.8× faster than the single-connection `AsyncGraph` wrapper (`benches/concurrency_bench.rs`, `docs/benchmarks/graph_concurrency.md`).
- **eval** (#45 axis D): `graph-korean` scenario quantifying `graph-cjk` vs FTS5 `trigram` Korean recall — trigram recall@5 **0.286** vs cjk **1.000** (+0.714) on a 40-doc/28-query corpus, because 2-syllable Korean tokens form no trigram. Precision is identical (both substring-based). Dataset + invariant checker under `eval/datasets/`; results in `docs/benchmarks/korean-recall.md`.
- **eval** (ROADMAP v1.0.0 #3): `--strict` gate mode — exits non-zero if any module fails, errors, or disappears vs baseline, closing a leak where a dataset load failure or failing module exited 0.
- **ci** (ROADMAP v1.0.0 #3, #45 axis A): `bench-smoke` job (criterion `--test` single-pass — deterministic, blocking) and the `eval` job now runs `--strict --baseline`. Local-only timing comparison documented in `docs/benchmarks/README.md` with `make bench-save` / `bench-cmp`.
- **ci** (ROADMAP v1.0.0 #4): `.github/workflows/semver.yml` — `cargo-semver-checks` against the published crates.io version. For 0.x, breaking changes fail unless the minor version is bumped in the same PR (enforcing the "API 동결" discipline); a `semver-break-intended` label bypasses deliberate breaks.

### Fixed
- **bench** (`compute_bench`): UTF-8 char-boundary panic when slicing the Japanese fixture at byte 200 — caught immediately by the new `bench-smoke` gate. Slices replaced with `chars().take(200)`.
- **llm** (security M2): HTTP error response bodies are now routed through `redact_http_body` before being stored in `KernelError::Http` — a proxy that echoes the `Authorization` header in an error body can no longer leak the API key through error logs. Full masking under the `safety` feature.

### Changed
- **api** (ROADMAP v1.0.0 #1): public-surface audit — 8 internal-only `pub` items reduced to `pub(crate)` (`write_atomic`, `redact_credentials`, `LLMRequest::into_openai_messages`/`into_anthropic_messages`, `edges_among`, `remove_edges_for_node`, `edges_for_node`) and dead code `importance_for_type` removed. `list_node_ids` / `read_nodes_limited` stay `pub` (consumed by the bundled `migrate` binary / cross-feature).

### Docs
- (ROADMAP v1.0.0 #2): `# Example` doctests on primary entry surface — `estimate_tokens`, `mask_secrets`, `LLMRequest::builder`, `OpenAIClient::from_key`.
- (ROADMAP v1.0.0 #5): `docs/security-audit-2026-07.md` — full review (no High findings; M1 documented, M2 mitigated).
- (ROADMAP v1.0.0 #6): `docs/features.md` — full feature catalog + platform compatibility matrix.
- (ROADMAP v1.0.0 #3): `docs/benchmarks/compute.md` — measured token/RRF/cosine baselines.

## [0.17.0] - 2026-07-08

### Added
- **embedding** (`pgvector`): `pool()` getter and `remove_in_tx(&mut PgConnection, ids)` on `PgVectorIndex` — transaction integration so callers can prune/delete within a single atomic transaction alongside their own writes.

### Fixed
- **embedding** (`pgvector`): `add()` was missing the `::vector` cast on the vec-text literal (switched from `push_values` to manual `VALUES` assembly), causing a type mismatch. The Rust `add` path now actually inserts; previously a Python `COPY` bypass in `klr` masked the bug.

## [0.16.2] - 2026-07-08

### Added
- **embedding**: `embedding-fastembed-coreml` feature + `new_with_coreml()` constructor (mirrors the DirectML pattern). Adds the `coreml` execution-provider feature to `ort`, accelerating `bge-m3` on macOS GPU/ANE. The static `embedding-fastembed` build now links CoreML alongside the default ONNX Runtime.

## [0.16.1] - 2026-07-08

### Fixed
- **embedding** (`pgvector`): `pgvector::Vector` sqlx `Type` bind conflict in the `klr` environment — bind the vector as a string literal (`[1,2,3]::vector`) instead of a typed `Vector` to sidestep the sqlx `Type` mismatch.

## [0.16.0] - 2026-07-08

### Added
- **embedding** (#59): `pgvector` `AsyncVectorIndex` (`PgVectorIndex`) — PostgreSQL + the `pgvector` extension as a third async remote vector backend (cosine `<=>`, HNSW index), alongside qdrant/elastic.
- **llm** (#60): `RouterClient` — cost-aware routing (`Fallback` / `LowestCost`) with cross-provider fallback. Fall-through is error-class aware: transient errors (5xx, rate-limit `429`, timeout `408`) move on, permanent 4xx short-circuits. Composes with `RetryClient` / `MiddlewareClient` / `CacheClient`.

### Changed
- **deps** (#61): `rusqlite` 0.40 → 0.37 (MSRV/build stability; drops the `rsqlite-vfs` transitive dependency).

## [0.15.0] - 2026-07-06

### Fixed
- **embedding** (#55): `embedding-fastembed-dynamic-linking` no longer pulls in
  `embedding-fastembed` (static ONNX download). Previously the dynamic feature
  was a superset of the static one, so Cargo feature unification silently
  activated both `ort-load-dynamic` and `ort-download-binaries-*` on the shared
  `fastembed`/`ort-sys` crate, turning the static path into a no-op (the #50
  failure mode) — the escape hatch never actually worked on its own. The two
  features are now mutually exclusive; `fastembed`'s ort features are selected
  by the consuming feature (`embedding-fastembed` → static archive,
  `embedding-fastembed-dynamic-linking` → runtime dylib load), and a
  `compile_error!` in `src/lib.rs` makes any conflict a hard build error
  instead of a silent dead link.
- **embedding** (#55, review fix): `FastembedProvider`, `LazyFastembedProvider`,
  `EmbeddingCache`, `is_model_cached`, and `EmbeddingModel::as_fastembed` were
  gated only on `feature = "embedding-fastembed"`, so the restructure above left
  `embedding-fastembed-dynamic-linking` compiling the bare `fastembed` crate with
  **no llm-kernel embedding API** — `unresolved import FastembedProvider`. Those
  gates now also fire under `embedding-fastembed-dynamic-linking`, so the dynamic
  escape hatch exposes the same API as the static path.

### Added
- **ci** (#55): `release-link-check` job builds `cargo build --release
  --features embedding-fastembed` on `ubuntu-latest` + `windows-latest` to
  catch static ONNX Runtime link regressions at PR time — the failure mode
  downstream consumers (e.g. alcove) previously discovered only at release /
  `cargo-dist` time. It also builds `--features embedding-fastembed-dynamic-linking`
  on `ubuntu-22.04` (glibc 2.35) to prove the escape hatch compiles on exactly
  the baseline alcove had to roll back from.

### Changed
- **ci**: `cargo {test,clippy,doc,check} --all-features` replaced with
  `--features full` throughout CI and `AGENTS.md`. `embedding-fastembed` and
  `embedding-fastembed-dynamic-linking` are now mutually exclusive, so
  `--all-features` (which activates both) no longer builds; `full` enables every
  feature except the dynamic escape hatch. This change unmasked a pre-existing
  macOS regression: previously `--all-features` enabled the broken
  dynamic-linking feature, which skipped the static ort link, so `macos-check`
  passed without ever linking the ONNX archive. With `--features full` the
  static link is real, so `macos-check` now injects the `libclang_rt.osx.a` link
  path (`RUSTFLAGS=-L…/rustlib/<host>/lib`) that the Xcode 16+ runner image no
  longer puts on the default search path (#55 "compiler-rt path regression").
- **docs** (#55): README + AGENTS.md document that the static ONNX archive
  requires glibc ≥2.38 (ubuntu 24.04+) / a current MSVC CRT, and that older
  baselines (ubuntu 22.04, glibc 2.35) must use
  `embedding-fastembed-dynamic-linking` plus a shipped
  `libonnxruntime.{so,dll}` — `cargo check` stays green because it does not
  link, so the failure surfaces only at `cargo build --release`.
- **docs**: added `[package.metadata.docs.rs] features = ["full"]` so docs.rs
  (which defaults to `--all-features`) doesn't trip the new mutually-exclusive
  `compile_error!`. Trade-off: `--features full` activates the static ort
  archive download on every clippy/test/doc/check run (the previous
  `--all-features` skipped it via the now-removed no-op dynamic feature) —
  accepted as the cost of accurate static-link coverage.

## [0.14.0] - 2026-07-03

A forward-compatibility release: stops the per-minor breakage caused by adding
fields/variants to public types. **Several changes are breaking** — see
migration notes below.

### Added

- **stability**: `Default` is now derived on every growable public data struct — `ServiceDescriptor`, `ModelDescriptor`, `ModelCapabilities`, `ModelCost`, `ModelLimit`, `ModelModalities`, `ModelChoice` (provider); `GraphNode`, `GraphEdge`, `GraphNodeSummary`, `GraphStats` (graph); `ToolDescription`, `ResourceDescription`, `PromptDescription`, `PromptArgument` (mcp). Downstream can now future-proof against field additions with struct-update syntax: `GraphNode { id, node_type, ..Default::default() }`.

### Changed (breaking)

- **error**: `KernelError` is now `#[non_exhaustive]`. New error variants may be added in any minor release; exhaustive `match`es on `KernelError` must add a `_ =>` arm. Match only the variants you act on (e.g. `RateLimited` / `Http` for retry logic).
- **error**: `KernelError::Serialization` is now available whenever **any** feature that pulls `serde_json` is enabled (previously only under `provider`). Consumers of `mcp`, `search`, `graph`, etc. — which already link `serde_json` — now see the `Serialization` variant and can use the `#[from] serde_json::Error` conversion. The variant set of `KernelError` therefore depends on which features are enabled; treat it as `#[non_exhaustive]` regardless.
- **catalog/graph/mcp**: the read-mostly catalog and result types are now `#[non_exhaustive]` — `ServiceDescriptor`, `ModelDescriptor`, `ModelCapabilities`, `ModelCost`, `ModelLimit`, `ModelModalities`, `ModelChoice`, `GraphStats`, `GraphNodeSummary`. These are obtained from the catalog or from queries; external struct-literal construction is no longer supported for them (use the catalog / query APIs, or `Default::default()` + field assignment). Types downstream constructs directly (`GraphNode`, `GraphEdge`, the MCP `*Description` types) are **not** marked `non_exhaustive` so struct literals keep working — use `..Default::default()` to insulate them from future field additions.
- **llm** (breaking): `OpenAIClient::from_key` and `AnthropicClient::from_key` now return `Result<Self>` instead of `Self`. Previously a failure to build the timeout-bearing `reqwest::Client` silently fell back to a timeout-less `Client::default()`; it now propagates a `KernelError::Config`. Add `?` at call sites: `OpenAIClient::from_key(model, key)?`.

### Migration

- `match err { … }` on `KernelError` → add a `_ => { … }` arm.
- `ServiceDescriptor { … }` / `ModelDescriptor { … }` literals (outside the catalog) → construct via `Default::default()` + field assignment, or read from `ProviderIndex`.
- `OpenAIClient::from_key(m, k)` / `AnthropicClient::from_key(m, k)` → append `?`.

## [0.13.1] - 2026-07-03

### Fixed

- **llm**: streaming responses no longer corrupt multi-byte (CJK, emoji) text. The SSE reader decoded each network chunk independently with `String::from_utf8_lossy`, so a single UTF-8 codepoint split across two TCP chunks was replaced with `U+FFFD`. Decoding is now deferred to whole, newline-terminated lines buffered at the byte level (`\n` is never a UTF-8 lead/continuation byte, so a line boundary can't cut a codepoint). Affects both OpenAI and Anthropic stream paths.
- **embedding** (elastic): `add` / `remove` now chunk large batches into bounded `_bulk` requests (500 docs each) instead of building one unbounded NDJSON body that could exceed Elasticsearch's `http.max_content_length` (HTTP 413) or spike memory on very large upserts.
- **llm** (retry): an honored server `Retry-After` header is now clamped to 5 minutes, so a misconfigured or hostile endpoint returning e.g. `Retry-After: 999999` can no longer stall a task for days.

### Changed

- **deps**: `anyhow` is now an optional dependency pulled only by the `eval` / `catalog-sync` binaries. The default `provider` build and every library consumer no longer compile `anyhow` — it appeared only in the two CLI binaries, never in the library surface.

### Docs / Tooling

- **i18n**: all 10 translated READMEs (`de`, `es`, `fr`, `it`, `ja`, `ko`, `pt`, `ru`, `zh-Hans`, `zh-Hant`) resynced to the English README — added the `embedding-fastembed-dynamic-linking` feature-table row and the *Async discovery*, *Cross-engine federation*, *Vector indexing*, and *Prompt templates* sections that had drifted behind, and dropped the stale *Safety utilities* heading.
- **lint**: resolved all 34 `clippy` warnings under `--all-targets` (test/example/bench code) — the `criterion::black_box` deprecation is replaced with `std::hint::black_box`. `cargo clippy --all-features --all-targets -- -D warnings` is now clean.
- **ci**: added `.cargo/audit.toml` so a `cargo audit` failure on a transitive-only dependency (one no enabled feature compiles into an active path, e.g. `quinn-proto` via reqwest's optional QUIC support) can be suppressed via a documented escape hatch instead of hard-failing release CI.
- **docs**: `AGENTS.md` test count corrected (602 passed, 13 ignored).

## [0.13.0] - 2026-07-03

### Added

- **llm**: `LLMRequest::tools` and `LLMRequest::response_format` are now **forwarded to the provider APIs**. OpenAI receives `tools` (`type: "function"`) and `response_format` (`json_object` / `json_schema`); Anthropic receives `tools` (with `input_schema`) and, for `ResponseFormat::JsonSchema`, `output_config.format`. Previously both fields were accepted by the builder but silently dropped.
- **llm**: `LLMResponse::tool_calls: Vec<ToolCall>` — tool calls the model requested are parsed back from OpenAI `tool_calls` and Anthropic `tool_use` content blocks. `LLMResponse` now also captures `finish_reason` (OpenAI `finish_reason` / Anthropic `stop_reason`), `id`, and `created` from the provider response.
- **mcp**: protocol-version negotiation — `initialize` echoes the client's requested `protocolVersion` when supported (`2025-06-18`, `2025-03-26`, `2024-11-05`), otherwise proposes the server's latest (`2025-06-18`). Exposed via `McpServer::negotiate_protocol_version` and the `SUPPORTED_PROTOCOL_VERSIONS` / `LATEST_PROTOCOL_VERSION` constants.
- **mcp**: `ping` method (returns `{}`), **prompts** support (`prompts/list`, `prompts/get`, `McpServer::register_prompt` / `set_prompt_handler`, `PromptDescription` / `PromptArgument`), and `resources/templates/list`. The `prompts` capability is advertised in `initialize` when prompts are registered. Both stdio and HTTP/SSE transports support all new methods.
- **error**: `KernelError::Embedding` and `KernelError::Discovery` variants (with `KernelError::embedding` / `KernelError::discovery` constructors).

### Changed

- **error** (**breaking**): the `embedding` and `discovery` subsystems now return `crate::error::Result` (`KernelError`) instead of `anyhow::Result` — the `EmbeddingProvider` / `VectorIndex` / `AsyncVectorIndex` traits, all provider and index constructors (`FastembedProvider`, `OpenAIEmbeddingClient`, `Qwen3Provider`, `NomicMoeProvider`, `LazyFastembedProvider`, `TurbovecIndex`, `QdrantVectorIndex`, `ElasticsearchVectorIndex`), `DiscoverySource`, `chunk_batch`, the `discovery::fetch*` functions, and `provider::sync::*`. `anyhow` no longer appears in the library's public surface. Downstream code that matched on `anyhow::Error` must switch to `KernelError`.
- **mcp** (**breaking**): `McpServer::initialize_response` now takes the client's requested protocol version (`initialize_response(Option<&str>)`).
- **mcp**: `ToolDescription` and `ResourceDescription` now serialize with the correct MCP wire-format field names — `inputSchema` (was `input_schema`) and `mimeType` (was `mime_type`).
- **mcp**: JSON-RPC request `id`s are preserved verbatim (string **or** number) in responses, per JSON-RPC 2.0 — previously only integer ids round-tripped.
- **mcp**: `tools/call` reports **tool-execution failures in-band** as a result with `isError: true` (so the model can react), and reserves the JSON-RPC error path (`-32602`) for an unknown tool — matching the MCP spec.

### Fixed

- **embedding**: `LazyFastembedProvider::embed_batch` no longer panics with an index-out-of-bounds when the inner provider returns fewer vectors than inputs (a truncated/malformed response); it now returns a `KernelError::Embedding`.
- **llm**: `CacheClient::complete` offloads the synchronous `KvStore` read/write to `tokio::task::spawn_blocking`, so a slow or remote store (or a single-threaded runtime) no longer blocks the async reactor on the completion hot path.

### CI

- Isolated per-feature build/test matrix entries added for `cache`, `discovery-async`, `graph-async`, `graph-pool`, `graph-cjk`, `mcp`, `mcp-http`, `tokens`, `safety`, `telemetry`, `search`, `federation`, `embedding`, `embedding-openai`, `vector-index`, and `install`, so a missing `#[cfg]` gate is caught even when a sibling feature isn't co-enabled.

## [0.12.0] - 2026-07-02

### Changed

- **embedding** (breaking): `ModelState::Failed(String)` is now `ModelState::Failed { message: String, panicked: bool }`. Code matching `ModelState::Failed(msg)` must switch to `ModelState::Failed { message, .. }` (or use the new `ModelState::is_panic()` helper instead of matching the shape directly).

### Added

- **embedding**: new opt-in `embedding-fastembed-dynamic-linking` feature (forwards to `fastembed/ort-load-dynamic`) for deployments that can't satisfy the default static build's glibc 2.38+ requirement — e.g. Ubuntu 22.04 / Debian 12 hosts. Do not combine with a build that also enables plain `embedding-fastembed`/`full` elsewhere in the same feature graph (Cargo feature unification would re-merge both and reintroduce #50).
- **embedding**: `LazyFastembedProvider::reset()` clears a `Failed` state back to `NotLoaded`/`Cached` so a subsequent `ensure_model()` call retries the load (e.g. after a transient network failure during model download), instead of the provider being permanently stuck. `ModelState::is_panic()` lets callers distinguish "loader panicked" (ort/global state may be corrupted; retry with caution) from an ordinary load error (safe to retry).

### Fixed

- **embedding**: stopped force-enabling `ort-load-dynamic` on the Linux/Windows `fastembed` target dependency by default (#50). `ort-load-dynamic` forwards to `ort-sys/disable-linking`, which makes `ort-sys`'s build script early-return and **skip the static-archive download step entirely** — so `ort-download-binaries-rustls-tls` was a silent no-op and the resulting binary expected `libonnxruntime.so` to be supplied externally at runtime. Since llm-kernel never ships that library, `embedding-fastembed` on Linux deadlocked silently on first `.embed()` instead of failing cleanly. The default build now statically links ONNX Runtime and produces self-contained binaries. **Caveat:** ort's prebuilt static archive requires glibc 2.38+, resolved against the executing host's libc at runtime — Linux hosts on glibc <2.38 (e.g. Ubuntu 22.04, Debian 12) will fail to load the statically-linked binary at first ONNX Runtime init. Such deployments should enable the new opt-in `embedding-fastembed-dynamic-linking` feature instead (forwards to `fastembed/ort-load-dynamic`) and ensure `libonnxruntime.{so,dll}` is present on the runtime host. Do not combine `embedding-fastembed-dynamic-linking` with a build that also enables plain `embedding-fastembed`/`full` elsewhere in the same feature graph — Cargo feature unification would re-merge both and silently reintroduce #50.
- **embedding**: `LazyFastembedProvider`'s model-load path is now **panic-safe** in builds that unwind on panic (the default `dev`/`test` profile, and any `release` profile that doesn't override `panic`). A panic during `FastembedProvider::new()` (e.g. a missing `libonnxruntime.so` under dynamic loading) is caught via `catch_unwind` and converted into a `ModelState::Failed { .. }` transition that notifies all `Condvar` waiters, so concurrent callers receive a clean error instead of wedging forever on `futex` (confirmed in production via `/proc/PID/wchan`). Guards against any future ort/fastembed init failure mode, not just the dynamic-linking one. **Note:** this crate's own `[profile.release]` sets `panic = "abort"`, under which `catch_unwind` cannot intercept a panic — a panicking init in a release build of this crate still aborts the process rather than transitioning to `Failed`. This is an intentional tradeoff (a hard crash is a clearer failure signal than the previous silent deadlock), but it means the "clean error" guarantee above is scoped to unwinding builds; downstream crates that enable `panic = "abort"` in their own release profile inherit the same limitation.

## [0.11.0] - 2026-07-01

### Added

- **graph**: new optional `graph-pg-tls` feature adding TLS support to `PgGraph` connections, closing #48. `PgGraph::connect_native_tls(url)` is a one-call convenience constructor using `native-tls` with the system trust store (full certificate chain and hostname verification, not weakened) — covers the common case of a Postgres server requiring `sslmode=require`+ (e.g. RDS with `rds.force_ssl`). `PgGraph::connect_tls` / `connect_config_tls` are generic over any `postgres::tls::MakeTlsConnect` implementor for custom CAs, client certificates, or a caller-vendored connector. Existing `connect` / `connect_config` (`NoTls`) are unchanged — fully backward compatible, no new mandatory deps for `graph-pg` consumers.


## [0.10.0] - 2026-06-29

### Added

- **graph**: Graph algorithm module (`algo/`) closing the Neo4j/GDS algorithm gap — pure-Rust, zero-dependency, compiled in behind the existing `graph` feature (no `Cargo.toml` change, no `petgraph`). New `CsrGraph` compressed-sparse-row snapshot plus weighted **PageRank** with dangling-node redistribution (`algo/pagerank.rs`), **connected components** (union-find) and **label propagation** (`algo/community.rs`), **Dijkstra** weighted shortest path using `distance = -ln(weight)` (`algo/path.rs`), and **Jaccard / common-neighbors / Adamic-Adar / link prediction** (`algo/similarity.rs`). All re-exported from `graph` as free functions; iterative math is backend-agnostic for zero drift.
- **graph**: PageRank eval scenario (`query_type: "pagerank"` in `eval/datasets/graph.jsonl`) and criterion benchmarks for CSR build / PageRank / connected components / label propagation / Dijkstra / Jaccard in `benches/graph_bench.rs`.

### Changed

- **graph**: `smart_recall`'s graph boost (`W_GRAPH`) now ranks the top-100 candidates by true PageRank centrality over their induced subgraph, replacing the former neighbor-weight-sum (an approximate degree centrality). The SQLite (`recall.rs`) and PostgreSQL (`pg.rs`) recall paths share the same `pagerank_default`, permanently removing the boost-logic drift that previously existed between backends. New `store::edges_among` serves the induced-subgraph edge query.

### Fixed

- **deps**: patched `quinn-proto` 0.11.14 → 0.11.15 to clear **RUSTSEC-2026-0185** (lockfile-only — the crate is not activated under any feature, but cargo-audit scans the full lock and was failing the `audit` CI gate on every PR).
- **deps**: bumped `anyhow` 1.0.102 → 1.0.103.

## [0.9.2] - 2026-06-22

### Added

- **llm**: `LLMRequest` and `LLMResponse` now implement `Default`, enabling forward-compatible struct-update syntax (`LLMRequest { system: Some(..), ..LLMRequest::default() }`). `Default` for `LLMRequest` uses `temperature: 0.7`, matching the builder default — covered by the `default_matches_builder_default` test.
- **llm**: `LLMRequestBuilder::messages(Vec<ChatMessage>)` — set the full message list in one call (the existing `.message()` appends one at a time).
- **llm**: `LLMRequestBuilder::maybe_max_tokens(Option<u32>)` — set `max_tokens` from an `Option` directly, avoiding conditional chains for callers that hold a config `Option<u32>`.

### Changed

- **llm**: All `LLMRequest` examples in README, QUICKSTART, the 10 i18n READMEs, and `examples/` now use struct-update (`..LLMRequest::default()`) instead of exhaustive struct literals. **Call sites using `..LLMRequest::default()` will no longer break when new fields are added to `LLMRequest` in future releases** — this is the forward-compatible construction pattern going forward. Full struct literals still compile today but must be updated field-by-field on every `LLMRequest` field addition.

### Notes

- The `response_format` and `tools` fields added in 0.9.0 remain `Option` and default to `None`; they are not yet forwarded to provider APIs (planned for a future release). Existing call sites that did not set them are unaffected once migrated to struct-update.

## [0.9.1] - 2026-06-16

### Added

- **provider** (`catalog-sync` feature): `llm-kernel-sync-catalog` binary — refreshes `catalog.json` from the live models.dev catalog. `--check` reports drift without writing; the default writes atomically. Drives field-precedence merge: provider service fields (auth, base URL, tiers, setup) are kept from the catalog, model data (cost, limits, modalities, capabilities) comes from models.dev, and empty `api_base_url`/`npm_package`/`doc_url` are filled from upstream. New `src/provider/sync.rs` (`merge_catalog`, `CatalogDiff`, `PriceDelta`) + `src/bin/sync-catalog.rs`.
- **provider**: `provider::mapping` — `Mapping` enum + `resolve()` mapping each catalog provider id to its models.dev counterpart (8 exact, 7 aliased, 5 manual). New `src/provider/mapping.rs`.
- **provider**: `ProviderIndex::from_providers(Vec<ServiceDescriptor>)` public constructor and `ProviderIndex::with_discovered(&[ModelEntry])` (gated on `discovery`) — overlays runtime-discovered models onto the embedded catalog so `find_model`/`estimate_cost` see them. Resolves the catalog↔discovery gap.
- **provider**: catalog value types (`ModelCost`, `ModelLimit`, `ModelModalities`, `ModelCapabilities`, `ModelDescriptor`, `ServiceDescriptor`, `ModelChoice`) now derive `Serialize` and `PartialEq`.
- **discovery**: `fetch()` / `fetch_from(url)` no-cache fetch helpers; `ModelsDevPayload::entries()`, `provider_models(key)`, `provider_api_base`/`provider_npm`/`provider_doc` accessors.
- **discovery**: `ModelEntry` enriched with optional `cost`, `modalities`, `capabilities`, `family`, `release_date`, `knowledge` (mirroring `ModelDescriptor`) and `Default`; `From<ModelEntry> for ModelDescriptor`.

### Changed

- **discovery** (*breaking*): `ModelsDevPayload` now mirrors the real models.dev API — a provider-keyed map (`HashMap<provider_id, provider>`) — instead of the previous `{ models: Vec<ModelEntry> }` shape, which never parsed the live `https://models.dev/api.json`. The on-disk cache written by `fetch_and_cache` is now byte-identical to upstream.
- **catalog**: `catalog.json` refreshed from models.dev — 20 providers, 351 models (was ~57). Pricing/limits/modalities/capabilities now track models.dev (e.g. `glm-5` input 0.5→1.0, output 0.5→3.2). `glm-5` and `ZAI_API_KEY` preserved (catalog-wins for connection fields). Provider-doc comment corrected (16→20).
- **docs**: README "Model discovery" example updated for the new payload shape; new "Keeping the catalog fresh" section documents the runtime `with_discovered` path (always-current) versus the `sync-catalog` tool (offline baseline at release time).

### Notes

- The embedded catalog is frozen at compile time (`include_str!`), so the `sync-catalog` tool refreshes the **offline baseline** that ships with each crate release. For always-current data at runtime, fetch models.dev via `discovery` and merge with `ProviderIndex::with_discovered` — the library provides the fetch + merge; the application drives timing/caching.

## [0.9.0] - 2026-06-15

### Added

- **embedding** (`elastic` feature): `ElasticsearchVectorIndex` — `AsyncVectorIndex` over Elasticsearch 8.x (dense_vector cosine mapping, bulk upsert/delete, knn `_search`, `_count`), implemented with a **hand-rolled reqwest client** rather than the official `elasticsearch` crate (which is alpha-only — no stable release) so the dependency stays safe ahead of the v1.0.0 semver lock (new `src/embedding/elastic.rs`)
- **federation** (`federation` feature): `FederatedSearch` — concurrent cross-engine federation over multiple `AsyncVectorIndex` backends with a per-backend timeout, observable failure handling, and rank-based RRF fusion as the default (new `src/search/federation.rs`). The feature composes `search` + `embedding` and owns the `tokio` + `futures-util` deps so search-only and single-backend users compile no federation runtime.
- **search**: `FusionStrategy` enum + pure `federate_results` merge so a synchronous `TurbovecIndex` can participate in federation alongside the async backends

### Changed

- **search**: the pure fusion functions (`rrf_fuse`, `normalize_minmax`, `weighted_sum_fuse`, `combmnz_fuse`) are unchanged; the `search` feature remains light (serde_json only). Async cross-engine federation moved to a dedicated `federation` feature gate that owns `tokio` (+ `time`) and `futures-util`.
- **features**: new `elastic` feature gate — the reqwest driver is reused from `client-async` (no new transitive deps); `elastic` is included in `full`. Single crate, single publish. Main crate version 0.8.0 → 0.9.0.
- **infra**: `docker-compose.yml` gained an Elasticsearch service for the live integration test (local-dev only; CI self-skips)
- **elastic** (hardening, pre-v1.0.0 stabilization): the reqwest client now sets a 5 s connect timeout + 30 s request timeout so direct (non-federated) callers cannot hang on an unresponsive node; `redact_credentials` now redacts userinfo up to the **last** `@` in the authority (a password containing `@` no longer leaks its tail); bulk upsert/delete errors surface the first failing item's redacted JSON; index names are validated against the ES 8.x rules (lowercase, `[a-z0-9_.-]`, no leading `_`/`-`/`+`, ≤255 bytes) before any network call; `_count` no longer sends a no-op `track_total`; `FederatedSearch` collects per-backend weights only under `WeightedSum`.
- **elastic** (review hardening): the knn `num_candidates` is now computed by a shared `knn_num_candidates(k)` helper that caps candidates at `MAX_KNN_CANDIDATES = 1_000` (so a large `k` cannot ask ES to score thousands of candidates) while preserving the ES invariant `num_candidates >= k`; error response bodies embedded in `anyhow` errors are capped to `ERROR_BODY_MAX_CHARS = 1024` characters at a UTF-8 boundary (with a `... [truncated]` marker) so a verbose ES error cannot bloat logs, applied after `redact_credentials` so a credential past the cap stays masked; the `SearchHit.score` semantics (`(1 + cosine) / 2`, not comparable across backends) and the WeightedSum caveat are now documented in the module and `search` method docs.
- **federation** (review hardening): `FederatedSearch::search` now over-fetches each backend (`fetch_k = 2 * k`) before RRF/WeightedSum fusion and truncates the merged list to the requested `k`, so a document ranking just below `k` in one backend but near the top in another keeps its cross-backend rank-credit instead of being silently dropped.

### Notes

- Federation defaults to **RRF** (rank-based, scale-invariant) so heterogeneous raw scores across backends — Qdrant cosine `[0,1]`, Elasticsearch `_score = (1+cos)/2 ∈ [0,1]`, TurboVec raw cosine `[-1,1]` — fuse correctly with no normalization. `FusionStrategy::WeightedSum` is opt-in and applies per-list min-max normalization first.
- Elasticsearch connection-string credentials (`https://user:pass@host`) are used for the request but **never** leaked in errors — all error messages route through `redact_credentials`, which strips userinfo up to the last `@` in the authority (handles passwords that themselves contain `@`).
- The live Elasticsearch conformance test mirrors the Qdrant conformance body and self-skips without `LLMKERNEL_ELASTIC_URL`; it deletes its throwaway index on every exit path.

## [0.8.0] - 2026-06-14

### Added

- **graph** (`graph-pg` feature): `PgGraph` — a PostgreSQL `GraphBackend` over the synchronous `postgres` driver (ILIKE substring search, no extension required; identical `smart_recall` scoring; recursive-CTE BFS traversal; schema versioning via the trait)
- **graph** (`graph-pg`): `llm-kernel-migrate-graph` binary — a SQLite↔PostgreSQL migration CLI with a `--dry-run` planning mode
- **embedding** (`qdrant` feature): `QdrantVectorIndex` — `AsyncVectorIndex` over `qdrant-client` (upsert / remove / search / filtered search / count via the universal Query API)
- **embedding**: `AsyncVectorIndex` trait — the async, object-safe counterpart to `VectorIndex` for remote/shared backends whose clients are async-only (new `src/embedding/async_vector_index.rs`)
- **infra**: `docker-compose.yml` for opt-in local PostgreSQL + Qdrant to run the live integration tests (works with `docker compose` or `podman compose`)

### Changed

- **features**: new `graph-pg` and `qdrant` feature gates — drivers are optional and not in `default`; both are included in `full`. Single crate, single publish (no separate workspace crates). Main crate version 0.7.0 → 0.8.0.
- **embedding**: the `embedding` feature now pulls `async-trait` (for the `AsyncVectorIndex` trait); the existing synchronous `VectorIndex` is unchanged
- **ci**: `graph-pg` and `qdrant` added to the test matrix (live integration tests self-skip without `LLMKERNEL_PG_URL` / `LLMKERNEL_QDRANT_URL`, so CI without services stays green)
- **graph**: `compute_recency` is now `pub` so the PostgreSQL backend reuses the exact recency math — no scoring drift across backends

### Notes

- Both new backends are live-verified: `PgGraph` passes the full `GraphBackend` conformance and a SQLite→PostgreSQL migration round-trip; `QdrantVectorIndex` passes add / search / filter / remove against a live Qdrant. These live tests are env-gated and skip in CI.
- Driver dependencies (`postgres`, `qdrant-client`) are optional and only compiled when `graph-pg` / `qdrant` are enabled — the default (and `provider`-only) build is unchanged.

## [0.7.0] - 2026-06-14

### Added

- **graph**: `GraphBackend` trait — sync, object-safe, backend-agnostic interface for graph storage with **no `rusqlite` types in its surface**, ready for non-SQLite backends; includes the composite `smart_recall` and `related_nodes` operations (new `src/graph/backend.rs`)
- **graph**: `SqliteGraph` — bundled `GraphBackend` implementation wrapping the existing graph free-function API behind a mutex-guarded connection
- **graph**: schema migration framework expressed through `GraphBackend` (`current_version`, `migrate`) — version-to-version steps with transactional rollback; graph schema bumped to v2 (new `idx_nodes_created` index)
- **graph**: CJK-aware search via contiguous substring matching (`segment_cjk` utility + `search_nodes_cjk`) behind the new `graph-cjk` feature — **no FTS5 schema change**, so the feature toggles safely on any existing database (new `src/graph/cjk.rs`)
- **store**: `KvStore` trait (sync, object-safe) + `SqliteKvStore` implementation (new `src/store/kv.rs`)
- **llm**: `CacheClient` — response-cache wrapper for any `LLMClient`, backed by `KvStore`; client-namespaced key (no cross-provider collision on a shared store), optional TTL (`with_ttl`), `complete` cached, `stream_complete` pass-through (new `src/llm/cache.rs`, new `cache` feature)
- **mcp**: async tool handlers (`AsyncToolHandler`, `set_async_handler`, `call_tool_async`) alongside the existing synchronous handlers
- **mcp**: HTTP/SSE remote transport (`HttpTransport`, `serve`) behind the new `mcp-http` feature — JSON-RPC over `POST /mcp` (incl. `resources/read`) and SSE streaming via `POST /mcp/sse`, reusing the server's Bearer auth (new `src/mcp/http.rs`)

### Changed

- **graph**: schema version bumped 1 → 2; `init_graph_schema` is backward compatible and `SqliteGraph::open` migrates older databases transparently
- **features**: new `cache`, `graph-cjk`, and `mcp-http` feature gates; `mcp` now pulls `async-trait`; all three are included in the `full` feature set
- **deps**: `ort` remains pinned to `=2.0.0-rc.12` (no 2.0.0 stable yet); the pin now carries an explicit lockstep-with-fastembed comment
- **deps**: dev-dependency `tokio` for async tests

### Notes

- The existing sync graph free-function API (`upsert_node(&conn, …)`, `search_nodes(&conn, …)`, …) is unchanged. `GraphBackend` / `SqliteGraph` are additive and may be used alongside it.
- The LLM cache is a dedicated `LLMClient` wrapper rather than an `LLMClientMiddleware`, because the middleware trait is observe-only by design and cannot short-circuit a request with a cached response.

## [0.6.0] - 2026-06-13

### Added

- **search**: `SearchProvider` trait — unified sync interface for ranking backends; `KeywordIndex` term-frequency reference implementation (new `src/search/provider.rs`)
- **search**: `normalize_minmax`, `weighted_sum_fuse`, `combmnz_fuse` — min-max score normalization and alternative fusion strategies complementing existing RRF (new `src/search/fusion.rs`)
- **safety**: `detect_injection(text) → InjectionScore` — weighted regex rules over instruction-override, role-hijack, delimiter-escape, jailbreak, and payload-drop signals; aggregate score saturated to `[0.0, 1.0]` (new `src/safety/injection.rs`)
- **discovery**: async `DiscoverySource` trait + `ModelsDevSource` reqwest implementation behind the new `discovery-async` feature (new `src/discovery/source.rs`)
- **tokens**: `chunk_text(text, opts)` — sentence-boundary, token-budgeted chunking with overlap and CJK + Latin terminator awareness; `ChunkOptions` builder (new `src/tokens/chunk.rs`)
- **llm**: `PromptTemplate` — `{{variable}}` substitution, few-shot example support, and serde round-trip; reuses `render_prompt` (new `src/llm/template.rs`)
- **eval**: `injection` subcommand — measures detection accuracy, recall, and specificity over benign and injection corpora

### Changed

- **errors**: `KernelError` gains a `Search(String)` variant for search-backend failures
- **features**: new `discovery-async` feature gate (adds `discovery`, `reqwest`, `async-trait`, `tokio`); included in the `full` feature set
- **search**, **safety**, **tokens**, **llm**: new public items re-exported from their module roots

## [0.5.0] - 2026-06-13

### Added

- **llm**: `RetryClient` and `RetryConfig` — exponential backoff wrapper around any `LLMClient`, auto-retries 429 and 5xx with jitter (new `src/llm/retry.rs`)
- **llm**: `LLMClientMiddleware` trait with `on_request`/`on_response`/`on_error` async hooks and composable `MiddlewareClient` wrapper (new `src/llm/middleware.rs`)
- **llm**: `ConversationHistory` — ordered message list with role-alternation validation and token-budget-aware truncation that preserves the system message (new `src/llm/history.rs`, `tokens` feature)
- **embedding**: `chunk_batch` utility — splits a batch into provider-limit-sized chunks
- **embedding**: `LazyFastembedProvider::embed_batch` override — LRU cache lookup + batch merge of misses for true batching
- **config**: `FieldError` struct and `validate_config` — structured field-level TOML validation errors (path/expected/value) instead of raw serde strings
- **install**: `AgentKind` expanded with `Windsurf` and `RooCode` variants

### Changed

- **embedding**: `chunk_batch` and `validate_config` re-exported from their module roots
- **llm**: non-success HTTP responses now surface as `KernelError::Http { status, message }` instead of an `LlmApi` string; `RetryClient` retries on the structured 5xx status

### Fixed

- **llm**: `ConversationHistory::truncate_to_budget` now actually removes messages in place (was `&self`, left history untouched); signature is `&mut self`
- **llm**: `ConversationHistory::push` allows consecutive `Tool` messages (parallel tool results)
- **llm**: `RetryClient` jitter mixes `SystemTime` entropy so concurrent retriers desynchronize (real thundering-herd avoidance, no RNG dependency)
- **install**: removed the `Aider` variant — its config path wrote `mcpServers` JSON to `.aider.conf.yml`, which Aider does not consume

## [0.4.0] - 2026-06-12

### Added

- **llm**: `MessageRole` enum replacing stringly-typed role on `ChatMessage`
- **llm**: `ToolDefinition`, `ToolCall`, `ToolResult` — tool/function calling types (new `src/llm/tool.rs`)
- **llm**: `ContentPart` enum — multimodal content (Text, ImageUrl, ImageBase64)
- **llm**: `ResponseFormat` enum (Text, Json, JsonSchema) + JSON mode support
- **llm**: `LLMRequest` builder pattern (`.system().user_message().temperature().build()`) and `tools` field
- **tokens**: `TokenBudget` type (total, used, remaining, `try_reserve`, `release`) (new `src/tokens/budget.rs`)

### Changed

- **llm**: `ChatMessage` role now `MessageRole` instead of `String` (**breaking**)
- **llm**: `LLMRequest` content now `ContentPart`-based for multimodal support (**breaking**)

## [0.3.6] - 2026-06-12

### Added

- **embedding**: `normalize(&mut [f32])` — in-place L2 vector normalization utility
- **provider**: `ProviderIndex::estimate_cost(model_id, prompt_tokens, completion_tokens)` — USD cost estimator using catalog pricing data
- **llm**: `extract_xml_tag(text, tag)` — extract content from Claude-style `<tag>...</tag>` output
- **provider**: `CapabilityProfile` trait extended with default methods: `supports_tool_calling`, `supports_vision`, `supports_streaming`, `context_limit`; `ServiceDescriptor` implements all four from catalog data
- **llm**: `LLMResponse` gains optional fields `finish_reason`, `id`, `created`

### Changed

- **safety**: `mask_secrets` rewritten as single-pass `Regex::replace_all` (eliminates 3 separate loop passes over the input)
- **docs**: `#![deny(missing_docs)]` enforced crate-wide; all 187 previously undocumented public items now have doc comments

### Fixed

- **llm**: `OpenAIClient` and `AnthropicClient` struct literal initializers updated for new `LLMResponse` optional fields

## [0.3.5] - 2026-06-10

### Changed

- **vector-index**: absorb `llm-kernel-vector-index` subcrate into `llm-kernel` as `vector-index` feature gate — no separate crate needed; use `features = ["vector-index"]`
- **vector-index**: `TurbovecIndex` now re-exported as `llm_kernel::embedding::TurbovecIndex`
- **vector-index**: remove `load` from `VectorIndex` trait — trait is now fully object-safe (`dyn VectorIndex` usable); `TurbovecIndex::load` becomes an inherent method
- **vector-index**: atomic save pattern in `TurbovecIndex::save` (temp file → fsync → rename) for crash safety
- **vector-index**: `SearchHit` derives `Copy + PartialEq`; `PartialOrd` impl sorts descending by score, ascending by id on ties
- **full**: `vector-index` feature included in the `full` feature set

### Fixed

- **vector-index**: meta validation on `load` — rejects invalid `bit_width` (must be 2 or 4) and zero `dim`
- **vector-index**: cross-validate loaded index dim/bit_width against sidecar `.meta.json` on load
- **vector-index**: eliminate duplicate `validate_dim` calls in `add → add_with_ids` path

## [0.3.2] - 2026-06-09

### Fixed

- **llm**: add connect and total timeouts to reqwest Client to prevent indefinite hangs (#21)
- **safety**: expand `mask_secrets` patterns — `api_key`, `access_token`, `private_key`, `Basic` auth, AWS `AKIA`, GitHub tokens (#22)
- **store**: wrap SQLite migration in a transaction for atomicity (#23)

### Changed

- **errors**: unify `vault.rs` from `anyhow` to `KernelError::Vault`, add `discovery`/`store`/`config` prelude exports (#24)
- **llm**: extract `into_openai_messages`/`into_anthropic_messages` methods on `LLMRequest`, deduplicate 4 message builder blocks (#25)

### Added

- **tokens**: extend token estimation for Cyrillic, Greek, and Hebrew scripts; count whitespace at 0.25 token weight; add doc comments with `#![deny(missing_docs)]` (#26)

### Docs

- Update badge styles in README files (all 12 languages) for better visibility

## [0.3.0] - 2026-06-08

### Added

- `eval` feature gate: quality evaluation CLI (`llm-kernel-eval`) measuring token estimation accuracy, secret masking completeness, embedding correctness, search quality, and graph query precision
- `eval-full` feature gate: includes graph evaluation module on top of `eval`
- `--baseline <path>` flag for regression detection — compares current metrics against a golden JSON snapshot and exits 1 on any regression
- `eval/baseline.json` — golden baseline snapshot for CI regression checks
- CI `eval` job runs quality regression check on every push and PR
- `llm-kernel-vector-index` eval CLI (`llm-kernel-vector-index-eval`) measuring ANN recall, quantization impact, filtered search accuracy, and persistence round-trip integrity
- `llm-kernel-vector-index` `--baseline` flag for vector-index regression detection

## [0.3.0] - 2026-06-08

### Added

- `embedding`: `VectorIndex` trait — abstract interface for compressed vector indexes, zero dependencies. Concrete implementation: `crates/llm-kernel-vector-index` (TurboQuant)
- `embedding`: `SearchHit` type (`{ id: u64, score: f32 }`) for vector index search results
- `embedding`: `SearchHit::partial_cmp` — sorts by descending score with ascending ID tiebreak
- `embedding`: `VectorIndex::remove(&mut self, ids: &[u64])` — delete vectors by external ID (O(1) per ID)
- `llm-kernel-vector-index`: cross-validation of index dim/bit_width vs sidecar meta.json on load
- `llm-kernel-vector-index`: criterion benchmarks for add (1k/10k), search, filtered search, save/load (2-bit vs 4-bit)

## [0.2.6] - 2026-06-08

### Fixed

- `embedding`: `ort-load-dynamic` restricted to Windows only — Unix targets use static linking for reliable cross-platform builds
- `embedding`: switched ONNX Runtime backend from native-tls to rustls with dynamic loading for cross-platform compatibility
- Restored `Cargo.lock` to version control for reproducible builds

### Changed

- `embedding`: `NomicEmbedTextV15` and `NomicEmbedTextV15Q` now return correct task instruction prefixes (`search_query:` / `search_document:`) matching the official Nomic v1.5 model requirements

## [0.2.5] - 2026-06-08

### Fixed

- `embedding`: use `ort-load-dynamic` for all linux targets to avoid glibc 2.38 dependency (`__isoc23_strtol` etc on ubuntu-22.04)

### Added

- `embedding`: re-export `ort` for DirectML execution provider configuration
- **docs**: add `cargo generate-lockfile` to version bump checklist

## [0.2.4] - 2026-06-07

### Fixed

- `embedding`: `NomicEmbedTextV15` and `NomicEmbedTextV15Q` now return correct task instruction prefixes — `search_query:` / `search_document:` — matching the official Nomic v1.5 model requirements. Previously both returned `None`, producing suboptimal embeddings for search/retrieval workloads (fixes #11)

## [0.2.3] - 2026-06-07

### Fixed

- `embedding`: `ort-load-dynamic` now enabled for `aarch64-linux` targets, fixing cross-compile builds on ARM64

## [0.2.2] - 2026-06-07

### Fixed

- `embedding`: `ort-load-dynamic` restricted to Windows only — Unix targets use static linking for reliable cross-platform builds

## [0.2.1] - 2026-06-07

### Fixed

- `embedding`: switched ONNX Runtime backend from native-tls to rustls with dynamic loading (`ort-load-dynamic`) for cross-platform compatibility
- Restored `Cargo.lock` to version control for reproducible builds

## [0.2.0] - 2026-06-07

### Added

- `embedding-fastembed`: `EmbeddingModel` now exposes `size_mb()`, `model_id()`, `max_seq_length()` const methods for all 44 variants
- `embedding-fastembed`: `LazyFastembedProvider` — instant constructor with lazy model loading, `Condvar`-based concurrent access, and configurable idle eviction
- `embedding-fastembed`: `EmbeddingCache` — zero-dep LRU cache backed by `IndexMap` for query deduplication
- `embedding-fastembed`: `is_model_cached(model, cache_dir)` utility for checking HuggingFace cache
- `embedding-fastembed`: `ModelState` enum for introspecting provider lifecycle (`NotLoaded`, `Loading`, `Cached`, `Ready`, `Disabled`, `Failed`)
- `embedding-fastembed`: `LazyOpts` struct for configuring idle timeout, load timeout, and cache capacity

### Fixed

- `embedding-fastembed`: `ensure_model()` now transitions to `Failed` on load timeout, preventing permanent `Loading` state deadlock

## [0.1.1] - 2026-06-06

### Fixed

- `embedding`: `cosine_similarity` now accumulates in `f64` and returns `f64`, preventing precision loss in high-dimensional spaces (384–1024 dims) where `f32` rounding can flip ranking order between near-identical candidates (fixes #6)
  - **Breaking:** return type changed from `f32` → `f64` for both the free function and `EmbeddingResult::cosine_similarity`
- `embedding-openai`: `embed_batch` now sorts by `index` before mapping to input texts — OpenAI API does not guarantee response ordering, so the previous `zip` could silently corrupt text↔vector associations
- `embedding-openai`, `embedding-fastembed`: `&text[..64]` byte-slice replaced with char-boundary-safe `text_preview` helper — previously panicked on Korean/emoji/CJK input
- `embedding-fastembed`: removed unnecessary `prepared.clone()` in `embed_batch`

### Added

- `embedding-openai`: `OpenAIEmbeddingClient::new_with_model(api_key, model, dim)` for arbitrary model names and dimensions (closes #5)
- `embedding-fastembed-directml`: new feature gate; `FastembedProvider::new_with_directml` for DirectML GPU acceleration on Windows (closes #4)
- `embedding-fastembed`: `new_with_directml` doc warns about D3D12 initialisation latency
- `benches/compute_bench`: `cosine_similarity` criterion benchmarks for 128/384/768/1024 dims
- CI: `directml-check` job now runs `cargo clippy` on Windows in addition to `cargo check`

## [0.1.0] - 2026-06-06

### Changed

- Updated QUICKSTART and README to reflect current API (`prelude::*`, `GraphNode`, `smart_recall`, `SearchResult`, `rrf_fuse`)
- Fixed feature gate count in comparison table (20 modules)

### Note

First public-ready release. No API changes since 0.0.1 — all public types remain the same.

## [0.0.1] - 2026-06-05

### Added

#### Provider Catalog
- Embedded catalog with 16 providers and 114 models (`catalog.json`)
- `ProviderIndex` with O(1) lookup by provider name or model ID
- `CapabilityProfile` trait and `AuthStrategy` enum for auth mode logic
- Model pricing metadata (input/output cost per million tokens)

#### Knowledge Graph
- SQLite-backed graph with FTS5 (trigram tokenizer) full-text search
- `smart_recall` — composite scoring with 5 weighted signals (recency 20%, importance 35%, access 15%, FTS 20%, graph boost 10%)
- BFS traversal via recursive CTE (`related_nodes`)
- 1-hop neighbor lookup with weight aggregation (`graph_neighbors`)
- Full CRUD for nodes and edges
- Lifecycle management: importance decay, stale tagging, access tracking, stats
- `AsyncGraph` wrapper with `spawn_blocking` for tokio runtimes

#### MCP Server
- JSON-RPC 2.0 server framework with tool/resource registration
- Stdio transport loop with batch request support
- Bearer authentication with constant-time comparison
- Auto-generated auth tokens via xorshift PRNG

#### LLM Client (`client-async`)
- Async `LLMClient` trait with `complete()` and `stream_complete()`
- OpenAI and Anthropic implementations (sync + SSE streaming)
- `render_prompt()` with `{{variable}}` substitution
- `extract_json()` / `parse_json()` for structured LLM output extraction

#### Dynamic Model Discovery
- `models.dev` API fetcher with disk cache
- Ollama `/api/tags` model discovery
- OpenAI-compatible `/v1/models` discovery

#### Embedding
- `EmbeddingProvider` trait + `cosine_similarity()`
- OpenAI `text-embedding-3-small`/`large` client with batch support

#### Search
- Reciprocal Rank Fusion (`rrf_fuse`) for hybrid search result merging

#### Token Estimation
- Zero-dependency `estimate_tokens()` with Unicode-script heuristics (CJK, emoji, Arabic, Devanagari, Thai)

#### Telemetry
- Enum-gated `TelemetryEvent` variants (no free-form strings, no PII)
- `ConsoleSink` and `NoopSink` implementations

#### Safety
- `mask_secrets()` — Bearer tokens, API keys, passwords (all occurrences)
- `sanitize_output()` — bidi overrides, plane-14 tags, null bytes, C1 controls
- `classify_failure()` — regex-based error classification into 10 categories
- `strip_ansi()` — ANSI escape code removal

#### Installation Wizard
- MCP config generation for 5 agent types: Claude Desktop, Cursor, Copilot, OpenCode, Cline

#### Security
- `SecretVault` with dotenv-style load/save and symlink guards
- Atomic file writes with 0o600 permissions
- Constant-time bearer token comparison
- Regex-based secret masking across all occurrences

#### Infrastructure
- SQLite store helpers with WAL mode, FTS5, and schema versioning
- TOML configuration loader with auto-create from template
- Criterion benchmarks for graph recall, BFS traversal, token estimation, and RRF fusion

#### CI/CD
- Feature matrix testing (9 combinations)
- `cargo audit` and CycloneDX SBOM generation
- Doc lint with `-D warnings`
- Dependabot weekly updates
- Release workflow — crates.io publish + GitHub Release on tag push
