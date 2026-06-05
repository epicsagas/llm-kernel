//! Embedding provider abstraction.
//!
//! Defines a trait for text embedding and provides common utilities.
//! Concrete backends (local ONNX, candle, OpenAI) are feature-gated.
//!
//! ```
//! use llm_kernel::embedding::{EmbeddingProvider, EmbeddingResult};
//! ```

pub mod types;

#[cfg(feature = "embedding-openai")]
pub mod openai;

pub use types::{EmbeddingProvider, EmbeddingResult, cosine_similarity};

#[cfg(feature = "embedding-openai")]
pub use openai::OpenAIEmbeddingClient;
