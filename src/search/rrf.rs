//! Reciprocal Rank Fusion (RRF) for combining multiple ranked result sets.
//!
//! RRF is a simple but effective fusion method:
//! `score(d) = Σ 1/(k + rank_i(d))` where k is a constant (typically 60).

use std::collections::HashMap;

use crate::search::types::SearchResult;

/// Fuse multiple ranked result lists using Reciprocal Rank Fusion.
///
/// `k` is the RRF constant (typically 60). Higher values smooth the
/// contribution of top-ranked results.
pub fn rrf_fuse(result_sets: &[Vec<SearchResult>], k: u32) -> Vec<SearchResult> {
    let mut scores: HashMap<String, f32> = HashMap::new();
    let mut texts: HashMap<String, String> = HashMap::new();

    for results in result_sets {
        for (rank, result) in results.iter().enumerate() {
            let entry = scores.entry(result.id.clone()).or_insert(0.0);
            *entry += 1.0 / (k as f32 + rank as f32 + 1.0);
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
}
