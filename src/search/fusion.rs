//! Score normalization and alternative list-fusion strategies.
//!
//! These operate on [`crate::search::SearchResult`] lists and complement the
//! rank-based [`crate::search::rrf_fuse`]. Score-based fusion requires
//! comparable scales, so normalize each list (e.g. with [`normalize_minmax`])
//! before fusing with [`weighted_sum_fuse`] or [`combmnz_fuse`].

use std::collections::HashMap;

use crate::search::SearchResult;

/// Normalize scores to `[0, 1]` in place via min-max scaling.
///
/// If the slice is empty this is a no-op. If every score is equal (min == max)
/// all scores are set to `1.0`. Otherwise each score becomes
/// `(score - min) / (max - min)`.
///
/// Non-finite scores (`NaN`, `±Infinity`) are clamped to `0.0` before scaling so
/// the `[0, 1]` output contract holds even when an upstream backend emits an
/// invalid score.
pub fn normalize_minmax(results: &mut [SearchResult]) {
    if results.is_empty() {
        return;
    }
    for r in results.iter_mut() {
        if !r.score.is_finite() {
            r.score = 0.0;
        }
    }
    let min = results
        .iter()
        .map(|r| r.score)
        .fold(f32::INFINITY, f32::min);
    let max = results
        .iter()
        .map(|r| r.score)
        .fold(f32::NEG_INFINITY, f32::max);
    if (min - max).abs() < f32::EPSILON {
        for r in results.iter_mut() {
            r.score = 1.0;
        }
        return;
    }
    let span = max - min;
    for r in results.iter_mut() {
        r.score = (r.score - min) / span;
    }
}

/// Fuse score-normalized lists by weighted sum.
///
/// Computes `score(d) = Σ_i weight_i * score_i(d)` across the input lists,
/// merging duplicate ids (keeping the first text seen) and sorting the output
/// descending by score.
///
/// # Precondition
///
/// `weights.len()` must equal `result_sets.len()` — this is enforced with a
/// runtime assertion (a mismatch is programmer error and panics rather than
/// silently dropping inputs). Each result set should already be normalized
/// (e.g. via [`normalize_minmax`]) so the weighted sum is meaningful.
pub fn weighted_sum_fuse(result_sets: &[Vec<SearchResult>], weights: &[f32]) -> Vec<SearchResult> {
    assert_eq!(
        result_sets.len(),
        weights.len(),
        "weighted_sum_fuse: result_sets.len() ({}) must equal weights.len() ({})",
        result_sets.len(),
        weights.len(),
    );
    let mut scores: HashMap<String, f32> = HashMap::new();
    let mut texts: HashMap<String, String> = HashMap::new();

    for (results, weight) in result_sets.iter().zip(weights.iter()) {
        for result in results {
            *scores.entry(result.id.clone()).or_insert(0.0) += weight * result.score;
            texts
                .entry(result.id.clone())
                .or_insert_with(|| result.text.clone());
        }
    }

    let mut fused: Vec<SearchResult> = scores
        .into_iter()
        .map(|(id, score)| SearchResult {
            text: texts.remove(&id).unwrap_or_default(),
            id,
            score,
        })
        .collect();

    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    fused
}

/// CombMNZ fusion.
///
/// For each distinct id, the fused score is `MNZ(d) * Σ_i score_i(d)`, where
/// `MNZ(d)` is the number of result lists whose top-`k` contains the document.
/// Text is taken from the first list that contains the document. The result is
/// sorted descending by score.
///
/// This is the canonical CombMNZ combination (Fox & Shaw, 1994): a document
/// appearing near the top of many lists is boosted multiplicatively, so broad
/// agreement across backends outranks a single high score.
///
/// Inputs should be normalized first (e.g. via [`normalize_minmax`]) so that
/// score magnitudes are comparable across lists.
pub fn combmnz_fuse(result_sets: &[Vec<SearchResult>], k: usize) -> Vec<SearchResult> {
    let mut scores: HashMap<String, f32> = HashMap::new();
    let mut texts: HashMap<String, String> = HashMap::new();
    let mut topk_counts: HashMap<String, usize> = HashMap::new();

    for results in result_sets {
        let topk = results.len().min(k);
        for (rank, result) in results.iter().enumerate() {
            *scores.entry(result.id.clone()).or_insert(0.0) += result.score;
            texts
                .entry(result.id.clone())
                .or_insert_with(|| result.text.clone());
            if rank < topk {
                *topk_counts.entry(result.id.clone()).or_insert(0) += 1;
            }
        }
    }

    let mut fused: Vec<SearchResult> = scores
        .into_iter()
        .map(|(id, sum)| SearchResult {
            text: texts.remove(&id).unwrap_or_default(),
            score: topk_counts.get(&id).copied().unwrap_or(0) as f32 * sum,
            id,
        })
        .collect();

    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    fused
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_results(ids: &[(&str, f32)]) -> Vec<SearchResult> {
        ids.iter()
            .map(|(id, score)| SearchResult {
                id: id.to_string(),
                score: *score,
                text: format!("text for {id}"),
            })
            .collect()
    }

    #[test]
    fn normalize_minmax_maps_extremes() {
        let mut results = make_results(&[("a", 0.2), ("b", 0.5), ("c", 0.9)]);
        normalize_minmax(&mut results);
        let a = results.iter().find(|r| r.id == "a").unwrap().score;
        let b = results.iter().find(|r| r.id == "b").unwrap().score;
        let c = results.iter().find(|r| r.id == "c").unwrap().score;
        assert!((a - 0.0).abs() < 1e-6);
        assert!((c - 1.0).abs() < 1e-6);
        assert!((b - (0.5 - 0.2) / (0.9 - 0.2)).abs() < 1e-6);
    }

    #[test]
    fn normalize_minmax_all_equal_sets_one() {
        let mut results = make_results(&[("a", 0.5), ("b", 0.5), ("c", 0.5)]);
        normalize_minmax(&mut results);
        for r in &results {
            assert!((r.score - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn normalize_minmax_empty_is_noop() {
        let mut results: Vec<SearchResult> = vec![];
        normalize_minmax(&mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn normalize_minmax_clamps_non_finite() {
        // NaN and Infinity must be folded to 0.0 so the output stays in [0, 1].
        let mut results = make_results(&[("a", f32::NAN), ("b", f32::INFINITY), ("c", 0.5)]);
        normalize_minmax(&mut results);
        for r in &results {
            assert!(r.score.is_finite(), "score {:?} is not finite", r.score);
            assert!(
                (0.0..=1.0).contains(&r.score),
                "score {:?} out of [0,1]",
                r.score
            );
        }
    }

    #[test]
    fn weighted_sum_fuse_formula() {
        // weights [0.7, 0.3]
        // a: 0.7*1.0 + 0.3*0   = 0.70
        // b: 0.7*0.5 + 0.3*1.0 = 0.65
        // c: 0.7*0   + 0.3*0.4 = 0.12
        // order: a > b > c
        let a = make_results(&[("a", 1.0), ("b", 0.5)]);
        let b = make_results(&[("b", 1.0), ("c", 0.4)]);
        let fused = weighted_sum_fuse(&[a, b], &[0.7, 0.3]);
        assert_eq!(fused.len(), 3);
        assert_eq!(fused[0].id, "a");
        assert_eq!(fused[1].id, "b");
        assert_eq!(fused[2].id, "c");
        let score_a = fused.iter().find(|r| r.id == "a").unwrap().score;
        let score_b = fused.iter().find(|r| r.id == "b").unwrap().score;
        let score_c = fused.iter().find(|r| r.id == "c").unwrap().score;
        assert!((score_a - 0.70).abs() < 1e-6);
        assert!((score_b - 0.65).abs() < 1e-6);
        assert!((score_c - 0.12).abs() < 1e-6);
    }

    #[test]
    fn combmnz_boosts_multi_list_doc() {
        // hot: top-1 of A only (count 1); cold: top-1 of B and C (count 2).
        // hot sum = 0.99 (A) + 0.1 (B) = 1.09  -> 1 * 1.09 = 1.09
        // cold sum = 0.1 (A) + 0.99 (B) + 0.99 (C) = 2.08 -> 2 * 2.08 = 4.16
        // cold ranks above hot despite hot's higher single-list score.
        let a = make_results(&[("hot", 0.99), ("cold", 0.1)]);
        let b = make_results(&[("cold", 0.99), ("hot", 0.1)]);
        let c = make_results(&[("cold", 0.99), ("warm", 0.1)]);
        let fused = combmnz_fuse(&[a, b, c], 1);
        assert_eq!(fused[0].id, "cold");
        let cold = fused.iter().find(|r| r.id == "cold").unwrap().score;
        let hot = fused.iter().find(|r| r.id == "hot").unwrap().score;
        assert!(cold > hot);
        assert!((cold - 4.16).abs() < 1e-5);
        assert!((hot - 1.09).abs() < 1e-5);
    }

    #[test]
    #[should_panic(expected = "must equal weights.len()")]
    fn weighted_sum_fuse_panics_on_length_mismatch() {
        let a = make_results(&[("a", 1.0)]);
        // Two result sets but only one weight -> precondition violation.
        let _ = weighted_sum_fuse(&[a.clone(), a], &[0.5]);
    }
}
