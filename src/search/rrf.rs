//! Reciprocal Rank Fusion (RRF) for combining multiple ranked result sets.
//!
//! RRF is a simple but effective fusion method:
//! `score(d) = Σ 1/(k + rank_i(d))` where k is a constant (typically 60).

use std::collections::HashMap;

use crate::search::types::SearchResult;

/// Fuse multiple ranked result lists using Reciprocal Rank Fusion.
///
/// `k` is the RRF constant (typically 60). Higher values smooth the
/// contribution of top-ranked results. Each set contributes equally (weight 1.0).
pub fn rrf_fuse(result_sets: &[Vec<SearchResult>], k: u32) -> Vec<SearchResult> {
    let weights = vec![1.0_f32; result_sets.len()];
    rrf_fuse_weighted(result_sets, &weights, k)
}

/// Weighted RRF — each result set multiplies its rank contribution by `weight`.
///
/// `weights` length must equal `result_sets` length. Lets a caller emphasize one
/// source (e.g. lexical) over another (e.g. vector) without dropping the rank
/// signal. `weights=[1.0; n]` reduces to [`rrf_fuse`].
pub fn rrf_fuse_weighted(
    result_sets: &[Vec<SearchResult>],
    weights: &[f32],
    k: u32,
) -> Vec<SearchResult> {
    assert_eq!(
        result_sets.len(),
        weights.len(),
        "rrf_fuse_weighted: result_sets and weights length must match"
    );
    // One map (not two) keyed by id: avoids a second lookup + `remove` per doc
    // on the build path. `text` is taken from the first sighting of each id.
    let mut agg: HashMap<String, (f32, String)> = HashMap::new();

    for (results, weight) in result_sets.iter().zip(weights.iter().copied()) {
        for (rank, result) in results.iter().enumerate() {
            let entry = agg
                .entry(result.id.clone())
                .or_insert_with(|| (0.0, result.text.clone()));
            entry.0 += weight * (1.0 / (k as f32 + rank as f32 + 1.0));
        }
    }

    let mut fused: Vec<SearchResult> = agg
        .into_iter()
        .map(|(id, (score, text))| SearchResult { text, id, score })
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
    fn empty_inputs() {
        let result = rrf_fuse(&[], 60);
        assert!(result.is_empty());
    }

    #[test]
    fn single_list() {
        let list = make_results(&[("a", 0.9), ("b", 0.5)]);
        let fused = rrf_fuse(&[list], 60);
        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].id, "a"); // rank 0 → highest RRF score
    }

    #[test]
    fn two_lists_overlap() {
        let bm25 = make_results(&[("a", 0.9), ("b", 0.7)]);
        let vector = make_results(&[("b", 0.95), ("c", 0.6)]);

        let fused = rrf_fuse(&[bm25, vector], 60);
        assert_eq!(fused.len(), 3);

        // "b" appears in both lists at rank 0 and 1 → should rank highest
        assert_eq!(fused[0].id, "b");
    }

    #[test]
    fn rrf_score_formula() {
        let list = make_results(&[("x", 1.0)]);
        let fused = rrf_fuse(&[list], 60);
        // rank 0 → 1/(60 + 0 + 1) = 1/61 ≈ 0.01639
        let expected = 1.0 / 61.0;
        assert!((fused[0].score - expected).abs() < 1e-6);
    }

    #[test]
    fn three_lists() {
        let a = make_results(&[("doc1", 1.0), ("doc2", 0.8)]);
        let b = make_results(&[("doc2", 0.9), ("doc3", 0.7)]);
        let c = make_results(&[("doc1", 0.8), ("doc3", 0.9)]);

        let fused = rrf_fuse(&[a, b, c], 60);
        assert_eq!(fused.len(), 3);

        // doc1: rank 0 in a, not in b, rank 1 in c → 1/61 + 0 + 1/62
        // doc2: rank 1 in a, rank 0 in b, not in c → 1/62 + 1/61 + 0
        // doc3: not in a, rank 1 in b, rank 1 in c → 0 + 1/62 + 1/62
        // doc1 and doc2 tie (same sum), doc3 is lowest
        let doc3_score = fused.iter().find(|r| r.id == "doc3").unwrap().score;
        let doc1_score = fused.iter().find(|r| r.id == "doc1").unwrap().score;
        assert!(doc1_score > doc3_score);
    }

    #[test]
    fn weighted_higher_weight_dominates() {
        // "a" ranks first only in set 0, "b" ranks first only in set 1.
        // With weights [2.0, 0.5], set 0 contributes 4× more per rank position,
        // so "a" must outscore "b".
        let lexical = make_results(&[("a", 0.9), ("b", 0.3)]);
        let vector = make_results(&[("b", 0.95), ("a", 0.2)]);

        let fused = rrf_fuse_weighted(&[lexical, vector], &[2.0, 0.5], 60);
        assert_eq!(fused.len(), 2);

        let a_score = fused.iter().find(|r| r.id == "a").unwrap().score;
        let b_score = fused.iter().find(|r| r.id == "b").unwrap().score;
        // a: 2.0/61 + 0.5/62,  b: 2.0/62 + 0.5/61
        assert!(
            a_score > b_score,
            "higher-weighted source must dominate: a={a_score}, b={b_score}"
        );
    }

    #[test]
    fn weighted_uniform_matches_rrf_fuse() {
        let a = make_results(&[("x", 1.0), ("y", 0.5)]);
        let b = make_results(&[("y", 0.9), ("z", 0.4)]);

        let plain = rrf_fuse(&[a.clone(), b.clone()], 60);
        let weighted = rrf_fuse_weighted(&[a, b], &[1.0, 1.0], 60);

        assert_eq!(plain.len(), weighted.len());
        for (p, w) in plain.iter().zip(weighted.iter()) {
            assert_eq!(p.id, w.id);
            assert!((p.score - w.score).abs() < 1e-9);
        }
    }

    #[test]
    #[should_panic(expected = "result_sets and weights length must match")]
    fn weighted_length_mismatch_panics() {
        let a = make_results(&[("x", 1.0)]);
        rrf_fuse_weighted(&[a], &[1.0, 2.0], 60);
    }
}
