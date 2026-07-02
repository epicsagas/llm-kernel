//! Error types for llm-kernel.

use thiserror::Error;

/// Errors that can occur when using llm-kernel.
#[derive(Debug, Error)]
pub enum KernelError {
    /// An LLM API returned an error response.
    #[error("LLM API error: {0}")]
    LlmApi(String),

    /// The LLM API rate-limited the request.
    #[error("LLM rate limited: retry after {0}s")]
    RateLimited(u64),

    /// An HTTP error occurred (non-200 status with code).
    #[error("HTTP {status}: {message}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Error message from the response body.
        message: String,
    },

    /// A request timed out.
    #[error("Request timed out after {0}s")]
    Timeout(u64),

    /// A configuration error (missing field, bad format, etc.).
    #[error("Config error: {0}")]
    Config(String),

    /// A store (SQLite) error.
    #[error("Store error: {0}")]
    Store(String),

    /// A secrets vault error.
    #[error("Vault error: {0}")]
    Vault(String),

    /// An I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A search backend error.
    #[error("Search error: {0}")]
    Search(String),

    /// An embedding provider or vector-index error.
    #[error("Embedding error: {0}")]
    Embedding(String),

    /// A model-discovery error.
    #[error("Discovery error: {0}")]
    Discovery(String),

    /// A serialization/deserialization error.
    #[cfg(feature = "provider")]
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl KernelError {
    /// Construct an [`KernelError::Embedding`] from any displayable error.
    ///
    /// Convenience for mapping external errors (HTTP clients, ONNX runtimes) at
    /// `?` sites: `.map_err(KernelError::embedding)?`.
    pub fn embedding(e: impl std::fmt::Display) -> Self {
        Self::Embedding(e.to_string())
    }

    /// Construct an [`KernelError::Discovery`] from any displayable error.
    pub fn discovery(e: impl std::fmt::Display) -> Self {
        Self::Discovery(e.to_string())
    }
}

/// Alias for `Result<T, KernelError>`.
pub type Result<T> = std::result::Result<T, KernelError>;
