//! Immutable action codec policy; default entrypoints retain their legacy context.
use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionCodecLimits {
    pub message_size_limit: usize,
    pub max_input_depth: usize,
    pub max_input_values: usize,
    pub max_binary_bytes: usize,
    pub(crate) hosted_actor: bool,
}

impl ActionCodecLimits {
    pub fn for_message_size(message_size_limit: usize) -> Self {
        Self {
            message_size_limit,
            max_input_depth: crate::MAX_ACTION_INPUT_DEPTH,
            max_input_values: crate::MAX_ACTION_INPUT_NODES,
            max_binary_bytes: crate::MAX_BINARY_BYTES,
            hosted_actor: false,
        }
    }

    pub fn for_hosted_actor(
        message_size_limit: usize,
        max_input_depth: usize,
        max_input_values: usize,
        max_binary_bytes: usize,
    ) -> Result<Self> {
        for (setting, value) in [
            ("transport.maxMessageBytes", message_size_limit),
            ("codec.maxInputDepth", max_input_depth),
            ("codec.maxInputValues", max_input_values),
            ("codec.maxBinaryBytes", max_binary_bytes),
        ] {
            if value == 0 {
                bail!("invalid_input: --actor-budgets-json {setting} must be positive");
            }
        }
        if message_size_limit <= crate::ACTION_ENVELOPE_RESERVE_BYTES {
            bail!("invalid_input: --actor-budgets-json transport.maxMessageBytes must exceed the {}-byte reserved action envelope", crate::ACTION_ENVELOPE_RESERVE_BYTES);
        }
        max_binary_bytes.div_ceil(3).checked_mul(4).ok_or_else(||
            anyhow::anyhow!("invalid_input: --actor-budgets-json codec.maxBinaryBytes base64 expansion exceeds platform usize"))?;
        Ok(Self {
            message_size_limit,
            max_input_depth,
            max_input_values,
            max_binary_bytes,
            hosted_actor: true,
        })
    }
}

/// Typed internal diagnostic; the public wire error remains `limit_exceeded`.
#[doc(hidden)]
#[derive(Debug)]
pub struct ActionBudgetViolation {
    pub setting: &'static str,
    pub observed: usize,
    pub limit: usize,
    pub units: &'static str,
    pub reason: String,
}
impl std::fmt::Display for ActionBudgetViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "limit_exceeded: {}: observed {} {}, limit {} {}; {}; ",
            self.setting, self.observed, self.units, self.limit, self.units, self.reason
        )?;
        f.write_str(&self.remediation())
    }
}
impl ActionBudgetViolation {
    pub(crate) fn remediation(&self) -> String {
        if let Some(field) = self.setting.strip_prefix("config.preview.") {
            format!("raise config.preview.{field}; if the desired value exceeds the worker preview.{field} ceiling, also raise preview.{field} in --actor-budgets-json and restart this worker")
        } else if self.setting == "maxBytes" {
            "raise per-call maxBytes within filesystem.maxTransferBytes or reduce the transfer"
                .into()
        } else {
            format!("lower the per-call request or raise {} in --actor-budgets-json and restart this worker",
                self.setting.strip_prefix("actor.").unwrap_or(self.setting))
        }
    }
}
impl std::error::Error for ActionBudgetViolation {}

#[doc(hidden)]
pub fn warn_near_budget(setting: &'static str, observed: usize, limit: usize, units: &'static str) {
    if observed >= limit.saturating_sub(limit / 5) {
        tracing::warn!(setting, observed, limit, units,
            "hosted actor budget nearing capacity; adjust the named per-call limit or raise its actor worker ceiling in --actor-budgets-json and restart this worker");
    }
}
