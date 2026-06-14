# Feasibility Study & Design Recommendations for Future Roadmap Milestones

This document provides a comprehensive technical evaluation, feasibility study, and architectural recommendations for the upcoming milestones of the `llm-kernel` library: **v0.6.0 (Search & Intelligence)**, **v0.8.0 (Backend Expansion)**, **v0.9.0 (Search Integrations)**, and **v1.0.0 (Production Readiness)**.

---

## 1. v0.6.0 — Search & Intelligence

This milestone focuses on unifying search interfaces, implementing score normalization/fusion, local safety classification, and token-aware document handling.

### 1.1 `SearchProvider` Trait & Score Normalization
The goal is to provide a single interface for sparse (BM25), dense (Vector), and hybrid search.

* **Feasibility**: Highly feasible. The main challenge is that BM25 scores are unbounded (e.g., `12.5`), while vector cosine similarity is bounded (usually `0.0` to `1.0` or `-1.0` to `1.0`).
* **Design Recommendations**:
  * The `SearchProvider` trait should return a standardized `SearchResult` containing a normalized score (`0.0` to `1.0`).
  * **Score Normalization Strategy**: Avoid simple Min-Max normalization for BM25 as it is highly sensitive to outlier documents in a single query result. Instead, implement a **Sigmoid-based normalization** or **Z-Score normalization** to smooth out scores before ranking fusion:
    $$\text{normalized\_score} = \frac{1}{1 + e^{-\alpha (\text{score} - \beta)}}$$
  * Define a `ScoreNormalizer` utility struct to encapsulate these algorithms.

### 1.2 Prompt Injection Detection (`src/safety/injection.rs`)
Running heavy ML classification models (like ONNX models) locally conflicts with `llm-kernel`'s "zero mandatory external deps" philosophy.

* **Feasibility**: Medium. Heuristic-only models are fast but easy to bypass, while ML models require bulky dependencies.
* **Design Recommendations**:
  * Implement a **two-tier hybrid classification engine**:
    1. **Tier 1 (Heuristics)**: Fast regex patterns checking for common instruction-override phrases (e.g., "ignore previous instructions", "system override").
    2. **Tier 2 (Vector/Semantic check - optional)**: Use the existing embedding feature to measure cosine similarity between the incoming user prompt and a small local vector dataset of known injection patterns. This is lightweight and requires no extra dependencies beyond the already optional `embedding` feature.
  * Expose an optional API to delegate validation to a fast LLM call (e.g., using GPT-4o-mini/Claude-3-Haiku) when high precision is required.

### 1.3 CJK-Aware Token Chunking (`src/tokens/chunk.rs`)
Chunking documents needs to respect token budgets while preserving semantic structure (sentence boundaries).

* **Feasibility**: High. CJK characters are multi-byte and space-less, meaning naive whitespace tokenizers cut characters mid-byte or fail to find word boundaries.
* **Design Recommendations**:
  * Use UTF-8 character boundary validation to prevent cutting characters in half.
  * Implement a **Sentence-aware chunker** that identifies sentence terminators (`.`, `?`, `!`, `。`, `？`, `！`) and aggregates them until the token limit is reached.
  * Utilize the existing `TokenBudget` from `v0.4.0` to determine the maximum chunk size dynamically.

---

## 2. v0.8.0 — Backend Expansion

This milestone focuses on adding PostgreSQL graph support, Qdrant vector indexing, and data migration utilities.

### 2.1 PostgreSQL `GraphBackend` (`src/graph/pg.rs`, `graph-pg` feature)
> **Shipped in v0.8.0** as an in-crate feature gate (`graph-pg`), not a separate crate. The recommendation below evaluated the trade-off; the project chose feature flags for consistency with `embedding-fastembed`/`mcp-http`.

Writing a PostgreSQL implementation of `GraphBackend` requires adapting to connection pooling and different SQL dialects.

* **Feasibility**: High. PostgreSQL natively supports recursive CTEs (Common Table Expressions) which are used for BFS traversal and neighbor lookups.
* **Design Recommendations**:
  * Separate connection pooling (`sqlx` or `tokio-postgres`) from the trait execution.
  * Make the `GraphBackend` trait async-first to natively support network-based databases like PostgreSQL, wrapping SQLite's synchronous file operations in `tokio::task::spawn_blocking` when needed.

### 2.2 SQLite-PostgreSQL Schema Mapping & Migration CLI
SQLite and PostgreSQL handle array columns and loose typing differently.

* **Feasibility**: Medium. SQLite stores tags/projects as CSV strings (e.g., `"rust,async"`), whereas PostgreSQL supports native array types (`TEXT[]`).
* **Design Recommendations**:
  * The migration CLI must parse SQLite's CSV-style strings and map them to native PostgreSQL `TEXT[]` arrays.
  * Implement strict transaction controls: read from SQLite, parse/map in Rust, and bulk-insert into PostgreSQL within a single transaction. If any insert fails, rollback the target PostgreSQL database to prevent half-migrated states.

---

## 3. v0.9.0 — Search Integrations

This milestone adds Elasticsearch and implements a cross-engine search federation system.

### 3.1 Elasticsearch vs Qdrant
Qdrant is a pure vector search engine; Elasticsearch is a hybrid search engine (BM25 + Dense vector).

* **Feasibility**: High. Elasticsearch's BM25 can act as both the sparse and dense backend, which needs to fit cleanly into `VectorSearch` or `SearchProvider` traits.
* **Design Recommendations**:
  * Implement Elasticsearch as a `SearchProvider` rather than just a `VectorSearch` provider, as it handles text matching natively.
  * Expose configurable hybrid search properties directly in the Elasticsearch provider configuration (e.g., using Elasticsearch's native kNN search combined with standard queries).

### 3.2 Search Federation & Concurrency
Federation queries multiple search backends (e.g., Qdrant + Elasticsearch + local SQLite) and merges results.

* **Feasibility**: High. Requires clean asynchronous processing.
* **Design Recommendations**:
  * Run backend queries concurrently using `tokio::spawn` or `futures::future::join_all` to minimize search latency:
    ```rust
    let futures = providers.iter().map(|p| p.search(query, limit));
    let results = futures::future::join_all(futures).await;
    ```
  * Implement a configurable Timeout for search providers. If one remote provider (e.g., Elasticsearch) takes too long, return results from the fast local provider (e.g., SQLite postings) immediately rather than hanging the user query.

---

## 4. v1.0.0 — Production Readiness

This milestone locks the API, establishes performance baselines, and integrates security checking.

### 4.1 SemVer & API Stability (`cargo-semver-checks`)
Preventing breaking changes is critical once `v1.0.0` is released.

* **Feasibility**: High.
* **Design Recommendations**:
  * Integrate `cargo-semver-checks` into the GitHub Actions CI pipeline as a blocking check on every pull request targeting `main`.
  * Audit the public API surface: restrict any internal helper struct/macro to `pub(crate)` instead of `pub` to prevent users from binding to internal APIs.

### 4.2 Performance Regression Gate (`--perf-baseline`)
Creating a CI regression gate based on performance benchmarks.

* **Feasibility**: High. We can use the existing `benches/compute_bench.rs` and `benches/graph_bench.rs` suites.
* **Design Recommendations**:
  * Extend the evaluation binary `llm-kernel-eval` to accept a `--perf-baseline <file>` flag.
  * The binary will execute a fixed set of synthetic search and tokenization workloads, measure execution times, compare them against a baseline JSON file, and fail (exit code `1`) if throughput drops by >15% or latency increases by >20%.
