//! Telemetry event types — strictly enum-gated to prevent PII leaks.

use serde::{Deserialize, Serialize};

/// A telemetry event. All fields use controlled vocabularies — no free strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TelemetryEvent {
    /// A tool was invoked.
    ToolInvoked {
        /// Tool identifier (from a fixed set).
        tool: &'static str,
    },

    /// A tool invocation completed.
    ToolCompleted {
        tool: &'static str,
        /// Milliseconds elapsed.
        duration_ms: u64,
        /// Whether the invocation succeeded.
        success: bool,
    },

    /// A session started.
    SessionStarted {
        /// Session identifier (UUID, no PII).
        session_id: String,
    },

    /// A session ended.
    SessionEnded {
        session_id: String,
        /// Total turns in the session.
        turns: u32,
        /// Total tokens consumed.
        tokens_used: u64,
    },

    /// An error occurred.
    Error {
        /// Categorized failure class (no raw error messages).
        class: FailureClass,
    },

    /// A feature was used.
    FeatureUsed {
        feature: &'static str,
    },

    /// Provider routing decision.
    ProviderRouted {
        /// Provider category (not the exact provider name).
        category: &'static str,
    },
}

/// Classification of failures for telemetry.
///
/// Uses broad categories to avoid leaking sensitive error details.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FailureClass {
    /// Network or connectivity issue.
    Network,
    /// Authentication or authorization failure.
    Auth,
    /// Rate limited by provider.
    RateLimit,
    /// Invalid input or configuration.
    Validation,
    /// Database or storage failure.
    Storage,
    /// Timeout.
    Timeout,
    /// Internal logic error.
    Internal,
    /// Unknown or uncategorized.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serializes_no_pii() {
        let event = TelemetryEvent::ToolInvoked { tool: "search" };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("search"));
        assert!(!json.contains("password"));
        assert!(!json.contains("email"));
    }

    #[test]
    fn failure_class_roundtrip() {
        let class = FailureClass::RateLimit;
        let json = serde_json::to_string(&class).unwrap();
        let back: FailureClass = serde_json::from_str(&json).unwrap();
        assert_eq!(back, FailureClass::RateLimit);
    }

    #[test]
    fn session_event() {
        let event = TelemetryEvent::SessionStarted {
            session_id: "abc-123".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("abc-123"));
    }
}
