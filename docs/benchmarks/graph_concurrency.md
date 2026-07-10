# Graph Concurrency Benchmark

Measured reader/writer concurrency for `AsyncPoolGraph` vs `AsyncGraph` —
issue #45, Axis E ("Concurrency: implemented, unproven").

> **Measured:** 2026-07-10 · **Toolchain:** rustc 1.95.0 (edition 2024) ·
> **Harness:** criterion 0.8 (`async_tokio`) · **Backend:** on-disk SQLite,
> WAL journal, 2000 nodes + 6000 edges, Apple Silicon (darwin 25.x)
>
> Related: [Benchmarks README](README.md) · issue #45 (Axis E) ·
> `src/graph/async_pool.rs`

## What changed (and why this benchmark exists)

The `AsyncPoolGraph` module doc claimed "Multiple read queries can execute
concurrently in WAL mode". Audit found `open()` never set `journal_mode = WAL`
nor `busy_timeout` — it ran under the default DELETE journal with no busy
timeout, where a writer's reserved/exclusive lock blocks readers and concurrent
writers fail immediately with `SQLITE_BUSY`. The pool's headline benefit was
not actually delivered.

**Fix:** `AsyncPoolGraph::open` now sets `journal_mode = WAL` on the file
connection (persists to the DB, so every pooled connection inherits it) and
applies `busy_timeout = 5000` + `synchronous = NORMAL` to **every** connection
(new connections don't inherit these per-connection PRAGMAs). See
`apply_concurrency_pragmas` in `src/graph/async_pool.rs`.

This benchmark measures that the fix works.

## Methodology

`benches/concurrency_bench.rs`. A background writer upserts nodes in a tight
loop. While it runs, a wave of **16 concurrent readers** each does one
`read_node`. The wave latency is the benchmarked quantity; throughput is
16 reads per iteration. Two backends, each on its own WAL file:

- **single-mutex** — `AsyncGraph` (`Arc<Mutex<Connection>>`): every read and
  write serializes on one mutex + one connection.
- **wal-pool-4** — `AsyncPoolGraph` (4 connections, WAL, semaphore-bounded):
  readers run on separate connections and, under WAL, do not block on the
  writer's lock.

Both files are WAL, so the only variable is the access pattern.

```bash
cargo bench --bench concurrency_bench --features graph-pool,graph-async
```

## Results (criterion median, 16-reader wave)

| Backend | wave latency | effective read throughput | ratio |
|---|---:|---:|---:|
| **single-mutex** (`AsyncGraph`) | **687 ms** | ~23 reads/s | 1.0× |
| **wal-pool-4** (`AsyncPoolGraph`) | **381 ms** | ~42 reads/s | **1.8×** |

## Interpretation

- **The WAL pool completes the concurrent reader wave ~1.8× faster under a
  sustained writer.** Under the single mutex, each of the 16 readers waits
  behind the writer's in-progress upsert transaction (one connection, one lock).
  Under the pool, readers take separate connections and WAL lets them proceed
  without waiting for the writer's lock to release.
- The absolute latencies (hundreds of ms for 16 reads) reflect deliberate,
  heavy writer contention — the writer is in a tight upsert loop for the whole
  wave. The *ratio* is the signal, not the absolute time.
- This is the **measured proof** that Axis E's "implemented, unproven" concern
  is now resolved *after* the WAL fix — without the fix the pool would behave
  like DELETE-journal SQLite (readers blocked / `SQLITE_BUSY`), not like the
  1.8×-faster result above.

## Reproduce

```bash
cargo bench --bench concurrency_bench --features graph-pool,graph-async
```

Per-iteration HTML reports are written under `target/criterion/`.
