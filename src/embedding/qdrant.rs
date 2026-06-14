//! Qdrant `AsyncVectorIndex` (`qdrant` feature).
//!
//! `QdrantVectorIndex` implements `AsyncVectorIndex` over the official
//! `qdrant-client`. It is the async counterpart to the in-memory `VectorIndex`
//! — remote vector services are async-only and naturally shared, so they
//! cannot implement the synchronous `VectorIndex`.

use anyhow::{Result, anyhow};
use qdrant_client::qdrant::point_id::PointIdOptions;
use qdrant_client::qdrant::{
    Condition, CountPointsBuilder, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter,
    PointStruct, PointsIdsList, QueryPointsBuilder, ScoredPoint, UpsertPointsBuilder,
    VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant};

use super::{AsyncVectorIndex, SearchHit};

/// Async vector index backed by a Qdrant collection.
///
/// The collection is created on construction (Cosine distance) if it does not
/// already exist. All operations are async over the official `qdrant-client`.
pub struct QdrantVectorIndex {
    client: Qdrant,
    collection: String,
    dim: usize,
}

impl QdrantVectorIndex {
    /// Connect to `url` (e.g. `http://localhost:6334`) and ensure `collection`
    /// exists with a Cosine-distance vector config of `dim` dimensions.
    pub async fn new(url: &str, collection: &str, dim: usize) -> Result<Self> {
        let client = Qdrant::from_url(url).build()?;
        let idx = Self {
            client,
            collection: collection.to_string(),
            dim,
        };
        idx.ensure_collection().await?;
        Ok(idx)
    }

    /// Create the collection if it does not already exist.
    async fn ensure_collection(&self) -> Result<()> {
        if !self.client.collection_exists(&self.collection).await? {
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(&self.collection).vectors_config(
                        VectorParamsBuilder::new(self.dim as u64, Distance::Cosine),
                    ),
                )
                .await?;
        }
        Ok(())
    }

    /// Drop the collection (useful for test cleanup or full reset).
    pub async fn delete_collection(&self) -> Result<()> {
        self.client.delete_collection(&self.collection).await?;
        Ok(())
    }

    /// Extract a `u64` `SearchHit` from a Qdrant `ScoredPoint`.
    ///
    /// Points with non-numeric IDs (UUIDs) are dropped — this index keys on
    /// `u64` external IDs, matching `super::VectorIndex`.
    fn scored_to_hit(point: &ScoredPoint) -> Option<SearchHit> {
        let id = point
            .id
            .as_ref()
            .and_then(|pid| match &pid.point_id_options {
                Some(PointIdOptions::Num(n)) => Some(*n),
                _ => None,
            })?;
        Some(SearchHit {
            id,
            score: point.score,
        })
    }
}

#[async_trait::async_trait]
impl AsyncVectorIndex for QdrantVectorIndex {
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
        let payload = Payload::try_from(serde_json::json!({}))
            .map_err(|e| anyhow!("invalid empty payload: {e}"))?;
        let points: Vec<PointStruct> = vectors
            .iter()
            .zip(ids.iter())
            .map(|(v, &id)| PointStruct::new(id, v.clone(), payload.clone()))
            .collect();
        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection, points).wait(true))
            .await?;
        Ok(())
    }

    async fn remove(&self, ids: &[u64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let id_list = PointsIdsList {
            ids: ids.iter().map(|&id| id.into()).collect(),
        };
        self.client
            .delete_points(
                DeletePointsBuilder::new(&self.collection)
                    .points(id_list)
                    .wait(true),
            )
            .await?;
        Ok(())
    }

    async fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchHit>> {
        let res = self
            .client
            .query(
                QueryPointsBuilder::new(&self.collection)
                    .query(query.to_vec())
                    .limit(k as u64)
                    .with_payload(false),
            )
            .await?;
        Ok(res.result.iter().filter_map(Self::scored_to_hit).collect())
    }

    async fn search_filtered(
        &self,
        query: &[f32],
        k: usize,
        allowlist: &[u64],
    ) -> Result<Vec<SearchHit>> {
        let filter = Filter::must([Condition::has_id(allowlist.iter().copied())]);
        let res = self
            .client
            .query(
                QueryPointsBuilder::new(&self.collection)
                    .query(query.to_vec())
                    .limit(k as u64)
                    .with_payload(false)
                    .filter(filter),
            )
            .await?;
        Ok(res.result.iter().filter_map(Self::scored_to_hit).collect())
    }

    async fn len(&self) -> Result<usize> {
        let res = self
            .client
            .count(CountPointsBuilder::new(&self.collection).exact(true))
            .await?;
        Ok(res.result.map(|c| c.count as usize).unwrap_or(0))
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::AsyncVectorIndex;

    const DIM: usize = 4;

    fn unique_collection() -> String {
        format!("llm_kernel_test_{}", std::process::id())
    }

    /// Live Qdrant conformance (skips without `LLMKERNEL_QDRANT_URL`).
    #[tokio::test]
    async fn live_qdrant_conformance() {
        let url = match std::env::var("LLMKERNEL_QDRANT_URL") {
            Ok(u) => u,
            Err(_) => {
                eprintln!("skipped: LLMKERNEL_QDRANT_URL unset (no live Qdrant)");
                return;
            }
        };

        let coll = unique_collection();
        let idx = QdrantVectorIndex::new(&url, &coll, DIM)
            .await
            .expect("connect + create collection");
        assert_eq!(idx.dim(), DIM);
        assert!(idx.is_empty().await.unwrap());

        idx.add(
            &[vec![1.0, 0.0, 0.0, 0.0], vec![0.0, 1.0, 0.0, 0.0]],
            &[1, 2],
        )
        .await
        .unwrap();
        assert_eq!(idx.len().await.unwrap(), 2);

        let hits = idx.search(&[1.0, 0.0, 0.0, 0.0], 1).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, 1);

        let filtered = idx
            .search_filtered(&[1.0, 0.0, 0.0, 0.0], 2, &[2])
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, 2);

        idx.add(&[vec![0.9, 0.1, 0.0, 0.0]], &[1]).await.unwrap();
        assert_eq!(idx.len().await.unwrap(), 2);

        idx.remove(&[1]).await.unwrap();
        assert_eq!(idx.len().await.unwrap(), 1);
        let after = idx.search(&[1.0, 0.0, 0.0, 0.0], 5).await.unwrap();
        assert!(after.iter().all(|h| h.id != 1));

        idx.delete_collection().await.unwrap();
    }
}
