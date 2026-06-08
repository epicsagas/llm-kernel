//! Compressed vector indexing via TurboQuant.
//!
//! Wraps the `turbovec` crate behind a clean abstraction for approximate
//! nearest neighbor search with 2-bit or 4-bit quantization (up to 16x
//! memory reduction). SIMD-accelerated on ARM (NEON) and x86 (AVX-512BW / AVX2).
//!
//! ```
//! use llm_kernel::embedding::vector_index::TurbovecIndex;
//!
//! let mut idx = TurbovecIndex::new(128, 4).unwrap();
//! idx.add(&[vec![0.1; 128], vec![0.2; 128]]);
//! let hits = idx.search(&vec![0.15; 128], 5);
//! ```

use std::path::Path;

use anyhow::{Result, anyhow, ensure};

/// A single search hit from vector index lookup.
#[derive(Debug, Clone)]
pub struct SearchHit {
    /// External identifier for the matched vector.
    pub id: u64,
    /// Similarity score (higher = more similar).
    pub score: f32,
}

/// Compressed vector index backed by TurboQuant.
///
/// Wraps `turbovec::IdMapIndex` with validation and a consistent
/// error-handling layer. Supports online ingest (no training step),
/// filtered search with allowlists, and persistence via `save`/`load`.
pub struct TurbovecIndex {
    inner: turbovec::IdMapIndex,
    dim: usize,
    bit_width: u8,
}

impl TurbovecIndex {
    /// Create a new index for vectors of the given dimension.
    ///
    /// `bit_width` must be 2 or 4, controlling the quantization level:
    /// - **2-bit**: 16x compression, lower recall at low k
    /// - **4-bit**: 8x compression, higher recall (recommended default)
    pub fn new(dim: usize, bit_width: u8) -> Result<Self> {
        ensure!(
            bit_width == 2 || bit_width == 4,
            "bit_width must be 2 or 4, got {bit_width}"
        );
        let inner = turbovec::IdMapIndex::new(dim, bit_width as usize)
            .map_err(|e| anyhow!("failed to create index: {e}"))?;
        Ok(Self {
            inner,
            dim,
            bit_width,
        })
    }

    /// Add vectors to the index. IDs are assigned sequentially starting
    /// from the current index length.
    pub fn add(&mut self, vectors: &[Vec<f32>]) -> Result<()> {
        if vectors.is_empty() {
            return Ok(());
        }
        for v in vectors {
            self.validate_dim(v)?;
        }
        let start_id = self.inner.len() as u64;
        let ids: Vec<u64> = (start_id..start_id + vectors.len() as u64).collect();
        self.add_with_ids(vectors, &ids)
    }

    /// Add vectors with explicit external IDs.
    pub fn add_with_ids(&mut self, vectors: &[Vec<f32>], ids: &[u64]) -> Result<()> {
        ensure!(
            vectors.len() == ids.len(),
            "vectors ({} entries) and ids ({} entries) must have the same length",
            vectors.len(),
            ids.len(),
        );
        for v in vectors {
            self.validate_dim(v)?;
        }
        // Flatten vectors into a single buffer — turbovec expects &[f32] with
        // dim*N elements.
        let flat: Vec<f32> = vectors.iter().flat_map(|v| v.iter().copied()).collect();
        self.inner
            .add_with_ids_2d(&flat, self.dim, ids)
            .map_err(|e| anyhow!("add failed: {e}"))?;
        Ok(())
    }

    /// Search for the `k` nearest neighbors of `query`.
    ///
    /// Returns up to `k` results sorted by descending similarity.
    /// Returns an empty vector if the index is empty.
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchHit>> {
        self.validate_dim(query)?;
        if self.inner.is_empty() {
            return Ok(vec![]);
        }
        let (scores, ids) = self.inner.search(query, k);
        Ok(scores
            .into_iter()
            .zip(ids)
            .map(|(score, id)| SearchHit { id, score })
            .collect())
    }

    /// Search restricted to an allowlist of candidate IDs.
    ///
    /// Useful for hybrid retrieval: first narrow candidates via BM25 or
    /// metadata filter, then dense-rerank within that set. Filtering
    /// happens inside the SIMD kernel — no over-fetching.
    ///
    /// Returns up to `min(k, allowlist.len())` results.
    pub fn search_filtered(
        &self,
        query: &[f32],
        k: usize,
        allowlist: &[u64],
    ) -> Result<Vec<SearchHit>> {
        self.validate_dim(query)?;
        if self.inner.is_empty() || allowlist.is_empty() {
            return Ok(vec![]);
        }
        let (scores, ids) = self.inner.search_with_allowlist(query, k, Some(allowlist));
        Ok(scores
            .into_iter()
            .zip(ids)
            .map(|(score, id)| SearchHit { id, score })
            .collect())
    }

    /// Number of vectors currently indexed.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Vector dimensionality this index was created for.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Quantization bit width (2 or 4).
    pub fn bit_width(&self) -> u8 {
        self.bit_width
    }

    /// Persist the index and metadata to disk.
    ///
    /// Creates two files: `{path}` (binary index) and `{path}.meta.json`
    /// (dimension and bit width for correct deserialization).
    pub fn save(&self, path: &Path) -> Result<()> {
        self.inner
            .write(path)
            .map_err(|e| anyhow!("failed to save vector index: {e}"))?;
        let meta = IndexMeta {
            dim: self.dim,
            bit_width: self.bit_width,
        };
        let meta_path = path.with_extension("meta.json");
        let json = serde_json::to_string_pretty(&meta)?;
        std::fs::write(&meta_path, json)?;
        Ok(())
    }

    /// Load a previously saved index from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let inner = turbovec::IdMapIndex::load(path)
            .map_err(|e| anyhow!("failed to load vector index: {e}"))?;
        let meta_path = path.with_extension("meta.json");
        let meta: IndexMeta = serde_json::from_str(&std::fs::read_to_string(&meta_path)?)?;
        Ok(Self {
            inner,
            dim: meta.dim,
            bit_width: meta.bit_width,
        })
    }

    fn validate_dim(&self, v: &[f32]) -> Result<()> {
        ensure!(
            v.len() == self.dim,
            "vector dimension mismatch: expected {}, got {}",
            self.dim,
            v.len(),
        );
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct IndexMeta {
    dim: usize,
    bit_width: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_index(dim: usize, bit_width: u8) -> TurbovecIndex {
        TurbovecIndex::new(dim, bit_width).unwrap()
    }

    fn random_vector(dim: usize, seed: f32) -> Vec<f32> {
        (0..dim).map(|i| (seed + i as f32 * 0.001).sin()).collect()
    }

    #[test]
    fn new_valid_bit_widths() {
        assert!(TurbovecIndex::new(128, 2).is_ok());
        assert!(TurbovecIndex::new(128, 4).is_ok());
    }

    #[test]
    fn new_invalid_bit_width() {
        assert!(TurbovecIndex::new(128, 3).is_err());
        assert!(TurbovecIndex::new(128, 8).is_err());
        assert!(TurbovecIndex::new(128, 1).is_err());
    }

    #[test]
    fn add_and_len() {
        let mut idx = make_index(64, 4);
        assert!(idx.is_empty());
        idx.add(&[random_vector(64, 1.0), random_vector(64, 2.0)])
            .unwrap();
        assert_eq!(idx.len(), 2);
    }

    #[test]
    fn add_empty() {
        let mut idx = make_index(64, 4);
        idx.add(&[]).unwrap();
        assert!(idx.is_empty());
    }

    #[test]
    fn add_with_explicit_ids() {
        let mut idx = make_index(64, 4);
        idx.add_with_ids(&[random_vector(64, 1.0)], &[42u64])
            .unwrap();
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn add_dimension_mismatch() {
        let mut idx = make_index(64, 4);
        let result = idx.add(&[vec![0.0; 32]]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("dimension mismatch")
        );
    }

    #[test]
    fn add_with_ids_length_mismatch() {
        let mut idx = make_index(64, 4);
        let result = idx.add_with_ids(&[random_vector(64, 1.0), random_vector(64, 2.0)], &[1u64]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("same length"));
    }

    #[test]
    fn search_empty_index() {
        let idx = make_index(64, 4);
        let hits = idx.search(&random_vector(64, 1.0), 5).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn search_returns_nearest() {
        let mut idx = make_index(64, 4);
        let target = random_vector(64, 3.0);
        idx.add_with_ids(
            &[
                random_vector(64, 100.0),
                target.clone(),
                random_vector(64, 200.0),
            ],
            &[0u64, 1u64, 2u64],
        )
        .unwrap();

        let hits = idx.search(&target, 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, 1);
    }

    #[test]
    fn search_dimension_mismatch() {
        let mut idx = make_index(64, 4);
        idx.add(&[random_vector(64, 1.0)]).unwrap();
        let result = idx.search(&vec![0.0; 32], 1);
        assert!(result.is_err());
    }

    #[test]
    fn search_filtered_with_allowlist() {
        let mut idx = make_index(64, 4);
        idx.add_with_ids(
            &[
                random_vector(64, 1.0),
                random_vector(64, 2.0),
                random_vector(64, 3.0),
            ],
            &[10u64, 20u64, 30u64],
        )
        .unwrap();

        let hits = idx
            .search_filtered(&random_vector(64, 1.0), 10, &[20u64, 30u64])
            .unwrap();
        let ids: Vec<u64> = hits.iter().map(|h| h.id).collect();
        assert!(ids.contains(&20));
        assert!(ids.contains(&30));
        assert!(!ids.contains(&10));
    }

    #[test]
    fn search_filtered_empty_allowlist() {
        let mut idx = make_index(64, 4);
        idx.add(&[random_vector(64, 1.0)]).unwrap();
        let hits = idx
            .search_filtered(&random_vector(64, 1.0), 5, &[])
            .unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.tvim");

        let mut idx = make_index(64, 4);
        idx.add_with_ids(
            &[random_vector(64, 1.0), random_vector(64, 2.0)],
            &[100u64, 200u64],
        )
        .unwrap();

        idx.save(&path).unwrap();
        let loaded = TurbovecIndex::load(&path).unwrap();

        assert_eq!(loaded.dim(), 64);
        assert_eq!(loaded.bit_width(), 4);
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn dim_and_bit_width_accessors() {
        let idx = make_index(128, 2);
        assert_eq!(idx.dim(), 128);
        assert_eq!(idx.bit_width(), 2);
    }
}
