//! Pluggable search backends behind a unified sync interface.
//!
//! [`SearchProvider`] is the common trait every ranking backend implements.
//! [`KeywordIndex`] is a minimal, dependency-free term-frequency backend used
//! for tests and simple keyword search.

use crate::search::SearchResult;

/// Unified sync interface for search backends producing ranked results.
pub trait SearchProvider: Send + Sync {
    /// Human-readable backend name.
    fn name(&self) -> &'static str;
    /// Run a query, returning up to `limit` ranked results.
    fn search(&self, query: &str, limit: usize) -> crate::error::Result<Vec<SearchResult>>;
}

/// A simple keyword (term-frequency) index over a fixed set of documents.
///
/// Scores each document by the total count of query-term occurrences in its
/// text (case-insensitive substring matching — a query term `rust` will match
/// inside `trust` or `rustic`). No stemming, no IDF weighting — a building
/// block, not a BM25 replacement.
pub struct KeywordIndex {
    /// Indexed documents as `(id, text)` pairs.
    docs: Vec<(String, String)>,
}

impl KeywordIndex {
    /// Build an index from `(id, text)` document pairs.
    pub fn new(docs: Vec<(String, String)>) -> Self {
        Self { docs }
    }
}

impl SearchProvider for KeywordIndex {
    fn name(&self) -> &'static str {
        "keyword"
    }

    fn search(&self, query: &str, limit: usize) -> crate::error::Result<Vec<SearchResult>> {
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let query_lower = query.to_lowercase();
        let terms: Vec<&str> = query_lower.split_ascii_whitespace().collect();

        let mut scored: Vec<SearchResult> = self
            .docs
            .iter()
            .filter_map(|(id, text)| {
                let text_lower = text.to_lowercase();
                let mut count: usize = 0;
                for term in &terms {
                    count += text_lower.matches(term).count();
                }
                if count == 0 {
                    None
                } else {
                    Some(SearchResult {
                        id: id.clone(),
                        score: count as f32,
                        text: text.clone(),
                    })
                }
            })
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit);
        Ok(scored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_docs() -> Vec<(String, String)> {
        vec![
            (
                "d1".to_string(),
                "the rust programming language is fast".to_string(),
            ),
            (
                "d2".to_string(),
                "python is a popular programming language".to_string(),
            ),
            (
                "d3".to_string(),
                "rust has zero cost abstractions".to_string(),
            ),
            ("d4".to_string(), "cooking recipes for dinner".to_string()),
        ]
    }

    #[test]
    fn all_terms_outranks_fewer() {
        let index = KeywordIndex::new(sample_docs());
        let results = index.search("rust programming language", 10).unwrap();
        // d1 contains all three query terms; d2 contains two; d3 contains one.
        assert_eq!(results[0].id, "d1");
        let d1 = results.iter().find(|r| r.id == "d1").unwrap().score;
        let d2 = results.iter().find(|r| r.id == "d2").unwrap().score;
        let d3 = results.iter().find(|r| r.id == "d3").unwrap().score;
        assert!(d1 > d2);
        assert!(d2 > d3);
    }

    #[test]
    fn empty_query_returns_empty() {
        let index = KeywordIndex::new(sample_docs());
        let results = index.search("", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn limit_is_respected() {
        let index = KeywordIndex::new(sample_docs());
        let results = index.search("programming language", 1).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn unknown_term_returns_only_matches() {
        let index = KeywordIndex::new(sample_docs());
        // "rust" appears only in d1 and d3; "xyzzy" appears nowhere.
        let results = index.search("rust xyzzy", 10).unwrap();
        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"d1"));
        assert!(ids.contains(&"d3"));
        assert!(!ids.contains(&"d2"));
        assert!(!ids.contains(&"d4"));
    }

    #[test]
    fn name_is_keyword() {
        let index = KeywordIndex::new(vec![]);
        assert_eq!(index.name(), "keyword");
    }

    #[test]
    fn zero_score_docs_excluded() {
        let index = KeywordIndex::new(sample_docs());
        let results = index.search("rust", 10).unwrap();
        // d4 does not contain "rust" -> excluded entirely.
        assert!(results.iter().all(|r| r.id != "d4"));
    }
}
