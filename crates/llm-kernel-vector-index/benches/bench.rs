//! Benchmarks for TurbovecIndex — add, search, filtered search, save/load.
//!
//! Run: `cargo bench` from `crates/llm-kernel-vector-index/`

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use llm_kernel::embedding::VectorIndex;
use llm_kernel_vector_index::TurbovecIndex;

/// Deterministic pseudo-random vector for reproducibility.
fn make_vector(dim: usize, seed: usize) -> Vec<f32> {
    (0..dim)
        .map(|i| {
            let x = (seed as f64 * 0.6180339887 + i as f64 * 0.3141592653).sin();
            x as f32
        })
        .collect()
}

fn batch_vectors(n: usize, dim: usize) -> Vec<Vec<f32>> {
    (0..n).map(|i| make_vector(dim, i)).collect()
}

// ── Add benchmarks ──────────────────────────────────────────────────

fn bench_add(c: &mut Criterion) {
    let dim = 384;
    let vectors_1k = batch_vectors(1_000, dim);
    let vectors_10k = batch_vectors(10_000, dim);

    let mut group = c.benchmark_group("add");

    for bit_width in [2u8, 4] {
        group.bench_with_input(BenchmarkId::new("1k", bit_width), &bit_width, |b, &bw| {
            b.iter(|| {
                let mut idx = TurbovecIndex::new(dim, bw).unwrap();
                idx.add(black_box(&vectors_1k)).unwrap();
            });
        });

        group.bench_with_input(BenchmarkId::new("10k", bit_width), &bit_width, |b, &bw| {
            b.iter(|| {
                let mut idx = TurbovecIndex::new(dim, bw).unwrap();
                idx.add(black_box(&vectors_10k)).unwrap();
            });
        });
    }

    group.finish();
}

// ── Search benchmarks ───────────────────────────────────────────────

fn bench_search(c: &mut Criterion) {
    let dim = 384;
    let vectors = batch_vectors(1_000, dim);
    let query = make_vector(dim, 999);

    let mut group = c.benchmark_group("search");

    for bit_width in [2u8, 4] {
        let mut idx = TurbovecIndex::new(dim, bit_width).unwrap();
        idx.add(&vectors).unwrap();

        group.bench_with_input(BenchmarkId::new("k10", bit_width), &idx, |b, idx| {
            b.iter(|| idx.search(black_box(&query), black_box(10)).unwrap());
        });
    }

    group.finish();
}

// ── Filtered search benchmarks ──────────────────────────────────────

fn bench_search_filtered(c: &mut Criterion) {
    let dim = 384;
    let vectors = batch_vectors(1_000, dim);
    let query = make_vector(dim, 999);
    // Allowlist: half the IDs.
    let allowlist: Vec<u64> = (0..500).collect();

    let mut group = c.benchmark_group("search_filtered");

    for bit_width in [2u8, 4] {
        let mut idx = TurbovecIndex::new(dim, bit_width).unwrap();
        idx.add(&vectors).unwrap();

        group.bench_with_input(BenchmarkId::new("k10", bit_width), &idx, |b, idx| {
            b.iter(|| {
                idx.search_filtered(black_box(&query), black_box(10), black_box(&allowlist))
                    .unwrap()
            });
        });
    }

    group.finish();
}

// ── Save/Load roundtrip benchmark ───────────────────────────────────

fn bench_save_load(c: &mut Criterion) {
    let dim = 384;
    let vectors = batch_vectors(1_000, dim);
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("bench.tvim");

    let mut group = c.benchmark_group("save_load");

    for bit_width in [2u8, 4] {
        let mut idx = TurbovecIndex::new(dim, bit_width).unwrap();
        idx.add(&vectors).unwrap();

        group.bench_with_input(
            BenchmarkId::new("1k_roundtrip", bit_width),
            &idx,
            |b, idx| {
                b.iter(|| {
                    idx.save(black_box(&path)).unwrap();
                    TurbovecIndex::load(black_box(&path)).unwrap();
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_add,
    bench_search,
    bench_search_filtered,
    bench_save_load,
);
criterion_main!(benches);
