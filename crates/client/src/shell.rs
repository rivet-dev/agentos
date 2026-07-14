//! Network (fetch) and Shell / terminal methods + supporting types.
//!
//! Thin protocol forwarding for PTY-backed shell handles.
//!
//! A shell is a guest process spawned with [`ExecuteRequest::pty`]: its `process_id` is what
//! `write_shell`/`close_shell` address on the wire, while the public boundary keeps an opaque
//! shell handle.
//!
//! Stream routing mirrors the TS real-process spawn path exactly: the public `data` stream
//! (`on_shell_data`) carries stdout ONLY, because TS wires only the kernel handle's `onData` (fed
//! exclusively by `stdoutHandlers`) into the data handlers. stderr is delivered on a SEPARATE channel
//! (`on_shell_stderr` + the [`OpenShellOptions::on_stderr`] callback), matching TS where stderr
//! reaches the host only through `stderrHandlers` / the `onStderr` option.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use agentos_sidecar_client::wire::{self, EventPayload, StreamChannel};
use agentos_sidecar_client::WireEventRecvError;
use anyhow::Result;

use crate::agent_os::{AgentOs, ShellEntry, ShellExit};
use crate::error::ClientError;
use crate::process::{install_output_callback, OutputCallback, StdinInput};
use crate::stream::{ByteStream, RoutedStreamEvent};

/// Channel capacity for a shell's data / stderr broadcasts.
const SHELL_DATA_CHANNEL_CAPACITY: usize = 1024;
const SHELL_REGISTRY_LIMIT: usize = 1024;

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

/// The sidecar-owned process id returned as an opaque shell handle (not a pid).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellHandle {
    pub shell_id: String,
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

impl AgentOs {
    /// The VM-scoped ownership scope used for every shell/fetch wire request.
    fn vm_ownership(&self) -> wire::OwnershipScope {
        wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: self.connection_id().to_string(),
            session_id: self.wire_session_id().to_string(),
            vm_id: self.vm_id().to_string(),
        })
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
    /// Open a PTY-backed shell. Returns the sidecar-owned process id as an opaque
    /// handle after the sidecar has returned the authoritative kernel pid. The
    /// exit task is tracked so `dispose` can drain it.
    ///
    /// Stdout is fanned into the shell's `data` broadcast (`on_shell_data`); stderr is fanned into a
    /// SEPARATE `stderr` broadcast (`on_shell_stderr` + the [`OpenShellOptions::on_stderr`] callback),
    /// matching the TS real-process routing where stderr never reaches the data stream.
    pub async fn open_shell(&self, mut options: OpenShellOptions) -> Result<ShellHandle> {
        let inner = self.inner();
        let mut shell_count = 0usize;
        inner.shells.scan(|_, _| shell_count += 1);
        if shell_count >= SHELL_REGISTRY_LIMIT {
            return Err(ClientError::Sidecar(format!(
                "shell registry limit exceeded: at most {SHELL_REGISTRY_LIMIT} live or failed shell routes can be tracked per VM"
            ))
            .into());
        }
        let (data_tx, _) = tokio::sync::broadcast::channel(SHELL_DATA_CHANNEL_CAPACITY);
        let (stderr_tx, _) = tokio::sync::broadcast::channel(SHELL_DATA_CHANNEL_CAPACITY);
        // Exit-code channel backing `wait_shell`.
        let (exit_tx, _) = tokio::sync::watch::channel(None::<ShellExit>);

        // Seed any caller-provided initial stderr callback into the stderr fan-out, matching the TS
        // initial-handler-set behavior (`stderrHandlers.add(options.onStderr)`).
        if let Some(cb) = options.on_stderr.take() {
            install_output_callback(stderr_tx.clone(), cb);
        }

        let execute = wire::ExecuteRequest {
            process_id: None,
            command: options.command.clone(),
            shell_command: None,
            runtime: None,
            entrypoint: None,
            args: options.args.clone(),
            env: (!options.env.is_empty()).then(|| options.env.clone().into_iter().collect()),
            cwd: options.cwd.clone(),
            wasm_permission_tier: None,
            pty: Some(wire::PtyOptions {
                cols: options.cols,
                rows: options.rows,
            }),
            keep_stdin_open: None,
            timeout_ms: None,
            capture_output: None,
        };

        let ownership = self.vm_ownership();
        let (response, events) = self
            .transport()
            .request_wire_with_process_events(
                ownership.clone(),
                wire::RequestPayload::ExecuteRequest(execute),
            )
            .await?;
        let (process_id, mut events) = match response {
            wire::ResponsePayload::ProcessStartedResponse(wire::ProcessStartedResponse {
                process_id,
                pid: Some(_),
            }) => {
                let events = events.ok_or_else(|| {
                    ClientError::Sidecar(String::from(
                        "open_shell: sidecar response did not bind a process event route",
                    ))
                })?;
                (process_id, events)
            }
            wire::ResponsePayload::ProcessStartedResponse(_) => {
                return Err(ClientError::Sidecar(
                    "open_shell: sidecar did not return a kernel pid".to_owned(),
                )
                .into());
            }
            wire::ResponsePayload::RejectedResponse(rejected) => {
                return Err(rejected_to_error(rejected).into());
            }
            other => {
                return Err(ClientError::Sidecar(format!(
                    "open_shell: unexpected Execute response {other:?}"
                ))
                .into());
            }
        };
        let shell_id = process_id.clone();

        let entry = ShellEntry {
            data_tx: data_tx.clone(),
            stderr_tx: stderr_tx.clone(),
            process_id: process_id.clone(),
            exit_tx: exit_tx.clone(),
            closing: AtomicBool::new(false),
        };
        // Recheck and insert under the process-registry lock after the asynchronous start. The
        // early check limits unnecessary starts; this atomic check is what enforces the bound when
        // many `open_shell` calls race. A shell that lost the reservation is killed immediately.
        let registered = {
            let _registry_guard = inner.process_registry_lock.lock();
            let mut shell_count = 0usize;
            inner.shells.scan(|_, _| shell_count += 1);
            if shell_count >= SHELL_REGISTRY_LIMIT {
                false
            } else {
                let _ = inner.shells.insert(shell_id.clone(), entry);
                true
            }
        };
        if !registered {
            self.abort_wire_process_after_route_failure(&process_id, "shell registry overflow")
                .await;
            return Err(ClientError::Sidecar(format!(
                "shell registry limit exceeded: at most {SHELL_REGISTRY_LIMIT} live or failed shell routes can be tracked per VM"
            ))
            .into());
        }

        // Background: fan stdout/stderr and retain the authoritative exit code.
        let agent = self.clone();
        let route_process_id = process_id.clone();
        let exit_shell_id = shell_id.clone();
        let exit_key = shell_id.clone();
        let pending_key = shell_id.clone();
        let handle = tokio::spawn(async move {
            let mut retain_route_failure = false;
            loop {
                let event = match events.recv().await {
                    Ok(event) => event,
                    Err(WireEventRecvError::Lagged { skipped }) => {
                        let _ = data_tx.send(RoutedStreamEvent::Lagged { skipped });
                        let _ = stderr_tx.send(RoutedStreamEvent::Lagged { skipped });
                        let _ = exit_tx.send(Some(ShellExit::EventStreamLagged { skipped }));
                        agent
                            .abort_wire_process_after_route_failure(&route_process_id, "shell")
                            .await;
                        retain_route_failure = true;
                        break;
                    }
                    Err(WireEventRecvError::Closed) => {
                        let _ = data_tx.send(RoutedStreamEvent::Closed {
                            context: "shell process exit",
                        });
                        let _ = stderr_tx.send(RoutedStreamEvent::Closed {
                            context: "shell process exit",
                        });
                        let _ = exit_tx.send(Some(ShellExit::EventStreamClosed));
                        break;
                    }
                };
                if event.ownership != ownership {
                    continue;
                }
                match &event.payload {
                    EventPayload::ProcessOutputEvent(output) => {
                        if output.process_id != route_process_id {
                            continue;
                        }
                        // stdout -> data stream; stderr -> separate stderr stream (TS routing).
                        match output.channel {
                            StreamChannel::Stdout => {
                                let _ = data_tx.send(RoutedStreamEvent::Data(output.chunk.clone()));
                            }
                            StreamChannel::Stderr => {
                                let _ =
                                    stderr_tx.send(RoutedStreamEvent::Data(output.chunk.clone()));
                            }
                        }
                    }
                    EventPayload::ProcessExitedEvent(exited) => {
                        if exited.process_id == route_process_id {
                            let _ = exit_tx.send(Some(ShellExit::Exited(exited.exit_code)));
                            break;
                        }
                    }
                    EventPayload::VmLifecycleEvent(_)
                    | EventPayload::CronDispatchEvent(_)
                    | EventPayload::StructuredEvent(_)
                    | EventPayload::ExtEnvelope(_) => {}
                }
            }

            // The `.finally` equivalent: remove from both the tracking set and the shells map (only
            // if it is still our entry, matching the TS identity check).
            agent.inner().pending_shell_exits.remove(&exit_key);
            if !retain_route_failure {
                agent.inner().shells.remove_if(&exit_shell_id, |existing| {
                    existing.process_id == route_process_id
                });
            }
            // remove_if takes `&mut V`; the comparison only reads, which is fine.
        });

        let _ = inner.pending_shell_exits.insert(pending_key, handle);

        Ok(ShellHandle { shell_id })
    }

    /// Write to a shell and await the sidecar response.
    pub async fn write_shell(
        &self,
        shell_id: &str,
        data: StdinInput,
    ) -> std::result::Result<(), ClientError> {
        let process_id = self.shell_wire_handle(shell_id)?;
        let chunk = stdin_chunk(data);
        let payload =
            wire::RequestPayload::WriteStdinRequest(wire::WriteStdinRequest { process_id, chunk });
        let response = self
            .transport()
            .request_wire(self.vm_ownership(), payload)
            .await?;
        match response {
            wire::ResponsePayload::StdinWrittenResponse(_) => Ok(()),
            wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
            other => Err(ClientError::Sidecar(format!(
                "write_shell: unexpected response {other:?}"
            ))),
        }
    }

    /// Subscribe to a shell's stdout data. SYNC register; multi-handler; dropping the returned stream
    /// is the unsubscribe. Carries stdout ONLY (stderr is on `on_shell_stderr`). Errors with
    /// [`ClientError::ShellNotFound`].
    pub fn on_shell_data(&self, shell_id: &str) -> std::result::Result<ByteStream, ClientError> {
        self.inner()
            .shells
            .read(shell_id, |_, entry| {
                (!entry.closing.load(Ordering::SeqCst)).then(|| {
                    byte_stream_for_shell_route(
                        entry.data_tx.subscribe(),
                        entry.exit_tx.borrow().clone(),
                    )
                })
            })
            .flatten()
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()))
    }

    /// Subscribe to a shell's stderr. SYNC register; multi-handler; dropping the returned stream is
    /// the unsubscribe. This is the dedicated stderr channel backing the TS `onStderr` option; stderr
    /// is never fanned into `on_shell_data`. Errors with [`ClientError::ShellNotFound`].
    pub fn on_shell_stderr(&self, shell_id: &str) -> std::result::Result<ByteStream, ClientError> {
        self.inner()
            .shells
            .read(shell_id, |_, entry| {
                (!entry.closing.load(Ordering::SeqCst)).then(|| {
                    byte_stream_for_shell_route(
                        entry.stderr_tx.subscribe(),
                        entry.exit_tx.borrow().clone(),
                    )
                })
            })
            .flatten()
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()))
    }

    /// Resize a shell's PTY winsize and await the sidecar response.
    pub async fn resize_shell(
        &self,
        shell_id: &str,
        cols: u16,
        rows: u16,
    ) -> std::result::Result<(), ClientError> {
        let process_id = self.shell_wire_handle(shell_id)?;
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
                "resize_shell: unexpected response {other:?}"
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
            let process = self.process_snapshot_entry_by_id(shell_id).await?;
            return process
                .filter(|process| process.status == wire::ProcessSnapshotStatus::Exited)
                .and_then(|process| process.exit_code)
                .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()));
        };
        loop {
            if let Some(exit) = exit_rx.borrow_and_update().clone() {
                return match exit {
                    ShellExit::Exited(code) => Ok(code),
                    ShellExit::EventStreamLagged { skipped } => {
                        Err(ClientError::EventStreamLagged { skipped })
                    }
                    ShellExit::EventStreamClosed => Err(ClientError::EventStreamClosed {
                        context: "shell process exit",
                    }),
                };
            }
            if exit_rx.changed().await.is_err() {
                return Err(ClientError::EventStreamClosed {
                    context: "shell process exit",
                });
            }
        }
    }

    /// Close a shell and await the sidecar signal response.
    pub async fn close_shell(&self, shell_id: &str) -> std::result::Result<(), ClientError> {
        let entry = self.inner().shells.read(shell_id, |_, entry| {
            (
                entry.process_id.clone(),
                entry.closing.load(Ordering::SeqCst),
            )
        });
        let Some((process_id, closing)) = entry else {
            let process = self.process_snapshot_entry_by_id(shell_id).await?;
            if process.is_some_and(|process| process.status == wire::ProcessSnapshotStatus::Exited)
            {
                return Ok(());
            }
            return Err(ClientError::ShellNotFound(shell_id.to_string()));
        };
        if closing {
            return Ok(());
        }
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
                let failed_route = self
                    .inner()
                    .shells
                    .read(shell_id, |_, entry| {
                        matches!(
                            entry.exit_tx.borrow().as_ref(),
                            Some(ShellExit::EventStreamLagged { .. })
                                | Some(ShellExit::EventStreamClosed)
                        )
                    })
                    .unwrap_or(false);
                if failed_route {
                    let _ = self.inner().shells.remove(shell_id);
                } else {
                    self.inner().shells.update(shell_id, |_, entry| {
                        entry.closing.store(true, Ordering::SeqCst);
                    });
                }
                Ok(())
            }
            wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
            other => Err(ClientError::Sidecar(format!(
                "close_shell: unexpected response {other:?}"
            ))),
        }
    }

    /// Look up the wire-side `process_id` for a shell id.
    fn shell_wire_handle(&self, shell_id: &str) -> std::result::Result<String, ClientError> {
        self.inner()
            .shells
            .read(shell_id, |_, entry| {
                (!entry.closing.load(Ordering::SeqCst)).then(|| entry.process_id.clone())
            })
            .flatten()
            .ok_or_else(|| ClientError::ShellNotFound(shell_id.to_string()))
    }
}

fn byte_stream_for_shell_route(
    rx: tokio::sync::broadcast::Receiver<RoutedStreamEvent<Vec<u8>>>,
    exit: Option<ShellExit>,
) -> ByteStream {
    match exit {
        Some(ShellExit::EventStreamLagged { skipped }) => {
            ByteStream::failed(RoutedStreamEvent::Lagged { skipped })
        }
        Some(ShellExit::EventStreamClosed) => ByteStream::failed(RoutedStreamEvent::Closed {
            context: "shell output",
        }),
        Some(ShellExit::Exited(_)) | None => ByteStream::new(rx),
    }
}

#[cfg(test)]
mod tests {
    use super::{byte_stream_for_shell_route, RoutedStreamEvent, ShellExit};
    use crate::error::ClientError;
    use futures::StreamExt;

    #[tokio::test]
    async fn late_shell_output_subscriber_receives_retained_route_failure() {
        let (_tx, rx) = tokio::sync::broadcast::channel::<RoutedStreamEvent<Vec<u8>>>(1);
        let mut stream =
            byte_stream_for_shell_route(rx, Some(ShellExit::EventStreamLagged { skipped: 8 }));
        assert!(matches!(
            stream.next().await,
            Some(Err(ClientError::EventStreamLagged { skipped: 8 }))
        ));
        assert!(stream.next().await.is_none());
    }
}
