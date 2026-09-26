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

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::Result;
use uuid::Uuid;

use agentos_sidecar_client::wire::{self, EventPayload, StreamChannel};

use crate::agent_os::{AgentOs, ShellEntry, TerminalEntry};
use crate::error::ClientError;
use crate::output_replay::{page_limits, require_page_progress, OutputReplayBuffer};
use crate::process::{
    install_output_callback, OutputCallback, ProcessStatus, ProcessStream, StdinInput,
};

/// Channel capacity for a shell's ordered terminal-data and diagnostic-stderr broadcasts.
const SHELL_DATA_CHANNEL_CAPACITY: usize = 1024;

/// Maximum active or spawning terminals created by `connect_terminal` per VM.
const TERMINAL_LIMIT: usize = 1024;

/// Default shell command used when [`OpenShellOptions::command`] is omitted (matches the kernel's
/// PTY-backed `sh`).
const DEFAULT_SHELL_COMMAND: &str = "sh";

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Callback-free options for portable `open_shell`.
#[derive(Default)]
pub struct OpenShellOptions {
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    /// Engine affinity inherited by standalone WASM commands launched by the shell.
    pub wasm_backend: Option<crate::process::StandaloneWasmBackend>,
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
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map a [`RejectedResponse`] into a [`ClientError::Kernel`] so the errno `code` survives.
fn rejected_to_error(rejected: wire::RejectedResponse) -> ClientError {
    ClientError::Kernel {
        code: rejected.code,
        message: rejected.message,
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
            return Err(ClientError::Sidecar(format!(
                "terminal limit exceeded: at most {TERMINAL_LIMIT} terminals can be active per VM"
            )));
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
        let counter = inner.shell_counter.fetch_add(1, Ordering::SeqCst) + 1;
        let shell_id = format!("shell-{counter}");
        // The wire-side process id used by write_shell/close_shell and event routing.
        let process_id = format!("shell-{}", Uuid::new_v4());

        let (data_tx, _) = tokio::sync::broadcast::channel(SHELL_DATA_CHANNEL_CAPACITY);
        let (stderr_tx, _) = tokio::sync::broadcast::channel(SHELL_DATA_CHANNEL_CAPACITY);
        let (event_tx, _) = tokio::sync::broadcast::channel(SHELL_DATA_CHANNEL_CAPACITY);
        // Spawn-readiness gate: write/close await this before issuing their wire request.
        let (spawned_tx, _) = tokio::sync::watch::channel(false);
        // Exit-code channel backing `wait_shell`.
        let (exit_tx, _) = tokio::sync::watch::channel(None::<i32>);
        let replay = Arc::new(parking_lot::Mutex::new(OutputReplayBuffer::new()));

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
            replay: replay.clone(),
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
        let handle = tokio::spawn(async move {
            let mut events = agent.transport().subscribe_wire_events();

            let response = match agent
                .transport()
                .request_wire(
                    ownership.clone(),
                    wire::RequestPayload::ExecuteRequest(execute),
                )
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    tracing::warn!(?error, shell_id = %exit_shell_id, "open_shell spawn failed");
                    // Drop the dead entry so later shell calls report ShellNotFound rather than hang.
                    agent.inner().shells.remove(&exit_shell_id);
                    agent.inner().pending_shell_exits.remove(&exit_key);
                    return;
                }
            };

            // Record the real kernel pid on the entry (TS `ShellHandle.pid`) and release the write
            // gate so any queued `write_shell`/`close_shell` proceed against the live spawn.
            if let wire::ResponsePayload::ProcessStartedResponse(wire::ProcessStartedResponse {
                pid: Some(pid),
                ..
            }) = response
            {
                agent
                    .inner()
                    .shells
                    .update(&exit_shell_id, |_, existing| existing.pid = pid);
            }
            // send_replace, not send: `watch::Sender::send` REFUSES to store the
            // value while no receiver exists (and the initial receiver is dropped
            // at channel creation), which left the spawn gate permanently false
            // for any write/resize issued after this point — they hung forever in
            // wait_for_spawn. send_replace stores unconditionally.
            let _ = spawned_tx.send_replace(true);

            loop {
                let (_scope, payload) = match events.recv().await {
                    Ok(value) => value,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
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
                        let replay_event = replay.lock().push(stream, &output.chunk);
                        let _ = event_tx.send(TerminalOutputEvent {
                            sequence: replay_event.sequence,
                            stream: replay_event.stream,
                            data: replay_event.data,
                            timestamp_ms: replay_event.timestamp_ms,
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
                            {
                                let mut retained = agent.inner().closed_shell_exit_codes.lock();
                                retained.push_back((exit_shell_id.clone(), exited.exit_code));
                                while retained.len() > crate::CLOSED_SHELL_EXIT_CODE_RETENTION_LIMIT
                                {
                                    retained.pop_front();
                                }
                            }
                            let _ = exit_tx.send_replace(Some(exited.exit_code));
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

        Ok(ShellHandle { shell_id })
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
            wait_for_spawn(spawned_rx).await;
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
            let exit_code = *entry.exit_tx.borrow();
            shells.push(TerminalInfo {
                shell_id: shell_id.clone(),
                pid: entry.pid,
                running: exit_code.is_none(),
                exit_code,
            });
        });
        shells.sort_by(|left, right| left.shell_id.cmp(&right.shell_id));
        shells
    }

    /// Read a bounded raw-byte terminal replay. Screen rendering is a client concern.
    pub fn snapshot_shell(
        &self,
        shell_id: &str,
        after: Option<u64>,
        max_bytes: Option<usize>,
    ) -> std::result::Result<TerminalSnapshot, ClientError> {
        self.snapshot_shell_page(shell_id, after, None, max_bytes)
    }

    /// Read a terminal replay page with the same event and byte bounds as
    /// process output replay.
    pub fn snapshot_shell_page(
        &self,
        shell_id: &str,
        after: Option<u64>,
        max_events: Option<usize>,
        max_bytes: Option<usize>,
    ) -> std::result::Result<TerminalSnapshot, ClientError> {
        let (max_events, max_bytes) = page_limits(max_events, max_bytes, "terminal.output.read")?;
        let (pid, page) = self
            .inner()
            .shells
            .read(shell_id, |_, entry| {
                (
                    entry.pid,
                    entry.replay.lock().read(after, max_events, max_bytes),
                )
            })
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_owned()))?;
        require_page_progress(&page, "terminal.output.read", max_bytes)?;
        Ok(TerminalSnapshot {
            shell_id: shell_id.to_owned(),
            pid,
            events: page
                .events
                .into_iter()
                .map(|event| TerminalOutputEvent {
                    sequence: event.sequence,
                    stream: event.stream,
                    data: event.data,
                    timestamp_ms: event.timestamp_ms,
                })
                .collect(),
            next_cursor: page.next_cursor,
            has_more: page.has_more,
            truncated: page.truncated,
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
        wait_for_spawn(spawned_rx).await;
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
        let mut rx = self
            .inner()
            .shells
            .read(shell_id, |_, entry| entry.event_tx.subscribe())
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_owned()))?;
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
        let mut rx = self
            .inner()
            .shells
            .read(shell_id, |_, entry| entry.exit_tx.subscribe())
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()))?;
        if let Some(exit_code) = *rx.borrow() {
            handler(ShellExit {
                shell_id: shell_id.to_string(),
                exit_code,
            });
            return Ok(crate::stream::Subscription::noop());
        }
        let shell_id = shell_id.to_string();
        let task = tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                if let Some(exit_code) = *rx.borrow() {
                    handler(ShellExit {
                        shell_id,
                        exit_code,
                    });
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
            wait_for_spawn(spawned_rx).await;
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
        wait_for_spawn(spawned_rx).await;
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
        let Some(mut exit_rx) = exit_rx else {
            // Entry already dropped: fall back to the recorded exit code (TS retention behavior).
            let retained = self.inner().closed_shell_exit_codes.lock();
            return retained
                .iter()
                .rev()
                .find(|(id, _)| id == shell_id)
                .map(|(_, code)| *code)
                .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()));
        };
        loop {
            if let Some(code) = *exit_rx.borrow_and_update() {
                return Ok(code);
            }
            if exit_rx.changed().await.is_err() {
                // Sender dropped without publishing a code (spawn failure / teardown): check the
                // retention map once more before reporting the shell unknown.
                let retained = self.inner().closed_shell_exit_codes.lock();
                return retained
                    .iter()
                    .rev()
                    .find(|(id, _)| id == shell_id)
                    .map(|(_, code)| *code)
                    .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()));
            }
        }
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
            wait_for_spawn(spawned_rx).await;
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
        self.inner().shells.remove(shell_id);
        wait_for_spawn(spawned_rx).await;
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
            wire::ResponsePayload::ProcessKilledResponse(_) => Ok(()),
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
    ) -> std::result::Result<(String, tokio::sync::watch::Receiver<bool>), ClientError> {
        self.inner()
            .shells
            .read(shell_id, |_, entry| {
                (entry.process_id.clone(), entry.spawned_tx.subscribe())
            })
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()))
    }
}

/// Wait until the shell's background `Execute` request has been acked (the readiness gate flips to
/// `true`). Returns immediately if it is already ready or the sender has dropped.
async fn wait_for_spawn(mut spawned_rx: tokio::sync::watch::Receiver<bool>) {
    if *spawned_rx.borrow() {
        return;
    }
    while spawned_rx.changed().await.is_ok() {
        if *spawned_rx.borrow() {
            return;
        }
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
