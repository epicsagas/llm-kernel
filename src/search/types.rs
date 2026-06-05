//! Search result types.

/// A single search result from any search backend.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Document/chunk identifier.
    pub id: String,
    /// Relevance score (0.0–1.0 for normalized, unbounded for raw BM25).
    pub score: f32,
    /// Text content of the matched document/chunk.
    pub text: String,
}
