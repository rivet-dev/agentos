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
//! the wire, while the public boundary keeps the synthetic `shell-N` id. Process output events for
//! that `process_id` (both stdout and stderr) are fanned into the shell's single `data` broadcast,
//! matching the PTY-style "stderr through the main onData stream" behavior of the kernel shell.

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
use crate::stream::ByteStream;
use crate::process::StdinInput;

/// Channel capacity for a shell's data broadcast.
const SHELL_DATA_CHANNEL_CAPACITY: usize = 1024;

/// Default shell command used when [`OpenShellOptions::command`] is omitted (matches the kernel's
/// PTY-backed `sh`).
const DEFAULT_SHELL_COMMAND: &str = "sh";

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Options for `open_shell`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenShellOptions {
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    // `on_stderr` becomes a subscription on the returned shell; recorded here for parity.
}

/// Options for `connect_terminal` (extends [`OpenShellOptions`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectTerminalOptions {
    pub base: OpenShellOptions,
    // `on_data` becomes a subscription; recorded here for parity.
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

/// Encode a [`StdinInput`] into the wire `chunk` string. The wire `chunk` is a UTF-8 string; raw
/// bytes are carried lossily, matching the sidecar's `String::from_utf8_lossy` framing of stdio.
fn stdin_chunk(data: StdinInput) -> String {
    match data {
        StdinInput::Text(text) => text,
        StdinInput::Bytes(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
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
    pub fn open_shell(&self, options: OpenShellOptions) -> Result<ShellHandle> {
        let inner = self.inner();
        let counter = inner.shell_counter.fetch_add(1, Ordering::SeqCst) + 1;
        let shell_id = format!("shell-{counter}");
        // The wire-side process id used by write_shell/close_shell and event routing.
        let process_id = format!("shell-{}", Uuid::new_v4());

        let (data_tx, _) = tokio::sync::broadcast::channel(SHELL_DATA_CHANNEL_CAPACITY);

        // Register the entry up front so write/resize/close can address it immediately, exactly like
        // the TS map insert before the handle's async work settles.
        let entry = ShellEntry {
            pid: 0,
            data_tx: data_tx.clone(),
            process_id: process_id.clone(),
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
        // stdout/stderr into the data broadcast, and complete when the process exits.
        let agent = self.clone();
        let ownership = self.vm_ownership();
        let route_process_id = process_id.clone();
        let exit_shell_id = shell_id.clone();
        let exit_key = counter;
        let handle = tokio::spawn(async move {
            let mut events = agent.transport().subscribe_events();

            if let Err(error) = agent
                .transport()
                .request(ownership.clone(), RequestPayload::Execute(execute))
                .await
            {
                tracing::warn!(?error, shell_id = %exit_shell_id, "open_shell spawn failed");
                // Drop the dead entry so later shell calls report ShellNotFound rather than hang.
                agent.inner().shells.remove(&exit_shell_id);
                agent.inner().pending_shell_exits.remove(&exit_key);
                return;
            }

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
                        // Both stdout and stderr fan into the single data stream (PTY semantics).
                        match output.channel {
                            StreamChannel::Stdout | StreamChannel::Stderr => {
                                let _ = data_tx.send(output.chunk.into_bytes());
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
            agent
                .inner()
                .shells
                .remove_if(&exit_shell_id, |existing| {
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
    /// The native wire protocol has no host-stdio PTY attach, so this spawns the terminal process,
    /// records its pid in `acp_terminal_pids` for dispose-time teardown, and returns the pid. Host
    /// stdin/stdout/stderr binding and raw-mode/resize wiring are deferred (see todo).
    pub async fn connect_terminal(&self, options: ConnectTerminalOptions) -> Result<u32> {
        let process_id = format!("terminal-{}", Uuid::new_v4());
        let command = options
            .base
            .command
            .clone()
            .unwrap_or_else(|| DEFAULT_SHELL_COMMAND.to_string());
        let execute = ExecuteRequest {
            process_id,
            command: Some(command),
            runtime: None,
            entrypoint: None,
            args: options.base.args.clone(),
            env: options.base.env.clone().into_iter().collect(),
            cwd: options.base.cwd.clone(),
            wasm_permission_tier: None,
        };

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
        let process_id = self.shell_process_id(shell_id)?;
        let chunk = stdin_chunk(data);

        // Fire-and-forget: the TS handle.write returns void; surface only the synchronous
        // ShellNotFound, and dispatch the wire write in the background.
        let agent = self.clone();
        let ownership = self.vm_ownership();
        tokio::spawn(async move {
            let payload = RequestPayload::WriteStdin(WriteStdinRequest { process_id, chunk });
            if let Err(error) = agent.transport().request(ownership, payload).await {
                tracing::warn!(?error, "write_shell failed");
            }
        });

        Ok(())
    }

    /// Subscribe to a shell's data. SYNC register; multi-handler; dropping the returned stream is the
    /// unsubscribe. Errors with [`ClientError::ShellNotFound`].
    pub fn on_shell_data(
        &self,
        shell_id: &str,
    ) -> std::result::Result<ByteStream, ClientError> {
        self.inner()
            .shells
            .read(shell_id, |_, entry| entry.data_tx.subscribe())
            .map(ByteStream::new)
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()))
    }

    /// Resize a shell's PTY winsize. SYNC. Errors with [`ClientError::ShellNotFound`].
    ///
    /// Validates shell existence (the load-bearing parity behavior). The native wire protocol has no
    /// winsize request, so the resize itself is currently a best-effort no-op (see todo).
    pub fn resize_shell(
        &self,
        shell_id: &str,
        cols: u16,
        rows: u16,
    ) -> std::result::Result<(), ClientError> {
        // Existence check matches the TS `if (!entry) throw Shell not found`.
        let _ = self.shell_process_id(shell_id)?;
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
        let process_id = self.shell_process_id(shell_id)?;

        // Immediate map delete, exactly like the TS `_shells.delete(shellId)`; the pending-exit task
        // remains tracked so `dispose` still drains it (two-phase teardown).
        self.inner().shells.remove(shell_id);

        // Fire-and-forget kill (SIGTERM).
        let agent = self.clone();
        let ownership = self.vm_ownership();
        tokio::spawn(async move {
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

    /// Look up the wire-side `process_id` for a shell id, or [`ClientError::ShellNotFound`].
    fn shell_process_id(&self, shell_id: &str) -> std::result::Result<String, ClientError> {
        self.inner()
            .shells
            .read(shell_id, |_, entry| entry.process_id.clone())
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()))
    }
}
