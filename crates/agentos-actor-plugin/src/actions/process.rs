//! Process actions. Each helper takes `&AgentOs` plus typed args and
//! delegates to the matching upstream `AgentOs::*` method. DTOs used
//! by `exec` and other arms that need camelCase serialization live
//! here so the dispatcher arms can reply directly.

use agentos_client::{
    AgentOs, ExecOptions, ExecResult, ProcessInfo, ProcessTreeNode, SpawnHandle, SpawnOptions,
    SpawnedProcessInfo,
};
use anyhow::Result;
use serde::Serialize;

/// JSON options for the `exec` action — the serializable subset of the TS
/// `ExecOptions` (env + cwd). Mirrors [`SpawnActionOptions`] minus the
/// stream-stdin flag, which only applies to long-running spawns.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecActionOptions {
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    pub cwd: Option<String>,
}

/// `exec(command, options?)` — port of [`AgentOs::exec`]. Forwards the optional
/// `env`/`cwd` so callers can run a command with credentials without dropping to
/// `spawn`. Returns an [`ExecResultDto`] with camelCase `exitCode` for the JS side.
pub async fn exec(
    vm: &AgentOs,
    command: &str,
    options: ExecActionOptions,
) -> Result<ExecResultDto> {
    let mut base = ExecOptions::default();
    base.env = options.env;
    if options.cwd.is_some() {
        base.cwd = options.cwd;
    }
    vm.exec(command, base).await.map(ExecResultDto::from)
}

/// JSON options for the `spawn` action — the serializable subset of the TS
/// `SpawnOptions` (output callbacks are replaced by the `processOutput` /
/// `processExit` broadcasts).
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnActionOptions {
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub stream_stdin: Option<bool>,
}

/// `spawn(command, args, options?)` — port of [`AgentOs::spawn`]. Returns the
/// [`SpawnHandle`] `{ pid }`; stdout/stderr chunks stream to connected clients
/// as `processOutput` events and the exit code broadcasts as `processExit`.
pub fn spawn(
    host: &crate::host_ctx::HostCtx,
    vm: &AgentOs,
    vars: &mut super::Vars,
    command: &str,
    args: Vec<String>,
    options: SpawnActionOptions,
) -> Result<SpawnHandle> {
    let mut base = ExecOptions::default();
    base.env = options.env;
    if options.cwd.is_some() {
        base.cwd = options.cwd;
    }
    let handle = vm.spawn(
        command,
        args,
        SpawnOptions {
            base,
            stream_stdin: options.stream_stdin,
            ..SpawnOptions::default()
        },
    )?;
    super::shell::spawn_process_output_pumps(host, vm, vars, handle.pid);
    Ok(handle)
}

/// `waitProcess(pid)` — port of [`AgentOs::wait_process`]. Returns the
/// exit code (`i32`).
pub async fn wait_process(vm: &AgentOs, pid: u32) -> Result<i32> {
    vm.wait_process(pid).await.map_err(anyhow::Error::from)
}

/// `killProcess(pid)` — port of [`AgentOs::kill_process`] (sync).
pub fn kill_process(vm: &AgentOs, pid: u32) -> Result<()> {
    vm.kill_process(pid).map_err(anyhow::Error::from)
}

/// `stopProcess(pid)` — port of [`AgentOs::stop_process`] (sync).
pub fn stop_process(vm: &AgentOs, pid: u32) -> Result<()> {
    vm.stop_process(pid).map_err(anyhow::Error::from)
}

/// `listProcesses()` — port of [`AgentOs::list_processes`]. Returns the
/// SDK-spawned processes (not kernel processes); already camelCase via
/// `#[serde(rename = "exitCode")]` on `SpawnedProcessInfo`.
pub fn list_processes(vm: &AgentOs) -> Vec<SpawnedProcessInfo> {
    vm.list_processes()
}

/// `allProcesses()` — port of [`AgentOs::all_processes`]. Returns the
/// full kernel process snapshot.
pub async fn all_processes(vm: &AgentOs) -> Result<Vec<ProcessInfo>> {
    vm.all_processes().await
}

/// `processTree()` — port of [`AgentOs::process_tree`]. Returns the
/// kernel process forest.
pub async fn process_tree(vm: &AgentOs) -> Result<Vec<ProcessTreeNode>> {
    vm.process_tree().await
}

/// `getProcess(pid)` — port of [`AgentOs::get_process`] (sync).
pub fn get_process(vm: &AgentOs, pid: u32) -> Result<SpawnedProcessInfo> {
    vm.get_process(pid).map_err(anyhow::Error::from)
}

/// `writeProcessStdin(pid, data)` — port of
/// [`AgentOs::write_process_stdin`]. Accepts string or bytes content
/// via the same coercion rules as `writeFile`.
pub fn write_process_stdin(
    vm: &AgentOs,
    pid: u32,
    data: super::filesystem::WriteFileContent,
) -> Result<()> {
    use agentos_client::StdinInput;
    let stdin = StdinInput::Bytes(data.into_bytes());
    vm.write_process_stdin(pid, stdin)
        .map_err(anyhow::Error::from)
}

/// `closeProcessStdin(pid)` — port of [`AgentOs::close_process_stdin`].
pub fn close_process_stdin(vm: &AgentOs, pid: u32) -> Result<()> {
    vm.close_process_stdin(pid).map_err(anyhow::Error::from)
}

// ---------------------------------------------------------------------------
// Action reply DTOs
// ---------------------------------------------------------------------------

/// Serializable mirror of [`ExecResult`] with camelCase `exitCode`. The
/// upstream type doesn't derive `Serialize`, and the field name is
/// `exit_code` (snake_case) which the JS test expects as `exitCode`.
#[derive(Serialize)]
pub struct ExecResultDto {
    #[serde(rename = "exitCode")]
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl From<ExecResult> for ExecResultDto {
    fn from(value: ExecResult) -> Self {
        Self {
            exit_code: value.exit_code,
            stdout: value.stdout,
            stderr: value.stderr,
        }
    }
}
