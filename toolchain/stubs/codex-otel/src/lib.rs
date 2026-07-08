//! WASM-compatible stub for codex-otel.
//!
//! On WASI, telemetry and metrics export over the network is unnecessary.
//! All initialization is a no-op, all metrics are silently dropped, and
//! all tracing/context functions return None or no-op.

pub mod config;
pub mod metrics;
pub mod provider;
pub mod trace_context;

use std::fmt;
use std::time::Duration;

pub use crate::metrics::MetricsError;
pub use crate::metrics::Result as MetricsResult;
pub use crate::metrics::runtime_metrics::RuntimeMetricTotals;
pub use crate::metrics::runtime_metrics::RuntimeMetricsSummary;
pub use crate::metrics::timer::Timer;
pub use crate::provider::OtelProvider;
pub use crate::trace_context::context_from_w3c_trace_context;
pub use crate::trace_context::current_span_trace_id;
pub use crate::trace_context::current_span_w3c_trace_context;
pub use crate::trace_context::set_parent_from_context;
pub use crate::trace_context::set_parent_from_w3c_trace_context;
pub use crate::trace_context::span_w3c_trace_context;
pub use crate::trace_context::traceparent_context_from_env;

/// Sanitize a metric tag value (stub — returns input unchanged).
pub fn sanitize_metric_tag_value(value: &str) -> String {
    value.to_string()
}

/// Tool decision source (stub).
#[derive(Debug, Clone)]
pub enum ToolDecisionSource {
    /// Automated reviewer.
    AutomatedReviewer,
    /// Config.
    Config,
    /// User.
    User,
}

impl fmt::Display for ToolDecisionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AutomatedReviewer => write!(f, "automated_reviewer"),
            Self::Config => write!(f, "config"),
            Self::User => write!(f, "user"),
        }
    }
}

/// Telemetry auth mode (stub).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryAuthMode {
    /// API key authentication.
    ApiKey,
    /// ChatGPT authentication.
    Chatgpt,
}

impl fmt::Display for TelemetryAuthMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey => write!(f, "ApiKey"),
            Self::Chatgpt => write!(f, "Chatgpt"),
        }
    }
}

/// Start a metrics timer using the globally installed metrics client (stub — always errors).
pub fn start_global_timer(_name: &str, _tags: &[(&str, &str)]) -> MetricsResult<Timer> {
    Err(MetricsError::ExporterDisabled)
}

// ---- AuthEnvTelemetryMetadata ----

/// Auth environment telemetry metadata (stub).
#[derive(Debug, Clone, Default)]
pub struct AuthEnvTelemetryMetadata {
    /// Auth mode.
    pub auth_mode: Option<String>,
}

// ---- SessionTelemetryMetadata ----

/// Session telemetry metadata (stub).
#[derive(Debug, Clone, Default)]
pub struct SessionTelemetryMetadata;

// ---- SessionTelemetry ----

/// Session telemetry (stub — all operations are no-ops).
#[derive(Debug, Clone, Default)]
pub struct SessionTelemetry;

impl SessionTelemetry {
    /// Create a new SessionTelemetry (stub).
    pub fn new() -> Self {
        Self
    }

    /// Set auth env metadata (stub — no-op).
    pub fn with_auth_env(self, _auth_env: AuthEnvTelemetryMetadata) -> Self {
        self
    }

    /// Set model info (stub — no-op).
    pub fn with_model(self, _model: &str, _slug: &str) -> Self {
        self
    }

    /// Set metrics service name (stub — no-op).
    pub fn with_metrics_service_name(self, _service_name: &str) -> Self {
        self
    }

    /// Set metrics client (stub — no-op).
    pub fn with_metrics(self, _metrics: crate::metrics::MetricsClient) -> Self {
        self
    }

    /// Set metrics client without metadata tags (stub — no-op).
    pub fn with_metrics_without_metadata_tags(
        self,
        _metrics: crate::metrics::MetricsClient,
    ) -> Self {
        self
    }

    /// Set metrics config (stub — no-op).
    pub fn with_metrics_config(self, _config: crate::metrics::MetricsConfig) -> MetricsResult<Self> {
        Ok(self)
    }

    /// Set provider metrics (stub — no-op).
    pub fn with_provider_metrics(self, _provider: &OtelProvider) -> Self {
        self
    }

    /// Record a counter (stub — no-op).
    pub fn counter(&self, _name: &str, _inc: i64, _tags: &[(&str, &str)]) {}

    /// Record a histogram (stub — no-op).
    pub fn histogram(&self, _name: &str, _value: i64, _tags: &[(&str, &str)]) {}

    /// Record a duration (stub — no-op).
    pub fn record_duration(&self, _name: &str, _duration: Duration, _tags: &[(&str, &str)]) {}

    /// Start a timer (stub — always errors).
    pub fn start_timer(&self, _name: &str, _tags: &[(&str, &str)]) -> Result<Timer, MetricsError> {
        Err(MetricsError::ExporterDisabled)
    }

    /// Shut down metrics (stub — no-op).
    pub fn shutdown_metrics(&self) -> MetricsResult<()> {
        Ok(())
    }

    /// Reset runtime metrics (stub — no-op).
    pub fn reset_runtime_metrics(&self) {}

    /// Get runtime metrics summary (stub — always None).
    pub fn runtime_metrics_summary(&self) -> Option<RuntimeMetricsSummary> {
        None
    }
}
