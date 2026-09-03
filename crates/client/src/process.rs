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
use scc::HashMap as SccHashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use agentos_sidecar_client::wire::{self, EventPayload, ProcessSnapshotStatus, StreamChannel};

use crate::agent_os::{AgentOs, ProcessEntry};
use crate::command_line::resolve_exec_command;
use crate::error::ClientError;
use crate::output_replay::page_limits;
use crate::stream::Subscription;
use crate::ResourceLimitDetails;

/// Client-local observation state; only a matching sidecar event supplies an exit code.
#[derive(Debug, Clone)]
pub(crate) enum ProcessOutcome {
    Pending,
    Failed { error: ClientError, rejected: bool },
    Exited(i32),
}

impl ProcessOutcome {
    pub(crate) fn completion(process_id: &str, exit_code: Option<i32>) -> Self {
        match exit_code {
            Some(code) => Self::Exited(code),
            None => Self::Failed {
                error: ClientError::TerminationFailed {
                    process_id: process_id.into(),
                    reason: "background execution completed without an exit status".into(),
                },
                rejected: false,
            },
        }
    }

    fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Exited(code) => Some(*code),
            _ => None,
        }
    }

    fn rejected(&self) -> bool {
        matches!(self, Self::Failed { rejected: true, .. })
    }

    fn reclaimable(&self) -> bool {
        self.exit_code().is_some() || self.rejected()
    }

    fn wait_result(&self) -> Option<std::result::Result<i32, ClientError>> {
        match self {
            Self::Pending => None,
            Self::Failed { error, .. } => Some(Err(error.clone())),
            Self::Exited(code) => Some(Ok(*code)),
        }
    }

    fn launch_failure(process_id: &str, error: ClientError) -> Self {
        // These variants are emitted only for an explicit RejectedResponse by send_execute.
        let rejected = matches!(
            error,
            ClientError::Kernel { .. } | ClientError::ResourceLimit { .. }
        );
        let error = if rejected {
            error
        } else {
            ClientError::TerminationFailed {
                process_id: process_id.into(),
                reason: error.to_string(),
            }
        };
        Self::Failed { error, rejected }
    }
}

async fn wait_for_process_outcome(
    mut rx: watch::Receiver<ProcessOutcome>,
    process_id: &str,
) -> std::result::Result<i32, ClientError> {
    loop {
        if let Some(result) = rx.borrow().wait_result() {
            return result;
        }
        if rx.changed().await.is_err() {
            return Err(ClientError::TerminationFailed {
                process_id: process_id.into(),
                reason: "exit observation channel closed before an exit event".into(),
            });
        }
    }
}

async fn next_spawn_event(
    events: &mut broadcast::Receiver<(wire::OwnershipScope, EventPayload)>,
    ownership: &wire::OwnershipScope,
    process_id: &str,
    outcome: &watch::Sender<ProcessOutcome>,
) -> Option<EventPayload> {
    let mut replay_outcome = outcome.subscribe();
    loop {
        if matches!(*replay_outcome.borrow(), ProcessOutcome::Exited(_)) {
            return None;
        }
        let event = tokio::select! {
            event = events.recv() => event,
            changed = replay_outcome.changed() => {
                if let Err(error) = changed {
                    tracing::error!(%process_id, %error, "process outcome channel closed during observation");
                    return None;
                }
                continue;
            }
        };
        match event {
            Ok((scope, payload)) if scope == *ownership => {
                if matches!(&payload, EventPayload::VmLifecycleEvent(event)
                    if matches!(event.state, wire::VmLifecycleState::Disposed | wire::VmLifecycleState::Failed))
                {
                    let reason = "VM ended before a process exit was observed";
                    tracing::error!(%process_id, reason, "spawn observation failed");
                    outcome.send_if_modified(|state| {
                        if !matches!(state, ProcessOutcome::Pending) {
                            return false;
                        }
                        *state = ProcessOutcome::Failed {
                            error: ClientError::TerminationFailed {
                                process_id: process_id.into(),
                                reason: reason.into(),
                            },
                            rejected: false,
                        };
                        true
                    });
                    return None;
                }
                if let EventPayload::ProcessExitedEvent(exited) = &payload {
                    if exited.process_id == process_id {
                        // send_replace retains fast exits even before anyone calls wait/subscribe.
                        outcome.send_replace(ProcessOutcome::Exited(exited.exit_code));
                    }
                }
                return Some(payload);
            }
            Ok(_) => continue,
            Err(error) => {
                let closed = matches!(error, broadcast::error::RecvError::Closed);
                let reason =
                    format!("spawn event stream failed before exit was confirmed: {error}");
                tracing::error!(%process_id, %reason, "spawn observation failed");
                outcome.send_if_modified(|state| {
                    if !matches!(state, ProcessOutcome::Pending) {
                        return false;
                    }
                    *state = ProcessOutcome::Failed {
                        error: ClientError::TerminationFailed {
                            process_id: process_id.into(),
                            reason,
                        },
                        rejected: false,
                    };
                    true
                });
                if closed {
                    return None;
                }
                // Keep observing after a lost event; a later real exit may still arrive.
            }
        }
    }
}

fn reconcile_replayed_exit(outcome: &watch::Sender<ProcessOutcome>, exit_code: Option<i32>) {
    if let Some(exit_code) = exit_code {
        outcome.send_if_modified(|state| {
            if matches!(state, ProcessOutcome::Exited(_)) {
                return false;
            }
            *state = ProcessOutcome::Exited(exit_code);
            true
        });
    }
}

/// Broadcast channel capacity for a spawned process's stdout/stderr fan-out.
const PROCESS_STREAM_CAPACITY: usize = 1024;

/// Maximum SDK-spawned process entries retained per VM.
const PROCESS_REGISTRY_LIMIT: usize = 1024;

/// Maximum first-observed process timestamp entries retained per VM.
const OBSERVED_PROCESS_TIME_LIMIT: usize = 4096;

/// Maximum bytes captured by `exec` across stdout and stderr.
const EXEC_OUTPUT_CAPTURE_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const EXEC_TERMINATION_CONFIRMATION_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30);
const EXEC_KILL_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Default guest working directory for `exec`/`spawn`, matching the TS sidecar client.
pub(crate) const DEFAULT_EXEC_CWD: &str = "/workspace";

/// Base value for the synthetic display-pid sequence used by `spawn` (TS `SYNTHETIC_PID_BASE`). The
/// first spawned process is assigned exactly this value.
pub(crate) const SYNTHETIC_PID_BASE: u64 = 1_000_000;

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Timing-mitigation mode for an execution.
#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
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

/// A dropped run future must not silently abandon a guest whose launch request is still in
/// flight. The request lives in its own task, so cancellation can wait for the launch response and
/// issue a kill even when the actor's action future has already been dropped.
struct ExecRunCleanup {
    vm: AgentOs,
    process_id: String,
    launch: Option<JoinHandle<std::result::Result<wire::ProcessStartedResponse, ClientError>>>,
    events: Option<broadcast::Receiver<(wire::OwnershipScope, EventPayload)>>,
    armed: bool,
}

impl ExecRunCleanup {
    fn events(&mut self) -> &mut broadcast::Receiver<(wire::OwnershipScope, EventPayload)> {
        self.events.as_mut().expect("exec event receiver")
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ExecRunCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let vm = self.vm.clone();
        let process_id = self.process_id.clone();
        let launch = self.launch.take();
        let mut events = self.events.take().expect("exec event receiver");
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            if let Some(launch) = launch {
                launch.abort();
            }
            tracing::error!(%process_id, "cancelled exec has no runtime for cleanup; guest status is unknown");
            return;
        };
        runtime.spawn(async move {
            if let Some(mut launch) = launch {
                match tokio::time::timeout(EXEC_TERMINATION_CONFIRMATION_TIMEOUT, &mut launch).await {
                    Ok(Ok(Ok(_))) => {}
                    Ok(Ok(Err(ClientError::Kernel { .. } | ClientError::ResourceLimit { .. }))) => {
                        // The sidecar rejected admission; it owns pre-registration rollback.
                        return;
                    }
                    Ok(Ok(Err(error))) => {
                        tracing::warn!(?error, %process_id, "cancelled exec launch failed before cleanup");
                    }
                    Ok(Err(error)) => {
                        tracing::warn!(?error, %process_id, "cancelled exec launch task failed");
                    }
                    Err(_) => {
                        // Abort the local waiter, not merely its JoinHandle. A request already
                        // accepted remotely cannot be revoked here; still attempt a bounded kill.
                        launch.abort();
                        tracing::error!(%process_id, "cancelled exec launch did not acknowledge within cleanup deadline; guest status is unknown");
                    }
                }
            }
            if let Err(error) = vm.stop_and_confirm_exec(&process_id, &mut events).await {
                tracing::error!(?error, %process_id, "cancelled exec could not confirm guest termination");
            }
        });
    }
}

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
            cwd: Some(DEFAULT_EXEC_CWD.to_string()),
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
#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpawnStdio {
    #[default]
    Pipe,
    Inherit,
}

/// Callback-free options for portable `spawn`.
#[derive(Default)]
pub struct SpawnOptions {
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub stdio: Option<SpawnStdio>,
    pub stdin_fd: Option<i32>,
    pub stdout_fd: Option<i32>,
    pub stderr_fd: Option<i32>,
    pub stream_stdin: Option<bool>,
    /// Retain a bounded sequenced output replay in the sidecar.
    pub retain_output: bool,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessStream {
    Stdout,
    Stderr,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutput {
    pub pid: u32,
    pub stream: ProcessStream,
    pub data: Vec<u8>,
    pub sequence: Option<u64>,
    #[serde(rename = "timestampMs")]
    pub timestamp_ms: Option<i64>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutputEvent {
    pub pid: u32,
    pub sequence: u64,
    pub stream: ProcessStream,
    pub data: Vec<u8>,
    #[serde(rename = "timestampMs")]
    pub timestamp_ms: i64,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutputReplay {
    pub pid: u32,
    pub events: Vec<ProcessOutputEvent>,
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<u64>,
    #[serde(rename = "hasMore")]
    pub has_more: bool,
    pub truncated: bool,
    /// Confirmed guest exit, including when the live exit event was missed.
    #[serde(rename = "exitCode")]
    pub exit_code: Option<i32>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessExit {
    pub pid: u32,
    #[serde(rename = "exitCode")]
    pub exit_code: i32,
}

/// Public JSON info for SDK-spawned processes.
#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnHandle {
    pub pid: u32,
}

/// Process status from the kernel process table.
#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessStatus {
    Running,
    Exited,
}

/// Full kernel process info (TS `KernelProcessInfo`).
#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    /// Rust-client registry identity for a process started through this VM's
    /// process API. Never infer it from `pid`: raw kernel and synthetic display
    /// PID ranges can overlap in a long-lived VM.
    #[serde(skip)]
    #[cfg_attr(feature = "contract", ts(skip))]
    pub tracked_pid: Option<u32>,
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
#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessTreeNode {
    #[serde(flatten)]
    pub info: ProcessInfo,
    pub children: Vec<ProcessTreeNode>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ProcessTreeKey {
    Kernel(u32),
    Tracked(u32),
}

struct ProcessTreeRecord {
    info: ProcessInfo,
    key: ProcessTreeKey,
    parent: Option<ProcessTreeKey>,
}

// ---------------------------------------------------------------------------
// Methods
// ---------------------------------------------------------------------------

impl AgentOs {
    /// Run a command to completion. The wire `Execute` request starts the process and returns a
    /// process id immediately; stdout/stderr are accumulated and the call resolves once the matching
    /// `ProcessExited` event arrives. This mirrors the TS pass-through to `kernel.exec` semantically:
    /// the result is the full captured stdout/stderr plus exit code.
    pub async fn exec_process(&self, command: &str, options: ExecOptions) -> Result<ExecResult> {
        // Parse the command line into a `(command, args)` pair the same way the sidecar's
        // child_process path does: shell-free argv lists spawn directly (preserving the command's
        // real exit code), while shell syntax or a builtin head runs under `sh -c <line>`.
        let (resolved_command, resolved_args) = resolve_exec_command(command)?;
        self.exec_argv_process(&resolved_command, &resolved_args, options)
            .await
    }

    /// Run a command to completion from an already-structured `(command, args)` argv, bypassing the
    /// `exec` command-line parser. Each `args` element is sent verbatim as a distinct argv element —
    /// no whitespace re-splitting, no shell metacharacter detection, and no routing through
    /// `sh -c`. Callers that already hold a structured argv (for example the cron `Exec` action)
    /// must use this so the structured-argv contract is preserved end to end.
    pub async fn exec_argv_process(
        &self,
        command: &str,
        args: &[String],
        mut options: ExecOptions,
    ) -> Result<ExecResult> {
        // The deadline covers launch, stdin delivery, and execution. Cleanup has its own bounded
        // confirmation window; it must not silently consume an unbounded slice of the actor's
        // action deadline.
        let timeout_deadline = options
            .timeout
            .filter(|ms| ms.is_finite() && *ms >= 0.0)
            .map(|ms| {
                tokio::time::Instant::now() + std::time::Duration::from_secs_f64(ms / 1000.0)
            });
        let process_id = self.next_process_id();

        // Subscribe to events BEFORE issuing the request so no output/exit is missed between the
        // request landing and the subscription being installed.
        let events = self.transport().subscribe_wire_events();

        let launch_vm = self.clone();
        let launch_id = process_id.clone();
        let resolved_command = command.to_owned();
        let resolved_args = args.to_vec();
        let env = options.env.clone();
        let cwd = options.cwd.clone();
        let launch = tokio::spawn(async move {
            launch_vm
                .send_execute(
                    &launch_id,
                    Some(resolved_command),
                    resolved_args,
                    env,
                    cwd,
                    false,
                )
                .await
        });
        let mut cleanup = ExecRunCleanup {
            vm: self.clone(),
            process_id: process_id.clone(),
            launch: Some(launch),
            events: Some(events),
            armed: true,
        };
        let launched = match timeout_deadline {
            Some(deadline) => {
                match tokio::time::timeout_at(
                    deadline,
                    cleanup.launch.as_mut().expect("launch task"),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => {
                        return Err(ClientError::TerminationFailed {
                            process_id,
                            reason: "Execute did not acknowledge before the execution deadline; cancellation cleanup is still attempting to stop it".to_owned(),
                        }
                        .into());
                    }
                }
            }
            None => cleanup.launch.as_mut().expect("launch task").await,
        };
        cleanup.launch.take();
        let started = match launched {
            Ok(Ok(started)) => started,
            Ok(Err(error @ (ClientError::Kernel { .. } | ClientError::ResourceLimit { .. }))) => {
                // A deterministic Execute rejection has no admitted process to wait for. The
                // sidecar owns rollback of any allocations made before sending its rejection.
                cleanup.disarm();
                return Err(error).context("exec: Execute request failed");
            }
            Ok(Err(error)) => return Err(error).context("exec: Execute request failed"),
            Err(error) => {
                return Err(
                    ClientError::Sidecar(format!("exec: launch task failed: {error}")).into(),
                )
            }
        };
        debug_assert_eq!(started.process_id, process_id);

        // Deliver any provided stdin, then close stdin so a non-interactive run observes EOF. This
        // mirrors the TS `runAndCapture` path (`proc.writeStdin(options.stdin); proc.closeStdin()`).
        let deliver_stdin = async {
            if let Some(stdin) = options.stdin.take() {
                let chunk = stdin_to_bytes(stdin);
                if let Err(error) = self.write_wire_stdin(&process_id, chunk).await {
                    tracing::warn!(?error, %process_id, "exec stdin write failed");
                }
            }
            if let Err(error) = self.close_wire_stdin(&process_id).await {
                tracing::warn!(?error, %process_id, "exec stdin close failed");
            }
        };
        if let Some(deadline) = timeout_deadline {
            if tokio::time::timeout_at(deadline, deliver_stdin)
                .await
                .is_err()
            {
                let stopped = self
                    .stop_and_confirm_exec(&process_id, cleanup.events())
                    .await;
                if stopped.is_ok() {
                    cleanup.disarm();
                    return Err(ClientError::ExecutionTimedOut { process_id }.into());
                }
                return Err(stopped.expect_err("failed confirmation").into());
            }
        } else {
            deliver_stdin.await;
        }

        let mut on_stdout = options.on_stdout.take();
        let mut on_stderr = options.on_stderr.take();

        let capture_stdio = options.capture_stdio.unwrap_or(true);
        let mut stdout = Vec::<u8>::new();
        let mut stderr = Vec::<u8>::new();
        let mut captured_output_bytes = 0usize;
        let mut capture_error: Option<ClientError> = None;
        let exit_code = loop {
            let frame = match timeout_deadline {
                Some(deadline) => {
                    match tokio::time::timeout_at(deadline, cleanup.events().recv()).await {
                        Ok(result) => result,
                        Err(_) => {
                            let stopped = self
                                .stop_and_confirm_exec(&process_id, cleanup.events())
                                .await;
                            if stopped.is_ok() {
                                cleanup.disarm();
                                return Err(ClientError::ExecutionTimedOut { process_id }.into());
                            }
                            return Err(stopped.expect_err("failed confirmation").into());
                        }
                    }
                }
                None => cleanup.events().recv().await,
            };
            let (ownership, payload) = match frame {
                Ok(frame) => frame,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(ClientError::TerminationFailed {
                        process_id,
                        reason: "event stream closed before process exit".to_owned(),
                    }
                    .into());
                }
            };
            if ownership != self.vm_scope() {
                continue;
            }
            match payload {
                EventPayload::ProcessOutputEvent(output) if output.process_id == process_id => {
                    match output.channel {
                        StreamChannel::Stdout => {
                            if let Some(cb) = on_stdout.as_mut() {
                                cb(&output.chunk);
                            }
                            if capture_stdio && capture_error.is_none() {
                                match append_exec_output(
                                    &mut stdout,
                                    &output.chunk,
                                    &mut captured_output_bytes,
                                    "stdout",
                                ) {
                                    Ok(()) => {}
                                    Err(error) => {
                                        self.stop_and_confirm_exec(&process_id, cleanup.events())
                                            .await?;
                                        cleanup.disarm();
                                        capture_error = Some(error);
                                        break -1;
                                    }
                                }
                            }
                        }
                        StreamChannel::Stderr => {
                            if let Some(cb) = on_stderr.as_mut() {
                                cb(&output.chunk);
                            }
                            if capture_stdio && capture_error.is_none() {
                                match append_exec_output(
                                    &mut stderr,
                                    &output.chunk,
                                    &mut captured_output_bytes,
                                    "stderr",
                                ) {
                                    Ok(()) => {}
                                    Err(error) => {
                                        self.stop_and_confirm_exec(&process_id, cleanup.events())
                                            .await?;
                                        cleanup.disarm();
                                        capture_error = Some(error);
                                        break -1;
                                    }
                                }
                            }
                        }
                    }
                }
                EventPayload::ProcessExitedEvent(exited) if exited.process_id == process_id => {
                    break exited.exit_code;
                }
                EventPayload::ProcessOutputEvent(_)
                | EventPayload::ProcessExitedEvent(_)
                | EventPayload::ExecutionOutputEvent(_)
                | EventPayload::ExecutionCompletedEvent(_)
                | EventPayload::VmLifecycleEvent(_)
                | EventPayload::StructuredEvent(_)
                | EventPayload::ExtEnvelope(_) => {}
            }
        };

        cleanup.disarm();

        if let Some(error) = capture_error {
            return Err(error.into());
        }

        Ok(ExecResult {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }

    /// A kill acknowledgement alone is not proof that the guest stopped. Drain until the matching
    /// exit event arrives, with one hard window covering both the signal and exit confirmation.
    async fn stop_and_confirm_exec(
        &self,
        process_id: &str,
        events: &mut broadcast::Receiver<(wire::OwnershipScope, EventPayload)>,
    ) -> std::result::Result<i32, ClientError> {
        confirm_exec_exit(
            process_id,
            &self.vm_scope(),
            events,
            self.signal_wire_process(process_id, "SIGKILL"),
            EXEC_KILL_ACK_TIMEOUT,
            EXEC_TERMINATION_CONFIRMATION_TIMEOUT,
        )
        .await
    }

    /// Spawn a process. SYNC; returns `{ pid }` only. Installs stdout/stderr fan-out over broadcast
    /// channels and wires exit via a background event-pump task. The user-facing `pid` is the
    /// SDK-allocated map key (the wire `process_id` is held inside the [`ProcessEntry`]).
    pub fn spawn_process(
        &self,
        command: &str,
        args: Vec<String>,
        options: SpawnOptions,
    ) -> Result<SpawnHandle> {
        let reservation = self.reserve_process_registry_slot()?;

        // Draw the public pid from the dedicated synthetic-pid space (TS `nextSyntheticPid`), seeded
        // at `SYNTHETIC_PID_BASE`. `exec` uses a separate counter so it never perturbs this sequence.
        let pid = self
            .inner()
            .synthetic_pid_counter
            .fetch_add(1, Ordering::SeqCst) as u32;
        let process_id = format!("proc-{pid}-{}", uuid::Uuid::new_v4());

        let (stdout_tx, _) = broadcast::channel::<Vec<u8>>(PROCESS_STREAM_CAPACITY);
        let (stderr_tx, _) = broadcast::channel::<Vec<u8>>(PROCESS_STREAM_CAPACITY);
        let (output_tx, _) = broadcast::channel::<ProcessOutput>(PROCESS_STREAM_CAPACITY);
        let (exit_tx, _) = watch::channel(ProcessOutcome::Pending);
        // Seeded `None`; filled with the kernel pid once the `Execute` response lands so
        // `all_processes`/`process_tree` can remap the kernel snapshot back to this display pid.
        let (kernel_pid_tx, _) = watch::channel::<Option<u32>>(None);
        let entry = ProcessEntry {
            command: command.to_owned(),
            args: args.clone(),
            stdout_tx: stdout_tx.clone(),
            stderr_tx: stderr_tx.clone(),
            output_tx: output_tx.clone(),
            exit_tx: exit_tx.clone(),
            process_id: process_id.clone(),
            kernel_pid: kernel_pid_tx.clone(),
            output_tasks: Vec::new(),
            retain_output: options.retain_output,
            execution_id: None,
            execution_generation: None,
            started_at: epoch_ms_now() as i64,
        };
        // `spawn` is documented as overwriting any prior entry for a freshly allocated pid; the pid
        // is monotonic so a collision is not expected.
        reservation.commit(pid, entry)?;

        // Subscribe to events before issuing the request so the pump sees everything.
        let events = self.transport().subscribe_wire_events();

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
                output_tx,
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
        tokio::spawn(async move {
            if let Err(error) = this.write_wire_stdin(&process_id, chunk).await {
                tracing::warn!(?error, pid, "write_process_stdin failed");
            }
        });
        Ok(())
    }

    /// Write stdin and wait for the sidecar acknowledgement.
    pub async fn write_process_stdin_awaited(
        &self,
        pid: u32,
        data: StdinInput,
    ) -> std::result::Result<(), ClientError> {
        let process_id = self.lookup_process_id(pid)?;
        self.write_wire_stdin(&process_id, stdin_to_bytes(data))
            .await
    }

    /// Close a spawned process's stdin. SYNC. Errors with `ProcessNotFound`.
    pub fn close_process_stdin(&self, pid: u32) -> std::result::Result<(), ClientError> {
        let process_id = self.lookup_process_id(pid)?;
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(error) = this.close_wire_stdin(&process_id).await {
                tracing::warn!(?error, pid, "close_process_stdin failed");
            }
        });
        Ok(())
    }

    /// Close stdin and wait for the sidecar acknowledgement.
    pub async fn close_process_stdin_awaited(
        &self,
        pid: u32,
    ) -> std::result::Result<(), ClientError> {
        let process_id = self.lookup_process_id(pid)?;
        self.close_wire_stdin(&process_id).await
    }

    /// Subscribe to the unified stdout/stderr event stream for a process.
    pub fn on_process_output(
        &self,
        pid: u32,
        mut handler: impl FnMut(ProcessOutput) + Send + 'static,
    ) -> std::result::Result<Subscription, ClientError> {
        let mut rx = self
            .inner()
            .processes
            .read(&pid, |_, entry| entry.output_tx.subscribe())
            .ok_or(ClientError::ProcessNotFound(pid))?;
        let task = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => handler(event),
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            pid,
                            skipped,
                            "process output subscriber lagged; recover with read_process_output"
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Ok(Subscription::new(move || task.abort()))
    }

    /// Register a once-only exit handler. If the process has already exited, the handler fires
    /// immediately and synchronously and a no-op unsubscribe is returned (the `watch` already holds
    /// a confirmed exit). Otherwise the handler fires once when the exit code lands. The exit code is
    /// `i32`, never null.
    pub fn on_process_exit(
        &self,
        pid: u32,
        handler: impl FnOnce(ProcessExit) + Send + 'static,
    ) -> std::result::Result<Subscription, ClientError> {
        let mut rx = self
            .inner()
            .processes
            .read(&pid, |_, entry| entry.exit_tx.subscribe())
            .ok_or(ClientError::ProcessNotFound(pid))?;

        // Already-exited branch: fire immediately + synchronously, return a no-op unsubscribe.
        if let Some(code) = rx.borrow().exit_code() {
            handler(ProcessExit {
                pid,
                exit_code: code,
            });
            return Ok(Subscription::noop());
        }

        if rx.borrow().rejected() {
            return Err(rx
                .borrow()
                .wait_result()
                .expect("rejected launch")
                .unwrap_err());
        }
        // Otherwise wait for a real exit, including after an ambiguous transport failure. The
        // returned `Subscription` cancels the waiting task on drop (= unsubscribe).
        let task = tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                if let Some(code) = rx.borrow().exit_code() {
                    handler(ProcessExit {
                        pid,
                        exit_code: code,
                    });
                    return;
                }
                if rx.borrow().rejected() {
                    return;
                }
            }
        });
        Ok(Subscription::new(move || task.abort()))
    }

    /// Await a spawned process's exit code. Unknown-pid lookup errors (synchronously in TS; here the
    /// lookup error is returned before any awaiting begins).
    pub async fn wait_process(&self, pid: u32) -> std::result::Result<i32, ClientError> {
        let (rx, process_id) = self
            .inner()
            .processes
            .read(&pid, |_, entry| {
                (entry.exit_tx.subscribe(), entry.process_id.clone())
            })
            .ok_or(ClientError::ProcessNotFound(pid))?;

        wait_for_process_outcome(rx, &process_id).await
    }

    /// Read bounded sequenced output retained for a spawned process.
    pub async fn read_process_output(
        &self,
        pid: u32,
        after: Option<u64>,
        max_events: Option<usize>,
        max_bytes: Option<usize>,
    ) -> std::result::Result<ProcessOutputReplay, ClientError> {
        let (process_id, retain_output, execution_id, execution_generation) = self
            .inner()
            .processes
            .read(&pid, |_, entry| {
                (
                    entry.process_id.clone(),
                    entry.retain_output,
                    entry.execution_id.clone(),
                    entry.execution_generation,
                )
            })
            .ok_or(ClientError::ProcessNotFound(pid))?;
        if !retain_output {
            return Err(ClientError::Sidecar(format!(
                "process {pid} was not spawned with output retention enabled"
            )));
        }
        if let Some(execution_id) = execution_id {
            let (max_events, max_bytes) =
                page_limits(max_events, max_bytes, "process.output.read")?;
            let generation = execution_generation.ok_or_else(|| {
                ClientError::Sidecar(format!(
                    "language process {pid} is missing its execution generation"
                ))
            })?;
            let response = self
                .transport()
                .request_wire(
                    self.vm_scope(),
                    wire::RequestPayload::ReadExecutionOutputRequest(
                        wire::ReadExecutionOutputRequest {
                            execution_id,
                            cursor: after
                                .map(|cursor| format!("{generation}:{}", cursor.saturating_add(1))),
                            limit: Some(u32::try_from(max_events).map_err(|_| {
                                ClientError::Sidecar(String::from(
                                    "process.output.read maxEvents exceeds the wire u32 range",
                                ))
                            })?),
                        },
                    ),
                )
                .await?;
            let page = match response {
                wire::ResponsePayload::ExecutionOutputPageResponse(page) => page,
                wire::ResponsePayload::RejectedResponse(rejected) => {
                    return Err(ClientError::from_rejection(rejected));
                }
                other => {
                    return Err(ClientError::Sidecar(format!(
                        "ReadExecutionOutput: unexpected response {other:?}"
                    )));
                }
            };
            let mut bytes = 0usize;
            let mut events = Vec::new();
            let mut has_more = page.has_more;
            for event in page.events {
                if bytes.saturating_add(event.chunk.len()) > max_bytes {
                    if events.is_empty() {
                        return Err(ClientError::ResourceLimit {
                            code: String::from("ERR_AGENTOS_RESOURCE_LIMIT"),
                            message: format!(
                                "process.output.read next retained event requires {} bytes, exceeding maxBytes={max_bytes}",
                                event.chunk.len()
                            ),
                            details: Box::new(ResourceLimitDetails {
                                limit_name: Some(String::from("output_replay_page_bytes")),
                                configured_limit: Some(max_bytes as u64),
                                requested: Some(event.chunk.len() as u64),
                                unit: Some(String::from("bytes")),
                                scope: Some(String::from("vm")),
                                operation: Some(String::from("process.output.read")),
                                configuration_path: Some(String::from("maxBytes")),
                                retryable: Some(true),
                                ..ResourceLimitDetails::default()
                            }),
                        });
                    }
                    has_more = true;
                    break;
                }
                bytes = bytes.saturating_add(event.chunk.len());
                events.push(ProcessOutputEvent {
                    pid,
                    sequence: event.sequence,
                    stream: match event.channel {
                        wire::ExecutionStreamChannel::Stdout => ProcessStream::Stdout,
                        wire::ExecutionStreamChannel::Stderr
                        | wire::ExecutionStreamChannel::Pty => ProcessStream::Stderr,
                    },
                    data: event.chunk,
                    timestamp_ms: event.timestamp_ms.min(i64::MAX as u64) as i64,
                });
            }
            let next_cursor = events.last().map(|event| event.sequence).or(after);
            return Ok(ProcessOutputReplay {
                pid,
                events,
                next_cursor,
                has_more,
                truncated: page.truncated,
                exit_code: self.get_process(pid)?.exit_code,
            });
        }
        let (max_events, max_bytes) =
            crate::output_replay::wire_page_limits(max_events, max_bytes, "process.output.read")?;
        let response = self
            .transport()
            .request_wire(
                self.vm_scope(),
                wire::RequestPayload::ReadProcessOutputRequest(wire::ReadProcessOutputRequest {
                    process_id: process_id.clone(),
                    after,
                    max_events: u32::try_from(max_events).map_err(|_| {
                        ClientError::Sidecar(String::from(
                            "process.output.read maxEvents exceeds the wire u32 range",
                        ))
                    })?,
                    max_bytes: u32::try_from(max_bytes).map_err(|_| {
                        ClientError::Sidecar(String::from(
                            "process.output.read maxBytes exceeds the wire u32 range",
                        ))
                    })?,
                }),
            )
            .await?;
        let page = match response {
            wire::ResponsePayload::ProcessOutputPageResponse(page) => page,
            wire::ResponsePayload::RejectedResponse(rejected) => {
                return Err(ClientError::from_rejection(rejected));
            }
            other => {
                return Err(ClientError::Sidecar(format!(
                    "ReadProcessOutput: unexpected response {other:?}"
                )));
            }
        };
        // A retained sidecar exit is stronger evidence than a missed live
        // event. Reconcile wait/get/list and wake the observation task, while
        // avoiding a concurrently reused registry entry.
        self.inner().processes.read(&pid, |_, entry| {
            if entry.process_id == process_id {
                reconcile_replayed_exit(&entry.exit_tx, page.exit_code);
            }
        });
        Ok(ProcessOutputReplay {
            pid,
            events: page
                .events
                .into_iter()
                .map(|event| ProcessOutputEvent {
                    pid,
                    sequence: event.sequence,
                    stream: match event.channel {
                        wire::StreamChannel::Stdout => ProcessStream::Stdout,
                        wire::StreamChannel::Stderr => ProcessStream::Stderr,
                    },
                    data: event.chunk,
                    timestamp_ms: event.timestamp_ms.min(i64::MAX as u64) as i64,
                })
                .collect(),
            next_cursor: page.next_cursor,
            has_more: page.has_more,
            truncated: page.truncated,
            exit_code: page.exit_code,
        })
    }

    /// List SDK-spawned processes only. `running = exit_code.is_none()`.
    pub fn list_processes(&self) -> Vec<SpawnedProcessInfo> {
        let mut out = Vec::new();
        self.inner().processes.scan(|pid, entry| {
            let exit_code = entry.exit_tx.borrow().exit_code();
            out.push(SpawnedProcessInfo {
                pid: *pid,
                command: entry.command.clone(),
                args: entry.args.clone(),
                running: exit_code.is_none(),
                exit_code,
                started_at: entry.started_at,
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
        Ok(self
            .process_tree_records()
            .await?
            .into_iter()
            .map(|record| record.info)
            .collect())
    }

    async fn process_tree_records(&self) -> Result<Vec<ProcessTreeRecord>> {
        let ownership = self.vm_scope();
        let response = self
            .transport()
            .request_wire(ownership, wire::RequestPayload::GetProcessSnapshotRequest)
            .await
            .context("all_processes: GetProcessSnapshot request failed")?;
        let snapshot = match response {
            wire::ResponsePayload::ProcessSnapshotResponse(snapshot) => snapshot,
            wire::ResponsePayload::RejectedResponse(rejected) => {
                return Err(ClientError::from_rejection(rejected).into());
            }
            other => {
                return Err(ClientError::Sidecar(format!(
                    "all_processes: unexpected response {other:?}"
                ))
                .into());
            }
        };

        // Snapshot the SDK process registry, keyed by wire `process_id`, capturing exit code,
        // command, and args. This mirrors the TS `trackedProcessesById` lookup used to build
        // `displayPidByKernelPid` and override fields.
        struct Tracked {
            display_pid: u32,
            exit_code: Option<i32>,
            command: String,
            args: Vec<String>,
        }
        let mut tracked_by_process_id: BTreeMap<String, Tracked> = BTreeMap::new();
        let mut display_pid_by_kernel_pid: BTreeMap<u32, u32> = BTreeMap::new();
        self.inner().processes.scan(|display_pid, entry| {
            let exit_code = entry.exit_tx.borrow().exit_code();
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
        let mut seen_process_ids: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        let mut out: Vec<ProcessTreeRecord> = Vec::new();

        // The snapshot may arrive before the ProcessStarted notification updates
        // the kernel-PID watch. Wire process identity is authoritative either way.
        for entry in &snapshot.processes {
            if let Some(tracked) = tracked_by_process_id.get(&entry.process_id) {
                display_pid_by_kernel_pid.insert(entry.pid, tracked.display_pid);
            }
        }

        for entry in snapshot.processes {
            let tracked = tracked_by_process_id.get(&entry.process_id);
            let display_pid = tracked.map_or(entry.pid, |process| process.display_pid);
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

            seen_process_ids.insert(entry.process_id.clone());
            out.push(ProcessTreeRecord {
                key: ProcessTreeKey::Kernel(entry.pid),
                parent: Some(ProcessTreeKey::Kernel(entry.ppid)),
                info: ProcessInfo {
                    pid: display_pid,
                    tracked_pid: tracked.map(|process| process.display_pid),
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
                },
            });
        }

        // Tracked processes not yet present in the snapshot (the spawn `Execute` has not surfaced in
        // the kernel table yet). TS fills these with `ppid:0, pgid/sid = pid`.
        self.inner().processes.scan(|display_pid, entry| {
            if seen_process_ids.contains(&entry.process_id) {
                return;
            }
            let exit_code = entry.exit_tx.borrow().exit_code();
            let process_key = format!("{}:{}", entry.process_id, display_pid);
            let start_time = self.observed_start_time(&process_key, now_ms);
            let (status, exit_time) = match exit_code {
                Some(_) => (
                    ProcessStatus::Exited,
                    Some(self.observed_exit_time(&entry.process_id, now_ms)),
                ),
                None => (ProcessStatus::Running, None),
            };
            out.push(ProcessTreeRecord {
                key: ProcessTreeKey::Tracked(*display_pid),
                parent: None,
                info: ProcessInfo {
                    pid: *display_pid,
                    tracked_pid: Some(*display_pid),
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
                },
            });
        });

        out.sort_by_key(|record| record.info.pid);
        Ok(out)
    }

    /// Return the first-observed start time for a process key, recording `now` the first time it is
    /// seen so later snapshots report a stable timestamp (TS `observedProcessStartTimes`).
    fn observed_start_time(&self, process_key: &str, now_ms: f64) -> f64 {
        let _guard = self.inner().observed_process_time_lock.lock();
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
        prune_string_f64_map(
            &self.inner().observed_process_start_times,
            OBSERVED_PROCESS_TIME_LIMIT,
        );
        // Re-read to honor a racing insert that may have won; either value is a valid first-observed
        // timestamp.
        self.inner()
            .observed_process_start_times
            .read(process_key, |_, value| *value)
            .unwrap_or(now_ms)
    }

    /// Return the first-observed exit time for an SDK process id, recording `now` on first sight.
    fn observed_exit_time(&self, process_id: &str, now_ms: f64) -> f64 {
        let _guard = self.inner().observed_process_time_lock.lock();
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
        prune_string_f64_map(
            &self.inner().observed_process_exit_times,
            OBSERVED_PROCESS_TIME_LIMIT,
        );
        self.inner()
            .observed_process_exit_times
            .read(process_id, |_, value| *value)
            .unwrap_or(now_ms)
    }

    /// Build the process forest from `all_processes`, linked by `ppid`.
    pub async fn process_tree(&self) -> Result<Vec<ProcessTreeNode>> {
        let processes = self.process_tree_records().await?;
        Ok(build_process_forest(processes))
    }

    /// Get a single SDK-spawned process's info. Errors (not None) when not found.
    pub fn get_process(&self, pid: u32) -> std::result::Result<SpawnedProcessInfo, ClientError> {
        self.inner()
            .processes
            .read(&pid, |pid, entry| {
                let exit_code = entry.exit_tx.borrow().exit_code();
                SpawnedProcessInfo {
                    pid: *pid,
                    command: entry.command.clone(),
                    args: entry.args.clone(),
                    running: exit_code.is_none(),
                    exit_code,
                    started_at: entry.started_at,
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

    /// Deliver a signal and wait for the sidecar acknowledgement.
    pub async fn signal_process_awaited(
        &self,
        pid: u32,
        signal: &str,
    ) -> std::result::Result<(), ClientError> {
        let (process_id, already_exited) = self
            .inner()
            .processes
            .read(&pid, |_, entry| {
                (
                    entry.process_id.clone(),
                    entry.exit_tx.borrow().reclaimable(),
                )
            })
            .ok_or(ClientError::ProcessNotFound(pid))?;
        if already_exited {
            return Ok(());
        }
        self.signal_wire_process(&process_id, signal).await
    }

    /// Resize a spawned process PTY and wait for the sidecar acknowledgement.
    pub async fn resize_process_pty_awaited(
        &self,
        pid: u32,
        cols: u16,
        rows: u16,
    ) -> std::result::Result<(), ClientError> {
        let process_id = self.lookup_process_id(pid)?;
        let response = self
            .transport()
            .request_wire(
                self.vm_scope(),
                wire::RequestPayload::ResizePtyRequest(wire::ResizePtyRequest {
                    process_id,
                    cols,
                    rows,
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::PtyResizedResponse(_) => Ok(()),
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            other => Err(ClientError::Sidecar(format!(
                "resize process PTY: unexpected response {other:?}"
            ))),
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Build the VM-scoped ownership for a wire request.
    pub(crate) fn vm_scope(&self) -> wire::OwnershipScope {
        wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: self.connection_id().to_string(),
            session_id: self.wire_session_id().to_string(),
            vm_id: self.vm_id().to_string(),
        })
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
        retain_output: bool,
    ) -> std::result::Result<wire::ProcessStartedResponse, ClientError> {
        let ownership = self.vm_scope();
        let response = self
            .transport()
            .request_wire(
                ownership,
                wire::RequestPayload::ExecuteRequest(wire::ExecuteRequest {
                    process_id: process_id.to_owned(),
                    command,
                    runtime: None,
                    entrypoint: None,
                    args,
                    env: env.into_iter().collect(),
                    cwd,
                    wasm_permission_tier: None,
                    retain_output,
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::ProcessStartedResponse(started) => Ok(started),
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
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
                (
                    entry.process_id.clone(),
                    entry.exit_tx.borrow().reclaimable(),
                )
            })
            .ok_or(ClientError::ProcessNotFound(pid))?;
        if already_exited {
            return Ok(());
        }
        let signal = signal.to_owned();
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(error) = this.signal_wire_process(&process_id, &signal).await {
                tracing::warn!(?error, pid, %signal, "signal_process failed");
            }
        });
        Ok(())
    }

    async fn write_wire_stdin(
        &self,
        process_id: &str,
        chunk: Vec<u8>,
    ) -> std::result::Result<(), ClientError> {
        let response = self
            .transport()
            .request_wire(
                self.vm_scope(),
                wire::RequestPayload::WriteStdinRequest(wire::WriteStdinRequest {
                    process_id: process_id.to_owned(),
                    chunk,
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::StdinWrittenResponse(_) => Ok(()),
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            other => Err(ClientError::Sidecar(format!(
                "write process stdin: unexpected response {other:?}"
            ))),
        }
    }

    async fn close_wire_stdin(&self, process_id: &str) -> std::result::Result<(), ClientError> {
        let response = self
            .transport()
            .request_wire(
                self.vm_scope(),
                wire::RequestPayload::CloseStdinRequest(wire::CloseStdinRequest {
                    process_id: process_id.to_owned(),
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::StdinClosedResponse(_) => Ok(()),
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            other => Err(ClientError::Sidecar(format!(
                "close process stdin: unexpected response {other:?}"
            ))),
        }
    }

    async fn signal_wire_process(
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
            .await?;
        match response {
            wire::ResponsePayload::ProcessKilledResponse(_) => Ok(()),
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            other => Err(ClientError::Sidecar(format!(
                "signal process: unexpected response {other:?}"
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

    /// Reserve one slot before an asynchronous spawn can reach the sidecar.
    /// Ordinary and language spawns share this admission path so concurrent
    /// submissions cannot all observe the same free registry capacity.
    pub(crate) fn reserve_process_registry_slot(
        &self,
    ) -> std::result::Result<ProcessRegistryReservation, ClientError> {
        let _guard = self.inner().process_registry_lock.lock();
        let pending = self
            .inner()
            .pending_process_registrations
            .load(Ordering::SeqCst);
        self.prune_exited_processes_locked(pending.saturating_add(1));
        let retained = self.process_registry_len_locked();
        if retained.saturating_add(pending) >= PROCESS_REGISTRY_LIMIT {
            return Err(process_registry_limit_error(
                retained.saturating_add(pending).saturating_add(1),
            ));
        }
        self.inner()
            .pending_process_registrations
            .fetch_add(1, Ordering::SeqCst);
        let admitted = retained.saturating_add(pending).saturating_add(1);
        if admitted >= PROCESS_REGISTRY_LIMIT * 4 / 5 {
            tracing::warn!(
                admitted,
                limit = PROCESS_REGISTRY_LIMIT,
                configuration_path = "PROCESS_REGISTRY_LIMIT",
                "process registry admission approaches its configured limit"
            );
        }
        Ok(ProcessRegistryReservation {
            client: self.clone(),
            active: true,
        })
    }

    fn prune_exited_processes_locked(&self, reserve_slots: usize) {
        let mut entries = Vec::new();
        self.inner().processes.scan(|pid, entry| {
            entries.push((*pid, entry.exit_tx.borrow().reclaimable(), entry.started_at));
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
        if let Some((_, entry)) = self.inner().processes.remove(&pid) {
            let _time_guard = self.inner().observed_process_time_lock.lock();
            let _ = self
                .inner()
                .observed_process_exit_times
                .remove(&entry.process_id);
            let fallback_start_key = format!("{}:{pid}", entry.process_id);
            let _ = self
                .inner()
                .observed_process_start_times
                .remove(&fallback_start_key);
            if let Some(kernel_pid) = *entry.kernel_pid.borrow() {
                let start_key = format!("{}:{kernel_pid}", entry.process_id);
                let _ = self.inner().observed_process_start_times.remove(&start_key);
            }
        }
    }

    /// Background pump for a spawned process: issue the `Execute` request, then fan kernel
    /// `ProcessOutput`/`ProcessExited` events for this process id into the per-process broadcast and
    /// watch channels. Exited entries are retained for post-exit inspection, then pruned oldest-first
    /// under registry pressure.
    #[allow(clippy::too_many_arguments)]
    async fn run_spawn(
        self,
        pid: u32,
        process_id: String,
        command: String,
        args: Vec<String>,
        options: SpawnOptions,
        mut events: broadcast::Receiver<(wire::OwnershipScope, EventPayload)>,
        stdout_tx: broadcast::Sender<Vec<u8>>,
        stderr_tx: broadcast::Sender<Vec<u8>>,
        output_tx: broadcast::Sender<ProcessOutput>,
        exit_tx: watch::Sender<ProcessOutcome>,
        kernel_pid_tx: watch::Sender<Option<u32>>,
    ) {
        match self
            .send_execute(
                &process_id,
                Some(command),
                args,
                options.env.clone(),
                options.cwd.clone(),
                options.retain_output,
            )
            .await
        {
            Ok(started) => {
                // Seed the kernel pid so `all_processes`/`process_tree` can remap this process's
                // kernel-snapshot entry back to its display pid.
                if let Some(kernel_pid) = started.pid {
                    kernel_pid_tx.send_replace(Some(kernel_pid));
                }
            }
            Err(error) => {
                // Launch rejection/transport failure is not a guest exit.
                let message = format!("{error}\n");
                let bytes = message.into_bytes();
                let _ = stderr_tx.send(bytes.clone());
                let _ = output_tx.send(ProcessOutput {
                    pid,
                    stream: ProcessStream::Stderr,
                    data: bytes,
                    sequence: None,
                    timestamp_ms: None,
                });
                tracing::error!(?error, pid, %process_id, "spawn: Execute request failed");
                let failure = ProcessOutcome::launch_failure(&process_id, error);
                let rejected = failure.rejected();
                exit_tx.send_replace(failure);
                if rejected {
                    let _guard = self.inner().process_registry_lock.lock();
                    self.prune_exited_processes_locked(0);
                    return;
                }
                // A failed transport acknowledgement cannot revoke admission. Keep
                // routing events and signals until a real exit or VM teardown.
            }
        }

        let ownership = self.vm_scope();
        while let Some(payload) =
            next_spawn_event(&mut events, &ownership, &process_id, &exit_tx).await
        {
            match payload {
                EventPayload::ProcessOutputEvent(output) if output.process_id == process_id => {
                    let bytes = output.chunk;
                    let stream = match output.channel {
                        StreamChannel::Stdout => ProcessStream::Stdout,
                        StreamChannel::Stderr => ProcessStream::Stderr,
                    };
                    let _ = output_tx.send(ProcessOutput {
                        pid,
                        stream,
                        data: bytes.clone(),
                        sequence: output.sequence,
                        timestamp_ms: output
                            .timestamp_ms
                            .map(|value| value.min(i64::MAX as u64) as i64),
                    });
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
                    break;
                }
                EventPayload::ProcessOutputEvent(_)
                | EventPayload::ProcessExitedEvent(_)
                | EventPayload::ExecutionOutputEvent(_)
                | EventPayload::ExecutionCompletedEvent(_)
                | EventPayload::VmLifecycleEvent(_)
                | EventPayload::StructuredEvent(_)
                | EventPayload::ExtEnvelope(_) => {}
            }
        }
        let _guard = self.inner().process_registry_lock.lock();
        self.prune_exited_processes_locked(0);
    }
}

pub(crate) struct ProcessRegistryReservation {
    client: AgentOs,
    active: bool,
}

impl ProcessRegistryReservation {
    pub(crate) fn commit(
        mut self,
        pid: u32,
        entry: ProcessEntry,
    ) -> std::result::Result<(), ClientError> {
        let client = self.client.clone();
        let _guard = client.inner().process_registry_lock.lock();
        client.inner().processes.insert(pid, entry).map_err(|_| {
            ClientError::Sidecar(format!(
                "process registry already contains public pid {pid}; retry the spawn"
            ))
        })?;
        client
            .inner()
            .pending_process_registrations
            .fetch_sub(1, Ordering::SeqCst);
        self.active = false;
        Ok(())
    }

    fn release(&mut self) {
        if self.active {
            self.client
                .inner()
                .pending_process_registrations
                .fetch_sub(1, Ordering::SeqCst);
            self.active = false;
        }
    }
}

impl Drop for ProcessRegistryReservation {
    fn drop(&mut self) {
        self.release();
    }
}

fn process_registry_limit_error(requested: usize) -> ClientError {
    ClientError::ResourceLimit {
        code: String::from("ERR_AGENTOS_RESOURCE_LIMIT"),
        message: format!(
            "process registry limit {PROCESS_REGISTRY_LIMIT} reached; wait for an exited process to be evicted or raise PROCESS_REGISTRY_LIMIT"
        ),
        details: Box::new(crate::ResourceLimitDetails {
            limit_name: Some(String::from("process_registry_entries")),
            configured_limit: Some(PROCESS_REGISTRY_LIMIT as u64),
            requested: Some(requested as u64),
            unit: Some(String::from("processes")),
            scope: Some(String::from("vm")),
            operation: Some(String::from("process.spawn")),
            configuration_path: Some(String::from("PROCESS_REGISTRY_LIMIT")),
            retryable: Some(true),
            ..Default::default()
        }),
    }
}

/// Keep draining events while awaiting the signal acknowledgement: a busy shared transport must
/// not evict the only exit event from this receiver during a slow kill response.
async fn confirm_exec_exit(
    process_id: &str,
    ownership: &wire::OwnershipScope,
    events: &mut broadcast::Receiver<(wire::OwnershipScope, EventPayload)>,
    kill: impl std::future::Future<Output = std::result::Result<(), ClientError>>,
    kill_ack_timeout: std::time::Duration,
    confirmation_timeout: std::time::Duration,
) -> std::result::Result<i32, ClientError> {
    let deadline = tokio::time::sleep(confirmation_timeout);
    let kill = tokio::time::timeout(kill_ack_timeout, kill);
    tokio::pin!(deadline, kill);
    let mut kill_finished = false;
    let mut kill_error = None;
    loop {
        let failure = tokio::select! {
            biased;
            _ = &mut deadline => Some(format!("no exit event within {}ms", confirmation_timeout.as_millis())),
            result = &mut kill, if !kill_finished => {
                kill_finished = true;
                kill_error = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error.to_string()),
                    Err(_) => Some(format!("SIGKILL did not acknowledge within {}ms", kill_ack_timeout.as_millis())),
                };
                None
            }
            frame = events.recv() => match frame {
                Ok((scope, EventPayload::ProcessExitedEvent(exited)))
                    if scope == *ownership && exited.process_id == process_id => return Ok(exited.exit_code),
                Ok(_) => None,
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(%process_id, skipped, "exec exit confirmation receiver lagged");
                    None
                }
                Err(broadcast::error::RecvError::Closed) => Some("event stream closed before exit confirmation".to_owned()),
            },
        };
        if let Some(mut reason) = failure {
            if let Some(kill_error) = kill_error {
                reason.push_str(&format!("; SIGKILL failed: {kill_error}"));
            }
            return Err(ClientError::TerminationFailed {
                process_id: process_id.to_owned(),
                reason,
            });
        }
    }
}

/// Link by raw kernel identity, never by potentially overlapping display PIDs.
/// Tracked entries not yet present in the snapshot occupy a separate namespace.
fn build_process_forest(processes: Vec<ProcessTreeRecord>) -> Vec<ProcessTreeNode> {
    use std::collections::BTreeMap as Map;

    let pids: std::collections::BTreeSet<ProcessTreeKey> =
        processes.iter().map(|record| record.key).collect();
    let mut children_of: Map<ProcessTreeKey, Vec<usize>> = Map::new();
    let mut roots: Vec<usize> = Vec::new();
    for (index, proc) in processes.iter().enumerate() {
        if let Some(parent) = proc.parent.filter(|parent| pids.contains(parent)) {
            children_of.entry(parent).or_default().push(index);
        } else {
            roots.push(index);
        }
    }

    fn build_node(
        index: usize,
        processes: &[ProcessTreeRecord],
        children_of: &Map<ProcessTreeKey, Vec<usize>>,
        seen: &mut std::collections::BTreeSet<usize>,
    ) -> ProcessTreeNode {
        let record = &processes[index];
        let info = record.info.clone();
        seen.insert(index);
        let child_indices: Vec<usize> = children_of
            .get(&record.key)
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

fn append_exec_output(
    buffer: &mut Vec<u8>,
    chunk: &[u8],
    captured_output_bytes: &mut usize,
    channel: &str,
) -> std::result::Result<(), ClientError> {
    let next_total = captured_output_bytes
        .checked_add(chunk.len())
        .ok_or_else(|| exec_output_limit_error(channel, usize::MAX))?;
    if next_total > EXEC_OUTPUT_CAPTURE_LIMIT_BYTES {
        return Err(exec_output_limit_error(channel, next_total));
    }
    buffer.extend_from_slice(chunk);
    *captured_output_bytes = next_total;
    Ok(())
}

fn exec_output_limit_error(channel: &str, size: usize) -> ClientError {
    ClientError::Sidecar(format!(
        "exec {channel} capture is {size} bytes, limit is {EXEC_OUTPUT_CAPTURE_LIMIT_BYTES}"
    ))
}

fn exited_pids_to_prune(mut entries: Vec<(u32, bool, i64)>, target_len: usize) -> Vec<u32> {
    if entries.len() <= target_len {
        return Vec::new();
    }
    let mut remove_count = entries.len() - target_len;
    entries.sort_by_key(|(pid, _, started_at)| (*started_at, *pid));
    let mut out = Vec::new();
    for (pid, exited, _) in entries {
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

fn prune_string_f64_map(map: &SccHashMap<String, f64>, limit: usize) {
    let mut keys = Vec::new();
    map.scan(|key, _| {
        keys.push(key.clone());
    });
    if keys.len() <= limit {
        return;
    }
    let remove_count = keys.len() - limit;
    keys.sort();
    for key in keys.into_iter().take(remove_count) {
        let _ = map.remove(&key);
    }
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
/// disposed VM. Mirrors the `pending_shell_exits` / terminal drain in `shutdown`.
pub(crate) fn drain_process_output_tasks(processes: &SccHashMap<u32, ProcessEntry>) {
    let mut tasks = Vec::new();
    processes.retain(|_, entry| {
        entry.exit_tx.send_if_modified(|state| {
            if !matches!(state, ProcessOutcome::Pending) { return false; }
            tracing::error!(process_id = %entry.process_id, "VM shutdown before process exit was observed");
            *state = ProcessOutcome::Failed {
                error: ClientError::TerminationFailed {
                    process_id: entry.process_id.clone(),
                    reason: "VM shutdown before process exit was observed".into(),
                },
                rejected: false,
            };
            true
        });
        tasks.append(&mut entry.output_tasks);
        false
    });
    for task in tasks {
        task.abort();
    }
}

/// Current wall-clock time as epoch milliseconds (TS `Date.now()`).
fn epoch_ms_now() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::{
        append_exec_output, drain_process_output_tasks, exited_pids_to_prune,
        install_output_callback, process_registry_limit_error, prune_string_f64_map, ExecOptions,
        OutputCallback, DEFAULT_EXEC_CWD, EXEC_OUTPUT_CAPTURE_LIMIT_BYTES, PROCESS_REGISTRY_LIMIT,
    };
    use super::{confirm_exec_exit, wire, ClientError, EventPayload};
    use crate::agent_os::ProcessEntry;
    use scc::HashMap as SccHashMap;
    use tokio::sync::{broadcast, watch};

    #[tokio::test]
    async fn spawn_launch_rejection_is_typed_and_retained_for_late_waiters() {
        let (outcome, _) = watch::channel(super::ProcessOutcome::Pending);
        outcome.send_replace(super::ProcessOutcome::launch_failure(
            "p",
            ClientError::Kernel {
                code: "ENOENT".into(),
                message: "command missing".into(),
            },
        ));
        assert_eq!(outcome.borrow().exit_code(), None);
        assert!(outcome.borrow().reclaimable());
        assert!(
            matches!(super::wait_for_process_outcome(outcome.subscribe(), "p").await,
            Err(ClientError::Kernel { code, .. }) if code == "ENOENT")
        );
    }

    #[tokio::test]
    async fn spawn_ambiguous_failure_keeps_observing_a_vm_scoped_real_exit() {
        let (outcome, _) = watch::channel(super::ProcessOutcome::Pending);
        outcome.send_replace(super::ProcessOutcome::launch_failure(
            "p",
            ClientError::Sidecar("lost acknowledgement".into()),
        ));
        assert_eq!(outcome.borrow().exit_code(), None);
        assert!(!outcome.borrow().reclaimable());
        assert!(matches!(
            super::wait_for_process_outcome(outcome.subscribe(), "p").await,
            Err(ClientError::TerminationFailed { .. })
        ));

        let (sender, mut events) = broadcast::channel(2);
        let (ownership, event) = exec_exit_frame("vm", "p");
        let (foreign_scope, _) = exec_exit_frame("other-vm", "p");
        sender
            .send((
                foreign_scope,
                EventPayload::ProcessExitedEvent(wire::ProcessExitedEvent {
                    process_id: "p".into(),
                    exit_code: 0,
                }),
            ))
            .unwrap();
        sender.send((ownership.clone(), event)).unwrap();
        let delivered = super::next_spawn_event(&mut events, &ownership, "p", &outcome).await;
        assert!(matches!(
            delivered,
            Some(EventPayload::ProcessExitedEvent(_))
        ));
        // There was still no watch receiver when the real exit was published.
        assert_eq!(
            super::wait_for_process_outcome(outcome.subscribe(), "p")
                .await
                .unwrap(),
            137
        );
        assert!(outcome.borrow().reclaimable());
    }

    #[tokio::test]
    async fn replayed_exit_recovers_a_missed_event_and_releases_the_observer() {
        for failed in [false, true] {
            let initial = if failed {
                super::ProcessOutcome::launch_failure(
                    "p",
                    ClientError::Sidecar("lost exit observation".into()),
                )
            } else {
                super::ProcessOutcome::Pending
            };
            let (outcome, _) = watch::channel(initial);
            let (_sender, mut events) = broadcast::channel(1);
            let (ownership, _) = exec_exit_frame("vm", "p");
            let observation = super::next_spawn_event(&mut events, &ownership, "p", &outcome);
            tokio::pin!(observation);
            tokio::select! {
                _ = &mut observation => panic!("no exit has been observed"),
                _ = tokio::task::yield_now() => {}
            }
            super::reconcile_replayed_exit(&outcome, None);
            assert_eq!(outcome.borrow().exit_code(), None);
            super::reconcile_replayed_exit(&outcome, Some(7));
            assert!(
                tokio::time::timeout(std::time::Duration::from_secs(1), observation)
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                super::wait_for_process_outcome(outcome.subscribe(), "p")
                    .await
                    .unwrap(),
                7
            );
            // Repeated replay does not synthesize another completion notification.
            let receiver = outcome.subscribe();
            super::reconcile_replayed_exit(&outcome, Some(7));
            assert!(!receiver.has_changed().unwrap());
        }
    }

    #[tokio::test]
    async fn spawn_event_stream_closure_and_lag_never_manufacture_success() {
        for lagged in [false, true] {
            let (outcome, _) = watch::channel(super::ProcessOutcome::Pending);
            let (sender, mut events) = broadcast::channel(1);
            if lagged {
                sender.send(exec_exit_frame("other-vm", "p")).unwrap();
                sender.send(exec_exit_frame("other-vm", "p")).unwrap();
            }
            drop(sender);
            let (ownership, _) = exec_exit_frame("vm", "p");
            assert!(
                super::next_spawn_event(&mut events, &ownership, "p", &outcome)
                    .await
                    .is_none()
            );
            assert_eq!(outcome.borrow().exit_code(), None);
            assert!(!outcome.borrow().reclaimable());
            assert!(matches!(
                super::wait_for_process_outcome(outcome.subscribe(), "p").await,
                Err(ClientError::TerminationFailed { .. })
            ));
        }
    }

    #[tokio::test]
    async fn spawn_closed_outcome_channel_is_a_typed_observation_failure() {
        let (outcome, rx) = watch::channel(super::ProcessOutcome::Pending);
        drop(outcome);
        assert!(matches!(
            super::wait_for_process_outcome(rx, "p").await,
            Err(ClientError::TerminationFailed { .. })
        ));
    }

    #[tokio::test]
    async fn spawn_vm_disposal_ends_observation_without_a_synthetic_exit() {
        let (outcome, _) = watch::channel(super::ProcessOutcome::Pending);
        let (sender, mut events) = broadcast::channel(1);
        let (ownership, _) = exec_exit_frame("vm", "p");
        sender
            .send((
                ownership.clone(),
                EventPayload::VmLifecycleEvent(wire::VmLifecycleEvent {
                    state: wire::VmLifecycleState::Disposed,
                }),
            ))
            .unwrap();
        assert!(
            super::next_spawn_event(&mut events, &ownership, "p", &outcome)
                .await
                .is_none()
        );
        assert_eq!(outcome.borrow().exit_code(), None);
        assert!(matches!(
            super::wait_for_process_outcome(outcome.subscribe(), "p").await,
            Err(ClientError::TerminationFailed { .. })
        ));
    }

    #[test]
    fn background_completion_without_status_remains_a_failure() {
        assert_eq!(
            super::ProcessOutcome::completion("p", Some(0)).exit_code(),
            Some(0)
        );
        let missing = super::ProcessOutcome::completion("p", None);
        assert_eq!(missing.exit_code(), None);
        assert!(matches!(
            missing.wait_result(),
            Some(Err(ClientError::TerminationFailed { .. }))
        ));
    }

    fn exec_exit_frame(vm_id: &str, process_id: &str) -> (wire::OwnershipScope, EventPayload) {
        (
            wire::OwnershipScope::VmOwnership(wire::VmOwnership {
                connection_id: "connection".into(),
                session_id: "session".into(),
                vm_id: vm_id.into(),
            }),
            EventPayload::ProcessExitedEvent(wire::ProcessExitedEvent {
                process_id: process_id.into(),
                exit_code: 137,
            }),
        )
    }

    #[tokio::test]
    async fn exec_confirmation_requires_matching_vm_and_process() {
        let (sender, mut events) = broadcast::channel(8);
        let (ownership, _) = exec_exit_frame("vm", "process");
        sender.send(exec_exit_frame("other-vm", "process")).unwrap();
        sender.send(exec_exit_frame("vm", "other-process")).unwrap();
        let error = confirm_exec_exit(
            "process",
            &ownership,
            &mut events,
            async { Ok(()) },
            std::time::Duration::from_millis(5),
            std::time::Duration::from_millis(20),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, ClientError::TerminationFailed { .. }));
    }

    #[tokio::test]
    async fn exec_confirmation_observes_exit_without_waiting_for_kill_ack() {
        let (sender, mut events) = broadcast::channel(8);
        let frame = exec_exit_frame("vm", "process");
        let ownership = frame.0.clone();
        sender.send(frame).unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            confirm_exec_exit(
                "process",
                &ownership,
                &mut events,
                std::future::pending(),
                std::time::Duration::from_secs(5),
                std::time::Duration::from_secs(30),
            ),
        )
        .await
        .expect("an exit event is proof even while kill acknowledgement is pending");
        assert_eq!(result.unwrap(), 137);
    }

    #[tokio::test]
    async fn exec_confirmation_preserves_signal_failure_on_closed_stream() {
        let (sender, mut events) = broadcast::channel(8);
        drop(sender);
        let (ownership, _) = exec_exit_frame("vm", "process");
        let error = confirm_exec_exit(
            "process",
            &ownership,
            &mut events,
            async { Err(ClientError::Sidecar("kill rejection".into())) },
            std::time::Duration::from_millis(5),
            std::time::Duration::from_millis(20),
        )
        .await
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("event stream closed"));
        assert!(message.contains("kill rejection"));
    }

    #[test]
    fn process_forest_separates_colliding_display_pids_and_fallbacks() {
        use super::{
            build_process_forest, ProcessInfo, ProcessStatus, ProcessTreeKey, ProcessTreeRecord,
        };
        let record = |pid, tracked_pid, key, parent, command: &str| ProcessTreeRecord {
            info: ProcessInfo {
                pid,
                tracked_pid,
                ppid: 0,
                pgid: pid,
                sid: pid,
                driver: "test".into(),
                command: command.into(),
                args: Vec::new(),
                cwd: "/".into(),
                status: ProcessStatus::Running,
                exit_code: None,
                start_time: 1.0,
                exit_time: None,
            },
            key,
            parent,
        };
        let roots = build_process_forest(vec![
            record(
                1_000_000,
                Some(1_000_000),
                ProcessTreeKey::Kernel(7),
                None,
                "tracked",
            ),
            record(
                1_000_000,
                None,
                ProcessTreeKey::Kernel(1_000_000),
                Some(ProcessTreeKey::Kernel(7)),
                "guest",
            ),
            record(
                8,
                None,
                ProcessTreeKey::Kernel(8),
                Some(ProcessTreeKey::Kernel(1_000_000)),
                "grandchild",
            ),
            record(
                8,
                Some(8),
                ProcessTreeKey::Tracked(8),
                None,
                "pending tracked",
            ),
        ]);
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].info.command, "tracked");
        assert_eq!(roots[0].children.len(), 1);
        let child = &roots[0].children[0];
        assert_eq!(child.info.pid, roots[0].info.pid);
        assert_eq!(child.info.tracked_pid, None);
        assert_eq!(child.info.command, "guest");
        assert_eq!(child.children.len(), 1);
        assert_eq!(child.children[0].info.command, "grandchild");
        assert_eq!(roots[1].info.command, "pending tracked");
        assert!(roots[1].children.is_empty());
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
        let (output_tx, _) = broadcast::channel(8);
        let (exit_tx, _) = watch::channel(super::ProcessOutcome::Pending);
        let exit_rx = exit_tx.subscribe();
        let (kernel_pid_tx, _) = watch::channel::<Option<u32>>(None);

        // A task that never completes on its own, standing in for an output-callback task that is
        // waiting on a `Closed` that the retained sender clone prevents.
        let task = tokio::spawn(async {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        });
        let abort_handle = task.abort_handle();

        let entry = ProcessEntry {
            command: "sleep".to_string(),
            args: vec!["3600".to_string()],
            stdout_tx,
            stderr_tx,
            output_tx,
            exit_tx,
            process_id: "proc-test".to_string(),
            kernel_pid: kernel_pid_tx,
            output_tasks: vec![task],
            retain_output: false,
            execution_id: None,
            execution_generation: None,
            started_at: 0,
        };
        let _ = processes.insert(1, entry);

        assert!(!abort_handle.is_finished(), "task should start alive");

        drain_process_output_tasks(&processes);

        assert!(matches!(
            super::wait_for_process_outcome(exit_rx, "proc-test").await,
            Err(ClientError::TerminationFailed { .. })
        ));

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
        let (output_tx, _) = broadcast::channel(8);
        let (exit_tx, _) = watch::channel(super::ProcessOutcome::Pending);
        let (kernel_pid_tx, _) = watch::channel::<Option<u32>>(None);

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_cb = Arc::clone(&calls);
        let cb: OutputCallback = Box::new(move |_chunk: &[u8]| {
            calls_cb.fetch_add(1, Ordering::SeqCst);
        });

        // The exact seam from `spawn_inner`: capture the returned handle in `output_tasks`.
        let output_tasks = vec![install_output_callback(stdout_tx.clone(), cb)];

        let entry = ProcessEntry {
            command: "sleep".to_string(),
            args: vec!["3600".to_string()],
            stdout_tx: stdout_tx.clone(),
            stderr_tx,
            output_tx,
            exit_tx,
            process_id: "proc-test".to_string(),
            kernel_pid: kernel_pid_tx,
            output_tasks,
            retain_output: false,
            execution_id: None,
            execution_generation: None,
            started_at: 0,
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
    fn exec_options_default_uses_workspace_cwd() {
        assert_eq!(
            ExecOptions::default().cwd.as_deref(),
            Some(DEFAULT_EXEC_CWD)
        );
    }

    #[test]
    fn append_exec_output_rejects_capture_over_limit() {
        let mut buffer = vec![0u8; EXEC_OUTPUT_CAPTURE_LIMIT_BYTES - 1];
        let mut captured = buffer.len();

        append_exec_output(&mut buffer, &[1], &mut captured, "stdout")
            .expect("chunk at limit should fit");
        assert_eq!(captured, EXEC_OUTPUT_CAPTURE_LIMIT_BYTES);

        let error = append_exec_output(&mut buffer, &[2], &mut captured, "stdout")
            .expect_err("chunk over limit should fail");
        assert!(
            error.to_string().contains("exec stdout capture is"),
            "unexpected error: {error}"
        );
        assert_eq!(captured, EXEC_OUTPUT_CAPTURE_LIMIT_BYTES);
        assert_eq!(buffer.len(), EXEC_OUTPUT_CAPTURE_LIMIT_BYTES);
    }

    #[test]
    fn exited_pid_pruning_keeps_live_entries_and_removes_oldest_exited() {
        let pids = exited_pids_to_prune(
            vec![(3, true, 30), (1, false, 10), (2, true, 40), (4, true, 20)],
            2,
        );
        assert_eq!(pids, vec![4, 3]);
    }

    #[test]
    fn process_registry_limit_error_is_typed_and_actionable() {
        let error = process_registry_limit_error(PROCESS_REGISTRY_LIMIT + 1);
        let ClientError::ResourceLimit {
            code,
            details,
            message,
        } = error
        else {
            panic!("expected typed resource limit");
        };
        assert_eq!(code, "ERR_AGENTOS_RESOURCE_LIMIT");
        assert_eq!(
            details.limit_name.as_deref(),
            Some("process_registry_entries")
        );
        assert_eq!(
            details.configured_limit,
            Some(PROCESS_REGISTRY_LIMIT as u64)
        );
        assert_eq!(details.requested, Some((PROCESS_REGISTRY_LIMIT + 1) as u64));
        assert_eq!(details.operation.as_deref(), Some("process.spawn"));
        assert_eq!(
            details.configuration_path.as_deref(),
            Some("PROCESS_REGISTRY_LIMIT")
        );
        assert!(message.contains("raise PROCESS_REGISTRY_LIMIT"));
    }

    #[test]
    fn observed_time_pruning_enforces_limit() {
        let map = SccHashMap::new();
        let _ = map.insert("b".to_string(), 2.0);
        let _ = map.insert("a".to_string(), 1.0);
        let _ = map.insert("c".to_string(), 3.0);

        prune_string_f64_map(&map, 2);

        assert!(map.read("a", |_, _| ()).is_none());
        assert!(map.read("b", |_, _| ()).is_some());
        assert!(map.read("c", |_, _| ()).is_some());
    }
}
