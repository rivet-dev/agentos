//! Runtime metrics (stub).

/// Cumulative metric totals (stub).
#[derive(Debug, Clone, Copy, Default)]
pub struct RuntimeMetricTotals {
    /// Input tokens.
    pub input_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
}

impl RuntimeMetricTotals {
    /// Check if empty.
    pub fn is_empty(self) -> bool {
        self.input_tokens == 0 && self.output_tokens == 0
    }

    /// Merge another totals into this one.
    pub fn merge(&mut self, other: Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }
}

/// Runtime metrics summary (stub).
#[derive(Debug, Clone, Copy, Default)]
pub struct RuntimeMetricsSummary {
    /// Input tokens.
    pub input_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
}

impl RuntimeMetricsSummary {
    /// Check if empty.
    pub fn is_empty(self) -> bool {
        self.input_tokens == 0 && self.output_tokens == 0
    }

    /// Merge another summary into this one.
    pub fn merge(&mut self, other: Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }
}
