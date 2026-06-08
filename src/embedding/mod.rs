//! Embedding provider abstraction.
//!
//! Defines a trait for text embedding and provides common utilities.
//! Concrete backends (local ONNX, candle, OpenAI) are feature-gated.
//!
//! ```
//! use llm_kernel::embedding::{EmbeddingProvider, EmbeddingResult};
//! ```

pub mod catalog;
pub mod types;

#[cfg(feature = "embedding-openai")]
pub mod openai;

#[cfg(feature = "embedding-fastembed")]
pub mod fastembed;

#[cfg(feature = "embedding-fastembed")]
pub mod lazy;

#[cfg(feature = "embedding-fastembed-qwen3")]
pub mod qwen3;

#[cfg(feature = "embedding-fastembed-nomic-moe")]
pub mod nomic_moe;

#[cfg(feature = "vector-index")]
pub mod vector_index;

pub use catalog::EmbeddingModel;
pub use types::{EmbeddingProvider, EmbeddingResult, cosine_similarity};

#[cfg(feature = "embedding-openai")]
pub use openai::OpenAIEmbeddingClient;

#[cfg(feature = "embedding-fastembed")]
pub use fastembed::FastembedProvider;

#[cfg(feature = "embedding-fastembed")]
pub use lazy::{EmbeddingCache, LazyFastembedProvider, LazyOpts, ModelState, is_model_cached};

#[cfg(feature = "embedding-fastembed-qwen3")]
pub use qwen3::Qwen3Provider;

#[cfg(feature = "embedding-fastembed-nomic-moe")]
pub use nomic_moe::NomicMoeProvider;

/// Re-export `ort` for DirectML execution provider configuration.
///
/// Consumers that need `DirectMLExecutionProvider` (e.g. to pass it to
/// `fastembed::TextInitOptions::with_execution_providers`) should use this
/// re-export rather than depending on `ort` directly — this ensures the
/// pinned version stays compatible with fastembed's ONNX Runtime.
#[cfg(feature = "embedding-fastembed-directml")]
pub use ort;

#[cfg(feature = "vector-index")]
pub use vector_index::{SearchHit, TurbovecIndex, VectorIndex};
