//! OTel provider (stub — initialization is a no-op).

use crate::config::OtelSettings;
use crate::metrics::MetricsClient;
use std::error::Error;

/// OpenTelemetry provider (stub — all operations are no-ops on WASI).
#[derive(Debug, Clone)]
pub struct OtelProvider;

impl OtelProvider {
    /// Create from settings (stub — always returns None, no provider needed on WASI).
    pub fn from(_settings: &OtelSettings) -> Result<Option<Self>, Box<dyn Error>> {
        Ok(None)
    }

    /// Shut down the provider (stub — no-op).
    pub fn shutdown(&self) {}

    /// Get metrics client (stub — always None).
    pub fn metrics(&self) -> Option<&MetricsClient> {
        None
    }
}
