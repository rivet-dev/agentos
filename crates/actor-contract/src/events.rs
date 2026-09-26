//! Bounded public actor event payloads.

use rivetkit::Event;
use serde::{Deserialize, Serialize};

use crate::cron::CronLaunchError;
use crate::filesystem::FileBytes;
use crate::process::{ActorExitStatus, ActorProcessId, ActorTerminalId};

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmBooted {
    pub generation: u64,
    pub config_revision: u64,
    pub booted_at_ms: i64,
}

impl Event for VmBooted {
    const NAME: &'static str = "vm.booted";
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmShutdown {
    pub generation: u64,
    pub reason: String,
    pub shutdown_at_ms: i64,
}

impl Event for VmShutdown {
    const NAME: &'static str = "vm.shutdown";
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmLimitWarning {
    pub limit: String,
    pub observed: u64,
    pub capacity: u64,
    pub message: String,
}

impl Event for VmLimitWarning {
    const NAME: &'static str = "vm.limitWarning";
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessOutputEvent {
    pub process: ActorProcessId,
    pub sequence: u64,
    pub stream: agentos_client::ProcessStream,
    pub data: FileBytes,
    pub timestamp_ms: i64,
}

impl Event for ProcessOutputEvent {
    const NAME: &'static str = "process.output";
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessExitEvent {
    pub process: ActorProcessId,
    pub status: ActorExitStatus,
}

impl Event for ProcessExitEvent {
    const NAME: &'static str = "process.exit";
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputEvent {
    pub terminal: ActorTerminalId,
    pub sequence: u64,
    pub stream: agentos_client::ProcessStream,
    pub data: FileBytes,
    pub timestamp_ms: i64,
}

impl Event for TerminalOutputEvent {
    const NAME: &'static str = "terminal.output";
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalExitEvent {
    pub terminal: ActorTerminalId,
    pub status: ActorExitStatus,
}

impl Event for TerminalExitEvent {
    const NAME: &'static str = "terminal.exit";
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronFiredEvent {
    pub schedule_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<ActorProcessId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<CronLaunchError>,
    pub fired_at_ms: i64,
}

impl Event for CronFiredEvent {
    const NAME: &'static str = "cron.fired";
}
