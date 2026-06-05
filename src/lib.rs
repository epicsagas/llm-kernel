//! # llm-kernel
//!
//! LLM provider catalog, async client, and model discovery for Rust applications.
//!
//! ## Modules
//!
//! | Feature       | Module      | Description                                         |
//! |---------------|-------------|-----------------------------------------------------|
//! | `provider`    | [`provider`]  | Provider catalog, model descriptors, pricing — **default** |
//! | `client-async`| [`llm`]       | Async LLM client (OpenAI, Anthropic) with SSE streaming |
//! | `discovery`   | [`discovery`] | Dynamic model discovery (models.dev, Ollama, OpenAI-compat) |
//! | `secrets`     | [`secrets`]   | SecretVault — dotenv-style credential management |
//! | `store`       | [`store`]     | SQLite init helpers (WAL, PRAGMA, schema versioning) |
//! | `config`      | [`config`]    | TOML config loader with auto-create from template |
//! | `graph`       | [`graph`]     | Knowledge graph — SQLite, FTS5, smart recall, BFS traversal |
//!
//! ## Quick start
//!
//! The [`prelude`] module re-exports the most commonly used types:
//!
//! ```ignore
//! use llm_kernel::prelude::*;
//!
//! // Browse the embedded provider catalog
//! let catalog = ProviderIndex::embedded();
//! for id in catalog.ids() {
//!     let provider = catalog.get(&id).unwrap();
//!     println!("{}", provider.display_name);
//! }
//!
//! // Query models with pricing and capabilities
//! for model in catalog.models_for("openai") {
//!     if let Some(cost) = &model.cost {
//!         println!("{} — ${:.2}/1M in, ${:.2}/1M out", model.id, cost.input, cost.output);
//!     }
//! }
//! ```
//!
//! ## Async client
//!
//! ```ignore
//! use llm_kernel::prelude::*;
//!
//! let config = ModelConfig {
//!     provider: "openai".into(),
//!     model: "gpt-4o".into(),
//!     api_key_env: "OPENAI_API_KEY".into(),
//!     base_url: None,
//!     temperature: 0.7,
//!     max_tokens: Some(1024),
//! };
//!
//! let client = OpenAIClient::new(&config)?;
//! let response = client.complete(LLMRequest {
//!     system: Some("You are a helpful assistant.".into()),
//!     messages: vec![ChatMessage::user("Hello!")],
//!     temperature: 0.7,
//!     max_tokens: Some(1024),
//!     model: None,
//! }).await?;
//! println!("{}", response.content);
//! ```
//!
//! ## Streaming
//!
//! ```ignore
//! let client = AnthropicClient::new(&config)?;
//! let stream = client.stream_complete(request).await?;
//! // Yields StreamEvent::Delta, StreamEvent::Usage, StreamEvent::Done
//! ```

pub mod error;

#[cfg(feature = "provider")]
pub mod provider;

#[cfg(feature = "discovery")]
pub mod discovery;

#[cfg(feature = "secrets")]
pub mod secrets;

#[cfg(feature = "client-async")]
pub mod llm;

#[cfg(feature = "store")]
pub mod store;

#[cfg(feature = "config")]
pub mod config;

#[cfg(feature = "graph")]
pub mod graph;

pub mod prelude;

/// Returns the crate name (`"llm-kernel"`).
pub fn name() -> &'static str {
    "llm-kernel"
}

/// Returns the crate version (from `Cargo.toml`).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
