//! Public cron DTOs and private durable invocation payload.

use serde::{Deserialize, Serialize};

use crate::process::ActorSpawnOptions;

macro_rules! action {
    ($name:ident => $output:ty, $wire_name:literal) => {
        impl rivetkit::Action for $name {
            type Output = $output;
            const NAME: &'static str = $wire_name;
        }
    };
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CronSchedule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub expression: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    pub command: String,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub args: Vec<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub options: ActorSpawnOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_history: Option<i64>,
}

action!(CronSchedule => ActorCronJob, "cron.schedule");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronList {}

action!(CronList => Vec<ActorCronJob>, "cron.list");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronCancel {
    pub name: String,
}

action!(CronCancel => bool, "cron.cancel");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorCronJob {
    pub name: String,
    pub expression: String,
    pub timezone: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub options: ActorSpawnOptions,
    pub config_revision: u64,
    pub next_run_at_ms: i64,
    pub last_run_at_ms: Option<i64>,
    pub max_history: i64,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronLaunchError {
    pub code: String,
    pub message: String,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CronInvoke {
    pub schedule_name: String,
    /// Kept only in RivetKit's durable schedule payload, never in public job
    /// metadata. A raw actor action call cannot forge a scheduled invocation.
    pub invoke_token: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub options: ActorSpawnOptions,
    pub config_revision: u64,
}

action!(CronInvoke => (), "__agentos.cron.invoke");
