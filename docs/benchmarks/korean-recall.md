# Korean Recall: graph-cjk vs FTS5 trigram

Measured recall/precision quantifying the gap the `graph-cjk` feature exists to
close — issue #45, Axis D ("Korean recall: implemented, unmeasured … asserted,
not measured"). The `graph-cjk` module's own doc comment claimed trigram "matches
poorly for short CJK queries"; this scenario measures that claim.

> **Measured:** 2026-07-10 · **Toolchain:** rustc 1.95.0 (edition 2024) ·
> **Harness:** `llm-kernel-eval graph-korean` (in-memory SQLite, FTS5 `trigram`
> vs `search_nodes_cjk` LIKE-AND), Apple Silicon (darwin 25.x)
>
> Related: [Graph Benchmarks](graph.md) · issue #45 (Axis D) · `src/graph/cjk.rs`

## Methodology

A 40-document Korean corpus (`eval/datasets/graph_korean_corpus.jsonl`) seeded
into a single in-memory graph. 28 queries (`graph_korean_queries.jsonl`) are run
through **both** search paths against the same DB:

- **trigram** — `graph::search::search_nodes` (FTS5 `nodes_fts MATCH`, the
  `tokenize='trigram'` table). Each whitespace token is phrase-quoted to give
  AND semantics matching the CJK path.
- **cjk** — `graph::cjk::search_nodes_cjk` (contiguous-substring `LIKE` AND).

Five query categories isolate the structural difference:

| Category | n | What it stresses |
|---|---:|---|
| `short-noun` | 8 | 2-syllable nouns (검색, 토큰, …) — <3 chars form no trigram |
| `particle-ending` | 6 | 2-syllable stems; documents contain only inflected forms (구축했다) |
| `compound-split` | 6 | 2+2 token queries (버킷 슬롯); documents hold the no-space compound (버킷슬롯) |
| `long-noun` | 4 | 4+ syllable compounds (프레임버퍼) — trigram forms trigrams → parity expected |
| `substring-fp` | 4 | base noun appearing inside a derivative (디렉터리 ⊂ 서브디렉터리) — both paths over-match |

Document `importance` is strictly descending by id so both paths (which rank by
`importance DESC`) get a deterministic tie-break. `expected_ids` are
hand-labelled and the dataset is checked for NFC normalisation and label
consistency by `eval/datasets/build_korean_corpus.py`.

```bash
cargo run --bin llm-kernel-eval --features eval-full -- graph-korean
```

## Results

| Metric | trigram (FTS5) | cjk (LIKE) | Δ |
|---|---:|---:|---:|
| **recall@5** | **0.2857** | **1.0000** | **+0.7143** |
| **recall@10** | 0.2857 | 1.0000 | +0.7143 |
| precision@5 | 0.9286 | 0.9286 | 0.0000 |
| false-positive rate | 0.0714 | 0.0714 | 0.0000 |

## Interpretation

- **The +71.4-point recall gain is entirely from <3-character queries.** trigram
  recall is exactly 8/28 = 0.2857 — the 8 queries where every token is ≥3 chars
  (`long-noun` + `substring-fp`). All 20 short-stem/compound queries retrieve
  *nothing* under trigram (a 2-char token forms no trigram, so the AND phrase is
  unsatisfiable), while the CJK substring path finds them all.
- **recall@10 == recall@5 for trigram** confirms this is a *matching* gap, not a
  *ranking* gap — trigram returns 0 results, so widening `k` cannot help.
- **Precision is identical** (0.9286) and the false-positive rate matches
  (0.0714). The `substring-fp` category shows the CJK path does **not** trade
  precision for recall: both paths over-match prefixed derivatives
  (서브디렉터리 for 디렉터리) equally, because trigram phrase matching is
  effectively exact-substring for ≥3-char tokens.
- **Design correction during measurement.** An earlier draft assumed trigram
  would also produce *scatter* false positives (non-contiguous trigram
  coincidence). Validation against real FTS5 showed this does not happen — FTS5
  verifies the phrase, so ≥3-char matching is exact-substring. That category was
  replaced by `long-noun` (parity), making the result honest rather than
  favourable: **cjk's advantage is specific to short queries; for long queries
  the two paths are equivalent.**

## Pass gate

`passed = cjk_recall_at_5 >= 0.85 && cjk_recall_at_5 > trigram_recall_at_5` —
currently `1.0000 >= 0.85 && 1.0000 > 0.2857` → **PASS**. Wired into
`eval/baseline.json` so future regressions in Korean recall are caught by the
`eval-full --baseline` CI job.

## Reproduce

```bash
cargo run --bin llm-kernel-eval --features eval-full -- graph-korean
python3 eval/datasets/build_korean_corpus.py   # dataset invariant check
```
