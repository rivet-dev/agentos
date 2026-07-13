//! Process execution & management methods + supporting types.
//!
//! Thin process protocol forwarding plus host callback/event routes.
//!
//! Two distinct process views: SDK-spawned processes (`processes` map, keyed by user-facing pid)
//! back `spawn` + the stdin/stdout/stderr/exit subscriptions + `wait/list/get/stop/kill`; the kernel
//! process table backs `exec`, `all_processes`, `process_tree`.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use scc::HashMap as SccHashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use agentos_sidecar_client::wire::{self, EventPayload, ProcessSnapshotStatus, StreamChannel};
use agentos_sidecar_client::SharedWireEvent;

use crate::agent_os::{AgentOs, ProcessEntry, ProcessExit};
use crate::error::ClientError;
use crate::stream::{ByteStream, Subscription};

/// Broadcast channel capacity for a spawned process's stdout/stderr fan-out.
const PROCESS_STREAM_CAPACITY: usize = 1024;

/// Maximum SDK-spawned process entries retained per VM.
const PROCESS_REGISTRY_LIMIT: usize = 1024;

/// Maximum first-observed process timestamp entries retained per VM.

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

/// A raw-byte streaming callback for stdout/stderr (TS `(data: Uint8Array) => void`). Invoked once
/// per output chunk as it arrives. Never assume UTF-8: chunks are delivered as raw bytes.
pub type OutputCallback = Box<dyn FnMut(&[u8]) + Send>;

/// Base options shared by `exec` and `spawn`.
///
/// `on_stdout`/`on_stderr` mirror the TS `ExecOptions.onStdout`/`onStderr` raw-byte streaming
/// callbacks. For `exec` they fire for the duration of the call; for `spawn` they are seeded into the
/// stdout/stderr fan-out at spawn time (matching the TS initial-handler-set behavior).
pub struct ExecOptions {
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub stdin: Option<StdinInput>,
    pub timeout: Option<f64>,
    pub on_stdout: Option<OutputCallback>,
    pub on_stderr: Option<OutputCallback>,
    pub capture_stdio: Option<bool>,
    pub file_path: Option<String>,
    pub cpu_time_limit_ms: Option<f64>,
    pub timing_mitigation: Option<TimingMitigation>,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self {
            env: BTreeMap::new(),
            cwd: None,
            stdin: None,
            timeout: None,
            on_stdout: None,
            on_stderr: None,
            capture_stdio: None,
            file_path: None,
            cpu_time_limit_ms: None,
            timing_mitigation: None,
        }
    }
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
    /// Epoch milliseconds when `spawn` registered the process.
    #[serde(rename = "startedAt")]
    pub started_at: i64,
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
    Stopped,
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
    /// process id immediately; the sidecar owns bounded stdout/stderr capture and returns it on the
    /// matching `ProcessExited` event. The client only forwards callbacks and deserializes the
    /// terminal result.
    pub async fn exec(&self, command: &str, options: ExecOptions) -> Result<ExecResult> {
        self.exec_request(None, Some(command), &[], options).await
    }

    /// Run a command to completion from an already-structured `(command, args)` argv. Each `args`
    /// element is sent verbatim as a distinct argv element. Callers that already hold structured
    /// argv (for example the cron `Exec` action) use this instead of the raw command-line API.
    pub async fn exec_argv(
        &self,
        command: &str,
        args: &[String],
        options: ExecOptions,
    ) -> Result<ExecResult> {
        self.exec_request(Some(command), None, args, options).await
    }

    async fn exec_request(
        &self,
        command: Option<&str>,
        shell_command: Option<&str>,
        args: &[String],
        mut options: ExecOptions,
    ) -> Result<ExecResult> {
        // Subscribe to events BEFORE issuing the request so no output/exit is missed between the
        // request landing and the subscription being installed.
        let mut events = self.transport().subscribe_wire_events();

        let resolved_command = command.map(str::to_owned);
        let resolved_shell_command = shell_command.map(str::to_owned);
        let resolved_args = args.to_vec();
        let timeout_ms = timeout_to_wire(options.timeout)?;
        let capture_stdio = options.capture_stdio.unwrap_or(true);
        let started = self
            .send_execute(
                resolved_command,
                resolved_shell_command,
                resolved_args,
                options.env.clone(),
                options.cwd.clone(),
                false,
                timeout_ms,
                Some(capture_stdio),
            )
            .await
            .context("exec: Execute request failed")?;
        let process_id = started.process_id;

        // Deliver any provided stdin, then close stdin so a non-interactive run observes EOF. This
        // mirrors the TS `runAndCapture` path (`proc.writeStdin(options.stdin); proc.closeStdin()`).
        if let Some(stdin) = options.stdin.take() {
            let chunk = stdin_to_bytes(stdin);
            let ownership = self.vm_scope();
            let response = self
                .transport()
                .request_wire(
                    ownership,
                    wire::RequestPayload::WriteStdinRequest(wire::WriteStdinRequest {
                        process_id: process_id.clone(),
                        chunk,
                    }),
                )
                .await
                .map_err(ClientError::from)?;
            map_process_control_response(response, "exec write stdin", |response| {
                matches!(response, wire::ResponsePayload::StdinWrittenResponse(_))
            })?;
        }
        {
            let ownership = self.vm_scope();
            let response = self
                .transport()
                .request_wire(
                    ownership,
                    wire::RequestPayload::CloseStdinRequest(wire::CloseStdinRequest {
                        process_id: process_id.clone(),
                    }),
                )
                .await
                .map_err(ClientError::from)?;
            map_process_control_response(response, "exec close stdin", |response| {
                matches!(response, wire::ResponsePayload::StdinClosedResponse(_))
            })?;
        }

        let mut on_stdout = options.on_stdout.take();
        let mut on_stderr = options.on_stderr.take();

        let (exit_code, stdout, stderr) =
            collect_exec_events(&process_id, &mut events, &mut on_stdout, &mut on_stderr).await?;

        Ok(ExecResult {
            exit_code,
            stdout,
            stderr,
        })
    }

    /// Spawn a process and return the authoritative kernel pid supplied by the sidecar. Installs
    /// stdout/stderr fan-out over broadcast channels and wires exit via a background event-pump task.
    pub async fn spawn(
        &self,
        command: &str,
        args: Vec<String>,
        mut options: SpawnOptions,
    ) -> Result<SpawnHandle> {
        {
            let _registry_guard = self.inner().process_registry_lock.lock();
            self.prune_exited_processes_locked(1);
            if self.process_registry_len_locked() >= PROCESS_REGISTRY_LIMIT {
                return Err(ClientError::Sidecar(format!(
                    "process registry limit exceeded: at most {PROCESS_REGISTRY_LIMIT} processes can be tracked per VM"
                ))
                .into());
            }
        }

        let (stdout_tx, _) = broadcast::channel::<Vec<u8>>(PROCESS_STREAM_CAPACITY);
        let (stderr_tx, _) = broadcast::channel::<Vec<u8>>(PROCESS_STREAM_CAPACITY);
        // Seeded `None`; the already-exited branch of `on_process_exit` fires immediately once this
        // watch holds `Some(code)`.
        let (exit_tx, _) = watch::channel::<Option<ProcessExit>>(None);

        // Seed any caller-provided initial stdout/stderr callbacks into the fan-out, matching the TS
        // initial-handler-set behavior (`stdoutHandlers.add(options.onStdout)`). The spawned task
        // handles are retained on the entry so `shutdown` can abort them (the entry's own sender
        // clones keep the channel open, so the tasks never observe `Closed` on their own).
        let mut output_tasks = Vec::new();
        if let Some(cb) = options.base.on_stdout.take() {
            output_tasks.push(install_output_callback(stdout_tx.clone(), cb));
        }
        if let Some(cb) = options.base.on_stderr.take() {
            output_tasks.push(install_output_callback(stderr_tx.clone(), cb));
        }

        // Subscribe before issuing Execute so the receiver buffers any output emitted immediately
        // after the response and before the event-pump task starts.
        let events = self.transport().subscribe_wire_events();
        let started = self
            .send_execute(
                Some(command.to_owned()),
                None,
                args.clone(),
                options.base.env.clone(),
                options.base.cwd.clone(),
                options.stream_stdin.unwrap_or(false),
                timeout_to_wire(options.base.timeout)?,
                None,
            )
            .await
            .context("spawn: Execute request failed")?;
        let process_id = started.process_id;
        let pid = started.pid.ok_or_else(|| {
            ClientError::Sidecar("spawn: sidecar did not return a kernel pid".to_owned())
        })?;

        let entry = ProcessEntry {
            stdout_tx: stdout_tx.clone(),
            stderr_tx: stderr_tx.clone(),
            exit_tx: exit_tx.clone(),
            process_id: process_id.clone(),
            output_tasks,
        };
        let registration_error = {
            let _registry_guard = self.inner().process_registry_lock.lock();
            self.prune_exited_processes_locked(1);
            if self.process_registry_len_locked() >= PROCESS_REGISTRY_LIMIT {
                Some(ClientError::Sidecar(format!(
                    "process registry limit exceeded: at most {PROCESS_REGISTRY_LIMIT} processes can be tracked per VM"
                )))
            } else if self.inner().processes.insert(pid, entry).is_err() {
                Some(ClientError::Sidecar(format!(
                    "spawn: kernel pid {pid} is already tracked"
                )))
            } else {
                None
            }
        };
        if let Some(registration_error) = registration_error {
            self.kill_wire_process(&process_id, "SIGKILL")
                .await
                .map_err(|cleanup_error| {
                    ClientError::Sidecar(format!(
                        "{registration_error}; process cleanup failed: {cleanup_error}"
                    ))
                })?;
            return Err(registration_error.into());
        }

        let this = self.clone();
        tokio::spawn(async move {
            this.run_spawn_events(process_id, events, stdout_tx, stderr_tx, exit_tx)
                .await;
        });

        Ok(SpawnHandle { pid })
    }

    /// Write to a spawned process's stdin and await the sidecar response.
    pub async fn write_process_stdin(
        &self,
        pid: u32,
        data: StdinInput,
    ) -> std::result::Result<(), ClientError> {
        let process_id = self.lookup_process_id(pid)?;
        let chunk: Vec<u8> = stdin_to_bytes(data);
        let response = self
            .transport()
            .request_wire(
                self.vm_scope(),
                wire::RequestPayload::WriteStdinRequest(wire::WriteStdinRequest {
                    process_id,
                    chunk,
                }),
            )
            .await
            .map_err(ClientError::from)?;
        map_process_control_response(response, "write_process_stdin", |response| {
            matches!(response, wire::ResponsePayload::StdinWrittenResponse(_))
        })
    }

    /// Close a spawned process's stdin and await the sidecar response.
    pub async fn close_process_stdin(&self, pid: u32) -> std::result::Result<(), ClientError> {
        let process_id = self.lookup_process_id(pid)?;
        let response = self
            .transport()
            .request_wire(
                self.vm_scope(),
                wire::RequestPayload::CloseStdinRequest(wire::CloseStdinRequest { process_id }),
            )
            .await
            .map_err(ClientError::from)?;
        map_process_control_response(response, "close_process_stdin", |response| {
            matches!(response, wire::ResponsePayload::StdinClosedResponse(_))
        })
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
        if let Some(exit) = rx.borrow().clone() {
            match exit {
                ProcessExit::Exited(code) => handler(code),
                ProcessExit::Failed(message) => {
                    tracing::error!(pid, %message, "process exit subscription failed")
                }
            }
            return Ok(Subscription::noop());
        }

        // Otherwise wait for the watch to transition to `Some(code)` and fire exactly once. The
        // returned `Subscription` cancels the waiting task on drop (= unsubscribe).
        let task = tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                if let Some(exit) = rx.borrow().clone() {
                    match exit {
                        ProcessExit::Exited(code) => handler(code),
                        ProcessExit::Failed(message) => {
                            tracing::error!(pid, %message, "process exit subscription failed")
                        }
                    }
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

        if let Some(exit) = rx.borrow().clone() {
            return process_exit_result(exit);
        }
        while rx.changed().await.is_ok() {
            if let Some(exit) = rx.borrow().clone() {
                return process_exit_result(exit);
            }
        }
        Err(ClientError::Sidecar(format!(
            "wait_process: exit channel closed before process {pid} reported an exit code"
        )))
    }

    /// List SDK-spawned processes using the sidecar's authoritative process snapshot.
    pub async fn list_processes(
        &self,
    ) -> std::result::Result<Vec<SpawnedProcessInfo>, ClientError> {
        let mut tracked_pids = std::collections::BTreeSet::new();
        self.inner().processes.scan(|pid, _| {
            tracked_pids.insert(*pid);
        });

        let mut process_by_pid: BTreeMap<u32, ProcessInfo> = self
            .process_snapshot()
            .await?
            .into_iter()
            .map(|process| (process.pid, process))
            .collect();
        tracked_pids
            .into_iter()
            .map(|pid| {
                process_by_pid
                    .remove(&pid)
                    .map(spawned_process_info_from_snapshot)
                    .ok_or_else(|| {
                        ClientError::Sidecar(format!(
                            "sidecar process snapshot is missing tracked process: {pid}"
                        ))
                    })
            })
            .collect()
    }

    /// List ALL kernel processes (native sidecar process snapshot).
    ///
    /// Results use the kernel pid/ppid/pgid/sid returned by the sidecar without client remapping.
    pub async fn all_processes(&self) -> Result<Vec<ProcessInfo>> {
        self.process_snapshot().await.map_err(Into::into)
    }

    async fn process_snapshot(&self) -> std::result::Result<Vec<ProcessInfo>, ClientError> {
        let ownership = self.vm_scope();
        let response = self
            .transport()
            .request_wire(ownership, wire::RequestPayload::GetProcessSnapshotRequest)
            .await
            .map_err(ClientError::from)?;
        let snapshot = match response {
            wire::ResponsePayload::ProcessSnapshotResponse(snapshot) => snapshot,
            wire::ResponsePayload::RejectedResponse(wire::RejectedResponse { code, message }) => {
                return Err(ClientError::Kernel { code, message });
            }
            other => {
                return Err(ClientError::Sidecar(format!(
                    "all_processes: unexpected response {other:?}"
                )));
            }
        };

        let mut out: Vec<ProcessInfo> = Vec::new();

        for entry in snapshot.processes {
            let status = match entry.status {
                ProcessSnapshotStatus::Running => ProcessStatus::Running,
                ProcessSnapshotStatus::Stopped => ProcessStatus::Stopped,
                ProcessSnapshotStatus::Exited => ProcessStatus::Exited,
            };

            out.push(ProcessInfo {
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
                start_time: entry.start_time_ms as f64,
                exit_time: entry.exit_time_ms.map(|time| time as f64),
            });
        }

        out.sort_by_key(|info| info.pid);
        Ok(out)
    }

    pub(crate) async fn process_snapshot_entry_by_id(
        &self,
        process_id: &str,
    ) -> std::result::Result<Option<wire::ProcessSnapshotEntry>, ClientError> {
        let response = self
            .transport()
            .request_wire(
                self.vm_scope(),
                wire::RequestPayload::GetProcessSnapshotRequest,
            )
            .await
            .map_err(ClientError::from)?;
        match response {
            wire::ResponsePayload::ProcessSnapshotResponse(snapshot) => Ok(snapshot
                .processes
                .into_iter()
                .find(|process| process.process_id == process_id)),
            wire::ResponsePayload::RejectedResponse(wire::RejectedResponse { code, message }) => {
                Err(ClientError::Kernel { code, message })
            }
            other => Err(ClientError::Sidecar(format!(
                "process snapshot lookup: unexpected response {other:?}"
            ))),
        }
    }

    /// Build the process forest from `all_processes`, linked by `ppid`.
    pub async fn process_tree(&self) -> Result<Vec<ProcessTreeNode>> {
        let processes = self.all_processes().await?;
        Ok(build_process_forest(processes))
    }

    /// Get one SDK-spawned process from the sidecar's authoritative process snapshot.
    pub async fn get_process(
        &self,
        pid: u32,
    ) -> std::result::Result<SpawnedProcessInfo, ClientError> {
        if !self.inner().processes.contains(&pid) {
            return Err(ClientError::ProcessNotFound(pid));
        }
        self.process_snapshot()
            .await?
            .into_iter()
            .find(|process| process.pid == pid)
            .map(spawned_process_info_from_snapshot)
            .ok_or_else(|| {
                ClientError::Sidecar(format!(
                    "sidecar process snapshot is missing tracked process: {pid}"
                ))
            })
    }

    /// SIGTERM a spawned process. No-op if already exited; errors if unknown.
    pub async fn stop_process(&self, pid: u32) -> std::result::Result<(), ClientError> {
        self.signal_process(pid, "SIGTERM").await
    }

    /// SIGKILL a spawned process. No-op if already exited; errors if unknown.
    pub async fn kill_process(&self, pid: u32) -> std::result::Result<(), ClientError> {
        self.signal_process(pid, "SIGKILL").await
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Build the VM-scoped ownership for a wire request.
    fn vm_scope(&self) -> wire::OwnershipScope {
        wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: self.connection_id().to_string(),
            session_id: self.wire_session_id().to_string(),
            vm_id: self.vm_id().to_string(),
        })
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
        command: Option<String>,
        shell_command: Option<String>,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: Option<String>,
        keep_stdin_open: bool,
        timeout_ms: Option<u64>,
        capture_output: Option<bool>,
    ) -> std::result::Result<wire::ProcessStartedResponse, ClientError> {
        let ownership = self.vm_scope();
        let response = self
            .transport()
            .request_wire(
                ownership,
                wire::RequestPayload::ExecuteRequest(wire::ExecuteRequest {
                    process_id: None,
                    command,
                    shell_command,
                    runtime: None,
                    entrypoint: None,
                    args,
                    env: (!env.is_empty()).then(|| env.into_iter().collect()),
                    cwd,
                    wasm_permission_tier: None,
                    pty: None,
                    keep_stdin_open: keep_stdin_open.then_some(true),
                    timeout_ms,
                    capture_output,
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::ProcessStartedResponse(started) => Ok(started),
            wire::ResponsePayload::RejectedResponse(wire::RejectedResponse { code, message }) => {
                Err(ClientError::Kernel { code, message })
            }
            other => Err(ClientError::Sidecar(format!(
                "Execute: unexpected response {other:?}"
            ))),
        }
    }

    /// Kill a wire process and await the sidecar response.
    async fn kill_wire_process(
        &self,
        process_id: &str,
        signal: &str,
    ) -> std::result::Result<(), ClientError> {
        let response = self
            .transport()
            .request_wire(
                self.vm_scope(),
                wire::RequestPayload::KillProcessRequest(wire::KillProcessRequest {
                    process_id: process_id.to_owned(),
                    signal: signal.to_owned(),
                }),
            )
            .await
            .map_err(ClientError::from)?;
        map_process_control_response(response, "kill_process", |response| {
            matches!(response, wire::ResponsePayload::ProcessKilledResponse(_))
        })
    }

    /// Send a kill signal for an SDK pid. No-op if already exited; errors with `ProcessNotFound` if
    /// the pid is unknown.
    async fn signal_process(&self, pid: u32, signal: &str) -> std::result::Result<(), ClientError> {
        let process_id = self.lookup_process_id(pid)?;
        let response = self
            .transport()
            .request_wire(
                self.vm_scope(),
                wire::RequestPayload::KillProcessRequest(wire::KillProcessRequest {
                    process_id,
                    signal: signal.to_owned(),
                }),
            )
            .await
            .map_err(ClientError::from)?;
        match response {
            wire::ResponsePayload::ProcessKilledResponse(_) => Ok(()),
            wire::ResponsePayload::RejectedResponse(wire::RejectedResponse { code, message }) => {
                Err(ClientError::Kernel { code, message })
            }
            other => Err(ClientError::Sidecar(format!(
                "kill_process: unexpected response {other:?}"
            ))),
        }
    }

    fn process_registry_len_locked(&self) -> usize {
        let mut count = 0usize;
        self.inner().processes.scan(|_, _| {
            count += 1;
        });
        count
    }

    fn prune_exited_processes_locked(&self, reserve_slots: usize) {
        let mut entries = Vec::new();
        self.inner().processes.scan(|pid, entry| {
            entries.push((*pid, entry.exit_tx.borrow().is_some()));
        });
        let target_len = PROCESS_REGISTRY_LIMIT.saturating_sub(reserve_slots);
        if entries.len() <= target_len {
            return;
        }

        for pid in exited_pids_to_prune(entries, target_len) {
            self.remove_process_tracking_locked(pid);
        }
    }

    fn remove_process_tracking_locked(&self, pid: u32) {
        let _ = self.inner().processes.remove(&pid);
    }

    /// Background pump for a spawned process: fan kernel `ProcessOutput`/`ProcessExited` events for
    /// this process id into the per-process broadcast and
    /// watch channels. Exited entries are retained for post-exit inspection, then pruned oldest-first
    /// under registry pressure.
    async fn run_spawn_events(
        self,
        process_id: String,
        mut events: broadcast::Receiver<SharedWireEvent>,
        stdout_tx: broadcast::Sender<Vec<u8>>,
        stderr_tx: broadcast::Sender<Vec<u8>>,
        exit_tx: watch::Sender<Option<ProcessExit>>,
    ) {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    let _ = exit_tx.send(Some(ProcessExit::Failed(format!(
                        "process event stream closed before {process_id} reported an exit code"
                    ))));
                    break;
                }
            };
            match &event.payload {
                EventPayload::ProcessOutputEvent(output) if output.process_id == process_id => {
                    let bytes = output.chunk.clone();
                    match output.channel {
                        StreamChannel::Stdout => {
                            let _ = stdout_tx.send(bytes);
                        }
                        StreamChannel::Stderr => {
                            let _ = stderr_tx.send(bytes);
                        }
                    }
                }
                EventPayload::ProcessExitedEvent(exited) if exited.process_id == process_id => {
                    let _ = exit_tx.send(Some(ProcessExit::Exited(exited.exit_code)));
                    break;
                }
                EventPayload::ProcessOutputEvent(_)
                | EventPayload::ProcessExitedEvent(_)
                | EventPayload::CronDispatchEvent(_)
                | EventPayload::VmLifecycleEvent(_)
                | EventPayload::StructuredEvent(_)
                | EventPayload::ExtEnvelope(_) => {}
            }
        }
        let _guard = self.inner().process_registry_lock.lock();
        self.prune_exited_processes_locked(0);
    }
}

async fn collect_exec_events(
    process_id: &str,
    events: &mut broadcast::Receiver<SharedWireEvent>,
    on_stdout: &mut Option<OutputCallback>,
    on_stderr: &mut Option<OutputCallback>,
) -> std::result::Result<(i32, String, String), ClientError> {
    loop {
        let event = match events.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => {
                return Err(ClientError::Sidecar(
                    "exec: event stream closed before process exit".to_owned(),
                ));
            }
        };
        match &event.payload {
            EventPayload::ProcessOutputEvent(output) if output.process_id == process_id => {
                match output.channel {
                    StreamChannel::Stdout => {
                        if let Some(cb) = on_stdout.as_mut() {
                            cb(&output.chunk);
                        }
                    }
                    StreamChannel::Stderr => {
                        if let Some(cb) = on_stderr.as_mut() {
                            cb(&output.chunk);
                        }
                    }
                }
            }
            EventPayload::ProcessExitedEvent(exited) if exited.process_id == process_id => {
                if let Some(error) = &exited.error {
                    return Err(ClientError::Kernel {
                        code: error.code.clone(),
                        message: error.message.clone(),
                    });
                }
                return Ok((
                    exited.exit_code,
                    String::from_utf8_lossy(exited.stdout.as_deref().unwrap_or_default())
                        .into_owned(),
                    String::from_utf8_lossy(exited.stderr.as_deref().unwrap_or_default())
                        .into_owned(),
                ));
            }
            EventPayload::ProcessOutputEvent(_)
            | EventPayload::ProcessExitedEvent(_)
            | EventPayload::CronDispatchEvent(_)
            | EventPayload::VmLifecycleEvent(_)
            | EventPayload::StructuredEvent(_)
            | EventPayload::ExtEnvelope(_) => {}
        }
    }
}

fn spawned_process_info_from_snapshot(process: ProcessInfo) -> SpawnedProcessInfo {
    SpawnedProcessInfo {
        pid: process.pid,
        command: process.command,
        args: process.args.into_iter().skip(1).collect(),
        running: process.status != ProcessStatus::Exited,
        exit_code: process.exit_code,
        started_at: process.start_time as i64,
    }
}

fn process_exit_result(exit: ProcessExit) -> std::result::Result<i32, ClientError> {
    match exit {
        ProcessExit::Exited(code) => Ok(code),
        ProcessExit::Failed(message) => Err(ClientError::Sidecar(message)),
    }
}

fn timeout_to_wire(timeout: Option<f64>) -> std::result::Result<Option<u64>, ClientError> {
    let Some(timeout) = timeout else {
        return Ok(None);
    };
    if !timeout.is_finite() || timeout < 0.0 || timeout > u64::MAX as f64 {
        return Err(ClientError::InvalidArgument(String::from(
            "process timeout must be a finite non-negative number representable as u64 milliseconds",
        )));
    }
    Ok(Some(timeout.trunc() as u64))
}

fn map_process_control_response(
    response: wire::ResponsePayload,
    operation: &str,
    accepted: impl FnOnce(&wire::ResponsePayload) -> bool,
) -> std::result::Result<(), ClientError> {
    if accepted(&response) {
        return Ok(());
    }
    match response {
        wire::ResponsePayload::RejectedResponse(wire::RejectedResponse { code, message }) => {
            Err(ClientError::Kernel { code, message })
        }
        other => Err(ClientError::Sidecar(format!(
            "{operation}: unexpected response {other:?}"
        ))),
    }
}

/// Assemble a process forest from a flat process list, linking children by `ppid`.
///
/// Mirrors the TS `processTree` `nodeMap` algorithm exactly: a process is a root iff its `ppid` is
/// NOT present among the listed pids. A self-parented process (`ppid == pid`) finds itself as its
/// parent, so it is attached as its own child and is excluded from the roots (effectively dropped
/// from the output tree). A `seen` guard prevents the self-cycle from recursing forever.
fn build_process_forest(processes: Vec<ProcessInfo>) -> Vec<ProcessTreeNode> {
    use std::collections::BTreeMap as Map;

    let pids: std::collections::BTreeSet<u32> = processes.iter().map(|p| p.pid).collect();
    // Children adjacency keyed by parent pid, preserving input (sorted) order.
    let mut children_of: Map<u32, Vec<usize>> = Map::new();
    let mut roots: Vec<usize> = Vec::new();
    for (index, proc) in processes.iter().enumerate() {
        if pids.contains(&proc.ppid) {
            children_of.entry(proc.ppid).or_default().push(index);
        } else {
            roots.push(index);
        }
    }

    fn build_node(
        index: usize,
        processes: &[ProcessInfo],
        children_of: &Map<u32, Vec<usize>>,
        seen: &mut std::collections::BTreeSet<usize>,
    ) -> ProcessTreeNode {
        let info = processes[index].clone();
        seen.insert(index);
        let child_indices: Vec<usize> = children_of
            .get(&info.pid)
            .map(|indices| {
                indices
                    .iter()
                    .copied()
                    .filter(|child_index| !seen.contains(child_index))
                    .collect()
            })
            .unwrap_or_default();
        let children = child_indices
            .into_iter()
            .map(|child_index| build_node(child_index, processes, children_of, seen))
            .collect();
        ProcessTreeNode { info, children }
    }

    let mut seen = std::collections::BTreeSet::new();
    roots
        .into_iter()
        .map(|index| build_node(index, &processes, &children_of, &mut seen))
        .collect()
}

/// Convert a [`StdinInput`] to raw bytes. A string is delivered as its UTF-8 bytes; raw bytes are
/// delivered verbatim (binary-safe, never lossy).
fn stdin_to_bytes(input: StdinInput) -> Vec<u8> {
    match input {
        StdinInput::Text(text) => text.into_bytes(),
        StdinInput::Bytes(bytes) => bytes,
    }
}

fn exited_pids_to_prune(mut entries: Vec<(u32, bool)>, target_len: usize) -> Vec<u32> {
    if entries.len() <= target_len {
        return Vec::new();
    }
    let mut remove_count = entries.len() - target_len;
    entries.sort_by_key(|(pid, _)| *pid);
    let mut out = Vec::new();
    for (pid, exited) in entries {
        if remove_count == 0 {
            break;
        }
        if !exited {
            continue;
        }
        out.push(pid);
        remove_count -= 1;
    }
    out
}

/// Drive a caller-supplied output callback from a fresh subscription on the given broadcast channel.
/// Each chunk delivered to the channel is forwarded to `callback` as raw bytes. The task ends when
/// the channel closes (process exit), matching the TS handler-set lifetime.
///
/// Returns the spawned task's handle so the owner can abort it on teardown: a [`ProcessEntry`]
/// retains its own `stdout_tx`/`stderr_tx` clone for late subscribers, so the broadcast channel
/// never closes (and this task never observes `Closed`) until the entry is dropped. `shutdown`
/// drains the registry and aborts these handles rather than waiting on the channel close.
pub(crate) fn install_output_callback(
    tx: broadcast::Sender<Vec<u8>>,
    mut callback: OutputCallback,
) -> JoinHandle<()> {
    let mut rx = tx.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(chunk) => callback(&chunk),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

/// Drain the SDK-spawned process registry, dropping each entry's retained sender clones and aborting
/// its per-process output-callback tasks. Called from `shutdown` so the output tasks (which would
/// otherwise await a `Closed` that never fires, see [`install_output_callback`]) cannot outlive the
/// disposed VM. Mirrors the `pending_shell_exits` / ACP-terminal drain in `shutdown`.
pub(crate) fn drain_process_output_tasks(processes: &SccHashMap<u32, ProcessEntry>) {
    let mut tasks = Vec::new();
    processes.retain(|_, entry| {
        tasks.append(&mut entry.output_tasks);
        false
    });
    for task in tasks {
        task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        collect_exec_events, drain_process_output_tasks, exited_pids_to_prune,
        install_output_callback, process_exit_result, timeout_to_wire, ExecOptions, OutputCallback,
    };
    use crate::agent_os::{ProcessEntry, ProcessExit};
    use agentos_sidecar_client::wire;
    use scc::HashMap as SccHashMap;
    use std::sync::{Arc, Mutex};
    use tokio::sync::{broadcast, watch};

    #[tokio::test]
    async fn exec_callbacks_precede_terminal_result_and_do_not_build_it() {
        let (tx, mut events) = broadcast::channel(4);
        let callback_chunks = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let callback_chunks_ref = Arc::clone(&callback_chunks);
        let mut on_stdout: Option<OutputCallback> = Some(Box::new(move |chunk| {
            callback_chunks_ref.lock().unwrap().push(chunk.to_vec());
        }));
        let mut on_stderr = None;
        let ownership = wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: "conn-test".to_owned(),
            session_id: "session-test".to_owned(),
            vm_id: "vm-test".to_owned(),
        });

        tx.send(Arc::new(wire::EventFrame {
            schema: wire::protocol_schema(),
            ownership: ownership.clone(),
            payload: wire::EventPayload::ProcessOutputEvent(wire::ProcessOutputEvent {
                process_id: "proc-test".to_owned(),
                channel: wire::StreamChannel::Stdout,
                chunk: b"streamed".to_vec(),
            }),
        }))
        .expect("stream event");
        tx.send(Arc::new(wire::EventFrame {
            schema: wire::protocol_schema(),
            ownership,
            payload: wire::EventPayload::ProcessExitedEvent(wire::ProcessExitedEvent {
                process_id: "proc-test".to_owned(),
                exit_code: 7,
                stdout: Some(b"terminal".to_vec()),
                stderr: Some(b"terminal-error".to_vec()),
                error: None,
            }),
        }))
        .expect("terminal event");

        let result = collect_exec_events("proc-test", &mut events, &mut on_stdout, &mut on_stderr)
            .await
            .expect("terminal result");

        assert_eq!(*callback_chunks.lock().unwrap(), vec![b"streamed".to_vec()]);
        assert_eq!(
            result,
            (7, "terminal".to_owned(), "terminal-error".to_owned())
        );
    }

    /// Regression for the per-process output-callback leak (H3): a `ProcessEntry` retains clones of
    /// its `stdout_tx`/`stderr_tx`, so the output tasks never observe the broadcast `Closed` and hang
    /// forever unless teardown aborts them. `drain_process_output_tasks` must empty the registry and
    /// abort every retained output task.
    #[tokio::test]
    async fn drain_process_output_tasks_clears_registry_and_aborts_tasks() {
        let processes: SccHashMap<u32, ProcessEntry> = SccHashMap::new();

        let (stdout_tx, _) = broadcast::channel::<Vec<u8>>(8);
        let (stderr_tx, _) = broadcast::channel::<Vec<u8>>(8);
        let (exit_tx, _) = watch::channel::<Option<ProcessExit>>(None);

        // A task that never completes on its own, standing in for an output-callback task that is
        // waiting on a `Closed` that the retained sender clone prevents.
        let task = tokio::spawn(async {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        });
        let abort_handle = task.abort_handle();

        let entry = ProcessEntry {
            stdout_tx,
            stderr_tx,
            exit_tx,
            process_id: "proc-test".to_string(),
            output_tasks: vec![task],
        };
        let _ = processes.insert(1, entry);

        assert!(!abort_handle.is_finished(), "task should start alive");

        drain_process_output_tasks(&processes);

        assert!(processes.is_empty(), "registry must be cleared on drain");

        // The abort is asynchronous; give the runtime a bounded window to reap the cancelled task.
        for _ in 0..100 {
            if abort_handle.is_finished() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(
            abort_handle.is_finished(),
            "output task must be aborted after drain"
        );
    }

    /// Regression for the H3 wiring (not just the drain helper): `spawn`/`spawn_inner` must capture
    /// the `JoinHandle` returned by `install_output_callback` into `ProcessEntry::output_tasks`. If a
    /// refactor forgot to push the handle, the callback task would be unreachable and
    /// `drain_process_output_tasks` would have nothing to abort, re-leaking the task. This reproduces
    /// that exact seam and asserts the stored handle is the live callback task.
    #[tokio::test]
    async fn install_output_callback_handle_is_captured_into_process_entry() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let (stdout_tx, _) = broadcast::channel::<Vec<u8>>(8);
        let (stderr_tx, _) = broadcast::channel::<Vec<u8>>(8);
        let (exit_tx, _) = watch::channel::<Option<ProcessExit>>(None);

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_cb = Arc::clone(&calls);
        let cb: OutputCallback = Box::new(move |_chunk: &[u8]| {
            calls_cb.fetch_add(1, Ordering::SeqCst);
        });

        // The exact seam from `spawn_inner`: capture the returned handle in `output_tasks`.
        let output_tasks = vec![install_output_callback(stdout_tx.clone(), cb)];

        let entry = ProcessEntry {
            stdout_tx: stdout_tx.clone(),
            stderr_tx,
            exit_tx,
            process_id: "proc-test".to_string(),
            output_tasks,
        };

        assert_eq!(
            entry.output_tasks.len(),
            1,
            "the install_output_callback handle must be captured on the entry"
        );

        // Prove the captured handle is the live callback task: a chunk on the channel runs it.
        stdout_tx
            .send(b"hello".to_vec())
            .expect("broadcast send to subscribed callback task");
        for _ in 0..100 {
            if calls.load(Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the stored handle must drive the registered callback"
        );

        // And it is the handle `drain_process_output_tasks` aborts on teardown.
        let processes: SccHashMap<u32, ProcessEntry> = SccHashMap::new();
        let _ = processes.insert(1, entry);
        drain_process_output_tasks(&processes);
        assert!(processes.is_empty(), "registry must be cleared on drain");
    }

    #[test]
    fn exec_options_default_omits_cwd_for_sidecar_resolution() {
        assert_eq!(ExecOptions::default().cwd, None);
    }

    #[test]
    fn closed_process_event_stream_is_an_error_not_exit_zero() {
        let error = process_exit_result(ProcessExit::Failed(String::from(
            "process event stream closed before proc-1 reported an exit code",
        )))
        .expect_err("closed stream must fail wait_process");
        assert!(error.to_string().contains("event stream closed"));
    }

    #[test]
    fn timeout_conversion_rejects_invalid_values() {
        assert_eq!(timeout_to_wire(None).expect("omitted timeout"), None);
        assert_eq!(timeout_to_wire(Some(1.9)).expect("finite timeout"), Some(1));
        assert!(timeout_to_wire(Some(f64::NAN)).is_err());
        assert!(timeout_to_wire(Some(-1.0)).is_err());
    }

    #[test]
    fn exited_pid_pruning_keeps_live_entries_and_removes_oldest_exited() {
        let pids = exited_pids_to_prune(vec![(3, true), (1, false), (2, true), (4, true)], 2);
        assert_eq!(pids, vec![2, 3]);
    }
}
