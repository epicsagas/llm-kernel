# Benchmarks

Performance and quality measurement for llm-kernel. Three concerns, three
different homes — the split exists because each needs a different fidelity and
runs in a different place.

> Related: issue #45 · ROADMAP v1.0.0 #3 · [graph.md](graph.md) ·
> [korean-recall.md](korean-recall.md)

## Role split

| Concern | Where | Why |
|---|---|---|
| **Quality regression** (token MAE, safety F1, recall, …) | **CI, blocking** — `eval-full --strict --baseline` | Deterministic metrics over fixed datasets; a regression is a real signal, not noise. |
| **Benchmark breakage** (compile / panic / fixture) | **CI, blocking** — `bench-smoke` (`cargo bench … -- --test`) | Single-pass execution is deterministic; catches UTF-8 slicing bugs, fixture regressions, compile errors. |
| **Timing regression** (criterion ns/iter) | **Local only** | Shared CI runners' CPU steal / turbo variance routinely exceeds criterion's 1% noise threshold — a timing gate there is flaky. Compare locally on a quiet machine. |

## Quality regression (CI)

```bash
# what CI runs (ci.yml `eval` job):
cargo run --bin llm-kernel-eval --features eval-full -- \
  --strict --baseline eval/baseline.json all
```

`--strict` exits non-zero if any module fails, errors, or disappears vs the
baseline. To refresh the golden baseline after an intended improvement:

```bash
cargo run --bin llm-kernel-eval --features eval-full -- --format json all \
  > eval/baseline.json
```

## Timing regression (local)

```bash
make bench-save    # on main: snapshot as the 'main' baseline
# ...switch to your branch...
make bench-cmp     # criterion reports regressions > 5% vs 'main'
```

`--noise-threshold 0.05` treats sub-5% changes as noise. Pull the median from
criterion's machine-readable output:

```bash
jq '.median.point_estimate' \
  target/criterion/<group>/<function>/<param>/new/estimates.json   # ns
```

## Baseline snapshots

The measured numbers in [graph.md](graph.md) and [korean-recall.md](korean-recall.md)
are snapshots from the reference machine (Apple Silicon) at a pinned commit —
**reproduction anchors, not CI gate values.** They are not compared in CI.
