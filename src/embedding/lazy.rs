//! Lazy-loading embedding provider with idle eviction and LRU query cache.
//!
//! Wraps [`FastembedProvider`] behind a state machine that defers model
//! download and ONNX session initialization until the first embed call.
//! After a configurable idle timeout, the ONNX session is dropped to
//! release memory while keeping model weights on disk for fast reload.
//!
//! ```ignore
//! use llm_kernel::embedding::{
//!     EmbeddingModel, LazyFastembedProvider, LazyOpts, EmbeddingProvider,
//! };
//!
//! let provider = LazyFastembedProvider::new(
//!     EmbeddingModel::BGESmallENV15,
//!     "/path/to/cache".into(),
//!     LazyOpts::default(),
//! );
//! // Constructor returns instantly — no download yet
//! let result = provider.embed("hello world")?; // triggers lazy load
//! ```

use std::path::PathBuf;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use indexmap::IndexMap;

use super::catalog::EmbeddingModel;
use super::fastembed::FastembedProvider;
use super::types::text_preview;
use super::types::{EmbeddingProvider, EmbeddingResult};

// ---------------------------------------------------------------------------
// ModelState
// ---------------------------------------------------------------------------

/// Lifecycle state of a lazy-loaded embedding model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelState {
    /// No weights on disk, never attempted.
    NotLoaded,
    /// Background thread downloading + initializing.
    Loading,
    /// Weights on disk, not in memory.
    Cached,
    /// ONNX session active, ready for inference.
    Ready,
    /// Feature disabled by configuration.
    Disabled,
    /// Last attempt failed (preserves error).
    Failed(String),
}

// ---------------------------------------------------------------------------
// LazyOpts
// ---------------------------------------------------------------------------

/// Configuration for [`LazyFastembedProvider`].
#[derive(Debug, Clone)]
pub struct LazyOpts {
    /// Seconds of inactivity before the ONNX session is dropped.
    /// `0` means never unload. Default: 600 (10 minutes).
    pub idle_timeout_secs: u64,
    /// Maximum seconds to wait for model initialisation. Default: 300.
    pub load_timeout_secs: u64,
    /// Maximum number of query embeddings to cache (LRU).
    /// `0` disables caching. Default: 64.
    pub query_cache_capacity: usize,
}

impl Default for LazyOpts {
    fn default() -> Self {
        Self {
            idle_timeout_secs: 600,
            load_timeout_secs: 300,
            query_cache_capacity: 64,
        }
    }
}

// ---------------------------------------------------------------------------
// EmbeddingCache (LRU)
// ---------------------------------------------------------------------------

/// LRU cache for query embedding vectors.
///
/// Zero external dependencies beyond `indexmap`. Prevents re-encoding
/// identical query strings in recurring MCP search workloads.
pub struct EmbeddingCache {
    capacity: usize,
    map: IndexMap<String, Vec<f32>>,
}

impl EmbeddingCache {
    /// Create a new cache with the given capacity. `0` disables caching.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            map: IndexMap::new(),
        }
    }

    /// Look up a cached embedding. A hit moves the entry to the MRU position.
    pub fn get(&mut self, key: &str) -> Option<&Vec<f32>> {
        if let Some(idx) = self.map.get_index_of(key) {
            self.map.move_index(idx, self.map.len() - 1);
            Some(&self.map[key])
        } else {
            None
        }
    }

    /// Insert an embedding. If at capacity, evicts the least-recently-used entry.
    pub fn insert(&mut self, key: String, value: Vec<f32>) {
        if self.capacity == 0 {
            return;
        }
        if let Some(idx) = self.map.get_index_of(&key) {
            self.map.move_index(idx, self.map.len() - 1);
            *self.map.get_mut(&key).unwrap() = value;
        } else {
            if self.map.len() >= self.capacity {
                self.map.shift_remove_index(0);
            }
            self.map.insert(key, value);
        }
    }
}

// ---------------------------------------------------------------------------
// LazyFastembedProvider
// ---------------------------------------------------------------------------

struct Inner {
    model: EmbeddingModel,
    cache_dir: PathBuf,
    state: ModelState,
    provider: Option<FastembedProvider>,
    last_used: Option<Instant>,
}

/// Lazy-loading embedding provider backed by [`FastembedProvider`].
///
/// The constructor is **instant** — it sets initial state to `Cached` if
/// model weights already exist on disk, or `NotLoaded` otherwise. The
/// first call to [`embed`](EmbeddingProvider::embed) triggers model
/// download and ONNX session initialisation.
///
/// Thread-safe: concurrent callers block on a `Condvar` with a
/// configurable timeout. After an idle period, the ONNX session is
/// dropped to release memory while keeping weights on disk for fast
/// reload.
pub struct LazyFastembedProvider {
    inner: Mutex<Inner>,
    cvar: Condvar,
    opts: LazyOpts,
    query_cache: Mutex<EmbeddingCache>,
}

impl LazyFastembedProvider {
    /// Create a new lazy provider.
    ///
    /// Returns instantly — no model download or ONNX initialisation.
    pub fn new(model: EmbeddingModel, cache_dir: PathBuf, opts: LazyOpts) -> Self {
        let initial_state = if is_model_cached(model, &cache_dir) {
            ModelState::Cached
        } else {
            ModelState::NotLoaded
        };
        Self {
            inner: Mutex::new(Inner {
                model,
                cache_dir,
                state: initial_state,
                provider: None,
                last_used: None,
            }),
            cvar: Condvar::new(),
            opts: opts.clone(),
            query_cache: Mutex::new(EmbeddingCache::new(opts.query_cache_capacity)),
        }
    }

    /// Current model lifecycle state.
    pub fn state(&self) -> ModelState {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.state.clone()
    }

    /// Whether the model is loaded and ready for inference.
    ///
    /// Cheaper than [`state`](Self::state) for hot-path polling — avoids
    /// cloning the `Failed(String)` variant.
    pub fn is_ready(&self) -> bool {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        matches!(guard.state, ModelState::Ready)
    }

    /// Block until the model is ready for inference.
    ///
    /// If the model is idle beyond the configured timeout, the ONNX session
    /// is evicted and reloaded from the on-disk cache. If the model is not
    /// yet loaded, this triggers download and initialisation on the calling
    /// thread. Concurrent callers wait on a `Condvar` for up to
    /// `load_timeout_secs`.
    pub fn ensure_model(&self) -> Result<(), String> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        // Idle eviction: drop ONNX session if quiet for too long.
        if matches!(guard.state, ModelState::Ready)
            && let Some(last) = guard.last_used
            && self.opts.idle_timeout_secs > 0
            && last.elapsed().as_secs() > self.opts.idle_timeout_secs
        {
            guard.provider = None;
            guard.state = ModelState::Cached;
            guard.last_used = None;
        }

        match &guard.state {
            ModelState::Ready => Ok(()),
            ModelState::Disabled => Err("model is disabled".into()),
            ModelState::Failed(e) => Err(e.clone()),
            ModelState::Loading => {
                // Wait for existing loading thread
                let timeout = Duration::from_secs(self.opts.load_timeout_secs);
                let result = self
                    .cvar
                    .wait_timeout_while(guard, timeout, |g| matches!(g.state, ModelState::Loading));
                match result {
                    Ok((mut g, timeout_result)) => {
                        if timeout_result.timed_out() {
                            if matches!(g.state, ModelState::Loading) {
                                g.state = ModelState::Failed("model loading timed out".into());
                                self.cvar.notify_all();
                            }
                            return Err("model loading timed out".into());
                        }
                        guard = g;
                    }
                    Err(e) => {
                        (guard, _) = e.into_inner();
                    }
                }
                match &guard.state {
                    ModelState::Ready => Ok(()),
                    ModelState::Failed(e) => Err(e.clone()),
                    other => Err(format!("unexpected state after wait: {other:?}")),
                }
            }
            ModelState::NotLoaded | ModelState::Cached => {
                // Start loading
                guard.state = ModelState::Loading;
                let model = guard.model;
                let cache_dir = guard.cache_dir.clone();
                // Notify waiters that we've started loading (they'll keep waiting)
                self.cvar.notify_all();
                drop(guard);

                // Do the actual loading (may download, takes seconds to minutes)
                let result = FastembedProvider::new(model, Some(cache_dir));

                let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                match result {
                    Ok(provider) => {
                        guard.state = ModelState::Ready;
                        guard.provider = Some(provider);
                        guard.last_used = Some(Instant::now());
                        self.cvar.notify_all();
                        Ok(())
                    }
                    Err(e) => {
                        guard.state = ModelState::Failed(e.to_string());
                        self.cvar.notify_all();
                        Err(e.to_string())
                    }
                }
            }
        }
    }
}

impl EmbeddingProvider for LazyFastembedProvider {
    fn dim(&self) -> usize {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.model.dimension()
    }

    fn name(&self) -> &str {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.model.as_str()
    }

    fn embed(&self, text: &str) -> anyhow::Result<EmbeddingResult> {
        // Check query cache
        {
            let mut cache = self.query_cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(cached) = cache.get(text) {
                return Ok(EmbeddingResult {
                    vector: cached.clone(),
                    text_preview: text_preview(text),
                });
            }
        }

        // Ensure model is loaded (includes idle eviction check)
        self.ensure_model().map_err(|e| anyhow::anyhow!("{e}"))?;

        // Embed
        let result = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.last_used = Some(Instant::now());
            match &guard.provider {
                Some(p) => p.embed(text),
                None => Err(anyhow::anyhow!("provider not available")),
            }
        }?;

        // Cache the result
        {
            let mut cache = self.query_cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.insert(text.to_string(), result.vector.clone());
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// Check if model weights exist in the HuggingFace cache directory.
///
/// Uses the `models--{org}--{repo}` folder naming convention that `hf-hub`
/// employs when caching model artefacts on disk.
pub fn is_model_cached(model: EmbeddingModel, cache_dir: &std::path::Path) -> bool {
    let model_code = model.model_code();
    let mut parts = model_code.splitn(2, '/');
    let org = parts.next().unwrap_or("");
    let repo = parts.next().unwrap_or("");
    if org.is_empty() || repo.is_empty() {
        return false;
    }
    let folder_name = format!("models--{org}--{repo}");
    cache_dir.join(&folder_name).is_dir()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_state_debug() {
        assert_eq!(format!("{:?}", ModelState::NotLoaded), "NotLoaded");
        assert_eq!(format!("{:?}", ModelState::Ready), "Ready");
    }

    #[test]
    fn lazy_opts_default() {
        let opts = LazyOpts::default();
        assert_eq!(opts.idle_timeout_secs, 600);
        assert_eq!(opts.load_timeout_secs, 300);
        assert_eq!(opts.query_cache_capacity, 64);
    }

    #[test]
    fn cache_hit_miss_eviction() {
        let mut cache = EmbeddingCache::new(2);
        assert!(cache.get("a").is_none());
        cache.insert("a".into(), vec![1.0]);
        assert!(cache.get("a").is_some());
        assert!(cache.get("b").is_none());
        cache.insert("b".into(), vec![2.0]);
        assert!(cache.get("a").is_some()); // a was accessed, not LRU
        cache.insert("c".into(), vec![3.0]); // evicts "b" (LRU)
        assert!(cache.get("b").is_none()); // evicted
        assert!(cache.get("a").is_some());
        assert!(cache.get("c").is_some());
    }

    #[test]
    fn cache_zero_capacity() {
        let mut cache = EmbeddingCache::new(0);
        cache.insert("a".into(), vec![1.0]);
        assert!(cache.get("a").is_none());
    }

    #[test]
    fn cache_update_existing() {
        let mut cache = EmbeddingCache::new(2);
        cache.insert("a".into(), vec![1.0]);
        cache.insert("a".into(), vec![2.0]);
        assert_eq!(cache.get("a").unwrap(), &vec![2.0]);
    }

    #[test]
    fn is_model_cached_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_model_cached(EmbeddingModel::BGESmallENV15, dir.path()));
    }

    #[test]
    fn is_model_cached_existing() {
        let dir = tempfile::tempdir().unwrap();
        // Use model_code() — the actual HF repo used by fastembed-rs.
        let model_code = EmbeddingModel::BGESmallENV15.model_code();
        let parts: Vec<&str> = model_code.splitn(2, '/').collect();
        let folder = dir
            .path()
            .join(format!("models--{}--{}", parts[0], parts[1]));
        std::fs::create_dir_all(&folder).unwrap();
        assert!(is_model_cached(EmbeddingModel::BGESmallENV15, dir.path()));
    }

    #[test]
    fn constructor_is_instant_no_model() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LazyFastembedProvider::new(
            EmbeddingModel::BGESmallENV15,
            dir.path().to_path_buf(),
            LazyOpts::default(),
        );
        assert_eq!(provider.state(), ModelState::NotLoaded);
    }

    #[test]
    fn constructor_detects_cached() {
        let dir = tempfile::tempdir().unwrap();
        // Use model_code() — matches the actual HF cache folder name.
        let model_code = EmbeddingModel::BGESmallENV15.model_code();
        let parts: Vec<&str> = model_code.splitn(2, '/').collect();
        let folder = dir
            .path()
            .join(format!("models--{}--{}", parts[0], parts[1]));
        std::fs::create_dir_all(&folder).unwrap();
        let provider = LazyFastembedProvider::new(
            EmbeddingModel::BGESmallENV15,
            dir.path().to_path_buf(),
            LazyOpts::default(),
        );
        assert_eq!(provider.state(), ModelState::Cached);
    }
}
