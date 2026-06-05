use super::catalog::ServiceDescriptor;

/// How a provider authenticates API requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStrategy {
    /// No authentication required (e.g. local Ollama).
    None,
    /// Hardcoded token in the catalog (e.g. a free-tier key).
    Literal,
    /// Read secret from environment variable.
    Secret,
    /// Unknown authentication mode.
    Unknown,
}

/// Capability profile for a provider — determines auth strategy and feature support.
pub trait CapabilityProfile {
    fn auth_strategy(&self) -> AuthStrategy;
    fn clears_anthropic_api_key(&self) -> bool;
    fn supports_model_tiers(&self) -> bool;
}

fn auth_mode_to_strategy(value: &str) -> AuthStrategy {
    match value {
        "none" => AuthStrategy::None,
        "literal" => AuthStrategy::Literal,
        "secret" => AuthStrategy::Secret,
        _ => AuthStrategy::Unknown,
    }
}

fn clears_api_key_for_family(family: &str) -> bool {
    matches!(family, "openrouter" | "local" | "custom_unknown")
}

fn supports_tiers_for_family(family: &str) -> bool {
    !matches!(family, "claude_strict")
}

impl CapabilityProfile for ServiceDescriptor {
    fn auth_strategy(&self) -> AuthStrategy {
        auth_mode_to_strategy(&self.auth_mode)
    }

    fn clears_anthropic_api_key(&self) -> bool {
        clears_api_key_for_family(&self.family)
    }

    fn supports_model_tiers(&self) -> bool {
        supports_tiers_for_family(&self.family)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_descriptor(auth_mode: &str, family: &str) -> ServiceDescriptor {
        ServiceDescriptor {
            id: "test".to_string(),
            display_name: "Test".to_string(),
            description: String::new(),
            category: "test".to_string(),
            family: family.to_string(),
            auth_mode: auth_mode.to_string(),
            key_var: String::new(),
            literal_auth_token: String::new(),
            base_url: String::new(),
            default_model: String::new(),
            model_tiers: HashMap::new(),
            model_choices: vec![],
            test_url: String::new(),
            setup: vec![],
            usage: vec![],
        }
    }

    #[test]
    fn test_auth_mode_mapping() {
        assert_eq!(auth_mode_to_strategy("none"), AuthStrategy::None);
        assert_eq!(auth_mode_to_strategy("literal"), AuthStrategy::Literal);
        assert_eq!(auth_mode_to_strategy("secret"), AuthStrategy::Secret);
        assert_eq!(auth_mode_to_strategy("other"), AuthStrategy::Unknown);
    }

    #[test]
    fn test_secret_provider_capability() {
        let desc = make_descriptor("secret", "openrouter");
        assert_eq!(desc.auth_strategy(), AuthStrategy::Secret);
        assert!(desc.clears_anthropic_api_key());
        assert!(desc.supports_model_tiers());
    }

    #[test]
    fn test_claude_strict_invariants() {
        assert!(!clears_api_key_for_family("claude_strict"));
        assert!(!supports_tiers_for_family("claude_strict"));
    }

    #[test]
    fn test_openrouter_invariants() {
        assert!(clears_api_key_for_family("openrouter"));
        assert!(supports_tiers_for_family("openrouter"));
    }

    #[test]
    fn test_local_family_clears_api_key() {
        assert!(clears_api_key_for_family("local"));
    }

    #[test]
    fn test_none_auth_strategy() {
        let desc = make_descriptor("none", "local");
        assert_eq!(desc.auth_strategy(), AuthStrategy::None);
    }
}
