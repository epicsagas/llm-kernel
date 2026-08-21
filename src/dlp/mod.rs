//! Data-loss-prevention (DLP) primitives.
//!
//! Layered detection for outbound LLM traffic:
//! - **L1** [`scan()`] — deterministic regex scan (secrets, Korean PII,
//!   filesystem paths) returning byte spans, categories, severity, and an
//!   overall [`Sensitivity`].
//! - **L2** [`fingerprint`] (feature `dlp-fingerprint`) — cosine-match
//!   outbound content against registered sensitive-document embeddings.
//! - **L3** [`ContentClassifier`](classifier::ContentClassifier) — trait for
//!   an optional local classifier on ambiguous cases.
//! - **Policy** [`policy::lookup`] — map a provider
//!   [`DataPolicy`] + [`Sensitivity`] to a [`PolicyAction`].
//!
//! ```
//! use llm_kernel::dlp::{Sensitivity, scan};
//!
//! let report = scan("Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345");
//! assert!(report.sensitivity >= Sensitivity::Confidential);
//! ```

/// L3 — optional local classifier for ambiguous content.
pub mod classifier;
/// Policy resolution — `DataPolicy` + `Sensitivity` → `PolicyAction`.
pub mod policy;
/// L1 — deterministic content scan.
pub mod scan;

/// L2 — fingerprint matching of registered sensitive documents.
#[cfg(feature = "dlp-fingerprint")]
pub mod fingerprint;

pub use crate::provider::policy::Sensitivity;
pub use policy::{DataPolicy, ImagePolicy, PolicyAction, PolicyThreshold, lookup};
pub use scan::{Finding, FindingCategory, ScanReport, Severity, Span, apply_redactions, scan};
