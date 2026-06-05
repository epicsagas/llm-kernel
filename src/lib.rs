//! # llm-kernel
//!
//! Shared LLM provider catalog, model discovery, credential management,
//! and async client library for Rust applications.
//!
//! ## Feature flags
//!
//! | Feature       | Description                                    |
//! |---------------|------------------------------------------------|
//! | `provider`    | Provider catalog (ProviderIndex, ServiceDescriptor) — **default** |
//! | `discovery`   | Dynamic model discovery (models.dev, Ollama, OpenAI-compat) |
//! | `client-async`| Async LLM client (OpenAI, Anthropic) with streaming |
//! | `secrets`     | SecretVault — dotenv-style credential management |
//! | `store`       | SQLite init helpers (WAL, PRAGMA, schema versioning) |
//! | `config`      | TOML config loader with auto-create |
//! | `full`        | All features                                   |
//!
//! ## Quick start
//!
//! ```ignore
//! use llm_kernel::prelude::*;
//!
//! let catalog = ProviderIndex::embedded();
//! let zai = catalog.get("zai").expect("zai provider");
//! println!("{}: {}", zai.display_name, zai.description);
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

pub mod prelude;

/// Returns the crate name (`"llm-kernel"`).
pub fn name() -> &'static str {
    "llm-kernel"
}

/// Returns the crate version (from `Cargo.toml`).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
