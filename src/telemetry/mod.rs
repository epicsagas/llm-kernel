//! Telemetry framework for Rust AI tools.
//!
//! Provides enum-gated event tracking (no free strings, no PII) with
//! adapters for PostHog analytics and Sentry error monitoring.
//!
//! ```
//! use llm_kernel::telemetry::{TelemetryEvent, TelemetrySink, ConsoleSink, ToolName};
//!
//! let mut sink = ConsoleSink::new("my-app");
//! sink.track(TelemetryEvent::ToolInvoked { tool: ToolName::Search });
//! ```

pub mod events;
pub mod sink;

pub use events::{FailureClass, FeatureName, ProviderCategory, TelemetryEvent, ToolName};
pub use sink::{ConsoleSink, TelemetrySink};
