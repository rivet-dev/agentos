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

/// Base value for the synthetic display-pid sequence used by `spawn` (TS `SYNTHETIC_PID_BASE`). The
/// first spawned process is assigned exactly this value.
pub(crate) const SYNTHETIC_PID_BASE: u64 = 1_000_000;

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
#[derive(Default)]
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
    pub async fn exec(&self, command: &str, mut options: ExecOptions) -> Result<ExecResult> {
        let process_id = self.next_process_id();

        // Subscribe to events BEFORE issuing the request so no output/exit is missed between the
        // request landing and the subscription being installed.
        let mut events = self.transport().subscribe_events();

        let started = self
            .send_execute(
                &process_id,
                Some(command.to_owned()),
                Vec::new(),
                options.env.clone(),
                options.cwd.clone(),
            )
            .await
            .context("exec: Execute request failed")?;
        debug_assert_eq!(started.process_id, process_id);

        // Deliver any provided stdin, then close stdin so a non-interactive run observes EOF. This
        // mirrors the TS `runAndCapture` path (`proc.writeStdin(options.stdin); proc.closeStdin()`).
        if let Some(stdin) = options.stdin.take() {
            let chunk = stdin_to_bytes(stdin);
            let ownership = self.vm_scope();
            let _ = self
                .transport()
                .request(
                    ownership,
                    RequestPayload::WriteStdin(WriteStdinRequest {
                        process_id: process_id.clone(),
                        chunk,
                    }),
                )
                .await;
        }
        {
            let ownership = self.vm_scope();
            let _ = self
                .transport()
                .request(
                    ownership,
                    RequestPayload::CloseStdin(CloseStdinRequest {
                        process_id: process_id.clone(),
                    }),
                )
                .await;
        }

        let mut on_stdout = options.on_stdout.take();
        let mut on_stderr = options.on_stderr.take();

        // A `timeout` (ms) bounds the run: when it elapses, SIGKILL the process and keep draining
        // until the exit event lands. This mirrors the TS `runAndCapture` timeout race that kills the
        // process and then awaits its exit code.
        let timeout_deadline = options
            .timeout
            .filter(|ms| ms.is_finite() && *ms >= 0.0)
            .map(|ms| {
                tokio::time::Instant::now() + std::time::Duration::from_secs_f64(ms / 1000.0)
            });
        let mut killed_for_timeout = false;

        let mut stdout = Vec::<u8>::new();
        let mut stderr = Vec::<u8>::new();
        let exit_code = loop {
            let recv = events.recv();
            let frame = match timeout_deadline {
                Some(deadline) => {
                    tokio::select! {
                        result = recv => result,
                        _ = tokio::time::sleep_until(deadline), if !killed_for_timeout => {
                            killed_for_timeout = true;
                            self.kill_wire_process(&process_id, "SIGKILL");
                            continue;
                        }
                    }
                }
                None => recv.await,
            };
            let (_, payload) = match frame {
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
                        StreamChannel::Stdout => {
                            if let Some(cb) = on_stdout.as_mut() {
                                cb(&output.chunk);
                            }
                            stdout.extend_from_slice(&output.chunk);
                        }
                        StreamChannel::Stderr => {
                            if let Some(cb) = on_stderr.as_mut() {
                                cb(&output.chunk);
                            }
                            stderr.extend_from_slice(&output.chunk);
                        }
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
        mut options: SpawnOptions,
    ) -> Result<SpawnHandle> {
        // Draw the public pid from the dedicated synthetic-pid space (TS `nextSyntheticPid`), seeded
        // at `SYNTHETIC_PID_BASE`. `exec` uses a separate counter so it never perturbs this sequence.
        let pid = self
            .inner()
            .synthetic_pid_counter
            .fetch_add(1, Ordering::SeqCst) as u32;
        let process_id = format!("proc-{pid}-{}", uuid::Uuid::new_v4());

        let (stdout_tx, _) = broadcast::channel::<Vec<u8>>(PROCESS_STREAM_CAPACITY);
        let (stderr_tx, _) = broadcast::channel::<Vec<u8>>(PROCESS_STREAM_CAPACITY);
        // Seeded `None`; the already-exited branch of `on_process_exit` fires immediately once this
        // watch holds `Some(code)`.
        let (exit_tx, _) = watch::channel::<Option<i32>>(None);
        // Seeded `None`; filled with the kernel pid once the `Execute` response lands so
        // `all_processes`/`process_tree` can remap the kernel snapshot back to this display pid.
        let (kernel_pid_tx, _) = watch::channel::<Option<u32>>(None);

        // Seed any caller-provided initial stdout/stderr callbacks into the fan-out, matching the TS
        // initial-handler-set behavior (`stdoutHandlers.add(options.onStdout)`).
        if let Some(cb) = options.base.on_stdout.take() {
            install_output_callback(stdout_tx.clone(), cb);
        }
        if let Some(cb) = options.base.on_stderr.take() {
            install_output_callback(stderr_tx.clone(), cb);
        }

        let entry = ProcessEntry {
            command: command.to_owned(),
            args: args.clone(),
            stdout_tx: stdout_tx.clone(),
            stderr_tx: stderr_tx.clone(),
            exit_tx: exit_tx.clone(),
            process_id: process_id.clone(),
            kernel_pid: kernel_pid_tx.clone(),
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
                kernel_pid_tx,
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
        let chunk: Vec<u8> = stdin_to_bytes(data);
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
    ///
    /// The kernel snapshot keys processes by their raw kernel pid. SDK-spawned root processes carry a
    /// synthetic display pid (the `spawn` return value); this remaps each snapshot entry's
    /// pid/ppid/pgid/sid back to that display pid via the per-process `kernel_pid` watch, so a caller
    /// can correlate `spawn()` with `all_processes()`/`process_tree()`. Results are sorted ascending
    /// by display pid (TS `snapshotProcesses` `.sort((l,r) => l.pid - r.pid)`).
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

        // Snapshot the SDK process registry, keyed by wire `process_id`, capturing the resolved
        // kernel pid (if landed), the display pid, exit code, command, and args. This mirrors the TS
        // `trackedProcessesById` lookup used to build `displayPidByKernelPid` and override fields.
        struct Tracked {
            display_pid: u32,
            exit_code: Option<i32>,
            command: String,
            args: Vec<String>,
        }
        let mut tracked_by_process_id: BTreeMap<String, Tracked> = BTreeMap::new();
        let mut display_pid_by_kernel_pid: BTreeMap<u32, u32> = BTreeMap::new();
        self.inner().processes.scan(|display_pid, entry| {
            let exit_code = *entry.exit_tx.borrow();
            if let Some(kernel_pid) = *entry.kernel_pid.borrow() {
                display_pid_by_kernel_pid.insert(kernel_pid, *display_pid);
            }
            tracked_by_process_id.insert(
                entry.process_id.clone(),
                Tracked {
                    display_pid: *display_pid,
                    exit_code,
                    command: entry.command.clone(),
                    args: entry.args.clone(),
                },
            );
        });

        let now_ms = epoch_ms_now();
        let mut seen_display_pids: std::collections::BTreeSet<u32> =
            std::collections::BTreeSet::new();
        let mut out: Vec<ProcessInfo> = Vec::new();

        for entry in snapshot.processes {
            let tracked = tracked_by_process_id.get(&entry.process_id);
            let display_pid = display_pid_by_kernel_pid
                .get(&entry.pid)
                .copied()
                .unwrap_or(entry.pid);
            let display_ppid = display_pid_by_kernel_pid
                .get(&entry.ppid)
                .copied()
                .unwrap_or(entry.ppid);
            let display_pgid = display_pid_by_kernel_pid
                .get(&entry.pgid)
                .copied()
                .unwrap_or(entry.pgid);
            let display_sid = display_pid_by_kernel_pid
                .get(&entry.sid)
                .copied()
                .unwrap_or(entry.sid);

            // First-observed start time, keyed by `"<process_id>:<kernel_pid>"` (TS `processKey`).
            let process_key = format!("{}:{}", entry.process_id, entry.pid);
            let start_time = self.observed_start_time(&process_key, now_ms);

            // Status/exit code: a tracked process whose SDK exit code is known is `exited`; otherwise
            // a tracked process is `running`; an untracked process uses the snapshot status.
            let (status, exit_code) = match tracked {
                Some(t) => match t.exit_code {
                    Some(code) => (ProcessStatus::Exited, Some(code)),
                    None => (ProcessStatus::Running, entry.exit_code),
                },
                None => {
                    let status = match entry.status {
                        ProcessSnapshotStatus::Running | ProcessSnapshotStatus::Stopped => {
                            ProcessStatus::Running
                        }
                        ProcessSnapshotStatus::Exited => ProcessStatus::Exited,
                    };
                    (status, entry.exit_code)
                }
            };

            // Exit time: only tracked-and-exited processes carry one (TS `tracked?.exitTime`).
            let exit_time = match (tracked, status) {
                (Some(_), ProcessStatus::Exited) => {
                    Some(self.observed_exit_time(&entry.process_id, now_ms))
                }
                _ => None,
            };

            let (command, args) = match tracked {
                Some(t) => (t.command.clone(), t.args.clone()),
                None => (entry.command, entry.args),
            };

            seen_display_pids.insert(display_pid);
            out.push(ProcessInfo {
                pid: display_pid,
                ppid: display_ppid,
                pgid: display_pgid,
                sid: display_sid,
                driver: entry.driver,
                command,
                args,
                cwd: entry.cwd,
                status,
                exit_code,
                start_time,
                exit_time,
            });
        }

        // Tracked processes not yet present in the snapshot (the spawn `Execute` has not surfaced in
        // the kernel table yet). TS fills these with `ppid:0, pgid/sid = pid`.
        self.inner().processes.scan(|display_pid, entry| {
            if seen_display_pids.contains(display_pid) {
                return;
            }
            let exit_code = *entry.exit_tx.borrow();
            let process_key = format!("{}:{}", entry.process_id, display_pid);
            let start_time = self.observed_start_time(&process_key, now_ms);
            let (status, exit_time) = match exit_code {
                Some(_) => (
                    ProcessStatus::Exited,
                    Some(self.observed_exit_time(&entry.process_id, now_ms)),
                ),
                None => (ProcessStatus::Running, None),
            };
            out.push(ProcessInfo {
                pid: *display_pid,
                ppid: 0,
                pgid: *display_pid,
                sid: *display_pid,
                driver: String::new(),
                command: entry.command.clone(),
                args: entry.args.clone(),
                cwd: String::new(),
                status,
                exit_code,
                start_time,
                exit_time,
            });
        });

        out.sort_by_key(|info| info.pid);
        Ok(out)
    }

    /// Return the first-observed start time for a process key, recording `now` the first time it is
    /// seen so later snapshots report a stable timestamp (TS `observedProcessStartTimes`).
    fn observed_start_time(&self, process_key: &str, now_ms: f64) -> f64 {
        if let Some(existing) = self
            .inner()
            .observed_process_start_times
            .read(process_key, |_, value| *value)
        {
            return existing;
        }
        let _ = self
            .inner()
            .observed_process_start_times
            .insert(process_key.to_owned(), now_ms);
        // Re-read to honor a racing insert that may have won; either value is a valid first-observed
        // timestamp.
        self.inner()
            .observed_process_start_times
            .read(process_key, |_, value| *value)
            .unwrap_or(now_ms)
    }

    /// Return the first-observed exit time for an SDK process id, recording `now` on first sight.
    fn observed_exit_time(&self, process_id: &str, now_ms: f64) -> f64 {
        if let Some(existing) = self
            .inner()
            .observed_process_exit_times
            .read(process_id, |_, value| *value)
        {
            return existing;
        }
        let _ = self
            .inner()
            .observed_process_exit_times
            .insert(process_id.to_owned(), now_ms);
        self.inner()
            .observed_process_exit_times
            .read(process_id, |_, value| *value)
            .unwrap_or(now_ms)
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
        env: BTreeMap<String, String>,
        cwd: Option<String>,
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
                    env,
                    cwd,
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

    /// Fire-and-forget kill of a wire process by its `process_id` (used by `exec` timeout). The TS
    /// timeout path calls `proc.kill(9)`, which maps to a `SIGKILL` kill request.
    fn kill_wire_process(&self, process_id: &str, signal: &str) {
        let process_id = process_id.to_owned();
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
        kernel_pid_tx: watch::Sender<Option<u32>>,
    ) {
        match self
            .send_execute(
                &process_id,
                Some(command),
                args,
                options.base.env.clone(),
                options.base.cwd.clone(),
            )
            .await
        {
            Ok(started) => {
                // Seed the kernel pid so `all_processes`/`process_tree` can remap this process's
                // kernel-snapshot entry back to its display pid.
                if let Some(kernel_pid) = started.pid {
                    let _ = kernel_pid_tx.send(Some(kernel_pid));
                }
            }
            Err(error) => {
                // The native TS launch-failure path emits the error message (plus a trailing
                // newline) on stderr and resolves the wait with exit code 1 (`startTrackedProcess`
                // catch -> stderr handlers + `finishProcess(entry, 1)`).
                let message = format!("{error}\n");
                let _ = stderr_tx.send(message.into_bytes());
                tracing::error!(?error, pid, %process_id, "spawn: Execute request failed");
                let _ = exit_tx.send(Some(1));
                return;
            }
        }

        loop {
            let (_, payload) = match events.recv().await {
                Ok(frame) => frame,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    // The event stream closed before an exit event landed. The TS fallback treats a
                    // process that has fully disappeared from the VM snapshot as reaped with exit
                    // code 0; mirror that terminal value so waiters resolve instead of hanging.
                    let _ = exit_tx.send(Some(0));
                    break;
                }
            };
            match payload {
                EventPayload::ProcessOutput(output) if output.process_id == process_id => {
                    let bytes = output.chunk;
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

/// Drive a caller-supplied output callback from a fresh subscription on the given broadcast channel.
/// Each chunk delivered to the channel is forwarded to `callback` as raw bytes. The task ends when
/// the channel closes (process exit), matching the TS handler-set lifetime.
pub(crate) fn install_output_callback(
    tx: broadcast::Sender<Vec<u8>>,
    mut callback: OutputCallback,
) {
    let mut rx = tx.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(chunk) => callback(&chunk),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Current wall-clock time as epoch milliseconds (TS `Date.now()`).
fn epoch_ms_now() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}
