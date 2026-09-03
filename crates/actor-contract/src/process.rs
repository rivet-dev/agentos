//! Transport-safe process and terminal action DTOs.

use std::collections::BTreeMap;

use agentos_client::{
    ExecOptions, OpenShellOptions, ProcessStatus, ProcessStream, ProcessTreeNode, SpawnOptions,
    SpawnStdio, StdinInput,
};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::filesystem::{FileBytes, FileContentInput};

pub const MAX_COMMAND_BYTES: usize = 16 * 1024;
pub const MAX_ARGUMENTS: usize = 1_024;
pub const MAX_ARGUMENT_BYTES: usize = 16 * 1024;
pub const MAX_ENVIRONMENT_ENTRIES: usize = 1_024;
pub const MAX_ENVIRONMENT_BYTES: usize = 256 * 1024;
pub const MAX_STDIN_BYTES: usize = 256 * 1024;
pub const MAX_EXEC_OUTPUT_BYTES: usize = 768 * 1024;
pub const MAX_PROCESS_LIST_ENTRIES: usize = 4_096;
pub const MAX_WAIT_MS: u64 = 5 * 60 * 1_000;
pub const DEFAULT_WAIT_MS: u64 = 30 * 1_000;
pub const MAX_TERMINAL_DIMENSION: u16 = 4_096;

macro_rules! action {
    ($name:ident => $output:ty, $wire_name:literal) => {
        impl rivetkit::Action for $name {
            type Output = $output;
            const NAME: &'static str = $wire_name;
        }
    };
}

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

action!(ProcessRun => ActorExecResult, "process.run");

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

action!(ProcessSpawn => ActorProcessId, "process.spawn");

macro_rules! process_id_action {
    ($name:ident => $output:ty, $wire_name:literal) => {
        #[cfg_attr(feature = "contract", derive(ts_rs::TS))]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            pub process: ActorProcessId,
        }

        action!($name => $output, $wire_name);
    };
}

process_id_action!(ProcessGet => ActorProcessInfo, "process.get");
process_id_action!(ProcessWait => ActorProcessExit, "process.wait");
process_id_action!(ProcessStdinClose => (), "process.stdin.close");

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

action!(ProcessList => Vec<ActorProcessInfo>, "process.list");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessTree {}

action!(ProcessTree => ActorProcessTree, "process.tree");

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
    pub const fn as_str(self) -> &'static str {
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

action!(ProcessSignal => (), "process.signal");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessStdinWrite {
    pub process: ActorProcessId,
    pub data: FileContentInput,
}

action!(ProcessStdinWrite => (), "process.stdin.write");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessPtyResize {
    pub process: ActorProcessId,
    pub cols: u16,
    pub rows: u16,
}

action!(ProcessPtyResize => (), "process.pty.resize");

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

action!(ProcessOutputRead => ActorOutputReplay, "process.output.read");

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
    pub stream: ProcessStream,
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

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalOpen {
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub options: ActorTerminalOptions,
}

action!(TerminalOpen => ActorTerminalId, "terminal.open");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalList {}

action!(TerminalList => Vec<ActorTerminalInfo>, "terminal.list");

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

action!(TerminalOutputRead => ActorOutputReplay, "terminal.output.read");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalStdinWrite {
    pub terminal: ActorTerminalId,
    pub data: FileContentInput,
}

action!(TerminalStdinWrite => (), "terminal.stdin.write");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalPtyResize {
    pub terminal: ActorTerminalId,
    pub cols: u16,
    pub rows: u16,
}

action!(TerminalPtyResize => (), "terminal.pty.resize");

macro_rules! terminal_id_action {
    ($name:ident => $output:ty, $wire_name:literal) => {
        #[cfg_attr(feature = "contract", derive(ts_rs::TS))]
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            pub terminal: ActorTerminalId,
        }

        action!($name => $output, $wire_name);
    };
}

terminal_id_action!(TerminalWait => ActorTerminalExit, "terminal.wait");
terminal_id_action!(TerminalClose => (), "terminal.close");

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorTerminalExit {
    pub terminal: ActorTerminalId,
    pub status: ActorExitStatus,
}

pub fn validate_exec_options(options: &ActorExecOptions) -> Result<()> {
    validate_environment(&options.env)?;
    if let Some(cwd) = &options.cwd {
        validate_string("process cwd", cwd, MAX_ARGUMENT_BYTES)?;
    }
    if let Some(stdin) = &options.stdin {
        validate_bytes("process stdin", stdin.byte_len(), MAX_STDIN_BYTES)?;
    }
    if options.timeout_ms == Some(0) {
        bail!("invalid_input: process timeoutMs must be greater than zero");
    }
    if options
        .timeout_ms
        .is_some_and(|timeout| timeout > MAX_WAIT_MS)
    {
        bail!("limit_exceeded: process timeout exceeds {MAX_WAIT_MS}ms; lower timeoutMs");
    }
    Ok(())
}

pub fn exec_options_into_core(options: ActorExecOptions) -> ExecOptions {
    ExecOptions {
        env: options.env,
        cwd: options.cwd,
        stdin: options.stdin.map(file_content_to_stdin),
        timeout: Some(options.timeout_ms.unwrap_or(MAX_WAIT_MS) as f64),
        capture_stdio: options.capture_stdio,
        ..ExecOptions::default()
    }
}

pub fn validate_spawn_options(options: &ActorSpawnOptions) -> Result<()> {
    validate_environment(&options.env)?;
    if let Some(cwd) = &options.cwd {
        validate_string("process cwd", cwd, MAX_ARGUMENT_BYTES)?;
    }
    Ok(())
}

pub fn spawn_options_into_core(options: ActorSpawnOptions) -> SpawnOptions {
    SpawnOptions {
        env: options.env,
        cwd: options.cwd,
        stdio: options.stdio,
        stdin_fd: options.stdin_fd,
        stdout_fd: options.stdout_fd,
        stderr_fd: options.stderr_fd,
        stream_stdin: Some(true),
        retain_output: true,
    }
}

pub fn validate_terminal_options(options: &ActorTerminalOptions) -> Result<()> {
    if let Some(command) = &options.command {
        validate_command(command)?;
    }
    validate_arguments(&options.args)?;
    validate_environment(&options.env)?;
    if let Some(cwd) = &options.cwd {
        validate_string("terminal cwd", cwd, MAX_ARGUMENT_BYTES)?;
    }
    validate_dimensions(options.cols, options.rows)
}

pub fn terminal_options_into_core(options: ActorTerminalOptions) -> OpenShellOptions {
    OpenShellOptions {
        command: options.command,
        args: options.args,
        env: options.env,
        cwd: options.cwd,
        cols: options.cols,
        rows: options.rows,
    }
}

pub fn actor_exec_result(result: agentos_client::ExecResult) -> Result<ActorExecResult> {
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

pub fn actor_process_info(
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

pub fn actor_process_tree_node(node: ProcessTreeNode, generation: u64) -> ActorProcessTreeNode {
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

pub fn exit_status(exit_code: Option<i32>) -> Option<ActorExitStatus> {
    exit_code.map(|exit_code| ActorExitStatus {
        exit_code,
        signal: None,
    })
}

pub fn validate_command(command: &str) -> Result<()> {
    validate_string("process command", command, MAX_COMMAND_BYTES)
}

pub fn validate_arguments(args: &[String]) -> Result<()> {
    validate_count("process argument count", args.len(), MAX_ARGUMENTS)?;
    for arg in args {
        // An empty argv element is meaningful to the guest; only the command
        // and cwd require a nonempty string.
        validate_bytes("process argument", arg.len(), MAX_ARGUMENT_BYTES)?;
    }
    Ok(())
}

pub fn validate_environment(env: &BTreeMap<String, String>) -> Result<()> {
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

pub fn validate_dimensions(cols: Option<u16>, rows: Option<u16>) -> Result<()> {
    for (name, value) in [("cols", cols), ("rows", rows)] {
        if value.is_some_and(|value| value == 0 || value > MAX_TERMINAL_DIMENSION) {
            bail!("limit_exceeded: terminal {name} must be between 1 and {MAX_TERMINAL_DIMENSION}");
        }
    }
    Ok(())
}

pub fn count_process_tree_nodes(roots: &[ProcessTreeNode]) -> usize {
    let mut count = 0usize;
    let mut pending = roots.iter().collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        count = count.saturating_add(1);
        pending.extend(node.children.iter());
    }
    count
}

pub fn validate_string(label: &str, value: &str, limit: usize) -> Result<()> {
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

pub fn validate_count(label: &str, actual: usize, limit: usize) -> Result<()> {
    if actual > limit {
        bail!("limit_exceeded: {label} is {actual}, limit is {limit}");
    }
    Ok(())
}

pub fn validate_bytes(label: &str, actual: usize, limit: usize) -> Result<()> {
    if actual > limit {
        bail!("limit_exceeded: {label} is {actual} bytes, limit is {limit}");
    }
    Ok(())
}

pub fn file_content_to_stdin(content: FileContentInput) -> StdinInput {
    match content {
        FileContentInput::Text(value) => StdinInput::Text(value),
        FileContentInput::Bytes(value) => StdinInput::Bytes(value),
    }
}
