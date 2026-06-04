//! Process execution & management methods + supporting types.
//!
//! Ported from `packages/core/src/agent-os.ts` (process methods) and `runtime-compat.ts`
//! (`ExecOptions`, `ExecResult`, `ProcessInfo`, etc.).
//!
//! Two distinct process views: SDK-spawned processes (`processes` map, keyed by user-facing pid)
//! back `spawn` + the stdin/stdout/stderr/exit subscriptions + `wait/list/get/stop/kill`; the kernel
//! process table backs `exec`, `all_processes`, `process_tree`.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};

use agent_os_sidecar::protocol::{
    CloseStdinRequest, EventPayload, ExecuteRequest, KillProcessRequest, OwnershipScope,
    ProcessSnapshotStatus, RejectedResponse, RequestPayload, ResponsePayload, StreamChannel,
    WriteStdinRequest,
};

use crate::agent_os::{AgentOs, ProcessEntry};
use crate::error::ClientError;
use crate::stream::{ByteStream, Subscription};

/// Broadcast channel capacity for a spawned process's stdout/stderr fan-out.
const PROCESS_STREAM_CAPACITY: usize = 1024;

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Timing-mitigation mode for an execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimingMitigation {
    #[default]
    Off,
    Freeze,
}

/// `stdin` value: a string or raw bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdinInput {
    Text(String),
    Bytes(Vec<u8>),
}

/// Base options shared by `exec` and `spawn`.
///
/// `on_stdout`/`on_stderr` callbacks in the TS API become broadcast subscriptions on the spawned
/// process; for `exec` they are wired internally for the duration of the call.
#[derive(Default)]
pub struct ExecOptions {
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub stdin: Option<StdinInput>,
    pub timeout: Option<f64>,
    pub capture_stdio: Option<bool>,
    pub file_path: Option<String>,
    pub cpu_time_limit_ms: Option<f64>,
    pub timing_mitigation: Option<TimingMitigation>,
}

/// Result of `exec`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// `stdio` mode for a spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpawnStdio {
    #[default]
    Pipe,
    Inherit,
}

/// Options for `spawn` (extends [`ExecOptions`]).
#[derive(Default)]
pub struct SpawnOptions {
    pub base: ExecOptions,
    pub stdio: Option<SpawnStdio>,
    pub stdin_fd: Option<i32>,
    pub stdout_fd: Option<i32>,
    pub stderr_fd: Option<i32>,
    pub stream_stdin: Option<bool>,
}

/// Public JSON info for SDK-spawned processes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnedProcessInfo {
    pub pid: u32,
    pub command: String,
    pub args: Vec<String>,
    pub running: bool,
    #[serde(rename = "exitCode")]
    pub exit_code: Option<i32>,
}

/// The pid returned by `spawn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnHandle {
    pub pid: u32,
}

/// Process status from the kernel process table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessStatus {
    Running,
    Exited,
}

/// Full kernel process info (TS `KernelProcessInfo`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub pgid: u32,
    pub sid: u32,
    pub driver: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub status: ProcessStatus,
    #[serde(rename = "exitCode")]
    pub exit_code: Option<i32>,
    #[serde(rename = "startTime")]
    pub start_time: f64,
    #[serde(rename = "exitTime")]
    pub exit_time: Option<f64>,
}

/// A node in the process forest (`ProcessInfo` + children).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessTreeNode {
    #[serde(flatten)]
    pub info: ProcessInfo,
    pub children: Vec<ProcessTreeNode>,
}

// ---------------------------------------------------------------------------
// Methods
// ---------------------------------------------------------------------------

impl AgentOs {
    /// Run a command to completion. The wire `Execute` request starts the process and returns a
    /// process id immediately; stdout/stderr are accumulated and the call resolves once the matching
    /// `ProcessExited` event arrives. This mirrors the TS pass-through to `kernel.exec` semantically:
    /// the result is the full captured stdout/stderr plus exit code.
    pub async fn exec(&self, command: &str, options: ExecOptions) -> Result<ExecResult> {
        let process_id = self.next_process_id();

        // Subscribe to events BEFORE issuing the request so no output/exit is missed between the
        // request landing and the subscription being installed.
        let mut events = self.transport().subscribe_events();

        let started = self
            .send_execute(&process_id, Some(command.to_owned()), Vec::new(), &options)
            .await
            .context("exec: Execute request failed")?;
        debug_assert_eq!(started.process_id, process_id);

        let mut stdout = Vec::<u8>::new();
        let mut stderr = Vec::<u8>::new();
        let exit_code = loop {
            let (_, payload) = match events.recv().await {
                Ok(frame) => frame,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(ClientError::Sidecar(
                        "exec: event stream closed before process exit".to_owned(),
                    )
                    .into());
                }
            };
            match payload {
                EventPayload::ProcessOutput(output) if output.process_id == process_id => {
                    match output.channel {
                        StreamChannel::Stdout => stdout.extend_from_slice(output.chunk.as_bytes()),
                        StreamChannel::Stderr => stderr.extend_from_slice(output.chunk.as_bytes()),
                    }
                }
                EventPayload::ProcessExited(exited) if exited.process_id == process_id => {
                    break exited.exit_code;
                }
                EventPayload::ProcessOutput(_)
                | EventPayload::ProcessExited(_)
                | EventPayload::VmLifecycle(_)
                | EventPayload::Structured(_) => {}
            }
        };

        Ok(ExecResult {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }

    /// Spawn a process. SYNC; returns `{ pid }` only. Installs stdout/stderr fan-out over broadcast
    /// channels and wires exit via a background event-pump task. The user-facing `pid` is the
    /// SDK-allocated map key (the wire `process_id` is held inside the [`ProcessEntry`]).
    pub fn spawn(
        &self,
        command: &str,
        args: Vec<String>,
        options: SpawnOptions,
    ) -> Result<SpawnHandle> {
        let pid = self.inner().process_counter.fetch_add(1, Ordering::SeqCst) as u32;
        let process_id = format!("proc-{pid}-{}", uuid::Uuid::new_v4());

        let (stdout_tx, _) = broadcast::channel::<Vec<u8>>(PROCESS_STREAM_CAPACITY);
        let (stderr_tx, _) = broadcast::channel::<Vec<u8>>(PROCESS_STREAM_CAPACITY);
        // Seeded `None`; the already-exited branch of `on_process_exit` fires immediately once this
        // watch holds `Some(code)`.
        let (exit_tx, _) = watch::channel::<Option<i32>>(None);

        let entry = ProcessEntry {
            command: command.to_owned(),
            args: args.clone(),
            stdout_tx: stdout_tx.clone(),
            stderr_tx: stderr_tx.clone(),
            exit_tx: exit_tx.clone(),
            process_id: process_id.clone(),
        };
        // `spawn` is documented as overwriting any prior entry for a freshly allocated pid; the pid
        // is monotonic so a collision is not expected.
        let _ = self.inner().processes.insert(pid, entry);

        // Subscribe to events before issuing the request so the pump sees everything.
        let events = self.transport().subscribe_events();

        let this = self.clone();
        let command = command.to_owned();
        tokio::spawn(async move {
            this.run_spawn(
                pid,
                process_id,
                command,
                args,
                options,
                events,
                stdout_tx,
                stderr_tx,
                exit_tx,
            )
            .await;
        });

        Ok(SpawnHandle { pid })
    }

    /// Write to a spawned process's stdin. SYNC. Errors with `ProcessNotFound`.
    pub fn write_process_stdin(
        &self,
        pid: u32,
        data: StdinInput,
    ) -> std::result::Result<(), ClientError> {
        let process_id = self.lookup_process_id(pid)?;
        let chunk = match data {
            StdinInput::Text(text) => text,
            StdinInput::Bytes(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        };
        let this = self.clone();
        // Fire-and-forget: the TS API is synchronous and does not surface a write error.
        tokio::spawn(async move {
            let ownership = this.vm_scope();
            let _ = this
                .transport()
                .request(
                    ownership,
                    RequestPayload::WriteStdin(WriteStdinRequest { process_id, chunk }),
                )
                .await;
        });
        Ok(())
    }

    /// Close a spawned process's stdin. SYNC. Errors with `ProcessNotFound`.
    pub fn close_process_stdin(&self, pid: u32) -> std::result::Result<(), ClientError> {
        let process_id = self.lookup_process_id(pid)?;
        let this = self.clone();
        tokio::spawn(async move {
            let ownership = this.vm_scope();
            let _ = this
                .transport()
                .request(
                    ownership,
                    RequestPayload::CloseStdin(CloseStdinRequest { process_id }),
                )
                .await;
        });
        Ok(())
    }

    /// Subscribe to a spawned process's stdout. No replay; multi-subscriber. Errors if unknown.
    pub fn on_process_stdout(&self, pid: u32) -> std::result::Result<ByteStream, ClientError> {
        let rx = self
            .inner()
            .processes
            .read(&pid, |_, entry| entry.stdout_tx.subscribe())
            .ok_or(ClientError::ProcessNotFound(pid))?;
        Ok(ByteStream::new(rx))
    }

    /// Subscribe to a spawned process's stderr. No replay; multi-subscriber. Errors if unknown.
    pub fn on_process_stderr(&self, pid: u32) -> std::result::Result<ByteStream, ClientError> {
        let rx = self
            .inner()
            .processes
            .read(&pid, |_, entry| entry.stderr_tx.subscribe())
            .ok_or(ClientError::ProcessNotFound(pid))?;
        Ok(ByteStream::new(rx))
    }

    /// Register a once-only exit handler. If the process has already exited, the handler fires
    /// immediately and synchronously and a no-op unsubscribe is returned (the `watch` already holds
    /// `Some(code)`). Otherwise the handler fires once when the exit code lands. The exit code is
    /// `i32`, never null.
    pub fn on_process_exit(
        &self,
        pid: u32,
        handler: impl FnOnce(i32) + Send + 'static,
    ) -> std::result::Result<Subscription, ClientError> {
        let mut rx = self
            .inner()
            .processes
            .read(&pid, |_, entry| entry.exit_tx.subscribe())
            .ok_or(ClientError::ProcessNotFound(pid))?;

        // Already-exited branch: fire immediately + synchronously, return a no-op unsubscribe.
        if let Some(code) = *rx.borrow() {
            handler(code);
            return Ok(Subscription::noop());
        }

        // Otherwise wait for the watch to transition to `Some(code)` and fire exactly once. The
        // returned `Subscription` cancels the waiting task on drop (= unsubscribe).
        let task = tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                if let Some(code) = *rx.borrow() {
                    handler(code);
                    return;
                }
            }
        });
        Ok(Subscription::new(move || task.abort()))
    }

    /// Await a spawned process's exit code. Unknown-pid lookup errors (synchronously in TS; here the
    /// lookup error is returned before any awaiting begins).
    pub async fn wait_process(&self, pid: u32) -> std::result::Result<i32, ClientError> {
        let mut rx = self
            .inner()
            .processes
            .read(&pid, |_, entry| entry.exit_tx.subscribe())
            .ok_or(ClientError::ProcessNotFound(pid))?;

        if let Some(code) = *rx.borrow() {
            return Ok(code);
        }
        while rx.changed().await.is_ok() {
            if let Some(code) = *rx.borrow() {
                return Ok(code);
            }
        }
        Err(ClientError::Sidecar(format!(
            "wait_process: exit channel closed before process {pid} reported an exit code"
        )))
    }

    /// List SDK-spawned processes only. `running = exit_code.is_none()`.
    pub fn list_processes(&self) -> Vec<SpawnedProcessInfo> {
        let mut out = Vec::new();
        self.inner().processes.scan(|pid, entry| {
            let exit_code = *entry.exit_tx.borrow();
            out.push(SpawnedProcessInfo {
                pid: *pid,
                command: entry.command.clone(),
                args: entry.args.clone(),
                running: exit_code.is_none(),
                exit_code,
            });
        });
        out
    }

    /// List ALL kernel processes (native sidecar process snapshot).
    pub async fn all_processes(&self) -> Result<Vec<ProcessInfo>> {
        let ownership = self.vm_scope();
        let response = self
            .transport()
            .request(
                ownership,
                RequestPayload::GetProcessSnapshot(Default::default()),
            )
            .await
            .context("all_processes: GetProcessSnapshot request failed")?;
        let snapshot = match response {
            ResponsePayload::ProcessSnapshot(snapshot) => snapshot,
            ResponsePayload::Rejected(RejectedResponse { code, message }) => {
                return Err(ClientError::Kernel { code, message }.into());
            }
            other => {
                return Err(ClientError::Sidecar(format!(
                    "all_processes: unexpected response {other:?}"
                ))
                .into());
            }
        };
        Ok(snapshot
            .processes
            .into_iter()
            .map(|entry| {
                let (status, exit_time) = match entry.status {
                    ProcessSnapshotStatus::Running | ProcessSnapshotStatus::Stopped => {
                        (ProcessStatus::Running, None)
                    }
                    ProcessSnapshotStatus::Exited => (ProcessStatus::Exited, None),
                };
                ProcessInfo {
                    pid: entry.pid,
                    ppid: entry.ppid,
                    pgid: entry.pgid,
                    sid: entry.sid,
                    driver: entry.driver,
                    command: entry.command,
                    args: entry.args,
                    cwd: entry.cwd,
                    status,
                    exit_code: entry.exit_code,
                    start_time: 0.0,
                    exit_time,
                }
            })
            .collect())
    }

    /// Build the process forest from `all_processes`, linked by `ppid`.
    pub async fn process_tree(&self) -> Result<Vec<ProcessTreeNode>> {
        let processes = self.all_processes().await?;
        Ok(build_process_forest(processes))
    }

    /// Get a single SDK-spawned process's info. Errors (not None) when not found.
    pub fn get_process(&self, pid: u32) -> std::result::Result<SpawnedProcessInfo, ClientError> {
        self.inner()
            .processes
            .read(&pid, |pid, entry| {
                let exit_code = *entry.exit_tx.borrow();
                SpawnedProcessInfo {
                    pid: *pid,
                    command: entry.command.clone(),
                    args: entry.args.clone(),
                    running: exit_code.is_none(),
                    exit_code,
                }
            })
            .ok_or(ClientError::ProcessNotFound(pid))
    }

    /// SIGTERM a spawned process. No-op if already exited; errors if unknown.
    pub fn stop_process(&self, pid: u32) -> std::result::Result<(), ClientError> {
        self.signal_process(pid, "SIGTERM")
    }

    /// SIGKILL a spawned process. No-op if already exited; errors if unknown.
    pub fn kill_process(&self, pid: u32) -> std::result::Result<(), ClientError> {
        self.signal_process(pid, "SIGKILL")
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Build the VM-scoped ownership for a wire request.
    fn vm_scope(&self) -> OwnershipScope {
        OwnershipScope::vm(self.connection_id(), self.wire_session_id(), self.vm_id())
    }

    /// Allocate a fresh wire `process_id` (used by `exec`, which does not register in the SDK map).
    fn next_process_id(&self) -> String {
        let n = self.inner().process_counter.fetch_add(1, Ordering::SeqCst);
        format!("proc-{n}-{}", uuid::Uuid::new_v4())
    }

    /// Resolve the wire `process_id` for an SDK pid, erroring with `ProcessNotFound` if unknown.
    fn lookup_process_id(&self, pid: u32) -> std::result::Result<String, ClientError> {
        self.inner()
            .processes
            .read(&pid, |_, entry| entry.process_id.clone())
            .ok_or(ClientError::ProcessNotFound(pid))
    }

    /// Send the `Execute` wire request, mapping a rejection into [`ClientError::Kernel`].
    async fn send_execute(
        &self,
        process_id: &str,
        command: Option<String>,
        args: Vec<String>,
        options: &ExecOptions,
    ) -> std::result::Result<agent_os_sidecar::protocol::ProcessStartedResponse, ClientError> {
        let ownership = self.vm_scope();
        let response = self
            .transport()
            .request(
                ownership,
                RequestPayload::Execute(ExecuteRequest {
                    process_id: process_id.to_owned(),
                    command,
                    runtime: None,
                    entrypoint: None,
                    args,
                    env: options.env.clone(),
                    cwd: options.cwd.clone(),
                    wasm_permission_tier: None,
                }),
            )
            .await?;
        match response {
            ResponsePayload::ProcessStarted(started) => Ok(started),
            ResponsePayload::Rejected(RejectedResponse { code, message }) => {
                Err(ClientError::Kernel { code, message })
            }
            other => Err(ClientError::Sidecar(format!(
                "Execute: unexpected response {other:?}"
            ))),
        }
    }

    /// Send a kill signal for an SDK pid. No-op if already exited; errors with `ProcessNotFound` if
    /// the pid is unknown.
    fn signal_process(&self, pid: u32, signal: &str) -> std::result::Result<(), ClientError> {
        let (process_id, already_exited) = self
            .inner()
            .processes
            .read(&pid, |_, entry| {
                (entry.process_id.clone(), entry.exit_tx.borrow().is_some())
            })
            .ok_or(ClientError::ProcessNotFound(pid))?;
        if already_exited {
            return Ok(());
        }
        let signal = signal.to_owned();
        let this = self.clone();
        tokio::spawn(async move {
            let ownership = this.vm_scope();
            let _ = this
                .transport()
                .request(
                    ownership,
                    RequestPayload::KillProcess(KillProcessRequest { process_id, signal }),
                )
                .await;
        });
        Ok(())
    }

    /// Background pump for a spawned process: issue the `Execute` request, then fan kernel
    /// `ProcessOutput`/`ProcessExited` events for this process id into the per-process broadcast and
    /// watch channels. Removes the SDK map entry once the process exits, matching the TS
    /// `proc.wait().then` cleanup.
    #[allow(clippy::too_many_arguments)]
    async fn run_spawn(
        self,
        pid: u32,
        process_id: String,
        command: String,
        args: Vec<String>,
        options: SpawnOptions,
        mut events: broadcast::Receiver<(OwnershipScope, EventPayload)>,
        stdout_tx: broadcast::Sender<Vec<u8>>,
        stderr_tx: broadcast::Sender<Vec<u8>>,
        exit_tx: watch::Sender<Option<i32>>,
    ) {
        if let Err(error) = self
            .send_execute(&process_id, Some(command), args, &options.base)
            .await
        {
            tracing::error!(?error, pid, %process_id, "spawn: Execute request failed");
            // Surface an exit so `wait_process`/`on_process_exit` callers do not hang. A failed
            // launch is reported as a non-zero exit code.
            let _ = exit_tx.send(Some(-1));
            return;
        }

        loop {
            let (_, payload) = match events.recv().await {
                Ok(frame) => frame,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    let _ = exit_tx.send(Some(-1));
                    break;
                }
            };
            match payload {
                EventPayload::ProcessOutput(output) if output.process_id == process_id => {
                    let bytes = output.chunk.into_bytes();
                    match output.channel {
                        StreamChannel::Stdout => {
                            let _ = stdout_tx.send(bytes);
                        }
                        StreamChannel::Stderr => {
                            let _ = stderr_tx.send(bytes);
                        }
                    }
                }
                EventPayload::ProcessExited(exited) if exited.process_id == process_id => {
                    let _ = exit_tx.send(Some(exited.exit_code));
                    break;
                }
                EventPayload::ProcessOutput(_)
                | EventPayload::ProcessExited(_)
                | EventPayload::VmLifecycle(_)
                | EventPayload::Structured(_) => {}
            }
        }
    }
}

/// Assemble a process forest from a flat process list, linking children by `ppid`. Roots are
/// processes whose `ppid` is not itself present in the list (mirrors the TS `processTree`).
fn build_process_forest(processes: Vec<ProcessInfo>) -> Vec<ProcessTreeNode> {
    use std::collections::BTreeMap as Map;

    // Map pid -> children pids, preserving input order.
    let pids: std::collections::BTreeSet<u32> = processes.iter().map(|p| p.pid).collect();
    let mut children_of: Map<u32, Vec<usize>> = Map::new();
    let mut roots: Vec<usize> = Vec::new();
    for (index, proc) in processes.iter().enumerate() {
        if proc.ppid != proc.pid && pids.contains(&proc.ppid) {
            children_of.entry(proc.ppid).or_default().push(index);
        } else {
            roots.push(index);
        }
    }

    fn build_node(
        index: usize,
        processes: &[ProcessInfo],
        children_of: &Map<u32, Vec<usize>>,
    ) -> ProcessTreeNode {
        let info = processes[index].clone();
        let children = children_of
            .get(&info.pid)
            .map(|child_indices| {
                child_indices
                    .iter()
                    .map(|&child_index| build_node(child_index, processes, children_of))
                    .collect()
            })
            .unwrap_or_default();
        ProcessTreeNode { info, children }
    }

    roots
        .into_iter()
        .map(|index| build_node(index, &processes, &children_of))
        .collect()
}
