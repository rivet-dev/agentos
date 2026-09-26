//! Network (fetch) and Shell / terminal methods + supporting types.
//!
//! Ported from `packages/core/src/agent-os.ts` (`fetch` + shell methods) and `runtime-compat.ts`
//! (`ShellHandle`, `OpenShellOptions`, `ConnectTerminalOptions`).
//!
//! Id-vs-PID is load-bearing: `open_shell` returns a synthetic `shell-N` id; `connect_terminal`
//! returns a PID and is NOT tracked in the shells map.
//!
//! The native wire protocol has no PTY/winsize request, so a shell is modeled as a guest process
//! spawned via [`ExecuteRequest`]: its `process_id` is what `write_shell`/`close_shell` address on
//! the wire, while the public boundary keeps the synthetic `shell-N` id.
//!
//! Stream routing mirrors the TS PTY path: the public `data` stream (`on_shell_data`) carries stdout
//! and stderr in the order received from the sidecar. stderr is also delivered on an optional
//! channel-specific diagnostic tap (`on_shell_stderr` + [`OpenShellOptions::on_stderr`]); terminal
//! renderers consume only `data` so prompts and control sequences are neither reordered nor doubled.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

use agentos_sidecar_client::wire::{self, EventPayload, StreamChannel};

use crate::agent_os::{AgentOs, ClosedShellEntry, ShellEntry, TerminalEntry};
use crate::error::ClientError;
use crate::process::{
    install_output_callback, OutputCallback, ProcessStatus, ProcessStream, StdinInput,
};

/// Channel capacity for a shell's ordered terminal-data and diagnostic-stderr broadcasts.
const SHELL_DATA_CHANNEL_CAPACITY: usize = 1024;

/// Maximum active or spawning shells and terminals per VM.
const TERMINAL_LIMIT: usize = 1024;

/// Default shell command used when [`OpenShellOptions::command`] is omitted (matches the kernel's
/// PTY-backed `sh`).
const DEFAULT_SHELL_COMMAND: &str = "sh";

type ShellOutcome = Option<std::result::Result<i32, ClientError>>;
type ShellSpawnReceiver = watch::Receiver<Option<std::result::Result<(), ClientError>>>;

fn publish_shell_outcome(
    sender: &watch::Sender<ShellOutcome>,
    result: std::result::Result<i32, ClientError>,
) {
    sender.send_if_modified(|outcome| {
        // Neither a late observation failure nor repeated replay may replace a
        // confirmed exit or emit another completion notification.
        if matches!(outcome, Some(Ok(_))) {
            return false;
        }
        *outcome = Some(result);
        true
    });
}

fn retain_shell_result(retained: &mut VecDeque<ClosedShellEntry>, entry: ClosedShellEntry) {
    if let Some(existing) = retained.iter_mut().find(|existing| {
        existing.shell_id == entry.shell_id && existing.process_id == entry.process_id
    }) {
        if existing.result.is_err() {
            *existing = entry;
        }
        return;
    }
    retained.push_back(entry);
    while retained.len() > crate::CLOSED_SHELL_EXIT_CODE_RETENTION_LIMIT {
        retained.pop_front();
    }
}

async fn next_shell_event(
    events: &mut broadcast::Receiver<(wire::OwnershipScope, EventPayload)>,
    ownership: &wire::OwnershipScope,
    outcome: &watch::Sender<ShellOutcome>,
) -> Option<std::result::Result<EventPayload, broadcast::error::RecvError>> {
    let mut replay_outcome = outcome.subscribe();
    loop {
        if matches!(*replay_outcome.borrow(), Some(Ok(_))) {
            return None;
        }
        tokio::select! {
            event = events.recv() => match event {
                Ok((scope, payload)) if scope == *ownership => return Some(Ok(payload)),
                Ok(_) => continue,
                Err(error) => return Some(Err(error)),
            },
            changed = replay_outcome.changed() => {
                if let Err(error) = changed {
                    tracing::error!(%error, "terminal outcome channel closed during observation");
                    return None;
                }
            }
        }
    }
}

async fn observe_shell_exit(mut receiver: watch::Receiver<ShellOutcome>) -> ShellOutcome {
    loop {
        if let Some(result) = receiver.borrow_and_update().clone() {
            return Some(result);
        }
        if receiver.changed().await.is_err() {
            return None;
        }
    }
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Callback-free options for portable `open_shell`.
#[derive(Default)]
pub struct OpenShellOptions {
    pub wasm_backend: Option<crate::process::StandaloneWasmBackend>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

/// Options for `connect_terminal` (extends [`OpenShellOptions`]).
///
/// `on_data` mirrors the TS `ConnectTerminalOptions.onData` raw-byte callback. When omitted, TS pipes
/// shell output to host stdout; the Rust port routes it through the shell's data subscription and
/// requires the caller to provide the sink because there is no host-process stdio to bind to.
#[derive(Default)]
pub struct ConnectTerminalOptions {
    pub base: OpenShellOptions,
    pub on_data: Option<OutputCallback>,
    pub on_stderr: Option<OutputCallback>,
}

/// The synthetic shell id returned by `open_shell` (`shell-N`, NOT a pid).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellHandle {
    pub shell_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellData {
    pub shell_id: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellExit {
    pub shell_id: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInfo {
    pub shell_id: String,
    pub pid: u32,
    pub running: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputEvent {
    pub sequence: u64,
    pub stream: ProcessStream,
    pub data: Vec<u8>,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSnapshot {
    pub shell_id: String,
    pub pid: u32,
    pub events: Vec<TerminalOutputEvent>,
    pub next_cursor: Option<u64>,
    pub has_more: bool,
    pub truncated: bool,
    pub exit_code: Option<i32>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map a [`RejectedResponse`] into a [`ClientError::Kernel`] so the errno `code` survives.
fn rejected_to_error(rejected: wire::RejectedResponse) -> ClientError {
    ClientError::from_rejection(rejected)
}

fn shell_started(response: wire::ResponsePayload) -> std::result::Result<u32, ClientError> {
    match response {
        wire::ResponsePayload::ProcessStartedResponse(wire::ProcessStartedResponse {
            pid: Some(pid),
            ..
        }) => Ok(pid),
        wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
        other => Err(ClientError::Sidecar(format!(
            "open_shell: expected a started process with a PID, received {other:?}"
        ))),
    }
}

struct ShellReservation(AgentOs);

impl Drop for ShellReservation {
    fn drop(&mut self) {
        release_counter(&self.0.inner().terminal_count);
    }
}

/// Encode a [`StdinInput`] into the wire `chunk` bytes. The wire `chunk` field is bare `data`
/// (`Vec<u8>`), so raw Binary stdin is carried verbatim (no lossy UTF-8 conversion), matching the
/// byte-exact TS `proc.writeStdin` contract.
fn stdin_chunk(data: StdinInput) -> Vec<u8> {
    match data {
        StdinInput::Text(text) => text.into_bytes(),
        StdinInput::Bytes(bytes) => bytes,
    }
}

fn try_reserve_counter(counter: &AtomicUsize, limit: usize) -> bool {
    counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
            (count < limit).then_some(count + 1)
        })
        .is_ok()
}

fn release_counter(counter: &AtomicUsize) {
    let _ = counter.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
        Some(count.saturating_sub(1))
    });
}

struct TerminalReservation<'a> {
    agent: &'a AgentOs,
    active: bool,
}

impl<'a> TerminalReservation<'a> {
    fn new(agent: &'a AgentOs) -> std::result::Result<Self, ClientError> {
        if !try_reserve_counter(&agent.inner().terminal_count, TERMINAL_LIMIT) {
            return Err(ClientError::ResourceLimit {
                code: "ERR_AGENTOS_RESOURCE_LIMIT".into(),
                message: format!("terminal limit {TERMINAL_LIMIT} reached; wait for existing terminals to exit or raise TERMINAL_LIMIT"),
                details: Box::new(crate::ResourceLimitDetails {
                    limit_name: Some("active_terminals".into()),
                    configured_limit: Some(TERMINAL_LIMIT as u64),
                    requested: Some(TERMINAL_LIMIT as u64 + 1),
                    configuration_path: Some("TERMINAL_LIMIT".into()),
                    unit: Some("terminals".into()),
                    scope: Some("vm".into()),
                    retryable: Some(true),
                    ..Default::default()
                }),
            });
        }
        let active = agent.inner().terminal_count.load(Ordering::SeqCst);
        if active >= TERMINAL_LIMIT * 4 / 5 {
            tracing::warn!(
                active,
                limit = TERMINAL_LIMIT,
                "terminal admission approaches TERMINAL_LIMIT; close unused terminals"
            );
        }
        Ok(Self {
            agent,
            active: true,
        })
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for TerminalReservation<'_> {
    fn drop(&mut self) {
        if self.active {
            release_counter(&self.agent.inner().terminal_count);
        }
    }
}

impl AgentOs {
    /// The VM-scoped ownership scope used for every shell/fetch wire request.
    fn vm_ownership(&self) -> wire::OwnershipScope {
        wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: self.connection_id().to_string(),
            session_id: self.wire_session_id().to_string(),
            vm_id: self.vm_id().to_string(),
        })
    }

    pub(crate) fn finish_terminal(&self, process_id: &str) {
        if self.inner().terminals.remove(process_id).is_some() {
            release_counter(&self.inner().terminal_count);
        }
    }

    async fn start_terminal(
        &self,
        execute: wire::ExecuteRequest,
        ownership: wire::OwnershipScope,
        pid_tx: tokio::sync::oneshot::Sender<std::result::Result<u32, ClientError>>,
        process_id: &str,
    ) -> Option<u32> {
        {
            let _terminal_lifecycle_guard = self.inner().terminal_lifecycle_lock.lock().await;
            if self.inner().disposed.load(Ordering::SeqCst) {
                let error = ClientError::Sidecar(
                    "cannot connect terminal after VM shutdown has started".to_string(),
                );
                let _ = pid_tx.send(Err(error));
                self.finish_terminal(process_id);
                return None;
            }
        }

        let result = match self
            .transport()
            .request_wire(ownership, wire::RequestPayload::ExecuteRequest(execute))
            .await
        {
            Ok(wire::ResponsePayload::ProcessStartedResponse(wire::ProcessStartedResponse {
                pid,
                ..
            })) => pid.ok_or_else(|| {
                ClientError::Sidecar("connect_terminal: sidecar did not return a pid".to_string())
            }),
            Ok(wire::ResponsePayload::RejectedResponse(rejected)) => {
                Err(rejected_to_error(rejected))
            }
            Ok(other) => Err(ClientError::Sidecar(format!(
                "unexpected response to connect_terminal: {other:?}"
            ))),
            Err(error) => Err(error.into()),
        };

        match result {
            Ok(pid) => {
                let _ = pid_tx.send(Ok(pid));
                Some(pid)
            }
            Err(error) => {
                let _ = pid_tx.send(Err(error));
                self.finish_terminal(process_id);
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shell / terminal
// ---------------------------------------------------------------------------
//
// Note: `fetch` (the Network half of this reference section) is scaffolded in `net.rs`, which owns
// the `impl AgentOs { fn fetch }` block. It is intentionally NOT defined here to avoid a duplicate
// definition; the helpers below (`rejected_to_error`, `vm_ownership`) are shared by both halves.

impl AgentOs {
    /// Open a PTY-backed shell. SYNC. Returns a synthetic `shell-N` id (NOT a pid).
    ///
    /// The shell id and its registry entry are allocated synchronously (matching the TS sync
    /// contract); the actual guest-process spawn, output fan-out, and exit-task registration happen
    /// on a background task because the wire spawn is async. The exit task is tracked in the
    /// pending-shell-exit set so `dispose` can drain it (two-phase teardown).
    ///
    /// Stdout and stderr are fanned into the shell's ordered `data` broadcast (`on_shell_data`).
    /// Stderr is also fanned into a dedicated diagnostic broadcast (`on_shell_stderr` and the
    /// [`OpenShellOptions::on_stderr`] callback); terminal renderers should consume only `data`.
    pub fn open_shell(&self, mut options: OpenShellOptions) -> Result<ShellHandle> {
        let inner = self.inner();
        if inner.disposed.load(Ordering::SeqCst) {
            return Err(ClientError::Sidecar(
                "cannot open terminal after VM shutdown has started".into(),
            )
            .into());
        }
        let mut reservation = TerminalReservation::new(self)?;
        let counter = inner.shell_counter.fetch_add(1, Ordering::SeqCst) + 1;
        let shell_id = format!("shell-{counter}");
        // The wire-side process id used by write_shell/close_shell and event routing.
        let process_id = format!("shell-{}", Uuid::new_v4());

        let (data_tx, _) = tokio::sync::broadcast::channel(SHELL_DATA_CHANNEL_CAPACITY);
        let (stderr_tx, _) = tokio::sync::broadcast::channel(SHELL_DATA_CHANNEL_CAPACITY);
        let (event_tx, _) = tokio::sync::broadcast::channel(SHELL_DATA_CHANNEL_CAPACITY);
        // Spawn-readiness gate: write/close await this before issuing their wire request.
        let (spawned_tx, _) = tokio::sync::watch::channel(None);
        // Exit-code channel backing `wait_shell`.
        let (exit_tx, _) = tokio::sync::watch::channel(None);
        // Register the entry up front so write/resize/close can address it immediately, exactly like
        // the TS map insert before the handle's async work settles.
        let entry = ShellEntry {
            pid: 0,
            data_tx: data_tx.clone(),
            stderr_tx: stderr_tx.clone(),
            event_tx: event_tx.clone(),
            process_id: process_id.clone(),
            spawned_tx: spawned_tx.clone(),
            exit_tx: exit_tx.clone(),
        };
        // `insert` fails only if the key already exists; the monotonic counter guarantees it cannot.
        let _ = inner.shells.insert(shell_id.clone(), entry);

        let command = options
            .command
            .clone()
            .unwrap_or_else(|| DEFAULT_SHELL_COMMAND.to_string());
        options
            .env
            .insert(String::from("AGENTOS_EXEC_TTY"), String::from("1"));
        // Seed the PTY winsize env exactly like the TS openShell (COLUMNS/LINES).
        if let Some(cols) = options.cols {
            options
                .env
                .insert(String::from("COLUMNS"), cols.to_string());
        }
        if let Some(rows) = options.rows {
            options.env.insert(String::from("LINES"), rows.to_string());
        }
        let execute = wire::ExecuteRequest {
            process_id: process_id.clone(),
            command: Some(command),
            runtime: None,
            entrypoint: None,
            args: options.args.clone(),
            env: options.env.clone().into_iter().collect(),
            cwd: options.cwd.clone(),
            wasm_permission_tier: None,
            retain_output: true,
            wasm_backend: options.wasm_backend.map(Into::into),
        };

        // Background: subscribe to events first (so no output is missed), issue the spawn, fan
        // stdout into the data broadcast and stderr into the stderr broadcast, and complete when the
        // process exits.
        let agent = self.clone();
        let ownership = self.vm_ownership();
        let route_process_id = process_id.clone();
        let exit_shell_id = shell_id.clone();
        let exit_key = counter;
        let (start_tx, start_rx) = tokio::sync::oneshot::channel();
        let task_reservation = ShellReservation(self.clone());
        reservation.disarm();
        let handle = tokio::spawn(async move {
            let _reservation = task_reservation;
            if start_rx.await.is_err() {
                return;
            }
            let mut events = agent.transport().subscribe_wire_events();

            let response = match agent
                .transport()
                .request_wire(
                    ownership.clone(),
                    wire::RequestPayload::ExecuteRequest(execute),
                )
                .await
            {
                Ok(response) => shell_started(response),
                Err(error) => Err(error.into()),
            };
            let kernel_pid = match response {
                Ok(pid) => pid,
                Err(error) => {
                    tracing::warn!(?error, shell_id = %exit_shell_id, "open_shell spawn failed");
                    agent.retain_shell_outcome(
                        &exit_shell_id,
                        &route_process_id,
                        0,
                        Err(error.clone()),
                    );
                    spawned_tx.send_replace(Some(Err(error.clone())));
                    publish_shell_outcome(&exit_tx, Err(error));
                    agent.inner().shells.remove(&exit_shell_id);
                    agent.inner().pending_shell_exits.remove(&exit_key);
                    return;
                }
            };

            // Record the real kernel pid on the entry (TS `ShellHandle.pid`) and release the write
            // gate so any queued `write_shell`/`close_shell` proceed against the live spawn.
            agent
                .inner()
                .shells
                .update(&exit_shell_id, |_, existing| existing.pid = kernel_pid);
            // send_replace, not send: `watch::Sender::send` REFUSES to store the
            // value while no receiver exists (and the initial receiver is dropped
            // at channel creation), which left the spawn gate permanently false
            // for any write/resize issued after this point — they hung forever in
            // wait_for_spawn. send_replace stores unconditionally.
            spawned_tx.send_replace(Some(Ok(())));

            while let Some(event) = next_shell_event(&mut events, &ownership, &exit_tx).await {
                let payload = match event {
                    Ok(value) => value,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, shell_id = %exit_shell_id, "terminal live events lost; recover output with terminal.output.read");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        let error = ClientError::TerminationFailed {
                            process_id: route_process_id.clone(),
                            reason: "terminal event stream closed before a confirmed exit".into(),
                        };
                        tracing::warn!(?error, "terminal exit unconfirmed");
                        agent.retain_shell_outcome(
                            &exit_shell_id,
                            &route_process_id,
                            kernel_pid,
                            Err(error.clone()),
                        );
                        publish_shell_outcome(&exit_tx, Err(error));
                        break;
                    }
                };
                match payload {
                    EventPayload::ProcessOutputEvent(output) => {
                        if output.process_id != route_process_id {
                            continue;
                        }
                        let stream = match output.channel {
                            StreamChannel::Stdout => ProcessStream::Stdout,
                            StreamChannel::Stderr => ProcessStream::Stderr,
                        };
                        let (Some(sequence), Some(timestamp_ms)) =
                            (output.sequence, output.timestamp_ms)
                        else {
                            tracing::error!(
                                shell_id = %exit_shell_id,
                                "retained terminal output event is missing its replay identity"
                            );
                            continue;
                        };
                        let _ = event_tx.send(TerminalOutputEvent {
                            sequence,
                            stream,
                            data: output.chunk.clone(),
                            timestamp_ms: timestamp_ms.min(i64::MAX as u64) as i64,
                        });
                        // Publish every PTY chunk from this single wire-event consumer so terminal
                        // control sequences retain their original stdout/stderr order.
                        let _ = data_tx.send(output.chunk.clone());
                        if output.channel == StreamChannel::Stderr {
                            // Channel identity remains available as an optional diagnostic tap.
                            let _ = stderr_tx.send(output.chunk);
                        }
                    }
                    EventPayload::ProcessExitedEvent(exited) => {
                        if exited.process_id == route_process_id {
                            // Record the exit code for `wait_shell`: live waiters observe the watch
                            // update; late waiters (after the entry is dropped below) find it in the
                            // bounded retention map, mirroring the TS closed-shell retention.
                            agent.retain_shell_outcome(
                                &exit_shell_id,
                                &route_process_id,
                                kernel_pid,
                                Ok(exited.exit_code),
                            );
                            publish_shell_outcome(&exit_tx, Ok(exited.exit_code));
                            break;
                        }
                    }
                    EventPayload::VmLifecycleEvent(_)
                    | EventPayload::ExecutionOutputEvent(_)
                    | EventPayload::ExecutionCompletedEvent(_)
                    | EventPayload::StructuredEvent(_)
                    | EventPayload::ExtEnvelope(_) => {}
                }
            }

            // The `.finally` equivalent: remove from both the tracking set and the shells map (only
            // if it is still our entry, matching the TS identity check).
            agent.inner().pending_shell_exits.remove(&exit_key);
            agent.inner().shells.remove_if(&exit_shell_id, |existing| {
                existing.process_id == route_process_id
            });
            // remove_if takes `&mut V`; the comparison only reads, which is fine.
        });

        let _ = inner.pending_shell_exits.insert(counter, handle);
        if start_tx.send(()).is_err() {
            tracing::warn!(%shell_id, "terminal task closed before launch");
        }

        Ok(ShellHandle { shell_id })
    }

    fn retain_shell_outcome(
        &self,
        shell_id: &str,
        process_id: &str,
        pid: u32,
        result: std::result::Result<i32, ClientError>,
    ) {
        let mut retained = self.inner().closed_shells.lock();
        retain_shell_result(
            &mut retained,
            ClosedShellEntry {
                shell_id: shell_id.to_owned(),
                process_id: process_id.to_owned(),
                pid,
                result,
            },
        );
    }

    fn retained_shell_entry(&self, shell_id: &str) -> Option<ClosedShellEntry> {
        self.inner()
            .closed_shells
            .lock()
            .iter()
            .rev()
            .find(|entry| entry.shell_id == shell_id)
            .cloned()
    }

    fn retained_shell_outcome(&self, shell_id: &str) -> std::result::Result<i32, ClientError> {
        self.retained_shell_entry(shell_id)
            .map(|entry| entry.result)
            .unwrap_or_else(|| Err(ClientError::ShellNotFound(shell_id.to_owned())))
    }

    /// Connect a terminal bound to host stdio. Returns a PID. NOT tracked in the shells map; cannot
    /// be addressed by other shell methods. Killed during dispose via the terminal registry.
    ///
    /// Mirrors the TS `connectTerminal`, which routes its `onData`/`onStderr` callbacks through
    /// `openShell`. The Rust port opens a shell, wires the caller's `on_data` to ordered terminal data
    /// and `on_stderr` to the optional diagnostic tap, then returns the shell's pid. Host
    /// stdin binding, terminal raw-mode, and SIGWINCH/resize forwarding are host-process concerns
    /// that have no native wire op and are intentionally not bound here.
    pub async fn connect_terminal(&self, options: ConnectTerminalOptions) -> Result<u32> {
        let ConnectTerminalOptions {
            base,
            on_data,
            on_stderr,
        } = options;

        let process_id = format!("terminal-{}", Uuid::new_v4());
        let command = base
            .command
            .clone()
            .unwrap_or_else(|| DEFAULT_SHELL_COMMAND.to_string());
        let (data_tx, _) = tokio::sync::broadcast::channel::<Vec<u8>>(SHELL_DATA_CHANNEL_CAPACITY);
        let (stderr_tx, _) =
            tokio::sync::broadcast::channel::<Vec<u8>>(SHELL_DATA_CHANNEL_CAPACITY);

        // onData defaults to host stdout in TS; the Rust port has no host process stdout to bind to,
        // so it only fans out when a sink is supplied. onStderr is diagnostic and independent.
        if let Some(cb) = on_data {
            install_output_callback(data_tx.clone(), cb);
        }
        if let Some(cb) = on_stderr {
            install_output_callback(stderr_tx.clone(), cb);
        }

        let execute = wire::ExecuteRequest {
            process_id: process_id.clone(),
            command: Some(command),
            runtime: None,
            entrypoint: None,
            args: base.args.clone(),
            env: base.env.clone().into_iter().collect(),
            cwd: base.cwd.clone(),
            wasm_permission_tier: None,
            retain_output: true,
            wasm_backend: base.wasm_backend.map(Into::into),
        };

        // Subscribe before issuing the spawn so no output is missed.
        let events = self.transport().subscribe_wire_events();
        let ownership = self.vm_ownership();
        let (pid_tx, pid_rx) = tokio::sync::oneshot::channel();
        let (start_tx, start_rx) = tokio::sync::oneshot::channel::<()>();
        let agent = self.clone();
        let route_process_id = process_id.clone();
        let exit_task = tokio::spawn(async move {
            if start_rx.await.is_err() {
                return;
            }
            let terminal_pid = match agent
                .start_terminal(execute, ownership, pid_tx, &route_process_id)
                .await
            {
                Some(pid) => pid,
                None => return,
            };
            let mut events = events;
            loop {
                let (_scope, payload) = match events.recv().await {
                    Ok(value) => value,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if terminal_process_finished(&agent, terminal_pid).await {
                            break;
                        }
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                match payload {
                    EventPayload::ProcessOutputEvent(output) => {
                        if output.process_id != route_process_id {
                            continue;
                        }
                        let _ = data_tx.send(output.chunk.clone());
                        if output.channel == StreamChannel::Stderr {
                            let _ = stderr_tx.send(output.chunk);
                        }
                    }
                    EventPayload::ProcessExitedEvent(exited) => {
                        if exited.process_id == route_process_id {
                            break;
                        }
                    }
                    EventPayload::VmLifecycleEvent(_)
                    | EventPayload::ExecutionOutputEvent(_)
                    | EventPayload::ExecutionCompletedEvent(_)
                    | EventPayload::StructuredEvent(_)
                    | EventPayload::ExtEnvelope(_) => {}
                }
            }
            agent.finish_terminal(&route_process_id);
        });

        {
            let _terminal_lifecycle_guard = self.inner().terminal_lifecycle_lock.lock().await;
            if self.inner().disposed.load(Ordering::SeqCst) {
                exit_task.abort();
                return Err(ClientError::Sidecar(
                    "cannot connect terminal after VM shutdown has started".to_string(),
                )
                .into());
            }
            let mut terminal_reservation = TerminalReservation::new(self)?;
            match self
                .inner()
                .terminals
                .insert(process_id.clone(), TerminalEntry { exit_task })
            {
                Ok(()) => {}
                Err((_, entry)) => {
                    entry.exit_task.abort();
                    return Err(ClientError::Sidecar(format!(
                        "terminal process id collision while tracking terminal: {process_id}"
                    ))
                    .into());
                }
            }
            terminal_reservation.disarm();
            if start_tx.send(()).is_err() {
                self.finish_terminal(&process_id);
                return Err(ClientError::Sidecar(
                    "terminal startup task ended before registration completed".to_string(),
                )
                .into());
            }
        }

        pid_rx
            .await
            .map_err(|_| {
                ClientError::Sidecar(
                    "terminal startup task ended before returning a pid".to_string(),
                )
            })?
            .map_err(Into::into)
    }

    /// Write to a shell. SYNC fire-and-forget. Errors with [`ClientError::ShellNotFound`].
    pub fn write_shell(
        &self,
        shell_id: &str,
        data: StdinInput,
    ) -> std::result::Result<(), ClientError> {
        let (process_id, spawned_rx) = self.shell_wire_handle(shell_id)?;
        let chunk = stdin_chunk(data);

        // Fire-and-forget: the TS handle.write returns void; surface only the synchronous
        // ShellNotFound, and dispatch the wire write in the background after the spawn lands. TS
        // openShell is fully synchronous so the spawn is always live by the time write runs; awaiting
        // the readiness gate reproduces that ordering and avoids dropping early input.
        let agent = self.clone();
        let ownership = self.vm_ownership();
        tokio::spawn(async move {
            if let Err(error) = wait_for_spawn(spawned_rx).await {
                tracing::warn!(?error, "write_shell launch failed");
                return;
            }
            let payload = wire::RequestPayload::WriteStdinRequest(wire::WriteStdinRequest {
                process_id,
                chunk,
            });
            if let Err(error) = agent.transport().request_wire(ownership, payload).await {
                tracing::warn!(?error, "write_shell failed");
            }
        });

        Ok(())
    }

    /// List actor-addressable shells currently retained by Core.
    pub fn list_shells(&self) -> Vec<TerminalInfo> {
        let mut shells = Vec::new();
        self.inner().shells.scan(|shell_id, entry| {
            let outcome = entry.exit_tx.borrow();
            let exit_code = outcome
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .copied();
            shells.push(TerminalInfo {
                shell_id: shell_id.clone(),
                pid: entry.pid,
                running: outcome.is_none(),
                exit_code,
            });
        });
        shells.sort_by(|left, right| left.shell_id.cmp(&right.shell_id));
        shells
    }

    /// Read a bounded raw-byte terminal replay. Screen rendering is a client concern.
    pub async fn snapshot_shell(
        &self,
        shell_id: &str,
        after: Option<u64>,
        max_bytes: Option<usize>,
    ) -> std::result::Result<TerminalSnapshot, ClientError> {
        self.snapshot_shell_page(shell_id, after, None, max_bytes)
            .await
    }

    /// Read a terminal replay page with the same event and byte bounds as
    /// process output replay.
    pub async fn snapshot_shell_page(
        &self,
        shell_id: &str,
        after: Option<u64>,
        max_events: Option<usize>,
        max_bytes: Option<usize>,
    ) -> std::result::Result<TerminalSnapshot, ClientError> {
        let (max_events, max_bytes) =
            crate::output_replay::wire_page_limits(max_events, max_bytes, "terminal.output.read")?;
        let live = self.inner().shells.read(shell_id, |_, entry| {
            (
                entry.pid,
                entry.process_id.clone(),
                entry.exit_tx.borrow().clone(),
            )
        });
        let (pid, process_id, retained_exit) = match live {
            Some((pid, process_id, exit)) => (pid, process_id, exit),
            None => {
                let retained = self
                    .retained_shell_entry(shell_id)
                    .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_owned()))?;
                (retained.pid, retained.process_id, Some(retained.result))
            }
        };
        let response = self
            .transport()
            .request_wire(
                self.vm_ownership(),
                wire::RequestPayload::ReadProcessOutputRequest(wire::ReadProcessOutputRequest {
                    process_id: process_id.clone(),
                    after,
                    max_events: u32::try_from(max_events).map_err(|_| {
                        ClientError::Sidecar(String::from(
                            "terminal.output.read maxEvents exceeds the wire u32 range",
                        ))
                    })?,
                    max_bytes: u32::try_from(max_bytes).map_err(|_| {
                        ClientError::Sidecar(String::from(
                            "terminal.output.read maxBytes exceeds the wire u32 range",
                        ))
                    })?,
                }),
            )
            .await?;
        let page = match response {
            wire::ResponsePayload::ProcessOutputPageResponse(page) => page,
            wire::ResponsePayload::RejectedResponse(rejected) => {
                return Err(rejected_to_error(rejected));
            }
            other => {
                return Err(ClientError::Sidecar(format!(
                    "terminal.output.read: unexpected response {other:?}"
                )));
            }
        };
        let exit_code = match page.exit_code {
            Some(exit_code) => {
                // Persist before waking live waiters so removal of the live
                // entry cannot create a gap for a late wait_shell caller.
                self.retain_shell_outcome(shell_id, &process_id, pid, Ok(exit_code));
                self.inner().shells.read(shell_id, |_, entry| {
                    if entry.process_id == process_id {
                        publish_shell_outcome(&entry.exit_tx, Ok(exit_code));
                    }
                });
                Some(exit_code)
            }
            None => retained_exit.transpose()?,
        };
        Ok(TerminalSnapshot {
            shell_id: shell_id.to_owned(),
            pid,
            events: page
                .events
                .into_iter()
                .map(|event| TerminalOutputEvent {
                    sequence: event.sequence,
                    stream: match event.channel {
                        StreamChannel::Stdout => ProcessStream::Stdout,
                        StreamChannel::Stderr => ProcessStream::Stderr,
                    },
                    data: event.chunk,
                    timestamp_ms: event.timestamp_ms.min(i64::MAX as u64) as i64,
                })
                .collect(),
            next_cursor: page.next_cursor,
            has_more: page.has_more,
            truncated: page.truncated,
            exit_code,
        })
    }

    /// Write to a shell and AWAIT the wire write. Same routing as [`Self::write_shell`], but the
    /// caller observes wire failures instead of a fire-and-forget warn — used by the actor plugin's
    /// `writeShell` action so a failed write rejects the action.
    pub async fn write_shell_awaited(
        &self,
        shell_id: &str,
        data: StdinInput,
    ) -> std::result::Result<(), ClientError> {
        let (process_id, spawned_rx) = self.shell_wire_handle(shell_id)?;
        let chunk = stdin_chunk(data);
        tracing::debug!(shell_id, "write_shell_awaited: waiting for spawn gate");
        wait_for_spawn(spawned_rx).await?;
        tracing::debug!(shell_id, "write_shell_awaited: issuing wire write");
        let payload =
            wire::RequestPayload::WriteStdinRequest(wire::WriteStdinRequest { process_id, chunk });
        let response = self
            .transport()
            .request_wire(self.vm_ownership(), payload)
            .await?;
        tracing::debug!(shell_id, "write_shell_awaited: wire write acked");
        match response {
            wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
            _ => Ok(()),
        }
    }

    /// Subscribe to a shell's ordered terminal data. SYNC register; multi-handler; dropping the
    /// returned stream is the unsubscribe. Carries stdout and stderr exactly once in wire order.
    /// Use [`Self::on_shell_stderr`] only as a channel-specific diagnostic tap, not as a second
    /// terminal-rendering stream. Errors with [`ClientError::ShellNotFound`].
    pub fn on_shell_data(
        &self,
        shell_id: &str,
        mut handler: impl FnMut(ShellData) + Send + 'static,
    ) -> std::result::Result<crate::stream::Subscription, ClientError> {
        let mut rx = self
            .inner()
            .shells
            .read(shell_id, |_, entry| entry.data_tx.subscribe())
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()))?;
        let shell_id = shell_id.to_string();
        let task = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(data) => handler(ShellData {
                        shell_id: shell_id.clone(),
                        data,
                    }),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Ok(crate::stream::Subscription::new(move || task.abort()))
    }

    /// Subscribe to sequenced terminal output retained by Core.
    pub fn on_shell_output(
        &self,
        shell_id: &str,
        mut handler: impl FnMut(TerminalOutputEvent) + Send + 'static,
    ) -> std::result::Result<crate::stream::Subscription, ClientError> {
        let rx = self
            .inner()
            .shells
            .read(shell_id, |_, entry| entry.event_tx.subscribe());
        let Some(mut rx) = rx else {
            // A fast exit can race actor event registration. The final output
            // is available through replay; no future hints will be emitted.
            self.retained_shell_outcome(shell_id)?;
            return Ok(crate::stream::Subscription::noop());
        };
        let task = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => handler(event),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            skipped,
                            "terminal output subscriber lagged; recover with snapshot_shell"
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Ok(crate::stream::Subscription::new(move || task.abort()))
    }

    /// Subscribe to a shell's stderr. SYNC register; multi-handler; dropping the returned stream is
    /// the unsubscribe. This is the optional diagnostic channel backing the TS `onStderr` option;
    /// stderr is also present once in ordered `on_shell_data`. Errors with
    /// [`ClientError::ShellNotFound`].
    pub fn on_shell_stderr(
        &self,
        shell_id: &str,
        mut handler: impl FnMut(ShellData) + Send + 'static,
    ) -> std::result::Result<crate::stream::Subscription, ClientError> {
        let mut rx = self
            .inner()
            .shells
            .read(shell_id, |_, entry| entry.stderr_tx.subscribe())
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()))?;
        let shell_id = shell_id.to_string();
        let task = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(data) => handler(ShellData {
                        shell_id: shell_id.clone(),
                        data,
                    }),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Ok(crate::stream::Subscription::new(move || task.abort()))
    }

    pub fn on_shell_exit(
        &self,
        shell_id: &str,
        handler: impl FnOnce(ShellExit) + Send + 'static,
    ) -> std::result::Result<crate::stream::Subscription, ClientError> {
        let rx = self
            .inner()
            .shells
            .read(shell_id, |_, entry| entry.exit_tx.subscribe());
        let Some(mut rx) = rx else {
            handler(ShellExit {
                shell_id: shell_id.to_owned(),
                exit_code: self.retained_shell_outcome(shell_id)?,
            });
            return Ok(crate::stream::Subscription::noop());
        };
        if let Some(result) = rx.borrow().clone() {
            let exit_code = result?;
            handler(ShellExit {
                shell_id: shell_id.to_string(),
                exit_code,
            });
            return Ok(crate::stream::Subscription::noop());
        }
        let shell_id = shell_id.to_string();
        let task = tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                if let Some(result) = rx.borrow().clone() {
                    match result {
                        Ok(exit_code) => handler(ShellExit {
                            shell_id,
                            exit_code,
                        }),
                        Err(error) => {
                            tracing::warn!(?error, %shell_id, "terminal subscription failed before confirmed exit")
                        }
                    }
                    return;
                }
            }
        });
        Ok(crate::stream::Subscription::new(move || task.abort()))
    }

    /// Resize a shell's PTY winsize. SYNC fire-and-forget, mirroring the TS `ShellHandle.resize`
    /// (which dispatches `resizePty` in the background after the spawn lands). Errors with
    /// [`ClientError::ShellNotFound`].
    pub fn resize_shell(
        &self,
        shell_id: &str,
        cols: u16,
        rows: u16,
    ) -> std::result::Result<(), ClientError> {
        // Existence check matches the TS `if (!entry) throw Shell not found`.
        let (process_id, spawned_rx) = self.shell_wire_handle(shell_id)?;

        let agent = self.clone();
        let ownership = self.vm_ownership();
        tokio::spawn(async move {
            if let Err(error) = wait_for_spawn(spawned_rx).await {
                tracing::warn!(?error, "resize_shell launch failed");
                return;
            }
            let payload = wire::RequestPayload::ResizePtyRequest(wire::ResizePtyRequest {
                process_id,
                cols,
                rows,
            });
            if let Err(error) = agent.transport().request_wire(ownership, payload).await {
                tracing::warn!(?error, "resize_shell failed");
            }
        });

        Ok(())
    }

    /// Resize a shell and wait for the sidecar acknowledgement.
    pub async fn resize_shell_awaited(
        &self,
        shell_id: &str,
        cols: u16,
        rows: u16,
    ) -> std::result::Result<(), ClientError> {
        let (process_id, spawned_rx) = self.shell_wire_handle(shell_id)?;
        wait_for_spawn(spawned_rx).await?;
        let response = self
            .transport()
            .request_wire(
                self.vm_ownership(),
                wire::RequestPayload::ResizePtyRequest(wire::ResizePtyRequest {
                    process_id,
                    cols,
                    rows,
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::PtyResizedResponse(_) => Ok(()),
            wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
            other => Err(ClientError::Sidecar(format!(
                "resize shell: unexpected response {other:?}"
            ))),
        }
    }

    /// Wait for a shell to exit and return its process exit code (TS `waitShell`). Resolves
    /// immediately for a shell that already exited within the bounded retention window. Errors with
    /// [`ClientError::ShellNotFound`] for an unknown id.
    pub async fn wait_shell(&self, shell_id: &str) -> std::result::Result<i32, ClientError> {
        let exit_rx = self
            .inner()
            .shells
            .read(shell_id, |_, entry| entry.exit_tx.subscribe());
        let Some(exit_rx) = exit_rx else {
            // Entry already dropped: fall back to the recorded exit code (TS retention behavior).
            return self.retained_shell_outcome(shell_id);
        };
        // Sender teardown without an outcome falls back to bounded retention.
        observe_shell_exit(exit_rx)
            .await
            .unwrap_or_else(|| self.retained_shell_outcome(shell_id))
    }

    /// Close a shell. SYNC. `kill()` + immediate map delete; the exit task is still drained by
    /// `dispose`. Errors with [`ClientError::ShellNotFound`].
    pub fn close_shell(&self, shell_id: &str) -> std::result::Result<(), ClientError> {
        let (process_id, spawned_rx) = self.shell_wire_handle(shell_id)?;

        // Immediate map delete, exactly like the TS `_shells.delete(shellId)`; the pending-exit task
        // remains tracked so `dispose` still drains it (two-phase teardown).
        self.inner().shells.remove(shell_id);

        // Fire-and-forget kill (SIGTERM) after the spawn lands so the kill addresses a live process.
        let agent = self.clone();
        let ownership = self.vm_ownership();
        tokio::spawn(async move {
            if let Err(error) = wait_for_spawn(spawned_rx).await {
                tracing::warn!(?error, "close_shell launch failed");
                return;
            }
            let payload = wire::RequestPayload::KillProcessRequest(wire::KillProcessRequest {
                process_id,
                signal: String::from("SIGTERM"),
            });
            if let Err(error) = agent.transport().request_wire(ownership, payload).await {
                tracing::warn!(?error, "close_shell kill failed");
            }
        });

        Ok(())
    }

    /// Close a shell and wait for signal delivery to be acknowledged.
    pub async fn close_shell_awaited(
        &self,
        shell_id: &str,
    ) -> std::result::Result<(), ClientError> {
        let (process_id, spawned_rx) = self.shell_wire_handle(shell_id)?;
        wait_for_spawn(spawned_rx).await?;
        let response = self
            .transport()
            .request_wire(
                self.vm_ownership(),
                wire::RequestPayload::KillProcessRequest(wire::KillProcessRequest {
                    process_id,
                    signal: String::from("SIGTERM"),
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::ProcessKilledResponse(_) => {
                self.inner().shells.remove(shell_id);
                Ok(())
            }
            wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
            other => Err(ClientError::Sidecar(format!(
                "close shell: unexpected response {other:?}"
            ))),
        }
    }

    /// Look up the wire-side `process_id` and the spawn-readiness receiver for a shell id, or
    /// [`ClientError::ShellNotFound`].
    fn shell_wire_handle(
        &self,
        shell_id: &str,
    ) -> std::result::Result<(String, ShellSpawnReceiver), ClientError> {
        self.inner()
            .shells
            .read(shell_id, |_, entry| {
                (entry.process_id.clone(), entry.spawned_tx.subscribe())
            })
            .ok_or_else(|| {
                self.retained_shell_outcome(shell_id)
                    .err()
                    .unwrap_or_else(|| ClientError::ShellNotFound(shell_id.to_owned()))
            })
    }
}

/// Wait until the shell's background `Execute` request has been acked (the readiness gate flips to
/// ready). A failed launch must not issue follow-up I/O against a nonexistent process.
async fn wait_for_spawn(
    mut spawned_rx: tokio::sync::watch::Receiver<Option<std::result::Result<(), ClientError>>>,
) -> std::result::Result<(), ClientError> {
    loop {
        if let Some(result) = spawned_rx.borrow_and_update().clone() {
            return result;
        }
        spawned_rx.changed().await.map_err(|_| {
            ClientError::Sidecar("terminal launch ended before readiness was confirmed".into())
        })?;
    }
}

async fn terminal_process_finished(agent: &AgentOs, pid: u32) -> bool {
    match agent.all_processes().await {
        Ok(processes) => match processes.into_iter().find(|process| process.pid == pid) {
            Some(process) => process.status != ProcessStatus::Running,
            None => true,
        },
        Err(error) => {
            tracing::warn!(?error, pid, "terminal process snapshot failed");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn terminal_replayed_exit_recovers_waiters_and_retention_without_live_exit() {
        for failed in [false, true] {
            let failure = ClientError::TerminationFailed {
                process_id: "terminal-p".into(),
                reason: "terminal event stream lost its exit".into(),
            };
            let (outcome, _) = watch::channel(failed.then(|| Err(failure.clone())));
            let (_event_sender, mut events) = broadcast::channel(1);
            let ownership = wire::OwnershipScope::vm("connection", "session", "vm");
            let observer = next_shell_event(&mut events, &ownership, &outcome);
            tokio::pin!(observer);
            tokio::select! {
                _ = &mut observer => panic!("there is no confirmed terminal exit"),
                _ = tokio::task::yield_now() => {}
            }
            let mut retained = VecDeque::new();
            let entry = |result| ClosedShellEntry {
                shell_id: "shell-1".into(),
                process_id: "terminal-p".into(),
                pid: 42,
                result,
            };
            if failed {
                retain_shell_result(&mut retained, entry(Err(failure.clone())));
            }
            let waiter = observe_shell_exit(outcome.subscribe());
            tokio::pin!(waiter);
            if !failed {
                tokio::select! {
                    _ = &mut waiter => panic!("wait_shell must wait for a confirmed exit"),
                    _ = tokio::task::yield_now() => {}
                }
            }
            retain_shell_result(&mut retained, entry(Ok(17)));
            publish_shell_outcome(&outcome, Ok(17));
            assert_eq!(
                tokio::time::timeout(std::time::Duration::from_secs(1), &mut waiter)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap(),
                17
            );
            assert!(
                tokio::time::timeout(std::time::Duration::from_secs(1), observer)
                    .await
                    .unwrap()
                    .is_none()
            );
            let late_waiter = outcome.subscribe();
            publish_shell_outcome(&outcome, Ok(17));
            publish_shell_outcome(&outcome, Err(failure.clone()));
            retain_shell_result(&mut retained, entry(Ok(17)));
            retain_shell_result(&mut retained, entry(Err(failure)));
            assert!(!late_waiter.has_changed().unwrap());
            assert_eq!(observe_shell_exit(late_waiter).await.unwrap().unwrap(), 17);
            assert_eq!(
                retained.len(),
                1,
                "repeat replay must not consume retention"
            );
            assert_eq!(*retained[0].result.as_ref().unwrap(), 17);
        }
    }

    #[tokio::test]
    async fn terminal_launch_rejection_is_typed_and_does_not_open_the_io_gate() {
        let error = shell_started(wire::ResponsePayload::RejectedResponse(
            wire::RejectedResponse {
                code: "EACCES".into(),
                message: "execution denied".into(),
                limit_name: None,
                configured_limit: None,
                current_usage: None,
                requested: None,
                unit: None,
                scope: None,
                vm_id: None,
                session_generation: None,
                capability_id: None,
                operation: None,
                configuration_path: None,
                retryable: None,
                errno: None,
            },
        ))
        .unwrap_err();
        assert!(matches!(&error, ClientError::Kernel { code, .. } if code == "EACCES"));
        let (sender, receiver) = tokio::sync::watch::channel(None);
        sender.send_replace(Some(Err(error)));
        assert!(
            matches!(wait_for_spawn(receiver).await, Err(ClientError::Kernel { code, .. }) if code == "EACCES")
        );
        let (sender, receiver) = tokio::sync::watch::channel(None);
        drop(sender);
        assert!(wait_for_spawn(receiver).await.is_err());
    }

    use super::*;

    #[test]
    fn reserve_counter_enforces_limit_and_release_reopens_slot() {
        let counter = AtomicUsize::new(0);

        assert!(try_reserve_counter(&counter, 2));
        assert!(try_reserve_counter(&counter, 2));
        assert!(!try_reserve_counter(&counter, 2));
        release_counter(&counter);
        assert!(try_reserve_counter(&counter, 2));
    }
}
