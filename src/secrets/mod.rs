//! Credential management via dotenv-style vault.
//!
//! [`SecretVault`] loads and saves API keys from `.env` files with
//! symlink guards and atomic writes.
//!
//! ```
//! use llm_kernel::secrets::SecretVault;
//!
//! let vault = SecretVault::empty();
//! assert!(vault.is_empty());
//! ```

pub mod atomic;
pub mod vault;

pub use vault::{SecretVault, redact_credential};
