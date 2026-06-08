//! # llm-kernel-vector-index-eval
//!
//! Quality evaluation CLI for the TurbovecIndex vector index.
//!
//! Tests ANN recall, quantization impact, filtered search, and persistence
//! using synthetic clustered vector datasets.

use anyhow::Result;
use clap::{Parser, Subcommand};
use llm_kernel::embedding::VectorIndex;
use llm_kernel_vector_index::TurbovecIndex;
use serde::{Deserialize, Serialize};

// ── CLI ──────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "llm-kernel-vector-index-eval")]
#[command(about = "Quality evaluation for llm-kernel-vector-index")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output format
    #[arg(long, default_value = "markdown")]
    format: String,

    /// Baseline JSON to compare against (regression detection mode)
    #[arg(long)]
    baseline: Option<std::path::PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// ANN recall@K with clustered vectors
    Recall,
    /// Compare 2-bit vs 4-bit quantization quality
    Quantization,
    /// Filtered search accuracy
    Filtered,
    /// Persistence save/load round-trip integrity
    Persistence,
    /// Run all evaluations
    All,
}

// ── Report types ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct EvalReport {
    module: String,
    metrics: serde_json::Value,
    passed: bool,
}

impl std::fmt::Display for EvalReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "## {} [{}]",
            self.module,
            if self.passed { "PASS" } else { "FAIL" }
        )?;
        if let serde_json::Value::Object(map) = &self.metrics {
            for (k, v) in map {
                let display = match v {
                    serde_json::Value::Number(n) => {
                        if let Some(f) = n.as_f64() {
                            format!("{f:.4}")
                        } else {
                            v.to_string()
                        }
                    }
                    _ => v.to_string(),
                };
                writeln!(f, "  {k}: {display}")?;
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct EvalSummary {
    results: Vec<EvalReport>,
}

impl std::fmt::Display for EvalSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let passed = self.results.iter().filter(|r| r.passed).count();
        let total = self.results.len();
        writeln!(f, "┌─────────────────────────────────────────────┐")?;
        writeln!(f, "│  llm-kernel-vector-index-eval — Report      │")?;
        writeln!(f, "├─────────────────────────────────────────────┤")?;
        for r in &self.results {
            let status = if r.passed { "PASS" } else { "FAIL" };
            writeln!(f, "│  {:<15} │ {status}", r.module.as_str())?;
        }
        writeln!(f, "├─────────────────────────────────────────────┤")?;
        writeln!(
            f,
            "│  Total: {passed} / {total} passed                     "
        )?;
        writeln!(f, "└─────────────────────────────────────────────┘")?;
        for r in &self.results {
            writeln!(f)?;
            write!(f, "{r}")?;
        }
        Ok(())
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Deterministic pseudo-random vector from seed.
fn seeded_vector(dim: usize, seed: u64) -> Vec<f32> {
    (0..dim)
        .map(|i| {
            let s = (seed.wrapping_add(i as u64)).wrapping_mul(0x517cc1b727220a95);
            ((s >> 33) as i32) as f32 / (1i32 << 31) as f32
        })
        .collect()
}

/// Create clustered vectors: `n_clusters` groups of `cluster_size` vectors,
/// each group centered around a distinct centroid with small perturbation.
fn clustered_vectors(
    dim: usize,
    n_clusters: usize,
    cluster_size: usize,
) -> (Vec<Vec<f32>>, Vec<u64>) {
    let mut vectors = Vec::with_capacity(n_clusters * cluster_size);
    let mut ids = Vec::with_capacity(n_clusters * cluster_size);
    for c in 0..n_clusters {
        let centroid = seeded_vector(dim, c as u64 * 1000);
        for i in 0..cluster_size {
            let noise = seeded_vector(dim, c as u64 * 1000 + i as u64 + 1);
            let v: Vec<f32> = centroid
                .iter()
                .zip(noise.iter())
                .map(|(&cent, &n)| cent * 0.9 + n * 0.1)
                .collect();
            let id = (c * cluster_size + i) as u64;
            vectors.push(v);
            ids.push(id);
        }
    }
    (vectors, ids)
}

// ── Eval: Recall@K ───────────────────────────────────────────────────────────

fn eval_recall() -> EvalReport {
    let dim = 64;
    let n_clusters = 20;
    let cluster_size = 10;
    let (vectors, ids) = clustered_vectors(dim, n_clusters, cluster_size);

    let mut idx = TurbovecIndex::new(dim, 4).unwrap();
    idx.add_with_ids(&vectors, &ids).unwrap();

    let k = 10;
    let mut total_recall = 0.0_f64;
    let mut scenarios = 0usize;

    // For each centroid, query with the centroid itself.
    // Expected: all cluster members should be in top-k.
    for c in 0..n_clusters {
        let query = seeded_vector(dim, c as u64 * 1000);
        let hits = idx.search(&query, k).unwrap();

        let expected_start = (c * cluster_size) as u64;
        let expected_end = expected_start + cluster_size as u64;
        let expected_ids: std::collections::HashSet<u64> = (expected_start..expected_end).collect();
        let found_ids: std::collections::HashSet<u64> = hits.iter().map(|h| h.id).collect();

        let tp = expected_ids.intersection(&found_ids).count() as f64;
        let recall = tp / expected_ids.len() as f64;
        total_recall += recall;
        scenarios += 1;
    }

    let avg_recall = total_recall / scenarios as f64;

    EvalReport {
        module: "recall".into(),
        metrics: serde_json::json!({
            "dim": dim,
            "n_vectors": vectors.len(),
            "k": k,
            "scenarios": scenarios,
            "avg_recall_at_k": avg_recall,
        }),
        passed: avg_recall >= 0.70,
    }
}

// ── Eval: Quantization comparison ────────────────────────────────────────────

fn eval_quantization() -> EvalReport {
    let dim = 64;
    let n_clusters = 10;
    let cluster_size = 10;
    let (vectors, ids) = clustered_vectors(dim, n_clusters, cluster_size);
    let _k = 5;

    let mut bw2_recall = 0.0_f64;
    let mut bw4_recall = 0.0_f64;

    for bit_width in [2u8, 4] {
        let mut idx = TurbovecIndex::new(dim, bit_width).unwrap();
        idx.add_with_ids(&vectors, &ids).unwrap();

        let mut total_recall = 0.0_f64;
        for c in 0..n_clusters {
            let query = seeded_vector(dim, c as u64 * 1000);
            let hits = idx.search(&query, cluster_size).unwrap();

            let expected_start = (c * cluster_size) as u64;
            let expected_end = expected_start + cluster_size as u64;
            let expected_ids: std::collections::HashSet<u64> =
                (expected_start..expected_end).collect();
            let found_ids: std::collections::HashSet<u64> = hits.iter().map(|h| h.id).collect();

            let tp = expected_ids.intersection(&found_ids).count() as f64;
            total_recall += tp / expected_ids.len() as f64;
        }
        let avg = total_recall / n_clusters as f64;
        if bit_width == 2 {
            bw2_recall = avg;
        } else {
            bw4_recall = avg;
        }
    }

    let degradation = bw2_recall - bw4_recall;

    EvalReport {
        module: "quantization".into(),
        metrics: serde_json::json!({
            "dim": dim,
            "n_vectors": vectors.len(),
            "recall_2bit": bw2_recall,
            "recall_4bit": bw4_recall,
            "degradation_2bit_vs_4bit": degradation,
        }),
        passed: bw4_recall >= 0.40,
    }
}

// ── Eval: Filtered search ────────────────────────────────────────────────────

fn eval_filtered() -> EvalReport {
    let dim = 64;
    let n_clusters = 10;
    let cluster_size = 10;
    let (vectors, ids) = clustered_vectors(dim, n_clusters, cluster_size);

    let mut idx = TurbovecIndex::new(dim, 4).unwrap();
    idx.add_with_ids(&vectors, &ids).unwrap();

    let k = 10;
    let mut total_precision = 0.0_f64;
    let mut scenarios = 0usize;

    // For each cluster, filter to only that cluster's IDs.
    // All results must be from the allowlist.
    for c in 0..n_clusters {
        let query = seeded_vector(dim, c as u64 * 1000);
        let expected_start = (c * cluster_size) as u64;
        let expected_end = expected_start + cluster_size as u64;
        let allowlist: Vec<u64> = (expected_start..expected_end).collect();

        let hits = idx.search_filtered(&query, k, &allowlist).unwrap();

        let allowed_set: std::collections::HashSet<u64> = allowlist.iter().copied().collect();
        let in_allowed = hits.iter().filter(|h| allowed_set.contains(&h.id)).count() as f64;
        let precision = if hits.is_empty() {
            1.0
        } else {
            in_allowed / hits.len() as f64
        };
        total_precision += precision;
        scenarios += 1;
    }

    let avg_precision = total_precision / scenarios as f64;

    EvalReport {
        module: "filtered".into(),
        metrics: serde_json::json!({
            "dim": dim,
            "n_vectors": vectors.len(),
            "scenarios": scenarios,
            "avg_filtered_precision": avg_precision,
        }),
        passed: avg_precision >= 0.95,
    }
}

// ── Eval: Persistence ────────────────────────────────────────────────────────

fn eval_persistence() -> EvalReport {
    let dim = 64;
    let n_clusters = 5;
    let cluster_size = 5;
    let (vectors, ids) = clustered_vectors(dim, n_clusters, cluster_size);

    let mut idx = TurbovecIndex::new(dim, 4).unwrap();
    idx.add_with_ids(&vectors, &ids).unwrap();

    // Get pre-save search results.
    let query = seeded_vector(dim, 0);
    let pre_hits = idx.search(&query, 5).unwrap();
    let pre_ids: Vec<u64> = pre_hits.iter().map(|h| h.id).collect();

    // Save to temp directory.
    let tmp_dir = std::env::temp_dir().join("llm-kernel-vector-index-eval");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let path = tmp_dir.join("eval.tvim");
    idx.save(&path).unwrap();

    // Load back.
    let loaded = TurbovecIndex::load(&path).unwrap();

    // Verify metadata.
    let meta_ok = loaded.dim() == dim && loaded.bit_width() == 4 && loaded.len() == vectors.len();

    // Verify search results match.
    let post_hits = loaded.search(&query, 5).unwrap();
    let post_ids: Vec<u64> = post_hits.iter().map(|h| h.id).collect();
    let ids_match = pre_ids == post_ids;

    // Verify save/load round-trip produces valid index file.
    let index_file_exists = path.exists();
    let meta_file_exists = path.with_extension("meta.json").exists();

    EvalReport {
        module: "persistence".into(),
        metrics: serde_json::json!({
            "dim": dim,
            "n_vectors": vectors.len(),
            "meta_preserved": meta_ok,
            "search_results_match": ids_match,
            "index_file_exists": index_file_exists,
            "meta_file_exists": meta_file_exists,
        }),
        passed: meta_ok && ids_match && index_file_exists && meta_file_exists,
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn compare_reports(current: &[EvalReport], baseline: &[EvalReport]) -> (Vec<String>, bool) {
    let base_metrics: std::collections::HashMap<&str, &serde_json::Map<String, serde_json::Value>> =
        baseline
            .iter()
            .filter(|r| r.metrics.get("error").is_none())
            .filter_map(|r| r.metrics.as_object().map(|m| (r.module.as_str(), m)))
            .collect();

    let higher = [
        "avg_recall_at_k",
        "avg_filtered_precision",
        "recall_2bit",
        "recall_4bit",
    ];
    let lower = ["degradation_2bit_vs_4bit"];

    let mut diffs = Vec::new();
    let mut has_regression = false;

    for report in current {
        if report.metrics.get("error").is_some() {
            continue;
        }
        let (Some(base_obj), Some(cur_obj)) = (
            base_metrics.get(report.module.as_str()),
            report.metrics.as_object(),
        ) else {
            continue;
        };

        for (key, cur_val) in cur_obj {
            let (Some(cur_f), Some(base_f)) =
                (cur_val.as_f64(), base_obj.get(key).and_then(|v| v.as_f64()))
            else {
                continue;
            };
            let delta = cur_f - base_f;
            if delta.abs() < 1e-10 {
                continue;
            }

            let (arrow, is_regression) = if higher.contains(&key.as_str()) {
                if delta < 0.0 {
                    ("↓", true)
                } else {
                    ("↑", false)
                }
            } else if lower.contains(&key.as_str()) {
                if delta > 0.0 {
                    ("↑ (worse)", true)
                } else {
                    ("↓ (better)", false)
                }
            } else {
                ("~", false)
            };

            if is_regression {
                has_regression = true;
            }

            diffs.push(format!(
                "  {}.{}: {:.4} → {:.4} {arrow} ({:+.4}){}",
                report.module,
                key,
                base_f,
                cur_f,
                delta,
                if is_regression { " ⚠ REGRESSION" } else { "" },
            ));
        }
    }

    (diffs, has_regression)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let reports: Vec<EvalReport> = match cli.command {
        Commands::Recall => vec![eval_recall()],
        Commands::Quantization => vec![eval_quantization()],
        Commands::Filtered => vec![eval_filtered()],
        Commands::Persistence => vec![eval_persistence()],
        Commands::All => vec![
            eval_recall(),
            eval_quantization(),
            eval_filtered(),
            eval_persistence(),
        ],
    };

    let summary = EvalSummary { results: reports };

    match cli.format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&summary).unwrap()),
        _ => println!("{summary}"),
    }

    if let Some(ref path) = cli.baseline {
        let data = std::fs::read_to_string(path)?;
        let baseline: EvalSummary = serde_json::from_str(&data)?;
        let (diffs, has_regression) = compare_reports(&summary.results, &baseline.results);
        if diffs.is_empty() {
            eprintln!("\n✅ No metric changes detected vs baseline.");
        } else {
            eprintln!("\n## Baseline Diff");
            for line in &diffs {
                eprintln!("{line}");
            }
            if has_regression {
                eprintln!("\n❌ Regression detected.");
                std::process::exit(1);
            } else {
                eprintln!("\n✅ No regression — all changes are improvements or neutral.");
            }
        }
    }

    Ok(())
}
