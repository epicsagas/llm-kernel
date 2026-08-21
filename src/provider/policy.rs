//! Provider data policy — what content may leave the machine for a provider.
//!
//! [`DataPolicy`] is a field of
//! [`ServiceDescriptor`](crate::provider::ServiceDescriptor), so this
//! vocabulary lives in `provider` and stays free of engine concerns.
//!
//! ```
//! use llm_kernel::provider::{DataPolicy, ProviderIndex, Sensitivity};
//!
//! let catalog = ProviderIndex::embedded();
//! let policy = catalog
//!     .get("openai")
//!     .map(DataPolicy::default_for)
//!     .unwrap_or_default();
//! // `Sensitivity` is ordered: Public < Internal < Confidential < Restricted.
//! assert!(Sensitivity::Restricted > Sensitivity::Public);
//! assert_eq!(policy.image, llm_kernel::provider::ImagePolicy::Allow);
//! ```

use serde::{Deserialize, Serialize};

/// Overall sensitivity grade of scanned content.
///
/// Variant order is the ordering (`Public < Internal < Confidential <
/// Restricted`) — policy thresholds compare with it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// No findings.
    #[default]
    Public,
    /// Low/medium findings only (filesystem paths, phone numbers).
    Internal,
    /// High-severity findings (bank accounts, generic credential assignments).
    Confidential,
    /// Critical findings (credentials, RRN) — must not leave unredacted.
    Restricted,
}

/// Action to take on content bound for a provider.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyAction {
    /// Send as-is.
    Allow,
    /// Replace flagged spans (the scan's `redact_spans`), then send.
    Redact,
    /// Send, but surface a warning to the user.
    Warn,
    /// Send to a different provider instead.
    ReRoute {
        /// Target provider id (e.g. `"ollama"`).
        provider: String,
    },
    /// Do not send.
    Block,
}

/// Whether binary (image) payloads may be sent to the provider.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImagePolicy {
    /// Images may be sent (compatibility first; strip via explicit policy).
    #[default]
    Allow,
    /// Images are stripped before the request leaves the machine.
    Strip,
}

/// One step of the sensitivity-to-action ladder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyThreshold {
    /// Minimum sensitivity at which `action` applies.
    pub min_sensitivity: Sensitivity,
    /// Action taken when content sensitivity is at or above the threshold.
    pub action: PolicyAction,
}

/// Per-provider data-loss-prevention policy, carried on
/// [`ServiceDescriptor`](crate::provider::ServiceDescriptor).
///
/// Catalog entries ship `None` in the first cut; [`DataPolicy::default_for`]
/// supplies code-level defaults.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DataPolicy {
    /// Whether binary (image) payloads are allowed.
    #[serde(default)]
    pub image: ImagePolicy,
    /// Sensitivity-thresholded actions. `lookup` picks the highest threshold
    /// satisfied by the content's sensitivity; `Allow` when none match.
    /// `Default` (empty) is the permissive policy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thresholds: Vec<PolicyThreshold>,
}

impl DataPolicy {
    /// Code-level default policy for a provider.
    ///
    /// Local providers (`family == "local"`: ollama, lmstudio, llamacpp) get
    /// the permissive policy — traffic never leaves the machine. Everything
    /// else gets: Restricted → Block, Confidential → ReRoute to `"ollama"`,
    /// Internal → Warn; images default to
    /// [`Allow`](ImagePolicy::Allow) for vision-workflow compatibility.
    pub fn default_for(svc: &crate::provider::ServiceDescriptor) -> Self {
        if svc.family == "local" {
            return Self::default();
        }
        Self {
            image: ImagePolicy::Allow,
            thresholds: vec![
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
                PolicyThreshold {
                    min_sensitivity: Sensitivity::Internal,
                    action: PolicyAction::Warn,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_permissive() {
        let policy = DataPolicy::default();
        assert!(policy.thresholds.is_empty());
        assert_eq!(policy.image, ImagePolicy::Allow);
    }

    #[test]
    fn sensitivity_is_ordered() {
        assert!(Sensitivity::Public < Sensitivity::Internal);
        assert!(Sensitivity::Internal < Sensitivity::Confidential);
        assert!(Sensitivity::Confidential < Sensitivity::Restricted);
    }

    #[test]
    fn data_policy_serde_roundtrip_keeps_reroute_payload() {
        let policy = DataPolicy {
            image: ImagePolicy::Strip,
            thresholds: vec![PolicyThreshold {
                min_sensitivity: Sensitivity::Confidential,
                action: PolicyAction::ReRoute {
                    provider: "ollama".to_string(),
                },
            }],
        };
        let json = serde_json::to_string(&policy).expect("serialize");
        let back: DataPolicy = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, policy);
        assert!(json.contains("\"re_route\""), "got: {json}");
    }

    #[test]
    fn default_for_local_provider_is_permissive() {
        let catalog = crate::provider::ProviderIndex::embedded();
        let ollama = catalog.get("ollama").expect("ollama in catalog");
        assert_eq!(ollama.family, "local");
        assert!(DataPolicy::default_for(ollama).thresholds.is_empty());
    }

    #[test]
    fn default_for_cloud_provider_has_thresholds() {
        let catalog = crate::provider::ProviderIndex::embedded();
        let openai = catalog.get("openai").expect("openai in catalog");
        let policy = DataPolicy::default_for(openai);
        assert_eq!(policy.image, ImagePolicy::Allow);
        assert_eq!(policy.thresholds.len(), 3);
        assert_eq!(policy.thresholds[0].action, PolicyAction::Block);
        assert_eq!(
            policy.thresholds[1].action,
            PolicyAction::ReRoute {
                provider: "ollama".to_string()
            }
        );
        assert_eq!(policy.thresholds[2].action, PolicyAction::Warn);
    }

    #[test]
    fn service_descriptor_data_policy_survives_serde_roundtrip() {
        // Regression for catalog-sync dropping unknown fields: a known
        // `data_policy` must round-trip through parse → serialize unchanged.
        let catalog = crate::provider::ProviderIndex::embedded();
        let mut svc = catalog.get("zai").expect("zai in catalog").clone();
        svc.data_policy = Some(DataPolicy {
            image: ImagePolicy::Strip,
            thresholds: vec![PolicyThreshold {
                min_sensitivity: Sensitivity::Internal,
                action: PolicyAction::Warn,
            }],
        });
        let json = serde_json::to_string_pretty(&svc).expect("serialize");
        let back: crate::provider::ServiceDescriptor =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.data_policy, svc.data_policy);
    }
}
