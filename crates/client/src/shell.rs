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
//! Stream routing mirrors the TS real-process spawn path exactly: the public `data` stream
//! (`on_shell_data`) carries stdout ONLY, because TS wires only the kernel handle's `onData` (fed
//! exclusively by `stdoutHandlers`) into the data handlers. stderr is delivered on a SEPARATE channel
//! (`on_shell_stderr` + the [`OpenShellOptions::on_stderr`] callback), matching TS where stderr
//! reaches the host only through `stderrHandlers` / the `onStderr` option. Fanning stderr into the
//! data stream is only correct for the synthetic-prompt PTY path, which this native real-process path
//! does not implement.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use anyhow::{Context, Result};
use uuid::Uuid;

use agent_os_sidecar::protocol::{
    EventPayload, ExecuteRequest, KillProcessRequest, OwnershipScope, ProcessStartedResponse,
    RejectedResponse, RequestPayload, ResponsePayload, StreamChannel, WriteStdinRequest,
};

use crate::agent_os::{AgentOs, ShellEntry};
use crate::error::ClientError;
use crate::process::{install_output_callback, OutputCallback, StdinInput};
use crate::stream::ByteStream;

/// Channel capacity for a shell's data / stderr broadcasts.
const SHELL_DATA_CHANNEL_CAPACITY: usize = 1024;

/// Default shell command used when [`OpenShellOptions::command`] is omitted (matches the kernel's
/// PTY-backed `sh`).
const DEFAULT_SHELL_COMMAND: &str = "sh";

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Options for `open_shell`.
///
/// `on_stderr` mirrors the TS `OpenShellOptions.onStderr` raw-byte callback: it is the dedicated
/// path stderr reaches the caller (stderr is never fanned into the data stream). It is seeded into
/// the stderr fan-out at open time, matching the TS `stderrHandlers.add(options.onStderr)` behavior.
#[derive(Default)]
pub struct OpenShellOptions {
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    pub on_stderr: Option<OutputCallback>,
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
}

/// The synthetic shell id returned by `open_shell` (`shell-N`, NOT a pid).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellHandle {
    pub shell_id: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map a [`RejectedResponse`] into a [`ClientError::Kernel`] so the errno `code` survives.
fn rejected_to_error(rejected: RejectedResponse) -> ClientError {
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

impl AgentOs {
    /// The VM-scoped ownership scope used for every shell/fetch wire request.
    fn vm_ownership(&self) -> OwnershipScope {
        OwnershipScope::vm(
            self.connection_id().to_string(),
            self.wire_session_id().to_string(),
            self.vm_id().to_string(),
        )
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
    /// Stdout is fanned into the shell's `data` broadcast (`on_shell_data`); stderr is fanned into a
    /// SEPARATE `stderr` broadcast (`on_shell_stderr` + the [`OpenShellOptions::on_stderr`] callback),
    /// matching the TS real-process routing where stderr never reaches the data stream.
    pub fn open_shell(&self, mut options: OpenShellOptions) -> Result<ShellHandle> {
        let inner = self.inner();
        let counter = inner.shell_counter.fetch_add(1, Ordering::SeqCst) + 1;
        let shell_id = format!("shell-{counter}");
        // The wire-side process id used by write_shell/close_shell and event routing.
        let process_id = format!("shell-{}", Uuid::new_v4());

        let (data_tx, _) = tokio::sync::broadcast::channel(SHELL_DATA_CHANNEL_CAPACITY);
        let (stderr_tx, _) = tokio::sync::broadcast::channel(SHELL_DATA_CHANNEL_CAPACITY);
        // Spawn-readiness gate: write/close await this before issuing their wire request.
        let (spawned_tx, _) = tokio::sync::watch::channel(false);

        // Seed any caller-provided initial stderr callback into the stderr fan-out, matching the TS
        // initial-handler-set behavior (`stderrHandlers.add(options.onStderr)`).
        if let Some(cb) = options.on_stderr.take() {
            install_output_callback(stderr_tx.clone(), cb);
        }

        // Register the entry up front so write/resize/close can address it immediately, exactly like
        // the TS map insert before the handle's async work settles.
        let entry = ShellEntry {
            pid: 0,
            data_tx: data_tx.clone(),
            stderr_tx: stderr_tx.clone(),
            process_id: process_id.clone(),
            spawned_tx: spawned_tx.clone(),
        };
        // `insert` fails only if the key already exists; the monotonic counter guarantees it cannot.
        let _ = inner.shells.insert(shell_id.clone(), entry);

        let command = options
            .command
            .clone()
            .unwrap_or_else(|| DEFAULT_SHELL_COMMAND.to_string());
        let execute = ExecuteRequest {
            process_id: process_id.clone(),
            command: Some(command),
            runtime: None,
            entrypoint: None,
            args: options.args.clone(),
            env: options.env.clone().into_iter().collect(),
            cwd: options.cwd.clone(),
            wasm_permission_tier: None,
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
            let mut events = agent.transport().subscribe_events();

            let response = match agent
                .transport()
                .request(ownership.clone(), RequestPayload::Execute(execute))
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
            if let ResponsePayload::ProcessStarted(ProcessStartedResponse { pid, .. }) = response {
                if let Some(pid) = pid {
                    agent
                        .inner()
                        .shells
                        .update(&exit_shell_id, |_, existing| existing.pid = pid);
                }
            }
            let _ = spawned_tx.send(true);

            loop {
                let (_scope, payload) = match events.recv().await {
                    Ok(value) => value,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                match payload {
                    EventPayload::ProcessOutput(output) => {
                        if output.process_id != route_process_id {
                            continue;
                        }
                        // stdout -> data stream; stderr -> separate stderr stream (TS routing).
                        match output.channel {
                            StreamChannel::Stdout => {
                                let _ = data_tx.send(output.chunk);
                            }
                            StreamChannel::Stderr => {
                                let _ = stderr_tx.send(output.chunk);
                            }
                        }
                    }
                    EventPayload::ProcessExited(exited) => {
                        if exited.process_id == route_process_id {
                            break;
                        }
                    }
                    EventPayload::VmLifecycle(_) | EventPayload::Structured(_) => {}
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
    /// be addressed by other shell methods. Killed during dispose via the ACP-terminal pid set.
    ///
    /// Mirrors the TS `connectTerminal`, which routes its `onData`/`onStderr` callbacks through
    /// `openShell`. The Rust port opens a shell, wires the caller's `on_data` to the shell's data
    /// stream and `on_stderr` to the shell's stderr stream, then returns the shell's pid. Host
    /// stdin binding, terminal raw-mode, and SIGWINCH/resize forwarding are host-process concerns
    /// that have no native wire op and are intentionally not bound here.
    pub async fn connect_terminal(&self, options: ConnectTerminalOptions) -> Result<u32> {
        let ConnectTerminalOptions { base, on_data } = options;

        let process_id = format!("terminal-{}", Uuid::new_v4());
        let command = base
            .command
            .clone()
            .unwrap_or_else(|| DEFAULT_SHELL_COMMAND.to_string());
        let (data_tx, _) = tokio::sync::broadcast::channel::<Vec<u8>>(SHELL_DATA_CHANNEL_CAPACITY);
        let (stderr_tx, _) =
            tokio::sync::broadcast::channel::<Vec<u8>>(SHELL_DATA_CHANNEL_CAPACITY);

        // Wire the caller's onData/onStderr to the terminal's streams (TS routes both through the
        // shell handle's onData/onStderr). onData defaults to host stdout in TS; the Rust port has no
        // host process stdout to bind to, so it only fans out when a sink is supplied.
        if let Some(cb) = on_data {
            install_output_callback(data_tx.clone(), cb);
        }
        if let Some(cb) = base.on_stderr {
            install_output_callback(stderr_tx.clone(), cb);
        }

        let execute = ExecuteRequest {
            process_id: process_id.clone(),
            command: Some(command),
            runtime: None,
            entrypoint: None,
            args: base.args.clone(),
            env: base.env.clone().into_iter().collect(),
            cwd: base.cwd.clone(),
            wasm_permission_tier: None,
        };

        // Subscribe before issuing the spawn so no output is missed.
        let mut events = self.transport().subscribe_events();
        let response = self
            .transport()
            .request(self.vm_ownership(), RequestPayload::Execute(execute))
            .await
            .context("connect_terminal spawn failed")?;

        let pid = match response {
            ResponsePayload::ProcessStarted(ProcessStartedResponse { pid, .. }) => {
                pid.context("connect_terminal: sidecar did not return a pid")?
            }
            ResponsePayload::Rejected(rejected) => return Err(rejected_to_error(rejected).into()),
            _ => anyhow::bail!("unexpected response to connect_terminal"),
        };

        // Fan terminal output to the caller's onData/onStderr sinks until the process exits.
        let route_process_id = process_id;
        tokio::spawn(async move {
            loop {
                let (_scope, payload) = match events.recv().await {
                    Ok(value) => value,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                match payload {
                    EventPayload::ProcessOutput(output) => {
                        if output.process_id != route_process_id {
                            continue;
                        }
                        match output.channel {
                            StreamChannel::Stdout => {
                                let _ = data_tx.send(output.chunk);
                            }
                            StreamChannel::Stderr => {
                                let _ = stderr_tx.send(output.chunk);
                            }
                        }
                    }
                    EventPayload::ProcessExited(exited) => {
                        if exited.process_id == route_process_id {
                            break;
                        }
                    }
                    EventPayload::VmLifecycle(_) | EventPayload::Structured(_) => {}
                }
            }
        });

        // NOT tracked in `_shells`; recorded for dispose-time terminal teardown only.
        let _ = self.inner().acp_terminal_pids.insert(pid);

        Ok(pid)
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
            let payload = RequestPayload::WriteStdin(WriteStdinRequest { process_id, chunk });
            if let Err(error) = agent.transport().request(ownership, payload).await {
                tracing::warn!(?error, "write_shell failed");
            }
        });

        Ok(())
    }

    /// Subscribe to a shell's stdout data. SYNC register; multi-handler; dropping the returned stream
    /// is the unsubscribe. Carries stdout ONLY (stderr is on `on_shell_stderr`). Errors with
    /// [`ClientError::ShellNotFound`].
    pub fn on_shell_data(&self, shell_id: &str) -> std::result::Result<ByteStream, ClientError> {
        self.inner()
            .shells
            .read(shell_id, |_, entry| entry.data_tx.subscribe())
            .map(ByteStream::new)
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()))
    }

    /// Subscribe to a shell's stderr. SYNC register; multi-handler; dropping the returned stream is
    /// the unsubscribe. This is the dedicated stderr channel backing the TS `onStderr` option; stderr
    /// is never fanned into `on_shell_data`. Errors with [`ClientError::ShellNotFound`].
    pub fn on_shell_stderr(&self, shell_id: &str) -> std::result::Result<ByteStream, ClientError> {
        self.inner()
            .shells
            .read(shell_id, |_, entry| entry.stderr_tx.subscribe())
            .map(ByteStream::new)
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()))
    }

    /// Resize a shell's PTY winsize. SYNC. Errors with [`ClientError::ShellNotFound`].
    ///
    /// Validates shell existence (the load-bearing parity behavior). The native wire protocol has no
    /// winsize request, so the resize itself is currently a best-effort no-op (the synthetic TS
    /// kernel path is likewise a no-op).
    pub fn resize_shell(
        &self,
        shell_id: &str,
        cols: u16,
        rows: u16,
    ) -> std::result::Result<(), ClientError> {
        // Existence check matches the TS `if (!entry) throw Shell not found`.
        let _ = self.shell_wire_handle(shell_id)?;
        tracing::warn!(
            shell_id = %shell_id,
            cols,
            rows,
            "resize_shell has no native winsize wire op; resize is a no-op"
        );
        Ok(())
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
            let payload = RequestPayload::KillProcess(KillProcessRequest {
                process_id,
                signal: String::from("SIGTERM"),
            });
            if let Err(error) = agent.transport().request(ownership, payload).await {
                tracing::warn!(?error, "close_shell kill failed");
            }
        });

        Ok(())
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
