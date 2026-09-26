//! Actor creation, configuration, and VM lifecycle contract types.

use std::str::FromStr;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{AgentOsActorConfig, AgentOsActorConfigInput};

macro_rules! action {
    ($name:ident => $output:ty, $wire_name:literal) => {
        impl rivetkit::Action for $name {
            type Output = $output;
            const NAME: &'static str = $wire_name;
        }
    };
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentOsActorCreateInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<AgentOsActorConfigInput>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigApplyState {
    Applying,
    Ready,
    RestartRequired,
    Failed,
}

impl ConfigApplyState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applying => "applying",
            Self::Ready => "ready",
            Self::RestartRequired => "restart_required",
            Self::Failed => "failed",
        }
    }
}

impl FromStr for ConfigApplyState {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "applying" => Ok(Self::Applying),
            "ready" => Ok(Self::Ready),
            "restart_required" => Ok(Self::RestartRequired),
            "failed" => Ok(Self::Failed),
            _ => bail!("invalid actor config apply state {value:?}"),
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VmLifecycleState {
    Initializing,
    Preloading,
    Booting,
    Ready,
    Degraded,
    Stopping,
    Failed,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmIssue {
    pub code: String,
    pub message: String,
    pub at_ms: i64,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageStartupStatus {
    pub required_total: u32,
    pub required_ready: u32,
    pub optional_preload_total: u32,
    pub optional_preload_ready: u32,
    pub optional_preload_failed: u32,
    pub optional_preload_skipped: u32,
    pub optional_preload_warmed_bytes: u64,
    pub optional_preload_deadline_hit: bool,
    pub optional_preload_coordinator_available: bool,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreSidecarStatus {
    pub state: String,
    pub active_vm_count: u32,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmStatusSnapshot {
    pub lifecycle: VmLifecycleState,
    pub config_state: ConfigApplyState,
    pub desired_config_revision: u64,
    pub applied_config_revision: Option<u64>,
    pub generation: u64,
    pub last_boot_at_ms: Option<i64>,
    pub last_shutdown_at_ms: Option<i64>,
    pub packages: PackageStartupStatus,
    pub issues: Vec<VmIssue>,
    pub core: Option<CoreSidecarStatus>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSnapshot {
    pub revision: u64,
    pub desired: AgentOsActorConfig,
    pub applied_revision: Option<u64>,
    #[serde(rename = "state")]
    pub status: ConfigApplyState,
    pub issues: Vec<VmIssue>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOsActorState {
    pub config: ConfigSnapshot,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigGet {}

action!(ConfigGet => ConfigSnapshot, "config.get");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigSet {
    pub config: AgentOsActorConfigInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

action!(ConfigSet => ConfigSnapshot, "config.set");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigPatch {
    pub patch: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

action!(ConfigPatch => ConfigSnapshot, "config.patch");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmStatus {}

action!(VmStatus => VmStatusSnapshot, "vm.status");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmRestart {}

action!(VmRestart => VmStatusSnapshot, "vm.restart");
