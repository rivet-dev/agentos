//! Shell actions — the actor-side port of the `AgentOs` PTY shell surface
//! (`openShell` / `writeShell` / `resizeShell` / `closeShell` / `waitShell`).
//!
//! `open_shell` subscribes the shell's stdout/stderr streams and pumps each
//! chunk to connected clients as `shellData` / `shellStderr` broadcasts (the
//! event-driven mirror of the TS `onShellData` subscription); a third task
//! broadcasts `shellExit` when the shell process exits. Pump task handles are
//! tracked in [`super::Vars::shell_tasks`] so VM teardown aborts them.

use agentos_client::{AgentOs, OpenShellOptions, StdinInput};
use anyhow::Result;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use crate::host_ctx::HostCtx;

use super::Vars;

/// JSON options for the `openShell` action — the serializable subset of the TS
/// `OpenShellOptions` (callbacks are replaced by the broadcast events).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenShellActionOptions {
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

/// Reply DTO for `openShell` (`{ shellId }`).
#[derive(Serialize)]
pub struct OpenShellDto {
    #[serde(rename = "shellId")]
    pub shell_id: String,
}

/// Broadcast one event whose single handler argument is `payload`. The wire
/// body is the CBOR array of handler args with the JSON-compat byte wrapping
/// (`["$Uint8Array", base64]`), byte-exact with the action-reply encoding, so
/// `data: ByteBuf` fields arrive client-side as real `Uint8Array`s.
fn broadcast_event<T: Serialize>(host: &HostCtx, name: &[u8], payload: &T) {
    match super::encode_event_arg(payload) {
        Ok(bytes) => {
            let status = host.broadcast(name.to_vec(), bytes);
            if status != rivet_actor_plugin_abi::AbiStatus::Ok {
                // A dropped delivery is invisible client-side (terminals just
                // miss output); the failure must at least be host-visible.
                tracing::warn!(
                    ?status,
                    event = %String::from_utf8_lossy(name),
                    "shell event broadcast failed"
                );
            }
        }
        Err(error) => {
            tracing::warn!(?error, "failed to encode shell event broadcast");
        }
    }
}

#[derive(Serialize)]
struct ShellDataEvent<'a> {
    #[serde(rename = "shellId")]
    shell_id: &'a str,
    data: serde_bytes::ByteBuf,
}

#[derive(Serialize)]
struct ShellExitEvent<'a> {
    #[serde(rename = "shellId")]
    shell_id: &'a str,
    #[serde(rename = "exitCode")]
    exit_code: i32,
}

pub(crate) fn encode_shell_data_event(shell_id: &str, data: Vec<u8>) -> Result<Vec<u8>> {
    super::encode_event_arg(&ShellDataEvent {
        shell_id,
        data: serde_bytes::ByteBuf::from(data),
    })
}

pub(crate) fn encode_shell_stderr_event(shell_id: &str, data: Vec<u8>) -> Result<Vec<u8>> {
    encode_shell_data_event(shell_id, data)
}

pub(crate) fn encode_shell_exit_event(shell_id: &str, exit_code: i32) -> Result<Vec<u8>> {
    super::encode_event_arg(&ShellExitEvent {
        shell_id,
        exit_code,
    })
}

/// `openShell(options)` — port of [`AgentOs::open_shell`]. Subscribes the
/// data/stderr streams and the exit code, forwarding them as `shellData` /
/// `shellStderr` / `shellExit` broadcasts.
pub fn open_shell(
    host: &HostCtx,
    vm: &AgentOs,
    vars: &mut Vars,
    options: OpenShellActionOptions,
) -> Result<OpenShellDto> {
    let handle = vm.open_shell(OpenShellOptions {
        command: options.command,
        args: options.args,
        env: options.env,
        cwd: options.cwd,
        cols: options.cols,
        rows: options.rows,
        on_stderr: None,
    })?;
    let shell_id = handle.shell_id;

    let mut data_stream = vm.on_shell_data(&shell_id)?;
    let mut stderr_stream = vm.on_shell_stderr(&shell_id)?;

    let data_host = host.clone();
    let data_shell_id = shell_id.clone();
    vars.track_shell_task(tokio::spawn(async move {
        while let Some(chunk) = data_stream.next().await {
            broadcast_event(
                &data_host,
                b"shellData",
                &ShellDataEvent {
                    shell_id: &data_shell_id,
                    data: serde_bytes::ByteBuf::from(chunk),
                },
            );
        }
    }));

    let stderr_host = host.clone();
    let stderr_shell_id = shell_id.clone();
    vars.track_shell_task(tokio::spawn(async move {
        while let Some(chunk) = stderr_stream.next().await {
            broadcast_event(
                &stderr_host,
                b"shellStderr",
                &ShellDataEvent {
                    shell_id: &stderr_shell_id,
                    data: serde_bytes::ByteBuf::from(chunk),
                },
            );
        }
    }));

    let exit_host = host.clone();
    let exit_vm = vm.clone();
    let exit_shell_id = shell_id.clone();
    vars.track_shell_task(tokio::spawn(async move {
        match exit_vm.wait_shell(&exit_shell_id).await {
            Ok(exit_code) => broadcast_event(
                &exit_host,
                b"shellExit",
                &ShellExitEvent {
                    shell_id: &exit_shell_id,
                    exit_code,
                },
            ),
            // No shellExit reaches clients on this path; VM teardown is covered
            // by the vmShutdown broadcast, but the failure itself must not vanish.
            Err(error) => tracing::warn!(
                ?error,
                shell_id = %exit_shell_id,
                "wait_shell failed; shellExit not broadcast"
            ),
        }
    }));

    Ok(OpenShellDto { shell_id })
}

#[derive(Serialize)]
struct ProcessOutputEvent<'a> {
    pid: u32,
    stream: &'a str,
    data: serde_bytes::ByteBuf,
}

#[derive(Serialize)]
struct ProcessExitEvent {
    pid: u32,
    #[serde(rename = "exitCode")]
    exit_code: i32,
}

pub(crate) fn encode_process_output_event(
    pid: u32,
    stream: &str,
    data: Vec<u8>,
) -> Result<Vec<u8>> {
    super::encode_event_arg(&ProcessOutputEvent {
        pid,
        stream,
        data: serde_bytes::ByteBuf::from(data),
    })
}

pub(crate) fn encode_process_exit_event(pid: u32, exit_code: i32) -> Result<Vec<u8>> {
    super::encode_event_arg(&ProcessExitEvent { pid, exit_code })
}

/// Subscribe a spawned process's stdout/stderr/exit and forward them to
/// connected clients as `processOutput` / `processExit` broadcasts. Used by
/// the `spawn` action so actor clients get the streaming the TS `SpawnOptions`
/// callbacks provide in-process.
pub fn spawn_process_output_pumps(host: &HostCtx, vm: &AgentOs, vars: &mut Vars, pid: u32) {
    let stdout = vm.on_process_stdout(pid);
    let stderr = vm.on_process_stderr(pid);

    if let Ok(mut stream) = stdout {
        let host = host.clone();
        vars.track_shell_task(tokio::spawn(async move {
            while let Some(chunk) = stream.next().await {
                broadcast_event(
                    &host,
                    b"processOutput",
                    &ProcessOutputEvent {
                        pid,
                        stream: "stdout",
                        data: serde_bytes::ByteBuf::from(chunk),
                    },
                );
            }
        }));
    }
    if let Ok(mut stream) = stderr {
        let host = host.clone();
        vars.track_shell_task(tokio::spawn(async move {
            while let Some(chunk) = stream.next().await {
                broadcast_event(
                    &host,
                    b"processOutput",
                    &ProcessOutputEvent {
                        pid,
                        stream: "stderr",
                        data: serde_bytes::ByteBuf::from(chunk),
                    },
                );
            }
        }));
    }
    let host = host.clone();
    let vm = vm.clone();
    vars.track_shell_task(tokio::spawn(async move {
        if let Ok(exit_code) = vm.wait_process(pid).await {
            broadcast_event(&host, b"processExit", &ProcessExitEvent { pid, exit_code });
        }
    }));
}

/// `writeShell(shellId, data)` — port of [`AgentOs::write_shell`], but awaited
/// so a failed wire write rejects the action instead of vanishing into the
/// fire-and-forget warn.
pub async fn write_shell(
    vm: &AgentOs,
    shell_id: &str,
    data: super::filesystem::WriteFileContent,
) -> Result<()> {
    vm.write_shell_awaited(shell_id, StdinInput::Bytes(data.into_bytes()))
        .await
        .map_err(anyhow::Error::from)
}

/// `resizeShell(shellId, cols, rows)` — port of [`AgentOs::resize_shell`].
pub fn resize_shell(vm: &AgentOs, shell_id: &str, cols: u16, rows: u16) -> Result<()> {
    vm.resize_shell(shell_id, cols, rows)
        .map_err(anyhow::Error::from)
}

/// `closeShell(shellId)` — port of [`AgentOs::close_shell`].
pub fn close_shell(vm: &AgentOs, shell_id: &str) -> Result<()> {
    vm.close_shell(shell_id).map_err(anyhow::Error::from)
}

/// `waitShell(shellId)` — port of [`AgentOs::wait_shell`]. Returns the exit code.
pub async fn wait_shell(vm: &AgentOs, shell_id: &str) -> Result<i32> {
    vm.wait_shell(shell_id).await.map_err(anyhow::Error::from)
}
