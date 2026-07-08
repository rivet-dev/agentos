//! Metrics timer (stub).

use crate::metrics::{MetricsError, Result};

/// Timer that records duration on drop.
#[derive(Debug)]
pub struct Timer {
    _private: (),
}

impl Timer {
    /// Record the elapsed duration with additional tags.
    pub fn record(&self, _additional_tags: &[(&str, &str)]) -> Result<()> {
        Err(MetricsError::ExporterDisabled)
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        // Metrics export is disabled in the WASI stub.
    }
}
