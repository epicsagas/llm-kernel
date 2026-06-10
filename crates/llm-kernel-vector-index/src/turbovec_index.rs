//! TurboQuant-backed vector index implementation.

use std::path::Path;

use anyhow::{Result, anyhow, ensure};
use llm_kernel::embedding::{SearchHit, VectorIndex};

/// Compressed vector index backed by TurboQuant.
///
/// Wraps `turbovec::IdMapIndex` with dimension validation and a consistent
/// error-handling layer. Supports online ingest (no training step),
/// filtered search with allowlists, and persistence via `save`/`load`.
pub struct TurbovecIndex {
    inner: turbovec::IdMapIndex,
    dim: usize,
    bit_width: u8,
}

impl std::fmt::Debug for TurbovecIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurbovecIndex")
            .field("dim", &self.dim)
            .field("bit_width", &self.bit_width)
            .field("len", &self.inner.len())
            .finish()
    }
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

    /// Quantization bit width (2 or 4).
    pub fn bit_width(&self) -> u8 {
        self.bit_width
    }

    /// Load a previously saved index from disk.
    ///
    /// This is an inherent method (not on the `VectorIndex` trait) so that the
    /// trait remains fully object-safe. Callers must use the concrete type:
    /// `TurbovecIndex::load(path)`.
    pub fn load(path: &Path) -> Result<Self> {
        let inner = turbovec::IdMapIndex::load(path)
            .map_err(|e| anyhow!("failed to load vector index: {e}"))?;
        let meta_path = path.with_extension("meta.json");
        let meta: IndexMeta = serde_json::from_str(&std::fs::read_to_string(&meta_path)?)?;
        ensure!(
            meta.bit_width == 2 || meta.bit_width == 4,
            "corrupted index meta: bit_width must be 2 or 4, got {}",
            meta.bit_width,
        );
        ensure!(
            meta.dim > 0,
            "corrupted index meta: dim must be positive, got {}",
            meta.dim,
        );

        // Cross-validate: loaded index vs sidecar metadata.
        let inner_dim = inner.dim();
        ensure!(
            inner_dim == 0 || inner_dim == meta.dim,
            "index-meta mismatch: index dim={}, meta dim={}",
            inner_dim,
            meta.dim,
        );
        let inner_bw = inner.bit_width();
        ensure!(
            inner_bw == meta.bit_width as usize,
            "index-meta mismatch: index bit_width={}, meta bit_width={}",
            inner_bw,
            meta.bit_width,
        );


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

    fn validate_dims(&self, vectors: &[Vec<f32>]) -> Result<()> {
        for v in vectors {
            self.validate_dim(v)?;
        }
        Ok(())
    }
}

impl VectorIndex for TurbovecIndex {
    fn add(&mut self, vectors: &[Vec<f32>]) -> Result<()> {
        if vectors.is_empty() {
            return Ok(());
        }
        self.validate_dims(vectors)?;
        let start_id = self.inner.len() as u64;
        let ids: Vec<u64> = (start_id..start_id + vectors.len() as u64).collect();
        // Skip validation — already checked above.
        let flat: Vec<f32> = vectors.iter().flat_map(|v| v.iter().copied()).collect();
        self.inner
            .add_with_ids_2d(&flat, self.dim, &ids)
            .map_err(|e| anyhow!("add failed: {e}"))?;
        Ok(())
    }

    fn add_with_ids(&mut self, vectors: &[Vec<f32>], ids: &[u64]) -> Result<()> {
        ensure!(
            vectors.len() == ids.len(),
            "vectors ({} entries) and ids ({} entries) must have the same length",
            vectors.len(),
            ids.len(),
        );
        self.validate_dims(vectors)?;
        let flat: Vec<f32> = vectors.iter().flat_map(|v| v.iter().copied()).collect();
        self.inner
            .add_with_ids_2d(&flat, self.dim, ids)
            .map_err(|e| anyhow!("add failed: {e}"))?;
        Ok(())
    }

    fn remove(&mut self, ids: &[u64]) -> Result<()> {
        for &id in ids {
            self.inner.remove(id);
        }
        Ok(())
    }

    fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchHit>> {
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

    fn search_filtered(
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

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn save(&self, path: &Path) -> Result<()> {
        // Atomic save: write to temp files, fsync, then rename.
        let tmp_index = path.with_extension("tvim.tmp");
        let tmp_meta = path.with_extension("meta.tmp");

        // Write index to temp file.
        self.inner
            .write(&tmp_index)
            .map_err(|e| anyhow!("failed to write vector index: {e}"))?;

        // Write meta to temp file.
        let meta = IndexMeta {
            dim: self.dim,
            bit_width: self.bit_width,
        };
        let json = serde_json::to_string_pretty(&meta)?;
        std::fs::write(&tmp_meta, &json)?;

        // Fsync temp files to ensure data is on disk.
        if let Ok(f) = std::fs::File::open(&tmp_index) {
            let _ = f.sync_all();
        }
        if let Ok(f) = std::fs::File::open(&tmp_meta) {
            let _ = f.sync_all();
        }

        // Atomic rename — POSIX guarantees rename is atomic.
        std::fs::rename(&tmp_meta, path.with_extension("meta.json"))?;
        std::fs::rename(&tmp_index, path)?;

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
    use llm_kernel::embedding::VectorIndex;
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
        let result = idx.search(&[0.0; 32], 1);
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
    fn load_rejects_corrupted_meta() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("corrupt.tvim");

        // Create a valid index and save it.
        let mut idx = make_index(64, 4);
        idx.add(&[random_vector(64, 1.0)]).unwrap();
        idx.save(&path).unwrap();

        // Corrupt the meta file with invalid bit_width.
        let meta_path = path.with_extension("meta.json");
        let bad_meta = r#"{"dim": 64, "bit_width": 7}"#;
        std::fs::write(&meta_path, bad_meta).unwrap();

        let result = TurbovecIndex::load(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("bit_width"));
    }

    #[test]
    fn load_rejects_zero_dim() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("zero.tvim");

        let mut idx = make_index(64, 4);
        idx.add(&[random_vector(64, 1.0)]).unwrap();
        idx.save(&path).unwrap();

        let meta_path = path.with_extension("meta.json");
        let bad_meta = r#"{"dim": 0, "bit_width": 4}"#;
        std::fs::write(&meta_path, bad_meta).unwrap();

        let result = TurbovecIndex::load(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("dim"));
    }

    #[test]
    fn dim_and_bit_width_accessors() {
        let idx = make_index(128, 2);
        assert_eq!(idx.dim(), 128);
        assert_eq!(idx.bit_width(), 2);
    }

    #[test]
    fn trait_object_compatibility() {
        let mut idx: Box<dyn VectorIndex> = Box::new(make_index(64, 4));
        idx.add(&[random_vector(64, 1.0)]).unwrap();
        assert_eq!(idx.len(), 1);
        assert!(!idx.is_empty());
    }

    #[test]
    fn remove_existing_id() {
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
        assert_eq!(idx.len(), 3);

        idx.remove(&[20u64]).unwrap();
        assert_eq!(idx.len(), 2);

        // Verify removed ID no longer appears in search results.
        let hits = idx.search(&random_vector(64, 2.0), 10).unwrap();
        let ids: Vec<u64> = hits.iter().map(|h| h.id).collect();
        assert!(!ids.contains(&20));
    }

    #[test]
    fn remove_nonexistent_id() {
        let mut idx = make_index(64, 4);
        idx.add_with_ids(&[random_vector(64, 1.0)], &[1u64])
            .unwrap();
        // Removing a non-existent ID should succeed silently.
        idx.remove(&[999u64]).unwrap();
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn remove_empty_ids() {
        let mut idx = make_index(64, 4);
        idx.add(&[random_vector(64, 1.0)]).unwrap();
        idx.remove(&[]).unwrap();
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn remove_via_trait_object() {
        let mut idx: Box<dyn VectorIndex> = Box::new(make_index(64, 4));
        idx.add_with_ids(&[random_vector(64, 1.0)], &[42u64])
            .unwrap();
        idx.remove(&[42u64]).unwrap();
        assert!(idx.is_empty());
    }

    #[test]
    fn load_detects_dim_mismatch() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mismatch.tvim");

        let mut idx = make_index(64, 4);
        idx.add(&[random_vector(64, 1.0)]).unwrap();
        idx.save(&path).unwrap();

        // Tamper with meta to report wrong dim.
        let meta_path = path.with_extension("meta.json");
        let bad_meta = r#"{"dim": 128, "bit_width": 4}"#;
        std::fs::write(&meta_path, bad_meta).unwrap();

        let result = TurbovecIndex::load(&path);
        // Should error because inner.dim() == 64 but meta says 128.
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("mismatch") || msg.contains("dim"),
            "expected mismatch error, got: {msg}"
        );
    }
}
