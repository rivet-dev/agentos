//! Metrics subsystem (stub — all operations are no-ops).

pub mod names;
pub mod runtime_metrics;
pub mod tags;
pub mod timer;

pub use self::timer::Timer;

use std::fmt;
use std::time::Duration;

/// Metrics error (stub).
#[derive(Debug)]
pub enum MetricsError {
    /// Metric name cannot be empty.
    EmptyMetricName,
    /// Metric name contains invalid characters.
    InvalidMetricName { name: String },
    /// Tag component is empty.
    EmptyTagComponent { label: String },
    /// Tag component contains invalid characters.
    InvalidTagComponent { label: String, value: String },
    /// Exporter is disabled.
    ExporterDisabled,
    /// Counter increment must be non-negative.
    NegativeCounterIncrement { name: String, inc: i64 },
    /// Invalid config.
    InvalidConfig { message: String },
}

impl fmt::Display for MetricsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMetricName => write!(f, "metric name cannot be empty"),
            Self::InvalidMetricName { name } => {
                write!(f, "metric name contains invalid characters: {name}")
            }
            Self::EmptyTagComponent { label } => write!(f, "{label} cannot be empty"),
            Self::InvalidTagComponent { label, value } => {
                write!(f, "{label} contains invalid characters: {value}")
            }
            Self::ExporterDisabled => write!(f, "metrics exporter is disabled"),
            Self::NegativeCounterIncrement { name, inc } => {
                write!(f, "counter increment must be non-negative for {name}: {inc}")
            }
            Self::InvalidConfig { message } => {
                write!(f, "invalid metrics configuration: {message}")
            }
        }
    }
}

impl std::error::Error for MetricsError {}

/// Result type for metrics operations.
pub type Result<T> = std::result::Result<T, MetricsError>;

/// Metrics exporter variant (stub).
#[derive(Clone, Debug)]
pub enum MetricsExporter {
    /// OTLP exporter (no-op on WASI).
    Otlp(crate::config::OtelExporter),
}

/// Metrics config (stub).
#[derive(Clone, Debug)]
pub struct MetricsConfig {
    /// Environment name.
    pub environment: String,
    /// Service name.
    pub service_name: String,
    /// Service version.
    pub service_version: String,
}

impl MetricsConfig {
    /// Create an OTLP config (stub).
    pub fn otlp(
        environment: impl Into<String>,
        service_name: impl Into<String>,
        service_version: impl Into<String>,
        _exporter: crate::config::OtelExporter,
    ) -> Self {
        Self {
            environment: environment.into(),
            service_name: service_name.into(),
            service_version: service_version.into(),
        }
    }

    /// Set export interval (stub — no-op).
    pub fn with_export_interval(self, _interval: Duration) -> Self {
        self
    }

    /// Enable runtime reader (stub — no-op).
    pub fn with_runtime_reader(self) -> Self {
        self
    }

    /// Add a default tag (stub — no-op).
    pub fn with_tag(self, _key: impl Into<String>, _value: impl Into<String>) -> Result<Self> {
        Ok(self)
    }
}

/// Metrics client (stub — all operations are no-ops).
#[derive(Clone, Debug)]
pub struct MetricsClient;

impl MetricsClient {
    /// Create a new metrics client (stub — returns immediately).
    pub fn new(_config: MetricsConfig) -> Result<Self> {
        Ok(Self)
    }

    /// Record a counter (stub — no-op).
    pub fn counter(&self, _name: &str, _inc: i64, _tags: &[(&str, &str)]) -> Result<()> {
        Ok(())
    }

    /// Record a histogram (stub — no-op).
    pub fn histogram(&self, _name: &str, _value: i64, _tags: &[(&str, &str)]) -> Result<()> {
        Ok(())
    }

    /// Record a duration (stub — no-op).
    pub fn record_duration(
        &self,
        _name: &str,
        _duration: Duration,
        _tags: &[(&str, &str)],
    ) -> Result<()> {
        Ok(())
    }

    /// Start a timer (stub — always errors).
    pub fn start_timer(&self, _name: &str, _tags: &[(&str, &str)]) -> Result<Timer> {
        Err(MetricsError::ExporterDisabled)
    }

    /// Shut down (stub — no-op).
    pub fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// Get the global metrics client (stub — always None).
pub fn global() -> Option<MetricsClient> {
    None
}
