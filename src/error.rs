use thiserror::Error;

#[derive(Debug, Error)]
pub enum KernelError {
    #[error("LLM API error: {0}")]
    LlmApi(String),

    #[error("LLM rate limited: retry after {0}s")]
    RateLimited(u64),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Store error: {0}")]
    Store(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, KernelError>;
