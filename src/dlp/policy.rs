//! Policy resolution — map a [`DataPolicy`] + [`Sensitivity`] to a [`PolicyAction`].
//!
//! ```
//! use llm_kernel::dlp::{DataPolicy, PolicyAction, PolicyThreshold, Sensitivity, lookup};
//!
//! let mut policy = DataPolicy::default();
//! policy.thresholds.push(PolicyThreshold {
//!     min_sensitivity: Sensitivity::Restricted,
//!     action: PolicyAction::Block,
//! });
//! assert_eq!(lookup(&policy, Sensitivity::Restricted), PolicyAction::Block);
//! assert_eq!(lookup(&policy, Sensitivity::Internal), PolicyAction::Allow);
//! ```

pub use crate::provider::policy::{
    DataPolicy, ImagePolicy, PolicyAction, PolicyThreshold, Sensitivity,
};

/// Resolve the action for `sensitivity` under `policy`.
///
/// Picks the action of the **highest** threshold whose `min_sensitivity` is
/// satisfied; [`PolicyAction::Allow`] when no threshold matches (the
/// permissive default). Works regardless of `thresholds` ordering.
pub fn lookup(policy: &DataPolicy, sensitivity: Sensitivity) -> PolicyAction {
    policy
        .thresholds
        .iter()
        .filter(|t| sensitivity >= t.min_sensitivity)
        .max_by_key(|t| t.min_sensitivity)
        .map(|t| t.action.clone())
        .unwrap_or(PolicyAction::Allow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud_policy() -> DataPolicy {
        DataPolicy {
            image: ImagePolicy::Allow,
            thresholds: vec![
                PolicyThreshold {
                    min_sensitivity: Sensitivity::Internal,
                    action: PolicyAction::Warn,
                },
                PolicyThreshold {
                    min_sensitivity: Sensitivity::Restricted,
                    action: PolicyAction::Block,
                },
                PolicyThreshold {
                    min_sensitivity: Sensitivity::Confidential,
                    action: PolicyAction::ReRoute {
                        provider: "ollama".to_string(),
                    },
                },
            ],
        }
    }

    #[test]
    fn highest_matching_threshold_wins_regardless_of_order() {
        let policy = cloud_policy(); // deliberately unordered
        assert_eq!(lookup(&policy, Sensitivity::Public), PolicyAction::Allow);
        assert_eq!(lookup(&policy, Sensitivity::Internal), PolicyAction::Warn);
        assert_eq!(
            lookup(&policy, Sensitivity::Confidential),
            PolicyAction::ReRoute {
                provider: "ollama".to_string()
            }
        );
        assert_eq!(
            lookup(&policy, Sensitivity::Restricted),
            PolicyAction::Block
        );
    }

    #[test]
    fn below_all_thresholds_allows() {
        assert_eq!(
            lookup(&cloud_policy(), Sensitivity::Public),
            PolicyAction::Allow
        );
    }

    #[test]
    fn empty_policy_always_allows() {
        let policy = DataPolicy::default();
        assert_eq!(
            lookup(&policy, Sensitivity::Restricted),
            PolicyAction::Allow
        );
    }
}
