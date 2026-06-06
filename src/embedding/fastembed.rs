//! Local ONNX embedding provider via fastembed-rs.
//!
//! Wraps [`fastembed::TextEmbedding`] behind the [`EmbeddingProvider`] trait.
//! Models are downloaded from HuggingFace on first use and cached locally.
//!
//! ```ignore
//! use llm_kernel::embedding::{EmbeddingModel, FastembedProvider};
//! use llm_kernel::embedding::EmbeddingProvider;
//!
//! let provider = FastembedProvider::new(EmbeddingModel::BGESmallENV15, None)?;
//! let result = provider.embed("hello world")?;
//! assert_eq!(result.vector.len(), 384);
//! ```

use std::path::PathBuf;
use std::sync::Mutex;

use crate::embedding::catalog::EmbeddingModel;
use crate::embedding::types::{EmbeddingProvider, EmbeddingResult};

/// Local ONNX embedding provider backed by fastembed-rs.
///
/// `TextEmbedding::embed()` requires `&mut self`, so the inner instance is
/// protected by a `Mutex`. Thread-safety is guaranteed by the `Send + Sync`
/// bounds on `EmbeddingProvider`.
pub struct FastembedProvider {
    inner: Mutex<fastembed::TextEmbedding>,
    model: EmbeddingModel,
}

impl FastembedProvider {
    /// Create a new provider.
    ///
    /// `cache_dir` overrides the HuggingFace model cache directory.
    /// Pass `None` to use the default cache location.
    pub fn new(model: EmbeddingModel, cache_dir: Option<PathBuf>) -> anyhow::Result<Self> {
        let mut options = fastembed::TextInitOptions::new(model.as_fastembed())
            .with_show_download_progress(false);
        if let Some(dir) = cache_dir {
            options = options.with_cache_dir(dir);
        }
        let te = fastembed::TextEmbedding::try_new(options)?;
        Ok(Self {
            inner: Mutex::new(te),
            model,
        })
    }

    /// Create with DirectML GPU execution on Windows.
    ///
    /// Requires the `embedding-fastembed-directml` feature and Windows OS.
    /// The DirectML runtime DLL must be present on the target system.
    ///
    /// **Initialization cost:** the first call initialises the D3D12 device and
    /// loads the DirectML DLL, which can take hundreds of milliseconds to
    /// several seconds. Create the provider once and reuse it across requests.
    ///
    /// `cache_dir` overrides the HuggingFace model cache directory.
    #[cfg(all(feature = "embedding-fastembed-directml", target_os = "windows"))]
    pub fn new_with_directml(
        model: EmbeddingModel,
        cache_dir: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        use ort::execution_providers::DirectMLExecutionProvider;
        let mut options = fastembed::TextInitOptions::new(model.as_fastembed())
            .with_show_download_progress(false)
            .with_execution_providers(vec![DirectMLExecutionProvider::default().build()]);
        if let Some(dir) = cache_dir {
            options = options.with_cache_dir(dir);
        }
        let te = fastembed::TextEmbedding::try_new(options)?;
        Ok(Self {
            inner: Mutex::new(te),
            model,
        })
    }

    /// Create with a custom maximum sequence length.
    pub fn with_max_length(
        model: EmbeddingModel,
        cache_dir: Option<PathBuf>,
        max_length: usize,
    ) -> anyhow::Result<Self> {
        let mut options = fastembed::TextInitOptions::new(model.as_fastembed())
            .with_show_download_progress(false)
            .with_max_length(max_length);
        if let Some(dir) = cache_dir {
            options = options.with_cache_dir(dir);
        }
        let te = fastembed::TextEmbedding::try_new(options)?;
        Ok(Self {
            inner: Mutex::new(te),
            model,
        })
    }
}

/// Returns up to 64 chars of `text`, appending `…` if truncated.
///
/// Uses character boundaries, so multibyte UTF-8 input never panics.
pub(crate) fn text_preview(text: &str) -> String {
    match text.char_indices().nth(64) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    }
}

impl EmbeddingProvider for FastembedProvider {
    fn dim(&self) -> usize {
        self.model.dimension()
    }

    fn name(&self) -> &str {
        self.model.as_str()
    }

    fn embed(&self, text: &str) -> anyhow::Result<EmbeddingResult> {
        let owned = match self.model.query_prefix() {
            Some(prefix) => format!("{prefix}{text}"),
            None => text.to_string(),
        };
        let mut te = self
            .inner
            .lock()
            .map_err(|e| anyhow::anyhow!("lock: {e}"))?;
        let embeddings = te.embed(vec![owned], None)?;
        let vector = embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty embedding output"))?;

        Ok(EmbeddingResult {
            vector,
            text_preview: text_preview(text),
        })
    }

    fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<EmbeddingResult>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let prefix = self.model.query_prefix();
        let prepared: Vec<String> = texts
            .iter()
            .map(|t| match prefix {
                Some(p) => format!("{p}{t}"),
                None => t.to_string(),
            })
            .collect();

        let mut te = self
            .inner
            .lock()
            .map_err(|e| anyhow::anyhow!("lock: {e}"))?;
        let embeddings = te.embed(prepared, None)?;

        Ok(embeddings
            .into_iter()
            .zip(texts.iter())
            .map(|(vector, &text)| EmbeddingResult {
                vector,
                text_preview: text_preview(text),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_name_matches_model() {
        // Doesn't need a model download — just checks the constructor doesn't
        // change the name mapping.
        for &m in EmbeddingModel::ALL {
            // Verify as_str() round-trips through as_fastembed()
            let fe = m.as_fastembed();
            assert_eq!(format!("{fe:?}"), m.as_str());
        }
    }

    #[test]
    #[ignore = "requires model download"]
    fn embed_single_text() {
        let dir = tempfile::tempdir().unwrap();
        let provider = FastembedProvider::new(
            EmbeddingModel::BGESmallENV15,
            Some(dir.path().to_path_buf()),
        )
        .unwrap();
        let result = provider.embed("hello world").unwrap();
        assert_eq!(result.vector.len(), 384);
        assert!(!result.vector.is_empty());
    }

    #[test]
    #[ignore = "requires model download"]
    fn embed_batch_texts() {
        let dir = tempfile::tempdir().unwrap();
        let provider = FastembedProvider::new(
            EmbeddingModel::BGESmallENV15,
            Some(dir.path().to_path_buf()),
        )
        .unwrap();
        let results = provider
            .embed_batch(&["hello", "world", "foo bar"])
            .unwrap();
        assert_eq!(results.len(), 3);
        for r in &results {
            assert_eq!(r.vector.len(), 384);
        }
    }
}
