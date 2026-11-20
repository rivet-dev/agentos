use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Immutable validation/persistence policy shared by preload producers and the
/// coordinator in this process. Configure before process preload or registry use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct PreloadValidationPolicy {
    pub max_url_bytes: usize,
    pub max_process_id_bytes: usize,
    pub max_supported_formats: usize,
    pub max_observation_window_ms: u64,
    pub max_observation_clock_skew_ms: u64,
    pub max_usage_count_per_observation: u64,
    pub max_candidate_score: u64,
    pub max_state_bytes: usize,
    pub max_artifact_bytes: u64,
}

impl Default for PreloadValidationPolicy {
    fn default() -> Self {
        Self {
            max_url_bytes: 4096,
            max_process_id_bytes: 128,
            max_supported_formats: 8,
            max_observation_window_ms: 24 * 60 * 60 * 1000,
            max_observation_clock_skew_ms: 5 * 60 * 1000,
            max_usage_count_per_observation: 1_000_000,
            max_candidate_score: 1_000_000_000_000,
            max_state_bytes: 16 * 1024 * 1024,
            max_artifact_bytes: 64 * 1024 * 1024 * 1024,
        }
    }
}

impl PreloadValidationPolicy {
    pub fn validate(&self) -> Result<()> {
        for (field, value) in [
            ("maxUrlBytes", self.max_url_bytes as u128),
            ("maxProcessIdBytes", self.max_process_id_bytes as u128),
            ("maxSupportedFormats", self.max_supported_formats as u128),
            (
                "maxObservationWindowMs",
                u128::from(self.max_observation_window_ms),
            ),
            (
                "maxUsageCountPerObservation",
                u128::from(self.max_usage_count_per_observation),
            ),
            ("maxCandidateScore", u128::from(self.max_candidate_score)),
            ("maxStateBytes", self.max_state_bytes as u128),
            ("maxArtifactBytes", u128::from(self.max_artifact_bytes)),
        ] {
            if value == 0 {
                bail!("preloadPolicy.{field} must be positive; set it in --preload-policy-json before startup");
            }
        }
        i64::try_from(self.max_observation_window_ms).map_err(|_| {
            anyhow::anyhow!(
                "preloadPolicy.maxObservationWindowMs exceeds signed timestamp representation"
            )
        })?;
        // Zero intentionally means no tolerance for future observations.
        i64::try_from(self.max_observation_clock_skew_ms).map_err(|_| {
            anyhow::anyhow!(
                "preloadPolicy.maxObservationClockSkewMs exceeds signed timestamp representation"
            )
        })?;
        Ok(())
    }

    pub(super) fn check(&self, field: &'static str, observed: u64, maximum: u64) -> Result<()> {
        if observed > maximum {
            return Err(PreloadLimitViolation::new(
                format!("preloadPolicy.{field}"), observed, maximum,
                if field.ends_with("Bytes") { "bytes" } else if field.ends_with("Ms") { "milliseconds" } else if field == "maxCandidateScore" { "score" } else { "count" },
                format!("raise {field} in --preload-policy-json before startup on producer/coordinator workers or reduce the input"),
            )?.into());
        }
        self.warn(field, observed, maximum);
        Ok(())
    }

    pub(super) fn warn(&self, field: &str, observed: u64, maximum: u64) {
        if observed >= maximum - maximum / 5 {
            tracing::warn!(configuration_path=%format!("preloadPolicy.{field}"),observed,capacity=maximum,
                "preload validation policy approaching limit; configure the named field in --preload-policy-json before startup or reduce input");
        }
    }
}

static VALIDATION_POLICY: OnceLock<PreloadValidationPolicy> = OnceLock::new();

pub fn configure_preload_validation_policy(policy: PreloadValidationPolicy) -> Result<()> {
    configure_in_slot(&VALIDATION_POLICY, policy)
}

pub(super) fn configure_in_slot(
    slot: &OnceLock<PreloadValidationPolicy>,
    policy: PreloadValidationPolicy,
) -> Result<()> {
    policy.validate()?;
    match slot.set(policy) {
        Ok(())=>Ok(()),
        Err(policy) if slot.get()==Some(&policy)=>Ok(()),
        Err(_)=>bail!("preload validation policy is already frozen differently; configure --preload-policy-json before preload/registry/config validation, or restart this worker; no live policy update is supported"),
    }
}

pub(super) fn validation_policy() -> &'static PreloadValidationPolicy {
    VALIDATION_POLICY.get_or_init(PreloadValidationPolicy::default)
}

/// The original owning error remains in the chain, with the existing framework
/// public error as its source. RivetError::extract therefore preserves typed
/// quantities without parsing Display strings or importing the actor crate.
#[derive(Debug)]
pub(super) struct PreloadLimitViolation {
    pub(super) setting: String,
    pub(super) observed: u64,
    pub(super) maximum: u64,
    pub(super) units: &'static str,
    pub(super) remediation: String,
    public: rivet_error::RivetError,
}
impl PreloadLimitViolation {
    pub(super) fn new(
        setting: String,
        observed: u64,
        maximum: u64,
        units: &'static str,
        remediation: String,
    ) -> Result<Self> {
        let message = format!(
            "limit_exceeded: {setting} observed={observed} maximum={maximum}; {remediation}"
        );
        let metadata=serde_json::value::to_raw_value(&serde_json::json!({"budget":{
            "setting":setting,"observed":observed,"limit":maximum,"units":units,"remediation":remediation,
        }})).context("serialize typed preload limit metadata")?;
        let public = rivet_error::RivetError {
            kind: rivet_error::RivetErrorKind::Dynamic {
                group: "user".into(),
                code: "limit_exceeded".into(),
                default_message: message.clone(),
            },
            meta: Some(metadata),
            message: Some(message),
            actor: None,
        };
        Ok(Self {
            setting,
            observed,
            maximum,
            units,
            remediation,
            public,
        })
    }
}
impl std::fmt::Display for PreloadLimitViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "limit_exceeded: {} observed={} maximum={} {}; {}",
            self.setting, self.observed, self.maximum, self.units, self.remediation
        )
    }
}
impl std::error::Error for PreloadLimitViolation {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.public)
    }
}
