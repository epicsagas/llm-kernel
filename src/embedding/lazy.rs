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
use std::sync::Arc;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use indexmap::IndexMap;

use super::catalog::EmbeddingModel;
use super::fastembed::FastembedProvider;
use super::types::text_preview;
use super::types::{EmbeddingProvider, EmbeddingResult};
use crate::error::{KernelError, Result};

// ---------------------------------------------------------------------------
// Load hook (panic-safe model instantiation)
// ---------------------------------------------------------------------------

/// Type of the model-load closure used by [`LazyFastembedProvider`].
///
/// Wrapped in `Arc` so the loader can be cheaply cloned out from under the
/// `Inner` lock and invoked without holding the mutex across the
/// (potentially minutes-long) model download/init.
pub(crate) type LoadFn =
    Arc<dyn Fn(EmbeddingModel, PathBuf) -> Result<FastembedProvider> + Send + Sync>;

/// Default loader: instantiate [`FastembedProvider`] directly.
///
/// This is a free function (rather than an inline call) so that tests can swap
/// it for an injectable panicking/failing loader via
/// [`LazyFastembedProvider::new_with_loader`] without needing real ONNX weights
/// on disk.
fn default_load(model: EmbeddingModel, cache_dir: PathBuf) -> Result<FastembedProvider> {
    FastembedProvider::new(model, Some(cache_dir))
}

/// Run `load` catching any panic, so a panicking ONNX init (e.g. a missing
/// `libonnxruntime.so` under `ort-load-dynamic`) is surfaced as a clean `Err`
/// instead of unwinding across the `Mutex`/`Condvar` boundary and wedging any
/// waiter parked in [`LazyFastembedProvider::ensure_model`]'s `Loading` branch
/// (see #50).
///
/// `AssertUnwindSafe` is sound here: on the `Err(panic)` path the closure's
/// captures are discarded entirely, and the values read afterwards
/// (`EmbeddingModel`/`PathBuf`) were copied in before the call — so no torn
/// shared state escapes.
///
/// The returned bool is `true` iff the failure came from a caught panic
/// (as opposed to an ordinary `Err` from `load`) — callers use this to mark
/// [`ModelState::Failed`] as panic-originated, since a panic during ort/ONNX
/// init may leave process-global state in a way that makes blind retries
/// riskier than retrying after an ordinary (e.g. network) failure.
fn load_catching_panics(
    load: &LoadFn,
    model: EmbeddingModel,
    cache_dir: PathBuf,
) -> (Result<FastembedProvider>, bool) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load(model, cache_dir)));
    match result {
        Ok(inner) => (inner, false),
        Err(payload) => {
            let msg = panic_message(&payload);
            (
                Err(KernelError::Embedding(format!(
                    "model init panicked: {msg}"
                ))),
                true,
            )
        }
    }
}

/// Best-effort string extraction from a panic payload.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}
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
    ///
    /// `panicked` is `true` when the failure came from a caught panic during
    /// loader init (e.g. ort/ONNX init panicking on a missing dylib) rather
    /// than an ordinary `Err` returned by the loader (e.g. a network error
    /// during model download). Callers that want to retry after `Failed`
    /// (via [`LazyFastembedProvider::reset`]) should treat a panic-origin
    /// failure with more caution — process-global ort state may be corrupted
    /// — whereas an ordinary failure is generally safe to retry blindly.
    Failed {
        /// Human-readable error description.
        message: String,
        /// `true` if the failure came from a caught panic during loader
        /// init, `false` if the loader returned an ordinary `Err`.
        panicked: bool,
    },
}

impl ModelState {
    /// Whether this is a `Failed` state that originated from a caught panic
    /// (as opposed to an ordinary loader `Err`).
    pub fn is_panic(&self) -> bool {
        matches!(self, ModelState::Failed { panicked: true, .. })
    }
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
    /// Injectable model loader. Defaults to [`default_load`] (which calls
    /// `FastembedProvider::new`); tests substitute a controllable loader.
    load_fn: LoadFn,
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
        Self::new_with_loader(model, cache_dir, opts, Arc::new(default_load))
    }

    /// Create a lazy provider with an injected model loader.
    ///
    /// Production callers should use [`new`](Self::new); this constructor exists
    /// so tests can drive the panic/failure paths of [`ensure_model`](Self::ensure_model)
    /// deterministically without real ONNX weights.
    pub fn new_with_loader(
        model: EmbeddingModel,
        cache_dir: PathBuf,
        opts: LazyOpts,
        load_fn: LoadFn,
    ) -> Self {
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
                load_fn,
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
    /// cloning the `Failed { message, .. }` variant's string.
    pub fn is_ready(&self) -> bool {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        matches!(guard.state, ModelState::Ready)
    }

    /// Clear a `Failed` state so the next [`ensure_model`](Self::ensure_model)
    /// call retries the load, instead of returning the cached error forever.
    ///
    /// No-op if the provider isn't currently `Failed`. Resets to `Cached` if
    /// weights are already on disk, `NotLoaded` otherwise — mirroring the
    /// initial-state logic in [`new_with_loader`](Self::new_with_loader).
    ///
    /// Callers should check [`ModelState::is_panic`] before calling this:
    /// retrying after a panic-origin failure re-enters the same loader (and
    /// thus the same ort/ONNX init path) that just panicked, which may hit
    /// corrupted process-global state rather than a fresh, safe retry.
    pub fn reset(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(guard.state, ModelState::Failed { .. }) {
            guard.state = if is_model_cached(guard.model, &guard.cache_dir) {
                ModelState::Cached
            } else {
                ModelState::NotLoaded
            };
        }
    }

    /// Block until the model is ready for inference.
    ///
    /// If the model is idle beyond the configured timeout, the ONNX session
    /// is evicted and reloaded from the on-disk cache. If the model is not
    /// yet loaded, this triggers download and initialisation on the calling
    /// thread. Concurrent callers wait on a `Condvar` for up to
    /// `load_timeout_secs`.
    pub fn ensure_model(&self) -> std::result::Result<(), String> {
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
            ModelState::Failed { message, .. } => Err(message.clone()),
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
                                g.state = ModelState::Failed {
                                    message: "model loading timed out".into(),
                                    panicked: false,
                                };
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
                    ModelState::Failed { message, .. } => Err(message.clone()),
                    other => Err(format!("unexpected state after wait: {other:?}")),
                }
            }
            ModelState::NotLoaded | ModelState::Cached => {
                // Start loading
                guard.state = ModelState::Loading;
                let model = guard.model;
                let cache_dir = guard.cache_dir.clone();
                // Clone the loader out from under the lock so we don't hold the
                // mutex across the (potentially minutes-long) model load.
                let load = Arc::clone(&guard.load_fn);
                // Notify waiters that we've started loading (they'll keep waiting)
                self.cvar.notify_all();
                drop(guard);

                // Do the actual loading (may download, takes seconds to minutes).
                // Wrapped in `catch_unwind` so a panicking ONNX init surfaces as
                // `Failed{..}` + `notify_all()` instead of wedging waiters (#50).
                let (result, panicked) = load_catching_panics(&load, model, cache_dir);

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
                        guard.state = ModelState::Failed {
                            message: e.to_string(),
                            panicked,
                        };
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

    fn embed(&self, text: &str) -> Result<EmbeddingResult> {
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
        self.ensure_model().map_err(KernelError::Embedding)?;

        // Embed
        let result = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.last_used = Some(Instant::now());
            match &guard.provider {
                Some(p) => p.embed(text),
                None => Err(KernelError::Embedding("provider not available".into())),
            }
        }?;

        // Cache the result
        {
            let mut cache = self.query_cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.insert(text.to_string(), result.vector.clone());
        }

        Ok(result)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<EmbeddingResult>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // Phase 1: check cache for all texts
        let mut results: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        let mut miss_indices: Vec<usize> = Vec::with_capacity(texts.len());
        let mut miss_texts: Vec<&str> = Vec::with_capacity(texts.len());

        {
            let mut cache = self.query_cache.lock().unwrap_or_else(|e| e.into_inner());
            for (i, text) in texts.iter().enumerate() {
                if let Some(cached) = cache.get(text) {
                    results[i] = Some(cached.clone());
                } else {
                    miss_indices.push(i);
                    miss_texts.push(*text);
                }
            }
        }

        // Phase 2: if all cache hits, return immediately
        if miss_indices.is_empty() {
            return Ok(results
                .into_iter()
                .zip(texts.iter())
                .map(|(opt, text)| EmbeddingResult {
                    vector: opt.unwrap(),
                    text_preview: text_preview(text),
                })
                .collect());
        }

        // Phase 3: ensure model is loaded
        self.ensure_model().map_err(KernelError::Embedding)?;

        // Phase 4: batch embed the misses through the inner provider
        let batch_results = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.last_used = Some(Instant::now());
            match &guard.provider {
                Some(p) => p.embed_batch(&miss_texts),
                None => Err(KernelError::Embedding("provider not available".into())),
            }
        }?;

        // A well-behaved provider returns exactly one vector per input. Guard
        // against a truncated/malformed response so the merge below indexes
        // `batch_results` safely instead of panicking on an out-of-bounds slot.
        if batch_results.len() != miss_texts.len() {
            return Err(KernelError::Embedding(format!(
                "provider returned {} embeddings for {} inputs",
                batch_results.len(),
                miss_texts.len()
            )));
        }

        // Phase 5: merge results and insert new entries into cache
        {
            let mut cache = self.query_cache.lock().unwrap_or_else(|e| e.into_inner());
            for (batch_idx, &result_idx) in miss_indices.iter().enumerate() {
                let vector = batch_results[batch_idx].vector.clone();
                cache.insert(texts[result_idx].to_string(), vector.clone());
                results[result_idx] = Some(vector);
            }
        }

        // Phase 6: assemble final results in original order. Every slot is now
        // populated (all cache-hit or filled in Phase 5), so unwrap is safe.
        results
            .into_iter()
            .zip(texts.iter())
            .map(|(opt, text)| {
                opt.map(|vector| EmbeddingResult {
                    vector,
                    text_preview: text_preview(text),
                })
                .ok_or_else(|| KernelError::Embedding("internal: unfilled embedding slot".into()))
            })
            .collect()
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

    // ----- panic-safety regression tests (#50) -----
    //
    // These exercise the contract that a panicking (or failing) model loader
    // transitions the provider to `Failed` and releases any waiter parked in
    // the `Loading` branch, instead of hanging forever. They use the injected
    // loader (`new_with_loader`) so no real ONNX weights are required.
    //
    // NOTE: the production `[profile.release]` sets `panic = "abort"`, under
    // which a panic terminates the process directly (no unwind to catch). The
    // `catch_unwind` guard therefore matters for panic-unwinding builds
    // (debug, and release builds that override `panic`), and these tests run
    // under the default test harness which unwinds. The guard is still
    // valuable under `panic = "abort"` for the *failure* (non-panic) path and
    // documents intent; the abort case is acceptable (a hard crash is a clearer
    // failure than the previous silent deadlock).

    fn panicking_loader() -> LoadFn {
        Arc::new(
            |_model: EmbeddingModel, _cache: PathBuf| -> Result<FastembedProvider> {
                panic!("simulated ort init failure (missing libonnxruntime.so)")
            },
        )
    }

    fn failing_loader(msg: &str) -> LoadFn {
        let msg = msg.to_string();
        Arc::new(
            move |_model: EmbeddingModel, _cache: PathBuf| -> Result<FastembedProvider> {
                Err(KernelError::Embedding(msg.clone()))
            },
        )
    }

    #[test]
    fn panic_during_load_transitions_to_failed() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LazyFastembedProvider::new_with_loader(
            EmbeddingModel::BGESmallENV15,
            dir.path().to_path_buf(),
            LazyOpts::default(),
            panicking_loader(),
        );
        assert_eq!(provider.state(), ModelState::NotLoaded);

        let err = provider.ensure_model().unwrap_err();
        assert!(
            err.contains("model init panicked"),
            "expected panic message, got: {err}"
        );
        match provider.state() {
            ModelState::Failed { message, panicked } => {
                assert!(
                    message.contains("model init panicked"),
                    "expected panic in Failed state, got: {message}"
                );
                assert!(
                    panicked,
                    "panic-originated failure should set panicked=true"
                );
            }
            other => panic!("expected Failed state after panic, got {other:?}"),
        }
        assert!(provider.state().is_panic());
    }

    #[test]
    fn failure_during_load_transitions_to_failed() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LazyFastembedProvider::new_with_loader(
            EmbeddingModel::BGESmallENV15,
            dir.path().to_path_buf(),
            LazyOpts::default(),
            failing_loader("download denied"),
        );
        let err = provider.ensure_model().unwrap_err();
        assert!(err.contains("download denied"), "got: {err}");
        match provider.state() {
            ModelState::Failed { panicked, .. } => {
                assert!(!panicked, "ordinary Err should not set panicked=true");
            }
            other => panic!("expected Failed state, got {other:?}"),
        }
        assert!(!provider.state().is_panic());
    }

    #[test]
    fn reset_after_failure_allows_retry() {
        let dir = tempfile::tempdir().unwrap();
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_clone = Arc::clone(&attempts);
        let loader: LoadFn = Arc::new(move |_m, _c| {
            attempts_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(KernelError::Embedding("transient failure".into()))
        });
        let provider = LazyFastembedProvider::new_with_loader(
            EmbeddingModel::BGESmallENV15,
            dir.path().to_path_buf(),
            LazyOpts::default(),
            loader,
        );

        assert!(provider.ensure_model().is_err());
        assert!(matches!(provider.state(), ModelState::Failed { .. }));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Without reset, ensure_model just replays the cached error.
        assert!(provider.ensure_model().is_err());
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);

        // After reset, ensure_model retries the loader.
        provider.reset();
        assert_eq!(provider.state(), ModelState::NotLoaded);
        assert!(provider.ensure_model().is_err());
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn reset_is_noop_when_not_failed() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LazyFastembedProvider::new_with_loader(
            EmbeddingModel::BGESmallENV15,
            dir.path().to_path_buf(),
            LazyOpts::default(),
            failing_loader("irrelevant"),
        );
        assert_eq!(provider.state(), ModelState::NotLoaded);
        provider.reset();
        assert_eq!(provider.state(), ModelState::NotLoaded);
    }

    #[test]
    fn concurrent_waiter_released_when_loader_panics() {
        // AC4: a waiter blocked in the `Loading` branch must be released (with
        // an `Err`) within a bounded time when the owning loader panics — not
        // hang until load_timeout_secs, and not deadlock.
        use std::sync::Barrier;
        use std::thread;

        let dir = tempfile::tempdir().unwrap();

        // A loader that waits on a barrier until the waiter is parked, then
        // panics — deterministically reproducing the #50 hang scenario.
        let barrier = Arc::new(Barrier::new(2));
        let loader_barrier = Arc::clone(&barrier);
        let loader: LoadFn = Arc::new(move |_m, _c| {
            // Let the waiter reach the `Loading` wait first.
            loader_barrier.wait();
            panic!("simulated ort init failure under contention");
        });

        let opts = LazyOpts {
            load_timeout_secs: 30, // generous; we must release well before this
            ..LazyOpts::default()
        };
        let provider = Arc::new(LazyFastembedProvider::new_with_loader(
            EmbeddingModel::BGESmallENV15,
            dir.path().to_path_buf(),
            opts,
            loader,
        ));

        // Waiter thread: parks in the `Loading` branch.
        let waiter_provider = Arc::clone(&provider);
        let waiter = thread::spawn(move || waiter_provider.ensure_model());

        // Give the loader the chance to run; it waits for the barrier, the
        // waiter parks in Loading, then this wait returns and the loader
        // panics. The provider's panic guard converts that to Failed + notify.
        barrier.wait();

        // The waiter must terminate (with Err) promptly rather than hang.
        let result = waiter.join().expect("waiter thread should not hang");
        assert!(result.is_err(), "waiter should receive Err, not Ok");
        assert!(
            matches!(provider.state(), ModelState::Failed { .. }),
            "provider should be Failed after loader panic, got {:?}",
            provider.state()
        );
        assert!(provider.state().is_panic());
    }
}
