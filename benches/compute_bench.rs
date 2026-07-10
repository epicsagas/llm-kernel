//! Benchmarks for pure-compute modules — token estimation, RRF fusion, and
//! embedding similarity.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use llm_kernel::embedding::cosine_similarity;
use llm_kernel::search::rrf::rrf_fuse;
use llm_kernel::search::types::SearchResult;
use llm_kernel::tokens::estimate_tokens;

// ── estimate_tokens ──────────────────────────────────────

fn bench_estimate_tokens(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokens");

    let ascii_text = "The quick brown fox jumps over the lazy dog. ".repeat(100);
    let cjk_text =
        "これは日本語のテストテキストです。漢字ひらがなカタカナを混ぜています。".repeat(50);
    let mixed_text = format!(
        "{}{}{}",
        ascii_text.chars().take(200).collect::<String>(),
        cjk_text.chars().take(200).collect::<String>(),
        "🎉🚀👍⚡🔥💯".repeat(20)
    );

    let cases = [
        ("ascii_4k", ascii_text.as_str()),
        ("cjk_2k", cjk_text.as_str()),
        ("mixed_1k", mixed_text.as_str()),
    ];

    for (label, text) in cases {
        group.bench_with_input(BenchmarkId::new("estimate", label), &text, |b, text| {
            b.iter(|| black_box(estimate_tokens(text)));
        });
    }

    group.finish();
}

// ── rrf_fuse ─────────────────────────────────────────────

fn make_results(n: usize) -> Vec<SearchResult> {
    (0..n)
        .map(|i| SearchResult {
            id: format!("doc{i}"),
            score: 1.0 / (i as f32 + 1.0),
            text: format!("Text content for document {i}"),
        })
        .collect()
}

fn bench_rrf_fuse(c: &mut Criterion) {
    let mut group = c.benchmark_group("rrf_fusion");

    for list_count in [3, 10, 20] {
        let result_sets: Vec<Vec<SearchResult>> =
            (0..list_count).map(|_| make_results(100)).collect();

        group.bench_with_input(
            BenchmarkId::new("lists", list_count),
            &result_sets,
            |b, sets| {
                b.iter(|| black_box(rrf_fuse(sets, 60)));
            },
        );
    }

    group.finish();
}

// ── cosine_similarity ─────────────────────────────────────

fn bench_cosine_similarity(c: &mut Criterion) {
    let mut group = c.benchmark_group("cosine_similarity");

    for dim in [128usize, 384, 768, 1024] {
        let scale = (dim as f32).sqrt().recip();
        let a: Vec<f32> = vec![scale; dim];
        let b: Vec<f32> = (0..dim).map(|i| (i as f32 * 0.001).sin() * scale).collect();

        group.bench_with_input(BenchmarkId::new("dim", dim), &(&a, &b), |bench, (a, b)| {
            bench.iter(|| black_box(cosine_similarity(a, b)));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_estimate_tokens,
    bench_rrf_fuse,
    bench_cosine_similarity
);
criterion_main!(benches);
