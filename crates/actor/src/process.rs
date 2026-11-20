use crate::budgets::{HostedActorBudgets, ValidationBound};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use agentos_client::{
    ExecOptions, OpenShellOptions, ProcessStatus, ProcessTreeNode, SpawnOptions, SpawnStdio,
    StdinInput,
};
use anyhow::{bail, Result};
use rivetkit::{Ctx, Handler};
use serde::{Deserialize, Serialize};

use crate::events::{ProcessExitEvent, ProcessOutputEvent, TerminalExitEvent, TerminalOutputEvent};
use crate::{AgentOsActor, FileBytes, FileContentInput};

#[cfg(test)]
const MAX_ARGUMENTS: usize = 1_024;
#[cfg(test)]
const MAX_ARGUMENT_BYTES: usize = 16 * 1024;
#[cfg(test)]
const MAX_WAIT_MS: u64 = 5 * 60 * 1_000;
const DEFAULT_WAIT_MS: u64 = 30 * 1_000;

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorProcessId {
    pub generation: u64,
    pub pid: u32,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorTerminalId {
    pub generation: u64,
    pub shell_id: String,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorExecOptions {
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<FileContentInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_stdio: Option<bool>,
}

impl ActorExecOptions {
    fn validate(&self, budgets: &HostedActorBudgets) -> Result<()> {
        validate_environment(&self.env, budgets)?;
        if let Some(cwd) = &self.cwd {
            validate_string("process cwd", cwd, budgets.execution.max_argument_bytes)?;
        }
        if let Some(stdin) = &self.stdin {
            validate_bytes(
                "process stdin",
                stdin.byte_len(),
                budgets.execution.max_stdin_bytes,
            )?;
        }
        if self.timeout_ms == Some(0) {
            bail!("invalid_input: process timeoutMs must be greater than zero");
        }
        if let Some(timeout) = self.timeout_ms {
            budgets.process.max_wait_ms.check(
                "process timeoutMs",
                usize::try_from(timeout).map_err(|_| {
                    anyhow::anyhow!("invalid_input: timeoutMs exceeds platform representation")
                })?,
            )?;
        }
        Ok(())
    }

    fn into_core(self, budgets: &HostedActorBudgets) -> ExecOptions {
        ExecOptions {
            env: self.env,
            cwd: self.cwd,
            stdin: self.stdin.map(file_content_to_stdin),
            timeout: Some(
                self.timeout_ms
                    .unwrap_or(budgets.process.max_wait_ms.maximum as u64) as f64,
            ),
            capture_stdio: self.capture_stdio,
            ..ExecOptions::default()
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorExecResult {
    pub status: ActorExitStatus,
    pub stdout: String,
    pub stderr: String,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessRun {
    pub command: String,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub args: Vec<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub options: ActorExecOptions,
}

crate::register_action!(ProcessRun => ActorExecResult, "process.run");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorSpawnOptions {
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdio: Option<SpawnStdio>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin_fd: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_fd: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_fd: Option<i32>,
}

impl ActorSpawnOptions {
    pub(crate) fn validate(&self, budgets: &HostedActorBudgets) -> Result<()> {
        validate_environment(&self.env, budgets)?;
        if let Some(cwd) = &self.cwd {
            validate_string("process cwd", cwd, budgets.execution.max_argument_bytes)?;
        }
        Ok(())
    }

    fn into_core(self) -> SpawnOptions {
        SpawnOptions {
            wasm_backend: None,
            env: self.env,
            cwd: self.cwd,
            stdio: self.stdio,
            stdin_fd: self.stdin_fd,
            stdout_fd: self.stdout_fd,
            stderr_fd: self.stderr_fd,
            stream_stdin: Some(true),
            retain_output: true,
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSpawn {
    pub command: String,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub args: Vec<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub options: ActorSpawnOptions,
}

crate::register_action!(ProcessSpawn => ActorProcessId, "process.spawn");

macro_rules! process_id_action {
    ($name:ident, $output:ty, $wire_name:literal) => {
        #[cfg_attr(feature = "contract", derive(ts_rs::TS))]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            pub process: ActorProcessId,
        }

        crate::register_action!($name => $output, $wire_name);
    };
}

process_id_action!(ProcessGet, ActorProcessInfo, "process.get");
process_id_action!(ProcessWait, ActorProcessExit, "process.wait");
process_id_action!(ProcessStdinClose, (), "process.stdin.close");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorProcessInfo {
    pub process: ActorProcessId,
    pub command: String,
    pub args: Vec<String>,
    pub running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<ActorExitStatus>,
    pub started_at_ms: i64,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorProcessExit {
    pub process: ActorProcessId,
    pub status: ActorExitStatus,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessList {}

crate::register_action!(ProcessList => Vec<ActorProcessInfo>, "process.list");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessTree {}

crate::register_action!(ProcessTree => ActorProcessTree, "process.tree");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorProcessTree {
    pub generation: u64,
    pub roots: Vec<ActorProcessTreeNode>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorProcessTreeNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<ActorProcessId>,
    pub pid: u32,
    pub ppid: u32,
    pub pgid: u32,
    pub sid: u32,
    pub driver: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub status: ProcessStatus,
    pub exit: Option<ActorExitStatus>,
    pub start_time_ms: f64,
    pub exit_time_ms: Option<f64>,
    pub children: Vec<ActorProcessTreeNode>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActorSignal {
    #[serde(rename = "SIGTERM")]
    Term,
    #[serde(rename = "SIGINT")]
    Interrupt,
    #[serde(rename = "SIGKILL")]
    Kill,
}

impl ActorSignal {
    fn as_str(self) -> &'static str {
        match self {
            Self::Term => "SIGTERM",
            Self::Interrupt => "SIGINT",
            Self::Kill => "SIGKILL",
        }
    }
}

/// Observed terminal signal. Request permissions continue to use `ActorSignal`.
#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[cfg_attr(feature = "contract", ts(type = "string"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorObservedSignal(u32);

const OBSERVED_SIGNAL_NAMES: [&str; 31] = [
    "SIGHUP",
    "SIGINT",
    "SIGQUIT",
    "SIGILL",
    "SIGTRAP",
    "SIGABRT",
    "SIGBUS",
    "SIGFPE",
    "SIGKILL",
    "SIGUSR1",
    "SIGSEGV",
    "SIGUSR2",
    "SIGPIPE",
    "SIGALRM",
    "SIGTERM",
    "SIGSTKFLT",
    "SIGCHLD",
    "SIGCONT",
    "SIGSTOP",
    "SIGTSTP",
    "SIGTTIN",
    "SIGTTOU",
    "SIGURG",
    "SIGXCPU",
    "SIGXFSZ",
    "SIGVTALRM",
    "SIGPROF",
    "SIGWINCH",
    "SIGIO",
    "SIGPWR",
    "SIGSYS",
];

impl TryFrom<u32> for ActorObservedSignal {
    type Error = anyhow::Error;
    fn try_from(value: u32) -> Result<Self> {
        if !(1..=64).contains(&value) {
            bail!("invalid_input: observed exitSignal must be in 1..=64, received {value}");
        }
        Ok(Self(value))
    }
}

impl ActorObservedSignal {
    pub fn number(self) -> u32 {
        self.0
    }
}

impl Serialize for ActorObservedSignal {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        match OBSERVED_SIGNAL_NAMES.get((self.0 - 1) as usize) {
            Some(name) => serializer.serialize_str(name),
            None => serializer.serialize_str(&self.0.to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for ActorObservedSignal {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if let Some(index) = OBSERVED_SIGNAL_NAMES.iter().position(|name| *name == value) {
            return Ok(Self(index as u32 + 1));
        }
        // Numeric strings are the canonical representation only for 32..=64.
        let number = value.parse::<u32>().map_err(serde::de::Error::custom)?;
        if !(32..=64).contains(&number) || value != number.to_string() {
            return Err(serde::de::Error::custom("invalid observed signal name"));
        }
        Ok(Self(number))
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorExitStatus {
    pub exit_code: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<ActorObservedSignal>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSignal {
    pub process: ActorProcessId,
    pub signal: ActorSignal,
}

crate::register_action!(ProcessSignal => (), "process.signal");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessStdinWrite {
    pub process: ActorProcessId,
    pub data: FileContentInput,
}

crate::register_action!(ProcessStdinWrite => (), "process.stdin.write");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessPtyResize {
    pub process: ActorProcessId,
    pub cols: u16,
    pub rows: u16,
}

crate::register_action!(ProcessPtyResize => (), "process.pty.resize");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessOutputRead {
    pub process: ActorProcessId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_events: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

crate::register_action!(ProcessOutputRead => ActorOutputReplay, "process.output.read");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorOutputReplay {
    pub generation: u64,
    pub events: Vec<ActorOutputEvent>,
    pub next_cursor: Option<u64>,
    pub has_more: bool,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<ActorExitStatus>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorOutputEvent {
    pub sequence: u64,
    pub stream: agentos_client::ProcessStream,
    pub data: FileBytes,
    pub timestamp_ms: i64,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorTerminalOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub args: Vec<String>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cols: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u16>,
}

impl ActorTerminalOptions {
    fn validate(&self, budgets: &HostedActorBudgets) -> Result<()> {
        if let Some(command) = &self.command {
            validate_command(command, budgets)?;
        }
        validate_arguments(&self.args, budgets)?;
        validate_environment(&self.env, budgets)?;
        if let Some(cwd) = &self.cwd {
            validate_string("terminal cwd", cwd, budgets.execution.max_argument_bytes)?;
        }
        validate_dimensions(self.cols, self.rows, budgets)?;
        Ok(())
    }

    fn into_core(self) -> OpenShellOptions {
        OpenShellOptions {
            wasm_backend: None,
            command: self.command,
            args: self.args,
            env: self.env,
            cwd: self.cwd,
            cols: self.cols,
            rows: self.rows,
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalOpen {
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub options: ActorTerminalOptions,
}

crate::register_action!(TerminalOpen => ActorTerminalId, "terminal.open");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalList {}

crate::register_action!(TerminalList => Vec<ActorTerminalInfo>, "terminal.list");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorTerminalInfo {
    pub terminal: ActorTerminalId,
    pub pid: u32,
    pub running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<ActorExitStatus>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalOutputRead {
    pub terminal: ActorTerminalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_events: Option<usize>,
}

crate::register_action!(TerminalOutputRead => ActorOutputReplay, "terminal.output.read");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalStdinWrite {
    pub terminal: ActorTerminalId,
    pub data: FileContentInput,
}

crate::register_action!(TerminalStdinWrite => (), "terminal.stdin.write");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalPtyResize {
    pub terminal: ActorTerminalId,
    pub cols: u16,
    pub rows: u16,
}

crate::register_action!(TerminalPtyResize => (), "terminal.pty.resize");

macro_rules! terminal_id_action {
    ($name:ident, $output:ty, $wire_name:literal) => {
        #[cfg_attr(feature = "contract", derive(ts_rs::TS))]
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            pub terminal: ActorTerminalId,
        }

        crate::register_action!($name => $output, $wire_name);
    };
}

terminal_id_action!(TerminalWait, ActorTerminalExit, "terminal.wait");
terminal_id_action!(TerminalClose, (), "terminal.close");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorTerminalExit {
    pub terminal: ActorTerminalId,
    pub status: ActorExitStatus,
}

impl Handler<ProcessRun> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: ProcessRun,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorExecResult>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_command(&action.command, budgets)?;
            validate_arguments(&action.args, budgets)?;
            action.options.validate(budgets)?;
            // Acquiring the ready VM handle is part of the caller's run budget. Reserve the Core
            // cleanup window below the actor's six-minute action deadline.
            let deadline = tokio::time::Instant::now()
                .checked_add(Duration::from_millis(
                    action
                        .options
                        .timeout_ms
                        .unwrap_or(budgets.process.max_wait_ms.maximum as u64),
                ))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "invalid_input: process timeoutMs exceeds host deadline representation"
                    )
                })?;
            let vm = tokio::time::timeout_at(deadline, self.runtime.vm())
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "timeout: process.run exceeded its deadline before execution started"
                    )
                })??;
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                bail!("timeout: process.run exceeded its deadline before execution started");
            }
            let mut options = action.options.into_core(budgets);
            options.timeout = Some(remaining.as_secs_f64() * 1_000.0);
            let result = vm
                .exec_argv_process(&action.command, &action.args, options)
                .await?;
            actor_exec_result(result, budgets)
        })
    }
}

impl Handler<ProcessSpawn> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        action: ProcessSpawn,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorProcessId>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_command(&action.command, budgets)?;
            validate_arguments(&action.args, budgets)?;
            action.options.validate(budgets)?;
            let status = self.runtime.status().await;
            let vm = self.runtime.vm_at_generation(status.generation).await?;
            let handle = vm.spawn(&action.command, action.args, action.options.into_core())?;
            let process = ActorProcessId {
                generation: status.generation,
                pid: handle.pid,
            };
            attach_process_events(&vm, ctx, process)?;
            Ok(process)
        })
    }
}

impl Handler<ProcessGet> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: ProcessGet,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorProcessInfo>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let info = self
                .runtime
                .vm_at_generation(action.process.generation)
                .await?
                .get_process(action.process.pid)?;
            actor_process_info(action.process.generation, info)
        })
    }
}

impl Handler<ProcessList> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        _action: ProcessList,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<ActorProcessInfo>>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            let status = self.runtime.status().await;
            let entries = self
                .runtime
                .vm_at_generation(status.generation)
                .await?
                .list_processes();
            validate_count(
                "process.list result",
                entries.len(),
                budgets.process.max_process_list_entries,
            )?;
            entries
                .into_iter()
                .map(|info| actor_process_info(status.generation, info))
                .collect()
        })
    }
}

impl Handler<ProcessTree> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        _action: ProcessTree,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorProcessTree>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            let status = self.runtime.status().await;
            let vm = self.runtime.vm_at_generation(status.generation).await?;
            let roots = vm.process_tree().await?;
            validate_count(
                "process.tree result",
                count_process_tree_nodes(&roots),
                budgets.process.max_process_list_entries,
            )?;
            Ok(ActorProcessTree {
                generation: status.generation,
                roots: roots
                    .into_iter()
                    .map(|node| actor_process_tree_node(node, status.generation))
                    .collect::<Result<_>>()?,
            })
        })
    }
}

impl Handler<ProcessWait> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: ProcessWait,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorProcessExit>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let vm = self
                .runtime
                .vm_at_generation(action.process.generation)
                .await?;
            let exit = tokio::time::timeout(
                Duration::from_millis(DEFAULT_WAIT_MS),
                vm.wait_process_exit(action.process.pid),
            )
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "timeout: process.wait exceeded {DEFAULT_WAIT_MS}ms; call again to continue waiting"
                )
            })??;
            Ok(ActorProcessExit {
                process: action.process,
                status: observed_exit_status(exit.exit_code, exit.exit_signal)?,
            })
        })
    }
}

impl Handler<ProcessSignal> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: ProcessSignal,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            self.runtime
                .vm_at_generation(action.process.generation)
                .await?
                .signal_process_awaited(action.process.pid, action.signal.as_str())
                .await?;
            Ok(())
        })
    }
}

impl Handler<ProcessStdinWrite> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: ProcessStdinWrite,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_bytes(
                "process stdin",
                action.data.byte_len(),
                budgets.execution.max_stdin_bytes,
            )?;
            self.runtime
                .vm_at_generation(action.process.generation)
                .await?
                .write_process_stdin(action.process.pid, file_content_to_stdin(action.data))
                .await?;
            Ok(())
        })
    }
}

impl Handler<ProcessStdinClose> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: ProcessStdinClose,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            self.runtime
                .vm_at_generation(action.process.generation)
                .await?
                .close_process_stdin(action.process.pid)
                .await?;
            Ok(())
        })
    }
}

impl Handler<ProcessPtyResize> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: ProcessPtyResize,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_dimensions(Some(action.cols), Some(action.rows), budgets)?;
            self.runtime
                .vm_at_generation(action.process.generation)
                .await?
                .resize_process_pty_awaited(action.process.pid, action.cols, action.rows)
                .await?;
            Ok(())
        })
    }
}

impl Handler<ProcessOutputRead> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: ProcessOutputRead,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorOutputReplay>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let vm = self
                .runtime
                .vm_at_generation(action.process.generation)
                .await?;
            let replay = vm
                .read_process_output(
                    action.process.pid,
                    action.after,
                    action.max_events,
                    action.max_bytes,
                )
                .await?;
            Ok(ActorOutputReplay {
                generation: action.process.generation,
                events: replay
                    .events
                    .into_iter()
                    .map(|event| ActorOutputEvent {
                        sequence: event.sequence,
                        stream: event.stream,
                        data: FileBytes(event.data),
                        timestamp_ms: event.timestamp_ms,
                    })
                    .collect(),
                next_cursor: replay.next_cursor,
                has_more: replay.has_more,
                truncated: replay.truncated,
                end: exit_status(replay.exit_code, replay.exit_signal)?,
            })
        })
    }
}

impl Handler<TerminalOpen> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        action: TerminalOpen,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorTerminalId>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            action.options.validate(budgets)?;
            let status = self.runtime.status().await;
            let vm = self.runtime.vm_at_generation(status.generation).await?;
            let handle = vm.open_shell(action.options.into_core())?;
            let terminal = ActorTerminalId {
                generation: status.generation,
                shell_id: handle.shell_id,
            };
            let output_ctx = ctx.clone();
            let output_terminal = terminal.clone();
            vm.on_shell_output(&terminal.shell_id, move |event| {
                if let Err(error) = output_ctx.emit(TerminalOutputEvent {
                    terminal: output_terminal.clone(),
                    sequence: event.sequence,
                    stream: event.stream.clone(),
                    data: FileBytes(event.data.clone()),
                    timestamp_ms: event.timestamp_ms,
                }) {
                    tracing::warn!(?error, shell_id = %output_terminal.shell_id, "emit terminal.output failed");
                }
            })?
            .detach();
            let exit_ctx = ctx;
            let exit_terminal = terminal.clone();
            vm.on_shell_exit(&terminal.shell_id, move |event| {
                let status = match observed_exit_status(event.exit_code, event.exit_signal) {
                    Ok(status) => status,
                    Err(error) => { tracing::error!(?error, "invalid observed terminal status"); return; }
                };
                if let Err(error) = exit_ctx.emit(TerminalExitEvent {
                    terminal: exit_terminal.clone(),
                    status,
                }) {
                    tracing::warn!(?error, shell_id = %exit_terminal.shell_id, "emit terminal.exit failed");
                }
            })?
            .detach();
            Ok(terminal)
        })
    }
}

impl Handler<TerminalList> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        _action: TerminalList,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<ActorTerminalInfo>>> + Send + 'static
    {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            let status = self.runtime.status().await;
            let terminals = self
                .runtime
                .vm_at_generation(status.generation)
                .await?
                .list_shells();
            validate_count(
                "terminal.list result",
                terminals.len(),
                budgets.process.max_process_list_entries,
            )?;
            terminals
                .into_iter()
                .map(|info| {
                    Ok(ActorTerminalInfo {
                        terminal: ActorTerminalId {
                            generation: status.generation,
                            shell_id: info.shell_id,
                        },
                        pid: info.pid,
                        running: info.running,
                        exit: exit_status(info.exit_code, info.exit_signal)?,
                    })
                })
                .collect::<Result<_>>()
        })
    }
}

impl Handler<TerminalOutputRead> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: TerminalOutputRead,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorOutputReplay>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let vm = self
                .runtime
                .vm_at_generation(action.terminal.generation)
                .await?;
            let snapshot = vm
                .snapshot_shell_page(
                    &action.terminal.shell_id,
                    action.after,
                    action.max_events,
                    action.max_bytes,
                )
                .await?;
            let end = exit_status(snapshot.exit_code, snapshot.exit_signal)?;
            Ok(ActorOutputReplay {
                generation: action.terminal.generation,
                events: snapshot
                    .events
                    .into_iter()
                    .map(|event| ActorOutputEvent {
                        sequence: event.sequence,
                        stream: event.stream,
                        data: FileBytes(event.data),
                        timestamp_ms: event.timestamp_ms,
                    })
                    .collect(),
                next_cursor: snapshot.next_cursor,
                has_more: snapshot.has_more,
                truncated: snapshot.truncated,
                end,
            })
        })
    }
}

impl Handler<TerminalStdinWrite> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: TerminalStdinWrite,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_bytes(
                "terminal write",
                action.data.byte_len(),
                budgets.execution.max_stdin_bytes,
            )?;
            self.runtime
                .vm_at_generation(action.terminal.generation)
                .await?
                .write_shell_awaited(
                    &action.terminal.shell_id,
                    file_content_to_stdin(action.data),
                )
                .await?;
            Ok(())
        })
    }
}

impl Handler<TerminalPtyResize> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: TerminalPtyResize,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let budgets = self.budgets.as_ref();
            validate_dimensions(Some(action.cols), Some(action.rows), budgets)?;
            self.runtime
                .vm_at_generation(action.terminal.generation)
                .await?
                .resize_shell_awaited(&action.terminal.shell_id, action.cols, action.rows)
                .await?;
            Ok(())
        })
    }
}

impl Handler<TerminalWait> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: TerminalWait,
    ) -> impl std::future::Future<Output = anyhow::Result<ActorTerminalExit>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let vm = self
                .runtime
                .vm_at_generation(action.terminal.generation)
                .await?;
            let exit = tokio::time::timeout(
                Duration::from_millis(DEFAULT_WAIT_MS),
                vm.wait_shell_exit(&action.terminal.shell_id),
            )
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "timeout: terminal.wait exceeded {DEFAULT_WAIT_MS}ms; call again to continue waiting"
                )
            })??;
            Ok(ActorTerminalExit {
                terminal: action.terminal,
                status: observed_exit_status(exit.exit_code, exit.exit_signal)?,
            })
        })
    }
}

impl Handler<TerminalClose> for AgentOsActor {
    fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        action: TerminalClose,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            self.runtime
                .vm_at_generation(action.terminal.generation)
                .await?
                .close_shell_awaited(&action.terminal.shell_id)
                .await?;
            Ok(())
        })
    }
}

fn actor_exec_result(
    result: agentos_client::ExecResult,
    budgets: &HostedActorBudgets,
) -> Result<ActorExecResult> {
    validate_bytes(
        "process exec output",
        result.stdout.len().saturating_add(result.stderr.len()),
        budgets.process.max_exec_output_bytes,
    )?;
    Ok(ActorExecResult {
        status: observed_exit_status(result.exit_code, result.exit_signal)?,
        stdout: result.stdout,
        stderr: result.stderr,
    })
}

fn actor_process_info(
    generation: u64,
    info: agentos_client::SpawnedProcessInfo,
) -> Result<ActorProcessInfo> {
    Ok(ActorProcessInfo {
        process: ActorProcessId {
            generation,
            pid: info.pid,
        },
        command: info.command,
        args: info.args,
        running: info.running,
        exit: exit_status(info.exit_code, info.exit_signal)?,
        started_at_ms: info.started_at,
    })
}

fn actor_process_tree_node(node: ProcessTreeNode, generation: u64) -> Result<ActorProcessTreeNode> {
    let info = node.info;
    Ok(ActorProcessTreeNode {
        process: info
            .tracked_pid
            .map(|pid| ActorProcessId { generation, pid }),
        pid: info.pid,
        ppid: info.ppid,
        pgid: info.pgid,
        sid: info.sid,
        driver: info.driver,
        command: info.command,
        args: info.args,
        cwd: info.cwd,
        status: info.status,
        exit: exit_status(info.exit_code, info.exit_signal)?,
        start_time_ms: info.start_time,
        exit_time_ms: info.exit_time,
        children: node
            .children
            .into_iter()
            .map(|child| actor_process_tree_node(child, generation))
            .collect::<Result<_>>()?,
    })
}

fn observed_exit_status(exit_code: i32, exit_signal: Option<u32>) -> Result<ActorExitStatus> {
    Ok(ActorExitStatus {
        exit_code,
        signal: exit_signal.map(ActorObservedSignal::try_from).transpose()?,
    })
}

fn exit_status(
    exit_code: Option<i32>,
    exit_signal: Option<u32>,
) -> Result<Option<ActorExitStatus>> {
    exit_code
        .map(|code| observed_exit_status(code, exit_signal))
        .transpose()
}

pub(crate) fn attach_process_events(
    vm: &agentos_client::AgentOs,
    ctx: Ctx<AgentOsActor>,
    process: ActorProcessId,
) -> Result<()> {
    let output_ctx = ctx.clone();
    let output_process = process;
    vm.on_process_output(process.pid, move |event| {
        let (Some(sequence), Some(timestamp_ms)) = (event.sequence, event.timestamp_ms) else {
            tracing::warn!(
                pid = event.pid,
                "retained process output lacked sequence metadata"
            );
            return;
        };
        if let Err(error) = output_ctx.emit(ProcessOutputEvent {
            process: output_process,
            sequence,
            stream: event.stream,
            data: FileBytes(event.data),
            timestamp_ms,
        }) {
            tracing::warn!(?error, pid = event.pid, "emit process.output failed");
        }
    })?
    .detach();
    vm.on_process_exit(process.pid, move |event| {
        let status = match observed_exit_status(event.exit_code, event.exit_signal) {
            Ok(status) => status,
            Err(error) => {
                tracing::error!(?error, "invalid observed process status");
                return;
            }
        };
        if let Err(error) = ctx.emit(ProcessExitEvent { process, status }) {
            tracing::warn!(?error, pid = event.pid, "emit process.exit failed");
        }
    })?
    .detach();
    Ok(())
}

fn file_content_to_stdin(content: FileContentInput) -> StdinInput {
    match content {
        FileContentInput::Text(value) => StdinInput::Text(value),
        FileContentInput::Bytes(value) => StdinInput::Bytes(value),
    }
}

pub(crate) fn validate_command(command: &str, budgets: &HostedActorBudgets) -> Result<()> {
    validate_string(
        "process command",
        command,
        budgets.execution.max_command_bytes,
    )
}

pub(crate) fn validate_arguments(args: &[String], budgets: &HostedActorBudgets) -> Result<()> {
    validate_count(
        "process argument count",
        args.len(),
        budgets.execution.max_arguments,
    )?;
    for arg in args {
        // An empty argv element is meaningful to the guest; only the command
        // and cwd require a nonempty string.
        validate_bytes(
            "process argument",
            arg.len(),
            budgets.execution.max_argument_bytes,
        )?;
    }
    Ok(())
}

fn validate_environment(
    env: &BTreeMap<String, String>,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    validate_count(
        "process environment entries",
        env.len(),
        budgets.execution.max_environment_entries,
    )?;
    let bytes = env.iter().try_fold(0usize, |total, (key, value)| {
        total
            .checked_add(key.len())
            .and_then(|sum| sum.checked_add(value.len()))
            .ok_or_else(|| anyhow::anyhow!("limit_exceeded: environment byte count overflow"))
    })?;
    validate_bytes(
        "process environment",
        bytes,
        budgets.execution.max_environment_bytes,
    )
}

fn validate_dimensions(
    cols: Option<u16>,
    rows: Option<u16>,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    for (name, value) in [("cols", cols), ("rows", rows)] {
        if let Some(value) = value {
            if value == 0 {
                bail!("limit_exceeded: terminal {name} must be positive");
            }
            budgets
                .execution
                .max_terminal_dimension
                .check(name, value as usize)?;
        }
    }
    Ok(())
}

fn count_process_tree_nodes(roots: &[ProcessTreeNode]) -> usize {
    let mut count = 0usize;
    let mut pending = roots.iter().collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        count = count.saturating_add(1);
        pending.extend(node.children.iter());
    }
    count
}

fn validate_string(label: &str, value: &str, limit: ValidationBound) -> Result<()> {
    if value.is_empty() {
        bail!("invalid_input: {label} must not be empty");
    }
    limit.check(label, value.len())
}

fn validate_count(label: &str, actual: usize, limit: ValidationBound) -> Result<()> {
    limit.check(label, actual)
}

fn validate_bytes(label: &str, actual: usize, limit: ValidationBound) -> Result<()> {
    limit.check(label, actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_handles_are_runtime_scoped() {
        let process = ActorProcessId {
            generation: 4,
            pid: 100,
        };
        let encoded = serde_json::to_value(process).expect("encode handle");
        assert_eq!(encoded["generation"], 4);
        assert_eq!(encoded["pid"], 100);
    }

    #[test]
    fn empty_process_argument_is_distinct_from_missing_argument() {
        validate_arguments(
            &[],
            &HostedActorBudgets::normalize(Default::default()).unwrap(),
        )
        .expect("no argument entries is valid");
        validate_arguments(
            &[String::new()],
            &HostedActorBudgets::normalize(Default::default()).unwrap(),
        )
        .expect("empty argv element is valid");
        assert!(validate_command(
            "",
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .unwrap_err()
        .to_string()
        .starts_with("invalid_input:"));
        assert!(validate_arguments(
            &["x".repeat(MAX_ARGUMENT_BYTES + 1)],
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .unwrap_err()
        .to_string()
        .starts_with("limit_exceeded:"));
        validate_arguments(
            &vec![String::new(); MAX_ARGUMENTS],
            &HostedActorBudgets::normalize(Default::default()).unwrap(),
        )
        .expect("empty entries still fit the argument-count boundary");
        assert!(validate_arguments(
            &vec![String::new(); MAX_ARGUMENTS + 1],
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .unwrap_err()
        .to_string()
        .starts_with("limit_exceeded: actor.execution.maxArguments:"));
        validate_arguments(
            &["é".repeat(MAX_ARGUMENT_BYTES / 2)],
            &HostedActorBudgets::normalize(Default::default()).unwrap(),
        )
        .expect("argument byte boundary is inclusive");
        assert!(validate_arguments(
            &["é".repeat(MAX_ARGUMENT_BYTES / 2 + 1)],
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .unwrap_err()
        .to_string()
        .starts_with("limit_exceeded: actor.execution.maxArgumentBytes:"));
    }

    #[test]
    fn process_action_dtos_preserve_empty_and_whitespace_arguments() {
        let args = serde_json::json!(["", " two words ", ""]);
        let input = serde_json::json!({ "command": "echo", "args": args });
        let run: ProcessRun = serde_json::from_value(input.clone()).unwrap();
        let spawn: ProcessSpawn = serde_json::from_value(input.clone()).unwrap();
        let terminal: ActorTerminalOptions = serde_json::from_value(input).unwrap();
        validate_arguments(
            &run.args,
            &HostedActorBudgets::normalize(Default::default()).unwrap(),
        )
        .unwrap();
        validate_arguments(
            &spawn.args,
            &HostedActorBudgets::normalize(Default::default()).unwrap(),
        )
        .unwrap();
        terminal
            .validate(&HostedActorBudgets::normalize(Default::default()).unwrap())
            .unwrap();
        assert_eq!(serde_json::to_value(&run).unwrap()["args"], args);
        assert_eq!(serde_json::to_value(&spawn).unwrap()["args"], args);
        assert_eq!(
            serde_json::to_value(terminal.into_core().args).unwrap(),
            args
        );
    }

    #[test]
    fn process_and_terminal_cwd_validation_has_consistent_public_errors() {
        for (cwd, expected) in [
            (String::new(), "invalid_input"),
            ("x".repeat(MAX_ARGUMENT_BYTES + 1), "limit_exceeded"),
        ] {
            for result in [
                ActorExecOptions {
                    cwd: Some(cwd.clone()),
                    ..Default::default()
                }
                .validate(&HostedActorBudgets::normalize(Default::default()).unwrap()),
                ActorSpawnOptions {
                    cwd: Some(cwd.clone()),
                    ..Default::default()
                }
                .validate(&HostedActorBudgets::normalize(Default::default()).unwrap()),
                ActorTerminalOptions {
                    cwd: Some(cwd.clone()),
                    ..Default::default()
                }
                .validate(&HostedActorBudgets::normalize(Default::default()).unwrap()),
            ] {
                let error =
                    crate::action_set::classify_public_error(result.expect_err("invalid cwd"));
                let error = error
                    .downcast_ref::<rivet_error::RivetError>()
                    .expect("typed error");
                let rivet_error::RivetErrorKind::Dynamic { code, .. } = &error.kind else {
                    panic!("expected a public agentOS error");
                };
                assert_eq!(code, expected);
                assert!(error.to_string().contains("cwd"));
            }
        }
        ActorTerminalOptions {
            cwd: Some("/".into()),
            ..Default::default()
        }
        .validate(&HostedActorBudgets::normalize(Default::default()).unwrap())
        .expect("nonempty terminal cwd is valid");
    }

    #[test]
    fn process_start_timestamp_names_its_unit() {
        let info = ActorProcessInfo {
            process: ActorProcessId {
                generation: 4,
                pid: 100,
            },
            command: "sh".into(),
            args: Vec::new(),
            running: true,
            exit: None,
            started_at_ms: 1_700_000_000_000,
        };
        let encoded = serde_json::to_value(info).expect("encode process info");
        assert_eq!(encoded["startedAtMs"], 1_700_000_000_000_i64);
        assert!(encoded.get("startedAt").is_none());
    }

    #[test]
    fn process_tree_timestamps_name_their_units() {
        let tree = ActorProcessTree {
            generation: 4,
            roots: vec![actor_process_tree_node(
                ProcessTreeNode {
                    info: agentos_client::ProcessInfo {
                        pid: 1_000_000,
                        tracked_pid: Some(123),
                        ppid: 1,
                        pgid: 100,
                        sid: 100,
                        driver: "node".into(),
                        command: "node".into(),
                        args: Vec::new(),
                        cwd: "/".into(),
                        status: ProcessStatus::Exited,
                        exit_code: Some(0),
                        exit_signal: None,
                        start_time: 1_700_000_000_000.25,
                        exit_time: Some(1_700_000_000_001.5),
                    },
                    children: vec![ProcessTreeNode {
                        info: agentos_client::ProcessInfo {
                            // An untracked kernel PID can overlap the synthetic range.
                            pid: 1_000_000,
                            tracked_pid: None,
                            ppid: 1_000_000,
                            pgid: 100,
                            sid: 100,
                            driver: "wasm".into(),
                            command: "sleep".into(),
                            args: vec!["5".into()],
                            cwd: "/work".into(),
                            status: ProcessStatus::Running,
                            exit_code: None,
                            exit_signal: None,
                            start_time: 1_700_000_000_000.5,
                            exit_time: None,
                        },
                        children: Vec::new(),
                    }],
                },
                4,
            )
            .expect("valid tree status")],
        };
        let encoded = serde_json::to_value(tree).expect("encode process tree");
        let root = &encoded["roots"][0];
        assert_eq!(root["startTimeMs"], 1_700_000_000_000.25);
        assert_eq!(root["exitTimeMs"], 1_700_000_000_001.5);
        assert_eq!(root["exit"]["exitCode"], 0);
        assert_eq!(root["process"]["generation"], 4);
        assert_eq!(root["pid"], 1_000_000);
        assert_eq!(root["process"]["pid"], 123);
        assert!(root.get("startTime").is_none());
        assert!(root.get("exitTime").is_none());
        assert!(root.get("exitCode").is_none());
        let child = &root["children"][0];
        assert_eq!(child["pid"], 1_000_000);
        assert_eq!(child["ppid"], 1_000_000);
        assert_eq!(child["args"], serde_json::json!(["5"]));
        assert_eq!(child["cwd"], "/work");
        assert_eq!(child["status"], "running");
        assert!(child.get("process").is_none());
        assert_eq!(child["startTimeMs"], 1_700_000_000_000.5);
        assert!(child["exitTimeMs"].is_null());
        assert!(child["exit"].is_null());
        assert!(child.get("startTime").is_none());
        assert!(child.get("exitCode").is_none());
    }

    #[test]
    fn observed_signal_names_are_canonical_and_do_not_expand_requests() {
        let cases: Vec<(u32, String)> =
            serde_json::from_str(include_str!("../tests/fixtures/observed-signals.json")).unwrap();
        for (number, name) in cases {
            let observed = ActorObservedSignal::try_from(number).unwrap();
            let copy = observed;
            assert_eq!(copy.number(), number);
            assert_eq!(serde_json::to_value(observed).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<ActorObservedSignal>(serde_json::json!(name)).unwrap(),
                observed
            );
        }
        for invalid in [0, 65, u32::MAX] {
            assert!(ActorObservedSignal::try_from(invalid).is_err());
        }
        for invalid in ["0", "15", "65", "032", "SIGRTMIN", "SIGIOT"] {
            assert!(
                serde_json::from_value::<ActorObservedSignal>(serde_json::json!(invalid)).is_err()
            );
        }
        assert!(serde_json::from_value::<ActorSignal>(serde_json::json!("SIGUSR1")).is_err());
        assert!(serde_json::from_value::<ActorSignal>(serde_json::json!("32")).is_err());
    }

    #[test]
    fn observed_status_never_infers_a_signal_from_exit_code() {
        for (exit_code, exit_signal, name) in [
            (143, Some(15), Some("SIGTERM")),
            (143, None, None),
            (0, None, None),
        ] {
            let status = observed_exit_status(exit_code, exit_signal).unwrap();
            let encoded = serde_json::to_value(status).unwrap();
            assert_eq!(encoded["exitCode"], exit_code);
            assert_eq!(encoded.get("signal").and_then(|value| value.as_str()), name);
        }
        assert!(observed_exit_status(143, Some(0)).is_err());
        assert!(observed_exit_status(143, Some(65)).is_err());
    }

    #[test]
    fn command_and_terminal_limits_are_bounded() {
        assert!(validate_command(
            "sh",
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .is_ok());
        assert!(validate_command(
            "",
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .is_err());
        assert!(validate_dimensions(
            Some(80),
            Some(24),
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .is_ok());
        assert!(validate_dimensions(
            Some(0),
            Some(24),
            &HostedActorBudgets::normalize(Default::default()).unwrap()
        )
        .is_err());
    }

    #[test]
    fn captured_run_forwards_a_bounded_core_timeout_by_default() {
        let default = ActorExecOptions::default();
        default
            .validate(&HostedActorBudgets::normalize(Default::default()).unwrap())
            .expect("default options are valid");
        assert_eq!(
            default
                .into_core(&HostedActorBudgets::normalize(Default::default()).unwrap())
                .timeout,
            Some(MAX_WAIT_MS as f64)
        );
        assert!(ActorExecOptions {
            timeout_ms: Some(0),
            ..Default::default()
        }
        .validate(&HostedActorBudgets::normalize(Default::default()).unwrap())
        .is_err());
    }
}

#[cfg(test)]
#[path = "../tests/unit/process_worker_budgets.rs"]
mod worker_budget_tests;
