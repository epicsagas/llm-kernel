//! L3 — optional local classifier for ambiguous content.
//!
//! Trait-only seam: when L1 regex evidence and L2 fingerprint matching leave
//! a case ambiguous, a local small model (e.g. an Ollama-backed classifier)
//! can upgrade the sensitivity grade. No implementation ships in this crate.

use crate::error::Result;
use crate::provider::policy::Sensitivity;

/// Optional local classifier that upgrades the L1 grade when regex evidence
/// is ambiguous.
///
/// Implementations must be `Send + Sync` (called from proxy request paths).
pub trait ContentClassifier: Send + Sync {
    /// Classify `content`; `Ok(None)` defers to the L1 [`Sensitivity`] (the
    /// `scan` result). `Err` falls back to the L1 grade
    /// at the call site.
    fn classify(&self, content: &str) -> Result<Option<Sensitivity>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Option<Sensitivity>);

    impl ContentClassifier for Fixed {
        fn classify(&self, _content: &str) -> Result<Option<Sensitivity>> {
            Ok(self.0)
        }
    }

    #[test]
    fn classifier_returns_verdict_or_none() {
        let upgrader = Fixed(Some(Sensitivity::Confidential));
        assert_eq!(
            upgrader.classify("ambiguous merger notes").unwrap(),
            Some(Sensitivity::Confidential)
        );

        let defer = Fixed(None);
        assert_eq!(defer.classify("anything").unwrap(), None);
    }

    #[test]
    fn classifier_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Fixed>();
    }
}
