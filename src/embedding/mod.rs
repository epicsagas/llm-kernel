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

/// BGE-M3 joint dense + sparse embedding (feature `embedding-fastembed`).
#[cfg(any(
    feature = "embedding-fastembed",
    feature = "embedding-fastembed-dynamic-linking"
))]
pub mod bgem3;

#[cfg(feature = "embedding-fastembed-qwen3")]
pub mod qwen3;

#[cfg(feature = "embedding-fastembed-nomic-moe")]
pub mod nomic_moe;

/// BGE-small-en-v1.5 via Rust-native MLX (feature `embedding-mlx`, macOS only).
#[cfg(all(feature = "embedding-mlx", target_os = "macos"))]
pub mod mlx;

/// Vector index trait and types (zero dependencies).
pub mod vector_index;

/// Sparse (lexical) vectors for hybrid retrieval (zero dependencies).
pub mod sparse;

/// Async vector index trait for remote/shared backends (needs `async_trait`).
pub mod async_vector_index;

/// Qdrant `AsyncVectorIndex` (feature `qdrant`).
#[cfg(feature = "qdrant")]
pub mod qdrant;

/// Elasticsearch `AsyncVectorIndex` (feature `elastic`).
#[cfg(feature = "elastic")]
pub mod elastic;

/// pgvector `AsyncVectorIndex` (feature `pgvector`) — PostgreSQL + pgvector ext.
#[cfg(feature = "pgvector")]
pub mod pgvector;

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
pub use bgem3::{BGEM3_DENSE_DIM, BGEM3_VOCAB_SIZE, Bgem3Provider, JointEmbedding};

#[cfg(any(
    feature = "embedding-fastembed",
    feature = "embedding-fastembed-dynamic-linking"
))]
pub use lazy::{EmbeddingCache, LazyFastembedProvider, LazyOpts, ModelState, is_model_cached};

#[cfg(feature = "embedding-fastembed-qwen3")]
pub use qwen3::Qwen3Provider;

#[cfg(feature = "embedding-fastembed-nomic-moe")]
pub use nomic_moe::NomicMoeProvider;

#[cfg(all(feature = "embedding-mlx", target_os = "macos"))]
pub use mlx::MlxEmbeddingProvider;

/// Re-export `ort` for DirectML execution provider configuration.
///
/// Consumers that need `DirectMLExecutionProvider` (e.g. to pass it to
/// `fastembed::TextInitOptions::with_execution_providers`) should use this
/// re-export rather than depending on `ort` directly — this ensures the
/// pinned version stays compatible with fastembed's ONNX Runtime.
#[cfg(feature = "embedding-fastembed-directml")]
pub use ort;

pub use async_vector_index::AsyncVectorIndex;
pub use sparse::SparseVector;
pub use vector_index::{Fusion, SearchHit, VectorIndex};

#[cfg(feature = "qdrant")]
pub use qdrant::QdrantVectorIndex;

#[cfg(feature = "elastic")]
pub use elastic::ElasticsearchVectorIndex;

#[cfg(feature = "pgvector")]
pub use pgvector::{PgSparseVectorIndex, PgVectorIndex, PgVectorOpts};

#[cfg(feature = "vector-index")]
pub use turbovec::{IndexMeta, TurbovecIndex};
