//! # llm-kernel
//!
//! Foundation library for Rust AI-native applications.
//!
//! Provides a composable, feature-gated set of modules for building
//! LLM-powered tools, agents, and servers:
//!
//! | Feature       | Module       | Description                                         |
//! |---------------|-------------|-----------------------------------------------------|
//! | `provider`    | [`provider`]  | Provider catalog, model descriptors, pricing — **default** |
//! | `client-async`| [`llm`]       | Async LLM client (OpenAI, Anthropic) with SSE streaming |
//! | `discovery`   | [`discovery`] | Dynamic model discovery (models.dev, Ollama, OpenAI-compat) |
//! | `secrets`     | [`secrets`]   | SecretVault — dotenv-style credential management |
//! | `store`       | [`store`]     | SQLite init helpers (WAL, PRAGMA, schema versioning) |
//! | `config`      | [`config`]    | TOML config loader with auto-create from template |
//! | `graph`       | [`graph`]     | Knowledge graph — SQLite, FTS5, smart recall, BFS traversal |
//! | `mcp`         | [`mcp`]       | MCP server framework — JSON-RPC 2.0, stdio transport |
//! | `tokens`      | [`tokens`]    | Token estimation with Unicode-script heuristics |
//! | `install`     | [`install`]   | AI tool installation wizard (Claude, Cursor, Copilot, etc.) |
//! | `search`      | [`search`]    | Hybrid search with Reciprocal Rank Fusion |
//! | `embedding`   | [`embedding`] | Embedding provider trait + cosine similarity |
//! | `telemetry`   | [`telemetry`] | Telemetry framework — enum-gated events, no PII |
//! | `safety`      | [`safety`]    | Secret masking, error classification, output sanitization |
//!
//! ## Quick start
//!
//! The [`prelude`] module re-exports the most commonly used types:
//!
//! ```no_run
//! use llm_kernel::prelude::*;
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

#[cfg(feature = "mcp")]
pub mod mcp;

#[cfg(feature = "tokens")]
pub mod tokens;

#[cfg(feature = "install")]
pub mod install;

#[cfg(feature = "search")]
pub mod search;

#[cfg(any(feature = "embedding", feature = "embedding-openai"))]
pub mod embedding;

#[cfg(feature = "telemetry")]
pub mod telemetry;

#[cfg(feature = "safety")]
pub mod safety;

pub mod prelude;

/// Returns the crate name (`"llm-kernel"`).
pub fn name() -> &'static str {
    "llm-kernel"
}

/// Returns the crate version (from `Cargo.toml`).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
