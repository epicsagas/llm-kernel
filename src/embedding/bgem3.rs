//! BGE-M3 joint (dense + sparse) embedding.
//!
//! One forward pass yields both a dense vector and a learned-lexical sparse
//! vector. That is what makes hybrid retrieval cheap here: no second model and
//! no separate BM25 index — the sparse side already carries the exact-term
//! signal (identifiers, statute numbers, rare jargon) that dense vectors blur.

use std::path::PathBuf;
use std::sync::Mutex;

use fastembed::{Bgem3Embedding, Bgem3InitOptions, Bgem3Model, SparseEmbedding};

use crate::embedding::sparse::SparseVector;
use crate::error::{KernelError, Result};

/// Dense width of BGE-M3.
pub const BGEM3_DENSE_DIM: usize = 1024;

/// Vocabulary size of BGE-M3's XLM-RoBERTa tokenizer — the dimension a
/// pgvector `sparsevec` column must declare for these sparse vectors.
pub const BGEM3_VOCAB_SIZE: usize = 250_002;

/// Texts handed to the model per call by default. Bounded on purpose; see
/// [`Bgem3Provider`]'s memory note.
const DEFAULT_BATCH_CAP: usize = 32;

/// One text's dense + sparse representation from a single BGE-M3 pass.
#[derive(Debug, Clone)]
pub struct JointEmbedding {
    /// Dense vector, [`BGEM3_DENSE_DIM`] wide.
    pub dense: Vec<f32>,
    /// Learned-lexical sparse vector over the model vocabulary.
    pub sparse: SparseVector,
}

/// Convert fastembed's sparse output into a [`SparseVector`], optionally
/// pruning to the `top_k` highest-magnitude weights.
fn sparse_from_fastembed(sp: SparseEmbedding, top_k: Option<usize>) -> Result<SparseVector> {
    let indices = sp
        .indices
        .into_iter()
        .map(|i| {
            u32::try_from(i)
                .map_err(|_| KernelError::Embedding(format!("sparse index {i} exceeds u32")))
        })
        .collect::<Result<Vec<u32>>>()?;
    let sv = SparseVector::new(indices, sp.values).ok_or_else(|| {
        KernelError::Embedding("BGE-M3 sparse indices/values length mismatch".into())
    })?;
    Ok(match top_k {
        Some(k) => sv.prune_top_k(k),
        None => sv,
    })
}

/// BGE-M3 joint embedder — dense + sparse from one pass.
///
/// # Memory
///
/// fastembed's BGE-M3 graph always emits a third, ColBERT (multi-vector)
/// output, and accumulates it for *every* text passed in a single call —
/// roughly `seq_len × 1024 × 4` bytes each, about 2 MB for a 512-token chunk.
/// Handing it a million chunks at once would therefore exhaust memory long
/// before it finished. This provider slices its input into
/// [`with_batch_cap`](Self::with_batch_cap)-sized runs and drops the ColBERT
/// tensors as soon as each run is converted, so bulk indexing stays flat.
///
/// # Model
///
/// fastembed ships BGE-M3 as an int8-quantized ONNX export
/// (`gpahal/bge-m3-onnx-int8`), not full precision.
pub struct Bgem3Provider {
    inner: Mutex<Bgem3Embedding>,
    batch_cap: usize,
    sparse_top_k: Option<usize>,
}

impl Bgem3Provider {
    /// Load BGE-M3, downloading it on first use.
    ///
    /// `cache_dir` overrides the HuggingFace model cache directory.
    pub fn new(cache_dir: Option<PathBuf>) -> Result<Self> {
        Self::build(Bgem3InitOptions::new(Bgem3Model::BGEM3Q), cache_dir)
    }

    /// Load BGE-M3 with an explicit maximum sequence length. Longer inputs are
    /// truncated; BGE-M3 itself supports up to 8192 tokens.
    pub fn with_max_length(max_length: usize, cache_dir: Option<PathBuf>) -> Result<Self> {
        Self::build(
            Bgem3InitOptions::new(Bgem3Model::BGEM3Q).with_max_length(max_length),
            cache_dir,
        )
    }

    fn build(mut options: Bgem3InitOptions, cache_dir: Option<PathBuf>) -> Result<Self> {
        if let Some(dir) = cache_dir {
            options = options.with_cache_dir(dir);
        }
        let model = Bgem3Embedding::try_new(options).map_err(KernelError::embedding)?;
        Ok(Self {
            inner: Mutex::new(model),
            batch_cap: DEFAULT_BATCH_CAP,
            sparse_top_k: None,
        })
    }

    /// Cap how many texts are sent to the model per call.
    ///
    /// This bounds peak memory: the discarded ColBERT output is accumulated per
    /// call, so a larger cap trades memory for slightly fewer invocations. A
    /// cap of 0 is treated as 1.
    pub fn with_batch_cap(mut self, cap: usize) -> Self {
        self.batch_cap = cap.max(1);
        self
    }

    /// Keep only the `k` highest-magnitude weights of each sparse vector.
    ///
    /// Set this when the target store bounds non-zero elements — pgvector
    /// refuses to build an HNSW index over a `sparsevec` beyond its cap. Off by
    /// default, since the limit belongs to the backend, not the model.
    pub fn with_sparse_top_k(mut self, k: usize) -> Self {
        self.sparse_top_k = Some(k);
        self
    }

    /// Dense vector width.
    pub fn dense_dim(&self) -> usize {
        BGEM3_DENSE_DIM
    }

    /// Vocabulary size — the `sparsevec(N)` dimension for the sparse side.
    pub fn vocab_size(&self) -> usize {
        BGEM3_VOCAB_SIZE
    }

    /// Embed `texts`, returning one dense + sparse pair each, in input order.
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<JointEmbedding>> {
        let mut out = Vec::with_capacity(texts.len());
        for run in texts.chunks(self.batch_cap) {
            let batch = {
                let mut model = self
                    .inner
                    .lock()
                    .map_err(|e| KernelError::Embedding(format!("lock: {e}")))?;
                model
                    .embed(run, Some(self.batch_cap))
                    .map_err(KernelError::embedding)?
            };
            if batch.dense.len() != batch.sparse.len() {
                return Err(KernelError::Embedding(format!(
                    "BGE-M3 returned {} dense and {} sparse vectors",
                    batch.dense.len(),
                    batch.sparse.len()
                )));
            }
            // `batch.colbert` is never read and is freed with `batch` here.
            for (dense, sp) in batch.dense.into_iter().zip(batch.sparse) {
                out.push(JointEmbedding {
                    dense,
                    sparse: sparse_from_fastembed(sp, self.sparse_top_k)?,
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fe_sparse(indices: Vec<usize>, values: Vec<f32>) -> SparseEmbedding {
        SparseEmbedding { indices, values }
    }

    #[test]
    fn converts_fastembed_sparse() {
        let sv = sparse_from_fastembed(fe_sparse(vec![7, 1], vec![0.25, 0.75]), None).unwrap();
        // Sorted by index, zeros dropped by SparseVector::new.
        assert_eq!(sv.indices(), &[1, 7]);
        assert_eq!(sv.values(), &[0.75, 0.25]);
    }

    #[test]
    fn prunes_to_top_k_when_requested() {
        let sv =
            sparse_from_fastembed(fe_sparse(vec![1, 2, 3], vec![0.1, 0.9, 0.5]), Some(2)).unwrap();
        assert_eq!(sv.nnz(), 2);
        assert_eq!(sv.indices(), &[2, 3]);
    }

    #[test]
    fn rejects_length_mismatch() {
        let err = sparse_from_fastembed(fe_sparse(vec![1, 2], vec![0.5]), None);
        assert!(err.is_err());
    }

    #[test]
    fn vocab_and_dense_dims_are_bgem3_shapes() {
        assert_eq!(BGEM3_DENSE_DIM, 1024);
        assert_eq!(BGEM3_VOCAB_SIZE, 250_002);
    }
}
