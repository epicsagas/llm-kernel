//! Safety utilities for AI-native applications.
//!
//! Provides secret masking, output sanitization, and error classification
//! to prevent PII leaks and injection attacks.
//!
//! ```
//! use llm_kernel::safety::{mask_secrets, sanitize_output};
//!
//! let masked = mask_secrets("key=sk-abc123def456 token=Bearer xyz789");
//! assert!(masked.contains("****"));
//!
//! let clean = sanitize_output("hello\u{202E}evil\u{FFF9}tags");
//! assert!(!clean.contains('\u{202E}'));
//! ```

pub mod classify;
/// Prompt-injection detection via weighted regex rules.
pub mod injection;
pub mod sanitize;

pub use classify::{FailureCategory, classify_failure};
pub use injection::{InjectionScore, detect_injection};
pub use sanitize::{mask_secrets, sanitize_output, strip_ansi};
