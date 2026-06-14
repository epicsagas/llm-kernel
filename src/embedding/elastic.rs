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

use anyhow::{Result, anyhow};
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;

use super::{AsyncVectorIndex, SearchHit};

/// Async vector index backed by an Elasticsearch 8.x index.
///
/// The index is created on construction (a `dense_vector` field with cosine
/// similarity) if it does not already exist. All operations are async over a
/// plain [`reqwest::Client`]. Connection-string credentials embedded in `url`
/// (e.g. `https://user:pass@host`) are used for the request but never leaked in
/// error messages — see [`redact_credentials`].
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
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| anyhow!(redact_credentials(&e.to_string())))?;
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
        let resp = self.delete(&format!("/{}", &self.index)).await?;
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
            .head(format!("{}/{}", &self.base_url, &self.index))
            .send()
            .await
            .map_err(|e| anyhow!(redact_credentials(&e.to_string())))?;
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
        let resp = self.put(&format!("/{}", &self.index), body).await?;
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
            .put(format!("{}{}", &self.base_url, path))
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!(redact_credentials(&e.to_string())))
    }

    async fn delete(&self, path: &str) -> Result<reqwest::Response> {
        self.client
            .delete(format!("{}{}", &self.base_url, path))
            .send()
            .await
            .map_err(|e| anyhow!(redact_credentials(&e.to_string())))
    }

    async fn ndjson(&self, path: &str, body: String) -> Result<reqwest::Response> {
        self.client
            .post(format!("{}{}", &self.base_url, path))
            .header(CONTENT_TYPE, "application/x-ndjson")
            .body(body)
            .send()
            .await
            .map_err(|e| anyhow!(redact_credentials(&e.to_string())))
    }

    async fn status_err(&self, resp: reqwest::Response) -> anyhow::Error {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow!(
            "elasticsearch returned status {status} for index `{}` [url redacted]: {}",
            &self.index,
            redact_credentials(&body)
        )
    }
}

#[async_trait::async_trait]
impl AsyncVectorIndex for ElasticsearchVectorIndex {
    async fn add(&self, vectors: &[Vec<f32>], ids: &[u64]) -> Result<()> {
        if vectors.len() != ids.len() {
            return Err(anyhow!(
                "vectors.len() ({}) must equal ids.len() ({})",
                vectors.len(),
                ids.len()
            ));
        }
        if vectors.is_empty() {
            return Ok(());
        }
        let mut body = String::new();
        for (v, &id) in vectors.iter().zip(ids.iter()) {
            body.push_str(
                &serde_json::to_string(&serde_json::json!({
                    "index": { "_index": &self.index, "_id": id.to_string() }
                }))
                .map_err(|e| anyhow!("bulk encode: {e}"))?,
            );
            body.push('\n');
            body.push_str(
                &serde_json::to_string(&serde_json::json!({
                    "ext_id": id,
                    "vector": v
                }))
                .map_err(|e| anyhow!("bulk encode: {e}"))?,
            );
            body.push('\n');
        }
        // `refresh=wait_for` makes the write immediately searchable, matching
        // Qdrant's `wait(true)` so the conformance test's subsequent searches
        // see the upsert without a race.
        let resp = self.ndjson("/_bulk?refresh=wait_for", body).await?;
        if !resp.status().is_success() {
            return Err(self.status_err(resp).await);
        }
        let parsed: BulkResponse = decode(resp).await?;
        if parsed.errors {
            return Err(anyhow!(
                "elasticsearch bulk upsert reported per-item errors [url redacted]"
            ));
        }
        Ok(())
    }

    async fn remove(&self, ids: &[u64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut body = String::new();
        for &id in ids {
            body.push_str(
                &serde_json::to_string(&serde_json::json!({
                    "delete": { "_index": &self.index, "_id": id.to_string() }
                }))
                .map_err(|e| anyhow!("bulk encode: {e}"))?,
            );
            body.push('\n');
        }
        let resp = self.ndjson("/_bulk?refresh=wait_for", body).await?;
        if !resp.status().is_success() {
            return Err(self.status_err(resp).await);
        }
        // Per-item `not_found` for deletes does NOT set `errors: true`, so this
        // mirrors Qdrant's "silently ignore missing ids" contract.
        let parsed: BulkResponse = decode(resp).await?;
        if parsed.errors {
            return Err(anyhow!(
                "elasticsearch bulk delete reported per-item errors [url redacted]"
            ));
        }
        Ok(())
    }

    async fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchHit>> {
        let num_candidates = k.max(1).saturating_mul(10);
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
            .post(format!("{}/{}/_search", &self.base_url, &self.index))
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!(redact_credentials(&e.to_string())))?;
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
        let num_candidates = k.max(1).saturating_mul(10);
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
            .post(format!("{}/{}/_search", &self.base_url, &self.index))
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!(redact_credentials(&e.to_string())))?;
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
            .post(format!("{}/{}/_count", &self.base_url, &self.index))
            .json(&serde_json::json!({ "track_total": true }))
            .send()
            .await
            .map_err(|e| anyhow!(redact_credentials(&e.to_string())))?;
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
pub fn redact_credentials(s: &str) -> String {
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
                // Userinfo ends at the next '@', '/', '?', or '#'.
                let end = after.find(['@', '/', '?', '#']).unwrap_or(after.len());
                let segment = &after[..end];
                if after.as_bytes().get(end) == Some(&b'@') {
                    out.push_str("<redacted>@");
                    rest = &after[end + 1..];
                } else {
                    out.push_str(segment);
                    rest = &after[end..];
                }
            }
        }
    }
    out
}

/// Decode a JSON response body into `T`, redacting any URL in errors.
async fn decode<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
    resp.json::<T>()
        .await
        .map_err(|e| anyhow!(redact_credentials(&e.to_string())))
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
            return Err(anyhow!("dim mismatch"));
        }
        if !idx.is_empty().await? {
            return Err(anyhow!("not empty at start"));
        }
        idx.add(
            &[vec![1.0, 0.0, 0.0, 0.0], vec![0.0, 1.0, 0.0, 0.0]],
            &[1, 2],
        )
        .await?;
        if idx.len().await? != 2 {
            return Err(anyhow!("len != 2 after add"));
        }

        let hits = idx.search(&[1.0, 0.0, 0.0, 0.0], 1).await?;
        if hits.len() != 1 || hits[0].id != 1 {
            return Err(anyhow!("nearest neighbor != id 1"));
        }

        let filtered = idx.search_filtered(&[1.0, 0.0, 0.0, 0.0], 2, &[2]).await?;
        if filtered.len() != 1 || filtered[0].id != 2 {
            return Err(anyhow!("filtered search != id 2"));
        }

        // Re-upsert id 1 with a different vector; count stays 2 (replace).
        idx.add(&[vec![0.9, 0.1, 0.0, 0.0]], &[1]).await?;
        if idx.len().await? != 2 {
            return Err(anyhow!("len != 2 after re-add"));
        }

        idx.remove(&[1]).await?;
        if idx.len().await? != 1 {
            return Err(anyhow!("len != 1 after remove"));
        }
        let after = idx.search(&[1.0, 0.0, 0.0, 0.0], 5).await?;
        if after.iter().any(|h| h.id == 1) {
            return Err(anyhow!("id 1 still present after remove"));
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
