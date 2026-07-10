//! Elasticsearch `AsyncVectorIndex` (`elastic` feature).
//!
//! `ElasticsearchVectorIndex` implements [`AsyncVectorIndex`] over a
//! hand-rolled [`reqwest`] client speaking Elasticsearch 8.x's REST API. It is
//! the async counterpart to the in-memory [`VectorIndex`](crate::embedding::VectorIndex)
//! and a sibling of [`QdrantVectorIndex`](crate::embedding::QdrantVectorIndex).
//!
//! # Why hand-rolled reqwest (not the `elasticsearch` crate)?
//!
//! The official `elasticsearch` crate has **no stable release** — every
//! published version is `-alpha.x` (`max_stable_version: None` on crates.io).
//! For a foundation library heading into the v1.0.0 semver lock, an alpha
//! dependency is a blocker. The REST surface this trait needs is small (index
//! create/delete, bulk upsert/delete, knn `_search`, `_count`), so a typed
//! reqwest client reuses the existing `client-async` reqwest dependency and adds
//! zero transitive crates.
//!
//! # Scope
//!
//! Elasticsearch is a *hybrid* engine (BM25 + dense vector). Per the v0.9.0
//! design decision, this implementation exposes only the **dense-vector**
//! contract of [`AsyncVectorIndex`] — native BM25 text search via a
//! [`SearchProvider`](crate::search::SearchProvider) is deferred to a later
//! milestone. The vector results federate cleanly with Qdrant and TurboVec
//! because federation defaults to rank-based RRF (scale-invariant).
//!
//! # Score semantics
//!
//! The `score` field of each [`SearchHit`] carries the Elasticsearch knn
//! `_score`, which for a `cosine`-similarity `dense_vector` field is
//! `(1 + cosine) / 2 ∈ [0, 1]` — *not* the raw cosine that Qdrant reports
//! (`[0, 1]` of a different monotonic map) nor the `[-1, 1]` raw cosine of the
//! in-memory `TurbovecIndex`. Cross-backend score magnitudes are therefore not
//! directly comparable. This is harmless under the federation default
//! (Reciprocal Rank Fusion — rank-based and scale-invariant), but
//! `WeightedSum` federation (behind the optional `federation` feature, which
//! min-max normalizes each list in isolation before a weighted sum) should be
//! used with care across these heterogeneous scales. See the federation
//! module's "Why RRF is the default" docs for the full rationale.

use std::time::Duration;

use crate::error::{KernelError, Result};
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;

use super::{AsyncVectorIndex, SearchHit};

/// Async vector index backed by an Elasticsearch 8.x index.
///
/// The index is created on construction (a `dense_vector` field with cosine
/// similarity) if it does not already exist. All operations are async over a
/// plain [`reqwest::Client`]. Connection-string credentials embedded in `url`
/// (e.g. `https://user:pass@host`) are used for the request but never leaked in
/// error messages — see `redact_credentials`.
pub struct ElasticsearchVectorIndex {
    client: reqwest::Client,
    /// Base URL, possibly containing `user:pass@` credentials. Used verbatim
    /// for requests; redacted everywhere else.
    base_url: String,
    index: String,
    dim: usize,
}

impl ElasticsearchVectorIndex {
    /// Connect to `url` (e.g. `http://localhost:9200`) and ensure `index`
    /// exists with a `dense_vector` field of `dim` dimensions and cosine
    /// similarity.
    pub async fn new(url: &str, index: &str, dim: usize) -> Result<Self> {
        validate_index_name(index)?;
        let client = reqwest::Client::builder()
            // Guard direct (non-federated) callers against an unresponsive node.
            // `FederatedSearch` additionally wraps each call in
            // `tokio::time::timeout`, but a bare `ElasticsearchVectorIndex` has
            // no such outer guard.
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| KernelError::Embedding(redact_credentials(&e.to_string())))?;
        let idx = Self {
            client,
            base_url: url.trim_end_matches('/').to_string(),
            index: index.to_string(),
            dim,
        };
        idx.ensure_index().await?;
        Ok(idx)
    }

    /// Drop the backing index (useful for test cleanup or full reset).
    pub async fn delete_index(&self) -> Result<()> {
        let resp = self.delete(&format!("/{}", self.index)).await?;
        // 200 (deleted) or 404 (already gone) are both fine.
        if !resp.status().is_success() && resp.status().as_u16() != 404 {
            return Err(self.status_err(resp).await);
        }
        Ok(())
    }

    /// Create the index with a dense_vector mapping if it does not exist.
    async fn ensure_index(&self) -> Result<()> {
        // HEAD /{index} → 200 if exists, 404 otherwise.
        let head = self
            .client
            .head(format!("{}/{}", self.base_url, self.index))
            .send()
            .await
            .map_err(|e| KernelError::Embedding(redact_credentials(&e.to_string())))?;
        if head.status().as_u16() == 200 {
            return Ok(());
        }
        // 404 → create. Any other status is an error.
        if head.status().as_u16() != 404 {
            return Err(self.status_err(head).await);
        }
        let body = serde_json::json!({
            "mappings": {
                "properties": {
                    "vector": {
                        "type": "dense_vector",
                        "dims": self.dim,
                        "index": true,
                        "similarity": "cosine"
                    },
                    "ext_id": { "type": "long" }
                }
            }
        });
        let resp = self.put(&format!("/{}", self.index), body).await?;
        if !resp.status().is_success() {
            return Err(self.status_err(resp).await);
        }
        Ok(())
    }

    /// Parse a numeric `u64` id from an ES `_id`. Pure — unit-testable offline.
    /// Non-numeric ids are dropped, matching `QdrantVectorIndex`.
    fn parse_id(_id: &str) -> Option<u64> {
        _id.parse::<u64>().ok()
    }

    // --- private HTTP helpers (all errors redacted) -----------------------

    async fn put(&self, path: &str, body: serde_json::Value) -> Result<reqwest::Response> {
        self.client
            .put(format!("{}{}", self.base_url, path))
            .json(&body)
            .send()
            .await
            .map_err(|e| KernelError::Embedding(redact_credentials(&e.to_string())))
    }

    async fn delete(&self, path: &str) -> Result<reqwest::Response> {
        self.client
            .delete(format!("{}{}", self.base_url, path))
            .send()
            .await
            .map_err(|e| KernelError::Embedding(redact_credentials(&e.to_string())))
    }

    async fn ndjson(&self, path: &str, body: String) -> Result<reqwest::Response> {
        self.client
            .post(format!("{}{}", self.base_url, path))
            .header(CONTENT_TYPE, "application/x-ndjson")
            .body(body)
            .send()
            .await
            .map_err(|e| KernelError::Embedding(redact_credentials(&e.to_string())))
    }

    async fn status_err(&self, resp: reqwest::Response) -> KernelError {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        // Redact FIRST (strip any embedded credentials), then cap the body so a
        // huge ES error response cannot bloat logs/errors. The order matters:
        // a credential past the cap is already masked before truncation runs.
        let body = truncate_error_body(&redact_credentials(&body));
        KernelError::Embedding(format!(
            "elasticsearch returned status {status} for index `{}` [url redacted]: {}",
            self.index, body
        ))
    }

    /// POST one already-built NDJSON bulk `body` and validate the response.
    /// `op` names the operation (`upsert`/`delete`) for error messages.
    async fn submit_bulk(&self, body: String, op: &str) -> Result<()> {
        let resp = self.ndjson("/_bulk?refresh=wait_for", body).await?;
        if !resp.status().is_success() {
            return Err(self.status_err(resp).await);
        }
        let parsed: BulkResponse = decode(resp).await?;
        if parsed.errors {
            return Err(KernelError::Embedding(format!(
                "elasticsearch bulk {op} reported per-item errors [url redacted]: {}",
                first_failing_bulk_item(&parsed.items)
            )));
        }
        Ok(())
    }
}

/// Max documents per `_bulk` request. A single request must stay under ES's
/// `http.max_content_length` (default 100 MB); at 500 docs even 1024-dim `f32`
/// vectors keep each batch a few MB, so large `add`/`remove` calls are chunked
/// instead of built into one unbounded body.
const BULK_CHUNK_SIZE: usize = 500;

#[async_trait::async_trait]
impl AsyncVectorIndex for ElasticsearchVectorIndex {
    async fn add(&self, vectors: &[Vec<f32>], ids: &[u64]) -> Result<()> {
        if vectors.len() != ids.len() {
            return Err(KernelError::Embedding(format!(
                "vectors.len() ({}) must equal ids.len() ({})",
                vectors.len(),
                ids.len()
            )));
        }
        if vectors.is_empty() {
            return Ok(());
        }
        // Chunk into bounded `_bulk` requests so a large batch can't build one
        // unbounded body that exceeds ES's `http.max_content_length` (413) or
        // spikes memory. `refresh=wait_for` on each batch makes the writes
        // immediately searchable, matching Qdrant's `wait(true)` so the
        // conformance test's subsequent searches see the upsert without a race.
        for (vchunk, idchunk) in vectors
            .chunks(BULK_CHUNK_SIZE)
            .zip(ids.chunks(BULK_CHUNK_SIZE))
        {
            let mut body = String::new();
            for (v, &id) in vchunk.iter().zip(idchunk.iter()) {
                body.push_str(
                    &serde_json::to_string(&serde_json::json!({
                        "index": { "_index": &self.index, "_id": id.to_string() }
                    }))
                    .map_err(|e| KernelError::Embedding(format!("bulk encode: {e}")))?,
                );
                body.push('\n');
                body.push_str(
                    &serde_json::to_string(&serde_json::json!({
                        "ext_id": id,
                        "vector": v
                    }))
                    .map_err(|e| KernelError::Embedding(format!("bulk encode: {e}")))?,
                );
                body.push('\n');
            }
            self.submit_bulk(body, "upsert").await?;
        }
        Ok(())
    }

    async fn remove(&self, ids: &[u64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        for idchunk in ids.chunks(BULK_CHUNK_SIZE) {
            let mut body = String::new();
            for &id in idchunk {
                body.push_str(
                    &serde_json::to_string(&serde_json::json!({
                        "delete": { "_index": &self.index, "_id": id.to_string() }
                    }))
                    .map_err(|e| KernelError::Embedding(format!("bulk encode: {e}")))?,
                );
                body.push('\n');
            }
            // Per-item `not_found` for deletes does NOT set `errors: true`, so
            // this mirrors Qdrant's "silently ignore missing ids" contract.
            self.submit_bulk(body, "delete").await?;
        }
        Ok(())
    }

    /// kNN search over the `dense_vector` field. Each `SearchHit.score` is the
    /// ES knn `_score` (`(1 + cosine) / 2`), which is not comparable across
    /// backends — see [Score semantics](self#score-semantics).
    async fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchHit>> {
        let num_candidates = knn_num_candidates(k);
        let body = serde_json::json!({
            "knn": {
                "field": "vector",
                "query_vector": query,
                "k": k,
                "num_candidates": num_candidates
            },
            "_source": false,
            "size": k
        });
        let resp = self
            .client
            .post(format!("{}/{}/_search", self.base_url, self.index))
            .json(&body)
            .send()
            .await
            .map_err(|e| KernelError::Embedding(redact_credentials(&e.to_string())))?;
        if !resp.status().is_success() {
            return Err(self.status_err(resp).await);
        }
        let parsed: SearchResponse = decode(resp).await?;
        Ok(parsed
            .hits
            .hits
            .into_iter()
            .filter_map(|h| {
                Self::parse_id(&h._id).map(|id| SearchHit {
                    id,
                    score: h._score,
                })
            })
            .collect())
    }

    async fn search_filtered(
        &self,
        query: &[f32],
        k: usize,
        allowlist: &[u64],
    ) -> Result<Vec<SearchHit>> {
        // An empty allowlist excludes every document (no candidates) → empty,
        // with NO fallback to an unfiltered search. Mirrors
        // `QdrantVectorIndex::search_filtered` exactly.
        if allowlist.is_empty() {
            return Ok(vec![]);
        }
        let num_candidates = knn_num_candidates(k);
        let allowlist: Vec<u64> = allowlist.to_vec();
        let body = serde_json::json!({
            "knn": {
                "field": "vector",
                "query_vector": query,
                "k": k,
                "num_candidates": num_candidates,
                "filter": [{ "terms": { "ext_id": allowlist } }]
            },
            "_source": false,
            "size": k
        });
        let resp = self
            .client
            .post(format!("{}/{}/_search", self.base_url, self.index))
            .json(&body)
            .send()
            .await
            .map_err(|e| KernelError::Embedding(redact_credentials(&e.to_string())))?;
        if !resp.status().is_success() {
            return Err(self.status_err(resp).await);
        }
        let parsed: SearchResponse = decode(resp).await?;
        Ok(parsed
            .hits
            .hits
            .into_iter()
            .filter_map(|h| {
                Self::parse_id(&h._id).map(|id| SearchHit {
                    id,
                    score: h._score,
                })
            })
            .collect())
    }

    async fn len(&self) -> Result<usize> {
        let resp = self
            .client
            .post(format!("{}/{}/_count", self.base_url, self.index))
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| KernelError::Embedding(redact_credentials(&e.to_string())))?;
        if !resp.status().is_success() {
            return Err(self.status_err(resp).await);
        }
        let parsed: CountResponse = decode(resp).await?;
        Ok(parsed.count as usize)
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

/// Strip `user:pass@` userinfo from any URL embedded in `s`.
///
/// Elasticsearch connection strings frequently embed basic-auth credentials
/// (`https://user:pass@host`). Every error this module produces is routed
/// through this function so credentials are never leaked in error messages or
/// logs. Pure — unit-testable offline. UTF-8 safe (operates on `&str` slices,
/// which are always char boundaries; the delimiters scanned are all ASCII).
///
/// Userinfo is everything before the **last** `@` within the URL authority
/// (matching the WHATWG URL spec), so a password that itself contains `@`
/// (`https://u:p@ss@host`) is fully redacted rather than leaking the tail.
pub(crate) fn redact_credentials(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    loop {
        match rest.find("://") {
            None => {
                out.push_str(rest);
                break;
            }
            Some(idx) => {
                // Copy the scheme and "://" verbatim.
                out.push_str(&rest[..idx + 3]);
                let after = &rest[idx + 3..];
                // The authority runs until the first path/query/fragment
                // delimiter. Within it, userinfo is everything before the LAST
                // '@' (so a password containing '@' is redacted whole).
                let auth_end = after.find(['/', '?', '#']).unwrap_or(after.len());
                let auth = &after[..auth_end];
                if let Some(at) = auth.rfind('@') {
                    out.push_str("<redacted>@");
                    out.push_str(&auth[at + 1..]);
                } else {
                    out.push_str(auth);
                }
                rest = &after[auth_end..];
            }
        }
    }
    out
}

/// Upper bound on the knn `num_candidates` Elasticsearch evaluates per shard.
///
/// ES scales `num_candidates` with `k` (a common heuristic is `10 * k`), but a
/// large `k` (e.g. 100) would otherwise ask ES to score 1 000 candidates —
/// pathological load for a foundation-library default. Capping at
/// [`MAX_KNN_CANDIDATES`] keeps the candidate pool bounded while staying well
/// above any realistic `k`. Pure — unit-testable offline.
const MAX_KNN_CANDIDATES: usize = 1_000;

/// Compute the knn `num_candidates` for a query returning the top `k` hits.
///
/// Returns `max(k, min(10 * k, MAX_KNN_CANDIDATES))`. ES requires
/// `num_candidates >= k` (it cannot return `k` neighbors from fewer than `k`
/// candidates), so the floor on `k` guarantees the invariant holds even when
/// the cap would otherwise clamp below it. `k == 0` does not underflow
/// (`k.max(1)`). Pure — unit-testable offline.
fn knn_num_candidates(k: usize) -> usize {
    let base = k.max(1).saturating_mul(10);
    base.min(MAX_KNN_CANDIDATES).max(k)
}

/// Maximum number of characters of an ES error response body to embed in a
/// [`KernelError`]. A huge ES error body (e.g. a verbose
/// `mapper_parsing_exception`) could otherwise bloat logs and error chains;
/// the cap keeps the diagnostic surface bounded while the `... [truncated]`
/// marker signals that more is available on the ES side.
const ERROR_BODY_MAX_CHARS: usize = 1024;

/// Cap `s` to [`ERROR_BODY_MAX_CHARS`] characters, appending a `... [truncated]`
/// marker when it is longer.
///
/// Truncation happens at a UTF-8 character boundary (never mid-codepoint), so
/// the function is safe on multibyte text. Intended to be applied AFTER
/// [`redact_credentials`], so a credential past the cap is already masked.
/// Pure — unit-testable offline.
fn truncate_error_body(s: &str) -> String {
    if s.chars().count() <= ERROR_BODY_MAX_CHARS {
        return s.to_string();
    }
    // `char_indices().nth(N)` lands on the byte offset of the (N+1)-th char —
    // a guaranteed char boundary, so slicing is UTF-8 safe.
    let cut = s
        .char_indices()
        .nth(ERROR_BODY_MAX_CHARS)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    format!("{}... [truncated]", &s[..cut])
}

/// Validate an Elasticsearch index name against the 8.x naming rules.
///
/// ES rejects index names that are empty, exceed 255 UTF-8 bytes, contain
/// uppercase letters or bytes outside `[a-z0-9_.-]`, or begin with `_`, `-`,
/// or `+` (`.` is reserved for hidden/system indices, so it is allowed but
/// discouraged). Validating up front turns ES's opaque
/// `invalid_index_name_exception` 400 into a clear `Err` before any network
/// call. Pure — unit-testable offline.
fn validate_index_name(index: &str) -> Result<()> {
    if index.is_empty() {
        return Err(KernelError::Embedding(
            "elasticsearch index name must not be empty".into(),
        ));
    }
    // ES hard-rejects the literal names "." and ".." (reserved), distinct from
    // the leading-dot allowance for hidden/system indices like `.myindex`.
    if index == "." || index == ".." {
        return Err(KernelError::Embedding(format!(
            "elasticsearch index name must not be `.` or `..` (reserved): `{}`",
            index
        )));
    }
    if index.len() > 255 {
        return Err(KernelError::Embedding(format!(
            "elasticsearch index name exceeds 255 bytes ({} bytes)",
            index.len()
        )));
    }
    match index.as_bytes()[0] {
        b'_' | b'-' | b'+' => {
            return Err(KernelError::Embedding(format!(
                "elasticsearch index name must not start with `_`, `-`, or `+`: `{}`",
                index
            )));
        }
        _ => {}
    }
    if let Some(bad) = index.bytes().find(|&c| {
        !(c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-' | b'.'))
    }) {
        return Err(KernelError::Embedding(format!(
            "elasticsearch index name contains an illegal byte 0x{bad:02x} (`{}`): \
             only lowercase a-z, 0-9, `_`, `-`, `.` are allowed",
            index
        )));
    }
    Ok(())
}

/// Render the first failing item of an ES `_bulk` response, redacted.
///
/// Each bulk item is `{ "<action>": { "_id": …, "status": N, "error": {…} } }`
/// where `<action>` is `index`/`create`/`update`/`delete`. An item counts as
/// failing when `status >= 400` or it carries an `error` object. Parsed as
/// opaque JSON so this is robust to ES version-specific item shape.
fn first_failing_bulk_item(items: &[serde_json::Value]) -> String {
    for item in items {
        if let Some(detail) = item.as_object().and_then(|o| o.values().next()) {
            let status = detail.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
            let has_error = detail.get("error").is_some();
            if status >= 400 || has_error {
                return redact_credentials(&item.to_string());
            }
        }
    }
    "(no failing item found)".into()
}

/// Decode a JSON response body into `T`, redacting any URL in errors.
async fn decode<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
    resp.json::<T>()
        .await
        .map_err(|e| KernelError::Embedding(redact_credentials(&e.to_string())))
}

#[derive(Deserialize)]
struct SearchResponse {
    hits: SearchHits,
}

#[derive(Deserialize)]
struct SearchHits {
    hits: Vec<SearchInnerHit>,
}

#[derive(Deserialize)]
struct SearchInnerHit {
    _id: String,
    _score: f32,
}

#[derive(Deserialize)]
struct CountResponse {
    count: u64,
}

#[derive(Deserialize)]
struct BulkResponse {
    errors: bool,
    #[serde(default)]
    items: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::AsyncVectorIndex;

    const DIM: usize = 4;

    fn unique_index() -> String {
        format!("llm_kernel_test_{}", std::process::id())
    }

    /// Build an index handle without connecting (no `ensure_index`). Lets the
    /// pure, pre-HTTP code paths (empty-allowlist short-circuit, redaction,
    /// id parsing) be unit-tested offline without an ES server.
    fn offline_index(base_url: &str, dim: usize) -> ElasticsearchVectorIndex {
        ElasticsearchVectorIndex {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            index: "llm_kernel_test_offline".to_string(),
            dim,
        }
    }

    #[test]
    fn parse_id_accepts_numeric_and_drops_rest() {
        assert_eq!(ElasticsearchVectorIndex::parse_id("42"), Some(42));
        assert_eq!(ElasticsearchVectorIndex::parse_id("0"), Some(0));
        assert_eq!(
            ElasticsearchVectorIndex::parse_id("18446744073709551615"),
            Some(u64::MAX)
        );
        // Non-numeric ids (ES can return string ids) are dropped.
        assert_eq!(ElasticsearchVectorIndex::parse_id("abc"), None);
        assert_eq!(ElasticsearchVectorIndex::parse_id(""), None);
        assert_eq!(ElasticsearchVectorIndex::parse_id("1.5"), None);
    }

    #[test]
    fn redact_credentials_strips_userinfo() {
        let cases = [
            ("http://u:pw@host:9200", "http://<redacted>@host:9200"),
            (
                "https://elastic:secret@es.local/x",
                "https://<redacted>@es.local/x",
            ),
            ("http://localhost:9200", "http://localhost:9200"),
            ("http://user@host", "http://<redacted>@host"),
            // Password containing '@' — userinfo spans to the LAST '@', so the
            // tail after the first '@' does not leak.
            ("https://u:p@ss@host:9200", "https://<redacted>@host:9200"),
            ("no url here", "no url here"),
            // Multibyte UTF-8 must survive intact (regression for the
            // byte-wise redaction that would corrupt non-ASCII).
            (
                "index 中文 — http://u:pw@h:9200",
                "index 中文 — http://<redacted>@h:9200",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(redact_credentials(input), expected, "input = {input:?}");
        }
        // The password never survives redaction.
        assert!(!redact_credentials("https://u:secret@host").contains("secret"));
        // An '@' embedded in the password must not leak the tail.
        let leaked = redact_credentials("https://u:p@ss@host:9200");
        assert!(!leaked.contains("p@ss"), "password tail leaked: {leaked}");
        assert!(!leaked.contains("ss@"), "password tail leaked: {leaked}");
    }

    #[test]
    fn validate_index_name_accepts_and_rejects() {
        // Valid.
        for ok in ["docs", "docs_v2", "my-index", "idx.2026", "a", "a.b-c_d"] {
            assert!(
                validate_index_name(ok).is_ok(),
                "{ok:?} should be a valid index name"
            );
        }
        // Rejected.
        for name in [
            "",            // empty
            "Docs",        // uppercase
            "with space",  // space
            "comma,idx",   // comma
            "_underscore", // leading _
            "-dash",       // leading -
            "+plus",       // leading +
            "bad/slash",   // slash
            "한글",        // non-ASCII
            ".",           // reserved literal
            "..",          // reserved literal
        ] {
            assert!(
                validate_index_name(name).is_err(),
                "{name:?} should be rejected"
            );
        }
        // 255-byte cap.
        assert!(validate_index_name(&"a".repeat(255)).is_ok());
        assert!(validate_index_name(&"a".repeat(256)).is_err());
    }

    /// `knn_num_candidates` scales 10x with `k`, clamps at the cap, and never
    /// drops below `k` (the ES `num_candidates >= k` invariant). Pure.
    #[test]
    fn knn_num_candidates_scales_caps_and_floors() {
        // Small k → 10*k (below the cap).
        assert_eq!(knn_num_candidates(1), 10);
        assert_eq!(knn_num_candidates(5), 50);
        assert_eq!(knn_num_candidates(50), 500);
        // Exactly at the cap boundary (10 * 100 = 1000 == cap).
        assert_eq!(knn_num_candidates(100), MAX_KNN_CANDIDATES);
        // Above the cap: clamped to the cap, but still >= k.
        assert_eq!(knn_num_candidates(200), MAX_KNN_CANDIDATES);
        assert!(knn_num_candidates(200) >= 200);
        // k == 0 must not underflow and still satisfy >= k.
        assert_eq!(knn_num_candidates(0), 10);
    }

    /// A short body is returned unchanged (no marker added). Pure.
    #[test]
    fn truncate_error_body_leaves_short_body_unchanged() {
        assert_eq!(truncate_error_body(""), "");
        assert_eq!(truncate_error_body("short error"), "short error");
        // Exactly at the cap: no truncation, no marker.
        let at_cap: String = "a".repeat(ERROR_BODY_MAX_CHARS);
        let out = truncate_error_body(&at_cap);
        assert_eq!(out.chars().count(), ERROR_BODY_MAX_CHARS);
        assert!(!out.contains("[truncated]"));
    }

    /// A body longer than the cap is cut at a char boundary and gets the
    /// truncation marker. Multibyte text must not panic or split a codepoint.
    /// Pure.
    #[test]
    fn truncate_error_body_caps_huge_body_with_marker() {
        // ASCII over-cap: cut to exactly ERROR_BODY_MAX_CHARS chars + marker.
        let huge: String = "a".repeat(ERROR_BODY_MAX_CHARS + 500);
        let out = truncate_error_body(&huge);
        assert!(out.ends_with("... [truncated]"));
        let kept = out.strip_suffix("... [truncated]").unwrap();
        assert_eq!(kept.chars().count(), ERROR_BODY_MAX_CHARS);

        // Multibyte (CJK) over-cap: truncation must land on a char boundary.
        // Build a body whose char count exceeds the cap but whose byte length
        // makes mid-codepoint slicing dangerous if done byte-wise.
        let cjk: String = "중".repeat(ERROR_BODY_MAX_CHARS + 10);
        let out_cjk = truncate_error_body(&cjk);
        // No panic == the slice was char-boundary safe (else this would have
        // panicked at runtime on the slice). Marker present.
        assert!(out_cjk.contains("[truncated]"));
        // The kept portion (before marker) is valid UTF-8 by construction; the
        // whole output is a String so it already is. Just assert the marker.
    }

    /// A credential past the cap is still redacted: `redact_credentials` runs
    /// BEFORE `truncate_error_body`, so the masked form survives truncation.
    /// Pure — simulates the `status_err` redact→truncate order.
    #[test]
    fn truncate_error_body_keeps_credentials_redacted() {
        // A body shorter than the cap but with an embedded credential URL:
        // redaction applies, truncation is a no-op, credential is gone.
        let with_cred = "error: see https://u:super-secret@host/idx for details";
        let out = truncate_error_body(&redact_credentials(with_cred));
        assert!(!out.contains("super-secret"), "credential leaked: {out}");
        assert!(out.contains("<redacted>"));

        // A body LONGER than the cap with the credential URL near the END
        // (past the cut point). redact ran first, so even though truncation
        // drops the tail, the credential was already masked before the cut —
        // and the masked prefix is what survives. Either way the secret never
        // appears in the output.
        let padding: String = "x".repeat(ERROR_BODY_MAX_CHARS + 50);
        let long_cred = format!("{padding} then https://u:p@ss@host:9200");
        let redacted = redact_credentials(&long_cred);
        let out2 = truncate_error_body(&redacted);
        assert!(
            !out2.contains("p@ss") && !out2.contains("super-secret"),
            "credential tail leaked: {out2}"
        );
    }

    /// The bulk-error detail helper picks the first failing item (status >= 400
    /// OR carrying an `error` object), redacts any URL embedded in the item, and
    /// falls back when no item qualifies. Pure — exercised offline.
    #[test]
    fn first_failing_bulk_item_picks_failing_and_redacts() {
        // First failing item (status 400 + error) is surfaced.
        let items = vec![
            serde_json::json!({ "index": { "_id": "1", "status": 200 } }),
            serde_json::json!({
                "index": { "_id": "2", "status": 400, "error": { "type": "mapper", "reason": "bad" } }
            }),
        ];
        let s = first_failing_bulk_item(&items);
        assert!(
            s.contains("\"_id\":\"2\""),
            "should name the failing item: {s}"
        );
        assert!(s.contains("400"));
        // error-only failure (no status field) is still detected.
        let err_only = vec![serde_json::json!({
            "delete": { "_id": "9", "error": { "type": "x", "reason": "y" } }
        })];
        assert!(first_failing_bulk_item(&err_only).contains("\"_id\":\"9\""));
        // A credentialed URL embedded in the item JSON is redacted.
        let with_url = vec![serde_json::json!({
            "index": { "_id": "3", "status": 500, "error": { "reason": "see https://u:secret@host" } }
        })];
        let leaked = first_failing_bulk_item(&with_url);
        assert!(!leaked.contains("secret"), "credential leaked: {leaked}");
        assert!(leaked.contains("<redacted>"));
        // No qualifying item → fallback string.
        let none = vec![serde_json::json!({ "index": { "_id": "1", "status": 200 } })];
        assert_eq!(first_failing_bulk_item(&none), "(no failing item found)");
    }

    /// AC3: an error message derived from a credentialed URL must not contain
    /// the password substring. Simulates the redaction applied to every error
    /// this module produces, without needing a live connection.
    #[test]
    fn credentialed_url_error_redacts_password() {
        let credentialed = "https://elastic:super-secret-pw@es.internal:9200/idx";
        // The way the module builds error strings: redact(reqwest-like text).
        let raw = format!("error sending request for url ({credentialed}): connection refused");
        let redacted = redact_credentials(&raw);
        assert!(
            !redacted.contains("super-secret-pw"),
            "password leaked in redacted error: {redacted}"
        );
        assert!(redacted.contains("<redacted>"));
    }

    /// AC3: empty allowlist short-circuits to an empty result BEFORE any HTTP
    /// is issued. No server is contacted (the offline handle points nowhere).
    #[tokio::test]
    async fn empty_allowlist_returns_empty_without_network() {
        let idx = offline_index("http://0.0.0.0:1", DIM);
        // No `ensure_index` was run and no server listens at :1 — this would
        // error if the code attempted a request. It returns empty instead.
        let res = idx.search_filtered(&[1.0, 0.0, 0.0, 0.0], 5, &[]).await;
        assert!(res.is_ok(), "empty allowlist must not error: {res:?}");
        assert!(res.unwrap().is_empty());
    }

    /// Conformance body returning `Result` so failures are errors (not panics),
    /// letting the caller clean up the throwaway index on every exit path.
    async fn run_live_conformance(idx: &ElasticsearchVectorIndex) -> Result<()> {
        if idx.dim() != DIM {
            return Err(KernelError::Embedding("dim mismatch".into()));
        }
        if !idx.is_empty().await? {
            return Err(KernelError::Embedding("not empty at start".into()));
        }
        idx.add(
            &[vec![1.0, 0.0, 0.0, 0.0], vec![0.0, 1.0, 0.0, 0.0]],
            &[1, 2],
        )
        .await?;
        if idx.len().await? != 2 {
            return Err(KernelError::Embedding("len != 2 after add".into()));
        }

        let hits = idx.search(&[1.0, 0.0, 0.0, 0.0], 1).await?;
        if hits.len() != 1 || hits[0].id != 1 {
            return Err(KernelError::Embedding("nearest neighbor != id 1".into()));
        }

        let filtered = idx.search_filtered(&[1.0, 0.0, 0.0, 0.0], 2, &[2]).await?;
        if filtered.len() != 1 || filtered[0].id != 2 {
            return Err(KernelError::Embedding("filtered search != id 2".into()));
        }

        // Re-upsert id 1 with a different vector; count stays 2 (replace).
        idx.add(&[vec![0.9, 0.1, 0.0, 0.0]], &[1]).await?;
        if idx.len().await? != 2 {
            return Err(KernelError::Embedding("len != 2 after re-add".into()));
        }

        idx.remove(&[1]).await?;
        if idx.len().await? != 1 {
            return Err(KernelError::Embedding("len != 1 after remove".into()));
        }
        let after = idx.search(&[1.0, 0.0, 0.0, 0.0], 5).await?;
        if after.iter().any(|h| h.id == 1) {
            return Err(KernelError::Embedding(
                "id 1 still present after remove".into(),
            ));
        }
        Ok(())
    }

    /// Live ES conformance (skips without `LLMKERNEL_ELASTIC_URL`). The
    /// throwaway index is deleted on EVERY exit path (pass or fail) so a
    /// mid-test failure cannot leak it.
    #[tokio::test]
    async fn live_elastic_conformance() {
        let url = match std::env::var("LLMKERNEL_ELASTIC_URL") {
            Ok(u) => u,
            Err(_) => {
                eprintln!("skipped: LLMKERNEL_ELASTIC_URL unset (no live Elasticsearch)");
                return;
            }
        };

        let index = unique_index();
        let idx = match ElasticsearchVectorIndex::new(&url, &index, DIM).await {
            Ok(i) => i,
            Err(e) => panic!("connect + create index: {e:?}"),
        };
        // Run the body, then ALWAYS delete the throwaway index before
        // propagating any failure — panic-safe cleanup.
        let result = run_live_conformance(&idx).await;
        let _ = idx.delete_index().await;
        result.expect("elasticsearch conformance failed");
    }
}
