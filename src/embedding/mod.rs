//! Embedding provider abstraction.
//!
//! Defines a trait for text embedding and provides common utilities.
//! Concrete backends (local ONNX, candle, OpenAI) are feature-gated.
//!
//! ```
//! use llm_kernel::embedding::{EmbeddingProvider, EmbeddingResult};
//! ```

pub mod types;

pub use types::{cosine_similarity, EmbeddingProvider, EmbeddingResult};
