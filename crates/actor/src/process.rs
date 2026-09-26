use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use agentos_client::{
    ExecOptions, OpenShellOptions, ProcessStatus, ProcessTreeNode, SpawnOptions, SpawnStdio,
    StdinInput,
};
use anyhow::{bail, Result};
use rivetkit::{Ctx, Handles};
use serde::{Deserialize, Serialize};

use crate::actions::BoxFuture;
use crate::events::{ProcessExitEvent, ProcessOutputEvent, TerminalExitEvent, TerminalOutputEvent};
use crate::{AgentOsActor, FileBytes, FileContentInput};

const MAX_COMMAND_BYTES: usize = 16 * 1024;
const MAX_ARGUMENTS: usize = 1_024;
const MAX_ARGUMENT_BYTES: usize = 16 * 1024;
const MAX_ENVIRONMENT_ENTRIES: usize = 1_024;
const MAX_ENVIRONMENT_BYTES: usize = 256 * 1024;
const MAX_STDIN_BYTES: usize = 256 * 1024;
const MAX_EXEC_OUTPUT_BYTES: usize = 768 * 1024;
const MAX_PROCESS_LIST_ENTRIES: usize = 4_096;
const MAX_WAIT_MS: u64 = 5 * 60 * 1_000;
const DEFAULT_WAIT_MS: u64 = 30 * 1_000;
const MAX_TERMINAL_DIMENSION: u16 = 4_096;

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
    fn validate(&self) -> Result<()> {
        validate_environment(&self.env)?;
        if let Some(cwd) = &self.cwd {
            validate_string("process cwd", cwd, MAX_ARGUMENT_BYTES)?;
        }
        if let Some(stdin) = &self.stdin {
            validate_bytes("process stdin", stdin.byte_len(), MAX_STDIN_BYTES)?;
        }
        if self.timeout_ms == Some(0) {
            bail!("invalid_input: process timeoutMs must be greater than zero");
        }
        if self.timeout_ms.is_some_and(|timeout| timeout > MAX_WAIT_MS) {
            bail!("limit_exceeded: process timeout exceeds {MAX_WAIT_MS}ms; lower timeoutMs");
        }
        Ok(())
    }

    fn into_core(self) -> ExecOptions {
        ExecOptions {
            env: self.env,
            cwd: self.cwd,
            stdin: self.stdin.map(file_content_to_stdin),
            timeout: Some(self.timeout_ms.unwrap_or(MAX_WAIT_MS) as f64),
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
    pub(crate) fn validate(&self) -> Result<()> {
        validate_environment(&self.env)?;
        if let Some(cwd) = &self.cwd {
            validate_string("process cwd", cwd, MAX_ARGUMENT_BYTES)?;
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

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorExitStatus {
    pub exit_code: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<ActorSignal>,
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
    fn validate(&self) -> Result<()> {
        if let Some(command) = &self.command {
            validate_command(command)?;
        }
        validate_arguments(&self.args)?;
        validate_environment(&self.env)?;
        if let Some(cwd) = &self.cwd {
            validate_string("terminal cwd", cwd, MAX_ARGUMENT_BYTES)?;
        }
        validate_dimensions(self.cols, self.rows)?;
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

impl Handles<ProcessRun> for AgentOsActor {
    type Future = BoxFuture<ActorExecResult>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessRun) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_command(&action.command)?;
            validate_arguments(&action.args)?;
            action.options.validate()?;
            // Acquiring the ready VM handle is part of the caller's run budget. Reserve the Core
            // cleanup window below the actor's six-minute action deadline.
            let deadline = tokio::time::Instant::now()
                + Duration::from_millis(action.options.timeout_ms.unwrap_or(MAX_WAIT_MS));
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
            let mut options = action.options.into_core();
            options.timeout = Some(remaining.as_secs_f64() * 1_000.0);
            let result = vm
                .exec_argv_process(&action.command, &action.args, options)
                .await?;
            actor_exec_result(result)
        })
    }
}

impl Handles<ProcessSpawn> for AgentOsActor {
    type Future = BoxFuture<ActorProcessId>;
    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: ProcessSpawn) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_command(&action.command)?;
            validate_arguments(&action.args)?;
            action.options.validate()?;
            let status = self.runtime.status().await;
            let vm = self.runtime.vm_at_generation(status.generation).await?;
            let handle =
                vm.spawn_process(&action.command, action.args, action.options.into_core())?;
            let process = ActorProcessId {
                generation: status.generation,
                pid: handle.pid,
            };
            attach_process_events(&vm, ctx, process)?;
            Ok(process)
        })
    }
}

impl Handles<ProcessGet> for AgentOsActor {
    type Future = BoxFuture<ActorProcessInfo>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessGet) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let info = self
                .runtime
                .vm_at_generation(action.process.generation)
                .await?
                .get_process(action.process.pid)?;
            Ok(actor_process_info(action.process.generation, info))
        })
    }
}

impl Handles<ProcessList> for AgentOsActor {
    type Future = BoxFuture<Vec<ActorProcessInfo>>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: ProcessList) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let status = self.runtime.status().await;
            let entries = self
                .runtime
                .vm_at_generation(status.generation)
                .await?
                .list_processes();
            validate_count(
                "process.list result",
                entries.len(),
                MAX_PROCESS_LIST_ENTRIES,
            )?;
            Ok(entries
                .into_iter()
                .map(|info| actor_process_info(status.generation, info))
                .collect())
        })
    }
}

impl Handles<ProcessTree> for AgentOsActor {
    type Future = BoxFuture<ActorProcessTree>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: ProcessTree) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let status = self.runtime.status().await;
            let vm = self.runtime.vm_at_generation(status.generation).await?;
            let roots = vm.process_tree().await?;
            validate_count(
                "process.tree result",
                count_process_tree_nodes(&roots),
                MAX_PROCESS_LIST_ENTRIES,
            )?;
            Ok(ActorProcessTree {
                generation: status.generation,
                roots: roots
                    .into_iter()
                    .map(|node| actor_process_tree_node(node, status.generation))
                    .collect(),
            })
        })
    }
}

impl Handles<ProcessWait> for AgentOsActor {
    type Future = BoxFuture<ActorProcessExit>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessWait) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let vm = self
                .runtime
                .vm_at_generation(action.process.generation)
                .await?;
            let exit_code = tokio::time::timeout(
                Duration::from_millis(DEFAULT_WAIT_MS),
                vm.wait_process(action.process.pid),
            )
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "timeout: process.wait exceeded {DEFAULT_WAIT_MS}ms; call again to continue waiting"
                )
            })??;
            Ok(ActorProcessExit {
                process: action.process,
                status: ActorExitStatus {
                    exit_code,
                    signal: None,
                },
            })
        })
    }
}

impl Handles<ProcessSignal> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessSignal) -> Self::Future {
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

impl Handles<ProcessStdinWrite> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessStdinWrite) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_bytes("process stdin", action.data.byte_len(), MAX_STDIN_BYTES)?;
            self.runtime
                .vm_at_generation(action.process.generation)
                .await?
                .write_process_stdin_awaited(action.process.pid, file_content_to_stdin(action.data))
                .await?;
            Ok(())
        })
    }
}

impl Handles<ProcessStdinClose> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessStdinClose) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            self.runtime
                .vm_at_generation(action.process.generation)
                .await?
                .close_process_stdin_awaited(action.process.pid)
                .await?;
            Ok(())
        })
    }
}

impl Handles<ProcessPtyResize> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessPtyResize) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_dimensions(Some(action.cols), Some(action.rows))?;
            self.runtime
                .vm_at_generation(action.process.generation)
                .await?
                .resize_process_pty_awaited(action.process.pid, action.cols, action.rows)
                .await?;
            Ok(())
        })
    }
}

impl Handles<ProcessOutputRead> for AgentOsActor {
    type Future = BoxFuture<ActorOutputReplay>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: ProcessOutputRead) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let vm = self
                .runtime
                .vm_at_generation(action.process.generation)
                .await?;
            let replay = vm.read_process_output(
                action.process.pid,
                action.after,
                action.max_events,
                action.max_bytes,
            )?;
            let info = vm.get_process(action.process.pid)?;
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
                end: exit_status(info.exit_code),
            })
        })
    }
}

impl Handles<TerminalOpen> for AgentOsActor {
    type Future = BoxFuture<ActorTerminalId>;
    fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: TerminalOpen) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            action.options.validate()?;
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
                if let Err(error) = exit_ctx.emit(TerminalExitEvent {
                    terminal: exit_terminal.clone(),
                    status: ActorExitStatus {
                        exit_code: event.exit_code,
                        signal: None,
                    },
                }) {
                    tracing::warn!(?error, shell_id = %exit_terminal.shell_id, "emit terminal.exit failed");
                }
            })?
            .detach();
            Ok(terminal)
        })
    }
}

impl Handles<TerminalList> for AgentOsActor {
    type Future = BoxFuture<Vec<ActorTerminalInfo>>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: TerminalList) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let status = self.runtime.status().await;
            let terminals = self
                .runtime
                .vm_at_generation(status.generation)
                .await?
                .list_shells();
            validate_count(
                "terminal.list result",
                terminals.len(),
                MAX_PROCESS_LIST_ENTRIES,
            )?;
            Ok(terminals
                .into_iter()
                .map(|info| ActorTerminalInfo {
                    terminal: ActorTerminalId {
                        generation: status.generation,
                        shell_id: info.shell_id,
                    },
                    pid: info.pid,
                    running: info.running,
                    exit: exit_status(info.exit_code),
                })
                .collect())
        })
    }
}

impl Handles<TerminalOutputRead> for AgentOsActor {
    type Future = BoxFuture<ActorOutputReplay>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: TerminalOutputRead) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let vm = self
                .runtime
                .vm_at_generation(action.terminal.generation)
                .await?;
            let snapshot = vm.snapshot_shell_page(
                &action.terminal.shell_id,
                action.after,
                action.max_events,
                action.max_bytes,
            )?;
            let end = vm
                .list_shells()
                .into_iter()
                .find(|info| info.shell_id == action.terminal.shell_id)
                .and_then(|info| exit_status(info.exit_code));
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

impl Handles<TerminalStdinWrite> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: TerminalStdinWrite) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_bytes("terminal write", action.data.byte_len(), MAX_STDIN_BYTES)?;
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

impl Handles<TerminalPtyResize> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: TerminalPtyResize) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            validate_dimensions(Some(action.cols), Some(action.rows))?;
            self.runtime
                .vm_at_generation(action.terminal.generation)
                .await?
                .resize_shell_awaited(&action.terminal.shell_id, action.cols, action.rows)
                .await?;
            Ok(())
        })
    }
}

impl Handles<TerminalWait> for AgentOsActor {
    type Future = BoxFuture<ActorTerminalExit>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: TerminalWait) -> Self::Future {
        Box::pin(async move {
            let _permit = self.admit_action()?;
            let vm = self
                .runtime
                .vm_at_generation(action.terminal.generation)
                .await?;
            let exit_code = tokio::time::timeout(
                Duration::from_millis(DEFAULT_WAIT_MS),
                vm.wait_shell(&action.terminal.shell_id),
            )
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "timeout: terminal.wait exceeded {DEFAULT_WAIT_MS}ms; call again to continue waiting"
                )
            })??;
            Ok(ActorTerminalExit {
                terminal: action.terminal,
                status: ActorExitStatus {
                    exit_code,
                    signal: None,
                },
            })
        })
    }
}

impl Handles<TerminalClose> for AgentOsActor {
    type Future = BoxFuture<()>;
    fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: TerminalClose) -> Self::Future {
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

fn actor_exec_result(result: agentos_client::ExecResult) -> Result<ActorExecResult> {
    validate_bytes(
        "process exec output",
        result.stdout.len().saturating_add(result.stderr.len()),
        MAX_EXEC_OUTPUT_BYTES,
    )?;
    Ok(ActorExecResult {
        status: ActorExitStatus {
            exit_code: result.exit_code,
            signal: None,
        },
        stdout: result.stdout,
        stderr: result.stderr,
    })
}

fn actor_process_info(
    generation: u64,
    info: agentos_client::SpawnedProcessInfo,
) -> ActorProcessInfo {
    ActorProcessInfo {
        process: ActorProcessId {
            generation,
            pid: info.pid,
        },
        command: info.command,
        args: info.args,
        running: info.running,
        exit: exit_status(info.exit_code),
        started_at_ms: info.started_at,
    }
}

fn actor_process_tree_node(node: ProcessTreeNode, generation: u64) -> ActorProcessTreeNode {
    let info = node.info;
    ActorProcessTreeNode {
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
        exit: exit_status(info.exit_code),
        start_time_ms: info.start_time,
        exit_time_ms: info.exit_time,
        children: node
            .children
            .into_iter()
            .map(|child| actor_process_tree_node(child, generation))
            .collect(),
    }
}

fn exit_status(exit_code: Option<i32>) -> Option<ActorExitStatus> {
    exit_code.map(|exit_code| ActorExitStatus {
        exit_code,
        signal: None,
    })
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
        if let Err(error) = ctx.emit(ProcessExitEvent {
            process,
            status: ActorExitStatus {
                exit_code: event.exit_code,
                signal: None,
            },
        }) {
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

pub(crate) fn validate_command(command: &str) -> Result<()> {
    validate_string("process command", command, MAX_COMMAND_BYTES)
}

pub(crate) fn validate_arguments(args: &[String]) -> Result<()> {
    validate_count("process argument count", args.len(), MAX_ARGUMENTS)?;
    for arg in args {
        // An empty argv element is meaningful to the guest; only the command
        // and cwd require a nonempty string.
        validate_bytes("process argument", arg.len(), MAX_ARGUMENT_BYTES)?;
    }
    Ok(())
}

fn validate_environment(env: &BTreeMap<String, String>) -> Result<()> {
    validate_count(
        "process environment entries",
        env.len(),
        MAX_ENVIRONMENT_ENTRIES,
    )?;
    let bytes = env.iter().try_fold(0usize, |total, (key, value)| {
        total
            .checked_add(key.len())
            .and_then(|sum| sum.checked_add(value.len()))
            .ok_or_else(|| anyhow::anyhow!("limit_exceeded: environment byte count overflow"))
    })?;
    validate_bytes("process environment", bytes, MAX_ENVIRONMENT_BYTES)
}

fn validate_dimensions(cols: Option<u16>, rows: Option<u16>) -> Result<()> {
    for (name, value) in [("cols", cols), ("rows", rows)] {
        if value.is_some_and(|value| value == 0 || value > MAX_TERMINAL_DIMENSION) {
            bail!("limit_exceeded: terminal {name} must be between 1 and {MAX_TERMINAL_DIMENSION}");
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

fn validate_string(label: &str, value: &str, limit: usize) -> Result<()> {
    if value.is_empty() {
        bail!("invalid_input: {label} must not be empty");
    }
    if value.len() > limit {
        bail!(
            "limit_exceeded: {label} is {} bytes, limit is {limit}",
            value.len()
        );
    }
    Ok(())
}

fn validate_count(label: &str, actual: usize, limit: usize) -> Result<()> {
    if actual > limit {
        bail!("limit_exceeded: {label} is {actual}, limit is {limit}");
    }
    Ok(())
}

fn validate_bytes(label: &str, actual: usize, limit: usize) -> Result<()> {
    if actual > limit {
        bail!("limit_exceeded: {label} is {actual} bytes, limit is {limit}");
    }
    Ok(())
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
        validate_arguments(&[]).expect("no argument entries is valid");
        validate_arguments(&[String::new()]).expect("empty argv element is valid");
        assert!(validate_command("")
            .unwrap_err()
            .to_string()
            .starts_with("invalid_input:"));
        assert!(validate_arguments(&["x".repeat(MAX_ARGUMENT_BYTES + 1)])
            .unwrap_err()
            .to_string()
            .starts_with("limit_exceeded:"));
        validate_arguments(&vec![String::new(); MAX_ARGUMENTS])
            .expect("empty entries still fit the argument-count boundary");
        assert!(validate_arguments(&vec![String::new(); MAX_ARGUMENTS + 1])
            .unwrap_err()
            .to_string()
            .starts_with("limit_exceeded: process argument count"));
        validate_arguments(&["é".repeat(MAX_ARGUMENT_BYTES / 2)])
            .expect("argument byte boundary is inclusive");
        assert!(
            validate_arguments(&["é".repeat(MAX_ARGUMENT_BYTES / 2 + 1)])
                .unwrap_err()
                .to_string()
                .starts_with("limit_exceeded: process argument")
        );
    }

    #[test]
    fn process_action_dtos_preserve_empty_and_whitespace_arguments() {
        let args = serde_json::json!(["", " two words ", ""]);
        let input = serde_json::json!({ "command": "echo", "args": args });
        let run: ProcessRun = serde_json::from_value(input.clone()).unwrap();
        let spawn: ProcessSpawn = serde_json::from_value(input.clone()).unwrap();
        let terminal: ActorTerminalOptions = serde_json::from_value(input).unwrap();
        validate_arguments(&run.args).unwrap();
        validate_arguments(&spawn.args).unwrap();
        terminal.validate().unwrap();
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
                .validate(),
                ActorSpawnOptions {
                    cwd: Some(cwd.clone()),
                    ..Default::default()
                }
                .validate(),
                ActorTerminalOptions {
                    cwd: Some(cwd.clone()),
                    ..Default::default()
                }
                .validate(),
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
        .validate()
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
                            start_time: 1_700_000_000_000.5,
                            exit_time: None,
                        },
                        children: Vec::new(),
                    }],
                },
                4,
            )],
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
    fn command_and_terminal_limits_are_bounded() {
        assert!(validate_command("sh").is_ok());
        assert!(validate_command("").is_err());
        assert!(validate_dimensions(Some(80), Some(24)).is_ok());
        assert!(validate_dimensions(Some(0), Some(24)).is_err());
    }

    #[test]
    fn captured_run_forwards_a_bounded_core_timeout_by_default() {
        let default = ActorExecOptions::default();
        default.validate().expect("default options are valid");
        assert_eq!(default.into_core().timeout, Some(MAX_WAIT_MS as f64));
        assert!(ActorExecOptions {
            timeout_ms: Some(0),
            ..Default::default()
        }
        .validate()
        .is_err());
    }
}
