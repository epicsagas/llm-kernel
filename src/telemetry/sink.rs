//! Telemetry sink trait and implementations.

use crate::telemetry::events::TelemetryEvent;

/// Trait for telemetry event sinks.
pub trait TelemetrySink: Send + Sync {
    /// Track a telemetry event.
    fn track(&mut self, event: TelemetryEvent);

    /// Flush any buffered events.
    fn flush(&mut self) {}
}

/// Console telemetry sink — logs events via `tracing::info!`.
/// Useful for development and debugging.
pub struct ConsoleSink {
    app_name: String,
}

impl ConsoleSink {
    /// Create a new console sink with the given app name.
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
        }
    }
}

impl TelemetrySink for ConsoleSink {
    fn track(&mut self, event: TelemetryEvent) {
        let json = serde_json::to_string(&event).unwrap_or_default();
        tracing::info!(app = %self.app_name, event = %json, "telemetry");
    }
}

/// No-op telemetry sink — discards all events.
/// Use when telemetry is opted out.
pub struct NoopSink;

impl TelemetrySink for NoopSink {
    fn track(&mut self, _event: TelemetryEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::events::FailureClass;

    #[test]
    fn console_sink_does_not_panic() {
        use crate::telemetry::events::ToolName;
        let mut sink = ConsoleSink::new("test");
        sink.track(TelemetryEvent::ToolInvoked {
            tool: ToolName::Search,
        });
        sink.track(TelemetryEvent::Error {
            class: FailureClass::Network,
        });
        sink.flush();
    }

    #[test]
    fn noop_sink_discards() {
        use crate::telemetry::events::ToolName;
        let mut sink = NoopSink;
        sink.track(TelemetryEvent::ToolInvoked {
            tool: ToolName::Search,
        });
        sink.flush();
    }
}
