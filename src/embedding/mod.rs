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

#[cfg(any(
    feature = "embedding-fastembed",
    feature = "embedding-fastembed-dynamic-linking"
))]
pub mod fastembed;

#[cfg(any(
    feature = "embedding-fastembed",
    feature = "embedding-fastembed-dynamic-linking"
))]
pub mod lazy;

#[cfg(feature = "embedding-fastembed-qwen3")]
pub mod qwen3;

#[cfg(feature = "embedding-fastembed-nomic-moe")]
pub mod nomic_moe;

/// Vector index trait and types (zero dependencies).
pub mod vector_index;

/// Async vector index trait for remote/shared backends (needs `async_trait`).
pub mod async_vector_index;

/// Qdrant `AsyncVectorIndex` (feature `qdrant`).
#[cfg(feature = "qdrant")]
pub mod qdrant;

/// Elasticsearch `AsyncVectorIndex` (feature `elastic`).
#[cfg(feature = "elastic")]
pub mod elastic;

#[cfg(feature = "vector-index")]
pub mod turbovec;

pub use catalog::EmbeddingModel;
pub use types::{EmbeddingProvider, EmbeddingResult, chunk_batch, cosine_similarity};

#[cfg(feature = "embedding-openai")]
pub use openai::OpenAIEmbeddingClient;

#[cfg(any(
    feature = "embedding-fastembed",
    feature = "embedding-fastembed-dynamic-linking"
))]
pub use fastembed::FastembedProvider;

#[cfg(any(
    feature = "embedding-fastembed",
    feature = "embedding-fastembed-dynamic-linking"
))]
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

pub use async_vector_index::AsyncVectorIndex;
pub use vector_index::{SearchHit, VectorIndex};

#[cfg(feature = "qdrant")]
pub use qdrant::QdrantVectorIndex;

#[cfg(feature = "elastic")]
pub use elastic::ElasticsearchVectorIndex;

#[cfg(feature = "vector-index")]
pub use turbovec::TurbovecIndex;
