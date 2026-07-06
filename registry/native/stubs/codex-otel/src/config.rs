//! OTel configuration types (stub).

use std::collections::HashMap;
use std::path::PathBuf;

/// OTel settings (stub).
#[derive(Clone, Debug)]
pub struct OtelSettings {
    /// Environment name.
    pub environment: String,
    /// Service name.
    pub service_name: String,
    /// Service version.
    pub service_version: String,
    /// Codex home directory.
    pub codex_home: PathBuf,
    /// Primary exporter.
    pub exporter: OtelExporter,
    /// Trace exporter.
    pub trace_exporter: OtelExporter,
    /// Metrics exporter.
    pub metrics_exporter: OtelExporter,
    /// Enable runtime metrics.
    pub runtime_metrics: bool,
}

/// HTTP protocol for OTLP (stub).
#[derive(Clone, Debug)]
pub enum OtelHttpProtocol {
    /// Binary protobuf.
    Binary,
    /// JSON.
    Json,
}

/// TLS configuration (stub).
#[derive(Clone, Debug, Default)]
pub struct OtelTlsConfig {
    /// CA certificate path.
    pub ca_certificate: Option<PathBuf>,
    /// Client certificate path.
    pub client_certificate: Option<PathBuf>,
    /// Client private key path.
    pub client_private_key: Option<PathBuf>,
}

/// Exporter configuration (stub).
#[derive(Clone, Debug)]
pub enum OtelExporter {
    /// Disabled.
    None,
    /// Statsig (no-op on WASI).
    Statsig,
    /// gRPC OTLP (no-op on WASI).
    OtlpGrpc {
        /// Endpoint URL.
        endpoint: String,
        /// Headers.
        headers: HashMap<String, String>,
        /// TLS config.
        tls: Option<OtelTlsConfig>,
    },
    /// HTTP OTLP (no-op on WASI).
    OtlpHttp {
        /// Endpoint URL.
        endpoint: String,
        /// Headers.
        headers: HashMap<String, String>,
        /// Protocol.
        protocol: OtelHttpProtocol,
        /// TLS config.
        tls: Option<OtelTlsConfig>,
    },
}
