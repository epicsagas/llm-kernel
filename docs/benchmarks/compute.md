# Compute Benchmarks

Performance baseline for the pure-compute modules (`src/tokens/`, `src/search/`,
`src/embedding/` base): token estimation, RRF fusion, and cosine similarity.

> **Measured:** 2026-07-10 · **Commit:** `57bdbe5` · **Toolchain:** rustc 1.95.0
> (edition 2024) · **Harness:** criterion 0.8 · **Machine:** Apple Silicon
> (darwin 25.x)
>
> These are reference-machine snapshots, **not CI gate values**. See
> [README.md](README.md) for the quality/timing/bench-smoke role split.
> Related: issue #45 · ROADMAP v1.0.0 #3.

## Methodology

`benches/compute_bench.rs`. Each routine runs over representative inputs:

- `estimate_tokens` — 4 KB ASCII, ~2 KB CJK (Japanese), ~1 KB mixed (ASCII+CJK+emoji).
- `rrf_fuse` — 3 / 10 / 20 result lists of 100 `SearchResult`s each (the fusion
  is `O(total_results · log)` due to the score map + sort).
- `cosine_similarity` — random unit vectors of dim 128 / 384 / 768 / 1024
  (covering `bge-small` → `bge-m3` → large embedding dims).

```bash
cargo bench --bench compute_bench --features search,tokens,embedding
```

## Results (criterion median point estimate)

### Token estimation

| Input | time |
|---|---:|
| ascii_4k | 11.0 µs |
| cjk_2k | 9.0 µs |
| mixed_1k | 1.5 µs |

Linear in input length; the Unicode chars-per-token heuristic (`src/tokens/`)
is a single pass, so ~3 ns/byte. The eval suite tracks estimation MAE
(`eval/datasets/tokens.jsonl`), not throughput.

### RRF fusion

| Lists × 100 results | time |
|---|---:|
| 3 lists | 32.9 µs |
| 10 lists | 83.8 µs |
| 20 lists | 131.2 µs |

Near-linear in total result count (~6.6 µs per 100 results). The dominant cost
is the `BTreeMap`/sort over the merged candidates, not the RRF arithmetic.

### Cosine similarity

| Dimension | time |
|---:|---:|
| 128 | 104.5 ns |
| 384 | 374.4 ns |
| 768 | 737.4 ns |
| 1024 | 988.3 ns |

**Effectively linear in dimension** (128→1024 is 8× the dim, 9.5× the time —
the small super-linearity is fixed per-call overhead amortised away at higher
dim). At ~1 ns/dimension, similarity over a shortlist is never the bottleneck;
the vector *search* (ANN) cost lives in the backend (`vector-index` / qdrant /
elastic / pgvector), not here.

## Interpretation

These three routines are the hot paths any RAG/search pipeline built on
llm-kernel hits per query. All are sub-100 µs at realistic sizes — token
estimation and fusion are negligible versus the LLM round-trip, and cosine is
negligible versus the ANN retrieval that feeds it. No optimisation work is
indicated; the numbers exist to catch *regressions*.

## Reproduce

```bash
cargo bench --bench compute_bench --features search,tokens,embedding
# median per case:
jq '.median.point_estimate' target/criterion/<group>/<case>/new/estimates.json
```
