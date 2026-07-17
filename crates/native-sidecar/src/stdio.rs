use crate::wire::{
    self, AuthenticatedResponse, ExtEnvelope, OwnershipScope, ProtocolCodecError, ProtocolFrame,
    RequestFrame, RequestId, RequestPayload, ResponseFrame, ResponsePayload, SessionOpenedResponse,
    SidecarResponseFrame, WireDispatchResult, WireFrameCodec,
};
use crate::{
    EventSinkTransport, Extension, ExtensionInterruptRequest, NativeSidecar, NativeSidecarConfig,
    SidecarError, SidecarRequestTransport,
};
use agentos_bridge::queue_tracker::{tracked_sync_channel, TrackedLimit, TrackedSyncSender};
use agentos_bridge::{
    BridgeTypes, ChmodRequest, ClockBridge, ClockRequest, CommandPermissionRequest,
    CreateDirRequest, CreateJavascriptContextRequest, CreateWasmContextRequest, DiagnosticRecord,
    DirectoryEntry, EnvironmentPermissionRequest, EventBridge, ExecutionBridge, ExecutionEvent,
    ExecutionHandleRequest, FileMetadata, FilesystemBridge, FilesystemPermissionRequest,
    FilesystemSnapshot, FlushFilesystemStateRequest, GuestContextHandle, KillExecutionRequest,
    LifecycleEventRecord, LoadFilesystemStateRequest, LogRecord, NetworkPermissionRequest,
    PathRequest, PermissionBridge, PermissionDecision, PersistenceBridge,
    PollExecutionEventRequest, RandomBridge, RandomBytesRequest, ReadDirRequest, ReadFileRequest,
    RenameRequest, ScheduleTimerRequest, ScheduledTimer, StartExecutionRequest, StartedExecution,
    StructuredEventRecord, SymlinkRequest, TruncateRequest, WriteExecutionStdinRequest,
    WriteFileRequest,
};
use agentos_native_sidecar_core::{
    generated_wire_blocking_extension_interrupt, BlockingExtensionInterrupt,
};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{symlink as create_symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{
    mpsc::{channel, unbounded_channel, Receiver},
    Notify,
};
use tokio::time;

// Guest sync fs/module RPCs are serviced by `pump_process_events` on this timer,
// so a blocked guest call waits up to one interval before the host even sees it.
// At 5ms this dominated per-call latency (~5ms/stat); 250us cuts it ~11x (stat
// 7.5s -> ~0.65s over 1500 ops) and the sub-ms tokio timer is honored. Idle
// pumps are cheap no-ops (try_recv + zero-timeout poll), so the higher cadence
// costs negligible CPU when no guest is issuing RPCs.
const EVENT_PUMP_INTERVAL: Duration = Duration::from_micros(250);
// Cadence of sidecar→host heartbeat frames. The host treats sustained inbound
// silence (several missed beats) as a dead or wedged sidecar and tears the
// process down, so this is a fixed protocol constant, not a tunable. Emitted
// from a dedicated thread so beats keep flowing while the dispatch loop is
// busy inside one long request.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
// Connection id stamped on heartbeat frames. Heartbeats are transport-level
// liveness — not tied to an authenticated connection — and the host consumes
// them at its frame layer without routing by ownership, so a fixed synthetic
// id is correct even before any client authenticates.
const HEARTBEAT_CONNECTION_ID: &str = "sidecar-transport";
const MAX_STDIN_FRAME_QUEUE: usize = 128;
const MAX_EVENT_READY_QUEUE: usize = 1;
// Defense-in-depth headroom for the host-bound frame queue: a burst of output
// frames from a busy turn should be buffered, so the writer only backpressures
// when the host genuinely stops reading stdout rather than on every spike.
const MAX_STDOUT_FRAME_QUEUE: usize = 4096;

#[cfg(test)]
fn request_frame(
    request_id: RequestId,
    ownership: OwnershipScope,
    payload: RequestPayload,
) -> RequestFrame {
    RequestFrame {
        schema: wire::protocol_schema(),
        request_id,
        ownership,
        payload,
    }
}

fn response_frame(
    request_id: RequestId,
    ownership: OwnershipScope,
    payload: ResponsePayload,
) -> ResponseFrame {
    ResponseFrame {
        schema: wire::protocol_schema(),
        request_id,
        ownership,
        payload,
    }
}

#[cfg(test)]
fn connection_ownership(connection_id: &str) -> OwnershipScope {
    OwnershipScope::ConnectionOwnership(wire::ConnectionOwnership {
        connection_id: connection_id.to_owned(),
    })
}

fn session_ownership(connection_id: &str, session_id: &str) -> OwnershipScope {
    OwnershipScope::SessionOwnership(wire::SessionOwnership {
        connection_id: connection_id.to_owned(),
        session_id: session_id.to_owned(),
    })
}

#[cfg(test)]
fn vm_ownership(connection_id: &str, session_id: &str, vm_id: &str) -> OwnershipScope {
    OwnershipScope::VmOwnership(wire::VmOwnership {
        connection_id: connection_id.to_owned(),
        session_id: session_id.to_owned(),
        vm_id: vm_id.to_owned(),
    })
}

fn wire_protocol_error(error: ProtocolCodecError) -> SidecarError {
    SidecarError::InvalidState(format!("invalid generated wire protocol frame: {error}"))
}

pub fn run() -> Result<(), Box<dyn Error>> {
    run_with_extensions(Vec::new())
}

pub fn run_with_extensions(extensions: Vec<Box<dyn Extension>>) -> Result<(), Box<dyn Error>> {
    // Initialize the embedded V8 runtime + platform now, on the long-lived main
    // thread, so it is never first-initialized on a transient worker thread (e.g. a
    // VM-create snapshot pre-warm thread that then exits — which corrupts V8's
    // platform and wedges later isolate creation). Best-effort.
    if let Err(error) = agentos_execution::v8_host::ensure_runtime_initialized() {
        eprintln!("embedded V8 runtime init failed at startup: {error}");
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run_async(extensions))
}

async fn run_async(extensions: Vec<Box<dyn Extension>>) -> Result<(), Box<dyn Error>> {
    let config = NativeSidecarConfig {
        compile_cache_root: Some(default_compile_cache_root()),
        ..NativeSidecarConfig::default()
    };
    let codec = WireFrameCodec::new(config.max_frame_bytes);
    let mut sidecar =
        NativeSidecar::with_config_and_extensions(LocalBridge::default(), config, extensions)?;
    let mut active_sessions = BTreeSet::<SessionScope>::new();
    let mut active_connections = BTreeSet::<String>::new();
    let (stdin_tx, mut stdin_rx) =
        channel::<Result<Option<ProtocolFrame>, String>>(MAX_STDIN_FRAME_QUEUE);
    let stdin_gauge = agentos_bridge::queue_tracker::register_queue(
        TrackedLimit::SidecarStdinFrames,
        MAX_STDIN_FRAME_QUEUE,
    );
    let (event_ready_tx, mut event_ready_rx) = channel::<()>(MAX_EVENT_READY_QUEUE);
    let (write_tx, write_rx) = tracked_sync_channel::<ProtocolFrame>(
        TrackedLimit::SidecarStdoutFrames,
        MAX_STDOUT_FRAME_QUEUE,
    );
    let (write_error_tx, mut write_error_rx) = unbounded_channel::<String>();

    // Forward limit-registry near-capacity warnings to the host: the global sink
    // fires (edge-triggered, from arbitrary threads) into this channel, and the
    // event loop below drains it and emits a `StructuredEvent` (name
    // "limit_warning"). The unbounded sender is Send+Sync and lives for the whole
    // process inside the global handler, so the receiver never sees a hangup.
    let (limit_warning_tx, mut limit_warning_rx) =
        unbounded_channel::<agentos_bridge::queue_tracker::LimitWarning>();
    agentos_bridge::queue_tracker::set_limit_warning_handler(Box::new(move |warning| {
        let _ = limit_warning_tx.send(warning.clone());
    }));
    let callback_transport = Arc::new(FrameSidecarRequestTransport::new(write_tx.clone()));
    sidecar.set_sidecar_request_transport(callback_transport.clone());
    // Live event sink: lets an extension stream `session/update` (and other)
    // events to stdout mid-dispatch instead of batching them until the request
    // resolves. Shares the same outbound `write_tx` channel as the batch path, so
    // ordering and backpressure are identical.
    let event_transport = Arc::new(FrameEventTransport::new(write_tx.clone()));
    sidecar.set_event_transport(event_transport);
    let mut event_pump = time::interval(EVENT_PUMP_INTERVAL);
    let process_event_notify = Arc::new(Notify::new());
    sidecar
        .javascript_engine
        .set_event_notify(Some(process_event_notify.clone()));
    let writer_codec = codec.clone();
    let reader_codec = codec.clone();
    let writer_error_tx = write_error_tx.clone();
    thread::spawn(move || {
        let mut writer = io::BufWriter::new(io::stdout());
        while let Ok(frame) = write_rx.recv() {
            if let Err(error) = write_frame(&writer_codec, &mut writer, &frame) {
                let _ = writer_error_tx.send(error.to_string());
                break;
            }
        }
    });
    spawn_heartbeat_thread(write_tx.clone(), HEARTBEAT_INTERVAL);

    thread::spawn({
        let callback_transport = callback_transport.clone();
        move || {
            let mut stdin = io::stdin();
            loop {
                let frame = match read_frame(&reader_codec, &mut stdin) {
                    Ok(Some(ProtocolFrame::SidecarResponseFrame(response))) => {
                        if callback_transport.accept_response(response.clone()) {
                            continue;
                        }
                        Ok(Some(ProtocolFrame::SidecarResponseFrame(response)))
                    }
                    Ok(Some(frame)) => Ok(Some(frame)),
                    other => other,
                }
                .map_err(|error: Box<dyn Error>| error.to_string());
                let should_stop = matches!(frame, Ok(None) | Err(_));
                match enqueue_stdin_frame(&stdin_tx, frame) {
                    Ok(()) => {
                        // Sample inbound queue depth so the centralized tracker
                        // can warn before host requests back up on the sidecar.
                        stdin_gauge.observe_depth(
                            stdin_tx.max_capacity().saturating_sub(stdin_tx.capacity()),
                        );
                    }
                    Err(StdinFrameQueueError::Closed) => break,
                }
                if should_stop {
                    break;
                }
            }
        }
    });

    flush_sidecar_requests(&mut sidecar, &write_tx)?;
    let mut pending_frame: Option<ProtocolFrame> = None;
    let mut limit_warning_closed = false;

    loop {
        if let Some(frame) = pending_frame.take() {
            handle_protocol_frame(
                frame,
                &mut sidecar,
                &mut stdin_rx,
                &mut pending_frame,
                &write_tx,
                &mut active_sessions,
                &mut active_connections,
            )
            .await?;
            continue;
        }

        tokio::select! {
            maybe_frame = stdin_rx.recv() => {
                let Some(frame) = maybe_frame else {
                    break;
                };
                let Some(frame) = frame.map_err(io::Error::other)? else {
                    break;
                };
                handle_protocol_frame(
                    frame,
                    &mut sidecar,
                    &mut stdin_rx,
                    &mut pending_frame,
                    &write_tx,
                    &mut active_sessions,
                    &mut active_connections,
                ).await?;
            }
            maybe_warning = limit_warning_rx.recv(), if !limit_warning_closed => {
                match maybe_warning {
                    Some(warning) => {
                        // A limit warning is process-global; deliver it ONCE. The
                        // stdio transport is single-client, so emit it to the first
                        // active connection (if any) rather than fanning out a copy
                        // per connection. Dropped if no client has authenticated yet
                        // (only the tracing log survives, which is acceptable).
                        if let Some(connection_id) = active_connections.iter().next() {
                            let mut detail = std::collections::HashMap::new();
                            detail.insert(String::from("limit"), warning.name.as_str().to_string());
                            detail.insert(
                                String::from("category"),
                                warning.category.as_str().to_string(),
                            );
                            detail.insert(String::from("observed"), warning.observed.to_string());
                            detail.insert(String::from("capacity"), warning.capacity.to_string());
                            detail.insert(
                                String::from("fillPercent"),
                                warning.fill_percent.to_string(),
                            );
                            let frame = crate::service::structured_event_frame(
                                connection_id,
                                "limit_warning",
                                detail,
                            )?;
                            send_output_frame(&write_tx, ProtocolFrame::EventFrame(frame))?;
                        }
                    }
                    None => {
                        // Sender dropped (only possible if another sidecar replaced
                        // the global handler in-process). Disarm this branch so the
                        // select! does not hot-spin on an always-ready closed
                        // receiver; do NOT break — that would tear down the sidecar.
                        limit_warning_closed = true;
                    }
                }
            }
            maybe_ready = event_ready_rx.recv() => {
                let Some(()) = maybe_ready else {
                    break;
                };
                loop {
                    let mut emitted_frame = false;
                    for session in active_sessions.iter().cloned().collect::<Vec<_>>() {
                        if let Some(frame) = sidecar
                            .poll_event_wire(&session.ownership_scope(), Duration::ZERO)
                            .await?
                        {
                            send_output_frame(&write_tx, ProtocolFrame::EventFrame(frame))?;
                            emitted_frame = true;
                        }
                    }

                    if !emitted_frame {
                        break;
                    }
                }
                flush_sidecar_requests(&mut sidecar, &write_tx)?;
            }
            _ = process_event_notify.notified() => {
                for session in active_sessions.iter().cloned().collect::<Vec<_>>() {
                    if sidecar.pump_process_events(&session.compat_ownership_scope()).await? {
                        let _ = event_ready_tx.try_send(());
                    }
                }
                flush_sidecar_requests(&mut sidecar, &write_tx)?;
            }
            _ = event_pump.tick() => {
                for session in active_sessions.iter().cloned().collect::<Vec<_>>() {
                    if sidecar.pump_process_events(&session.compat_ownership_scope()).await? {
                        let _ = event_ready_tx.try_send(());
                    }
                }
                for frame in sidecar.take_due_fs_change_event_frames(std::time::Instant::now())? {
                    send_output_frame(&write_tx, ProtocolFrame::EventFrame(frame))?;
                }
                flush_sidecar_requests(&mut sidecar, &write_tx)?;
            }
            maybe_write_error = write_error_rx.recv() => {
                if let Some(error) = maybe_write_error {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, error).into());
                }
            }
        }
    }

    cleanup_connections(&mut sidecar, &active_connections, &mut active_sessions).await;
    Ok(())
}

async fn handle_protocol_frame(
    frame: ProtocolFrame,
    sidecar: &mut NativeSidecar<LocalBridge>,
    stdin_rx: &mut Receiver<Result<Option<ProtocolFrame>, String>>,
    pending_frame: &mut Option<ProtocolFrame>,
    write_tx: &TrackedSyncSender<ProtocolFrame>,
    active_sessions: &mut BTreeSet<SessionScope>,
    active_connections: &mut BTreeSet<String>,
) -> Result<(), Box<dyn Error>> {
    match frame {
        ProtocolFrame::RequestFrame(request) => {
            let (dispatch, extra_responses) =
                dispatch_with_prompt_interrupt(sidecar, request.clone(), stdin_rx, pending_frame)
                    .await?;
            track_session_state(
                &dispatch.response.payload,
                active_sessions,
                active_connections,
            );

            send_output_frame(write_tx, ProtocolFrame::ResponseFrame(dispatch.response))?;
            for response in extra_responses {
                send_output_frame(write_tx, ProtocolFrame::ResponseFrame(response))?;
            }
            for event in dispatch.events {
                send_output_frame(write_tx, ProtocolFrame::EventFrame(event))?;
            }
            flush_sidecar_requests(sidecar, write_tx)?;
        }
        ProtocolFrame::SidecarResponseFrame(response) => {
            sidecar.accept_wire_sidecar_response(response)?;
            flush_sidecar_requests(sidecar, write_tx)?;
        }
        other => {
            return Err(format!(
                "expected request or sidecar_response frame on stdin, received {}",
                frame_kind(&other)
            )
            .into());
        }
    }
    // Drop any sessions the sidecar disposed while handling this frame from the
    // active-session set so the event pump stops iterating dead sessions (M5).
    untrack_disposed_sessions(&sidecar.take_disposed_sessions(), active_sessions);
    Ok(())
}

/// Remove every disposed session scope from the stdio transport's active-session
/// set. Without this the set is insert-only (`track_session_state` adds on
/// `SessionOpenedResponse` but nothing ever removed), so it grew per session for
/// the process lifetime and the ~250us event pump iterated every dead entry (M5).
fn untrack_disposed_sessions(
    disposed: &[(String, String)],
    active_sessions: &mut BTreeSet<SessionScope>,
) {
    for (connection_id, session_id) in disposed {
        active_sessions.remove(&SessionScope {
            connection_id: connection_id.clone(),
            session_id: session_id.clone(),
        });
    }
}

async fn dispatch_with_prompt_interrupt(
    sidecar: &mut NativeSidecar<LocalBridge>,
    request: RequestFrame,
    stdin_rx: &mut Receiver<Result<Option<ProtocolFrame>, String>>,
    pending_frame: &mut Option<ProtocolFrame>,
) -> Result<(WireDispatchResult, Vec<ResponseFrame>), Box<dyn Error>> {
    let Some(blocking_request) = blocking_extension_request(sidecar, &request) else {
        return Ok((sidecar.dispatch_wire(request).await?, Vec::new()));
    };

    let mut dispatch = Box::pin(sidecar.dispatch_wire(request.clone()));
    tokio::select! {
        result = dispatch.as_mut() => Ok((result?, Vec::new())),
        maybe_frame = stdin_rx.recv() => {
            let frame = decode_stdin_frame(maybe_frame)?;
            if let Some(frame) = frame {
                if let Some(interrupt) = extension_interrupt_response(&blocking_request, &request, &frame) {
                    drop(dispatch);
                    let mut extra_responses = Vec::new();
                    if let Some(response) = interrupt.interrupting_response {
                        extra_responses.push(response);
                    } else {
                        *pending_frame = Some(frame);
                    }
                    return Ok((interrupt.interrupted_dispatch, extra_responses));
                }
                *pending_frame = Some(frame);
            }
            Ok((dispatch.await?, Vec::new()))
        }
    }
}

fn decode_stdin_frame(
    maybe_frame: Option<Result<Option<ProtocolFrame>, String>>,
) -> Result<Option<ProtocolFrame>, Box<dyn Error>> {
    let Some(frame) = maybe_frame else {
        return Ok(None);
    };
    Ok(frame.map_err(io::Error::other)?)
}

struct BlockingExtensionRequest {
    namespace: String,
    payload: Vec<u8>,
    extension: Arc<dyn Extension>,
}

struct ExtensionInterruptDispatch {
    interrupted_dispatch: WireDispatchResult,
    interrupting_response: Option<ResponseFrame>,
}

fn blocking_extension_request(
    sidecar: &NativeSidecar<LocalBridge>,
    request: &RequestFrame,
) -> Option<BlockingExtensionRequest> {
    let RequestPayload::ExtEnvelope(envelope) = &request.payload else {
        return None;
    };
    let extension = sidecar.extensions.get(&envelope.namespace)?.clone();
    if !extension.is_blocking_request(&envelope.payload) {
        return None;
    }
    Some(BlockingExtensionRequest {
        namespace: envelope.namespace.clone(),
        payload: envelope.payload.clone(),
        extension,
    })
}

fn extension_interrupt_response(
    blocking_request: &BlockingExtensionRequest,
    active_request: &RequestFrame,
    frame: &ProtocolFrame,
) -> Option<ExtensionInterruptDispatch> {
    match frame {
        ProtocolFrame::RequestFrame(request) => {
            let interrupt = generated_wire_blocking_extension_interrupt(
                active_request,
                &blocking_request.namespace,
                request,
            )?;
            let interrupt = blocking_request.extension.interrupt_blocking_request(
                &blocking_request.payload,
                match interrupt {
                    BlockingExtensionInterrupt::ExtensionPayload(payload) => {
                        ExtensionInterruptRequest::ExtensionPayload(payload)
                    }
                    BlockingExtensionInterrupt::KillProcess => {
                        ExtensionInterruptRequest::KillProcess
                    }
                },
            )?;
            let interrupted_dispatch = interrupted_extension_dispatch(
                active_request,
                &blocking_request.namespace,
                interrupt.interrupted_response_payload,
            );
            let interrupting_response = interrupt.interrupting_response_payload.map(|payload| {
                response_frame(
                    request.request_id,
                    request.ownership.clone(),
                    ResponsePayload::ExtEnvelope(ExtEnvelope {
                        namespace: blocking_request.namespace.clone(),
                        payload,
                    }),
                )
            });
            Some(ExtensionInterruptDispatch {
                interrupted_dispatch,
                interrupting_response,
            })
        }
        // Response, Event, and SidecarRequest frames are sidecar-to-host only. If one
        // arrives on stdin it is requeued and rejected as a protocol error by
        // handle_protocol_frame, so it must not synthesize a cancelled prompt first.
        // SidecarResponse frames answer sidecar-initiated callbacks and may be the very
        // response the blocked prompt dispatch is waiting on, so they never interrupt.
        ProtocolFrame::ResponseFrame(_)
        | ProtocolFrame::EventFrame(_)
        | ProtocolFrame::SidecarRequestFrame(_)
        | ProtocolFrame::SidecarResponseFrame(_) => None,
    }
}

fn interrupted_extension_dispatch(
    request: &RequestFrame,
    namespace: &str,
    payload: Vec<u8>,
) -> WireDispatchResult {
    if !matches!(request.payload, RequestPayload::ExtEnvelope(_)) {
        unreachable!("interrupted extension dispatch requires an extension request");
    }

    let response = ResponsePayload::ExtEnvelope(ExtEnvelope {
        namespace: namespace.to_string(),
        payload,
    });
    WireDispatchResult {
        response: response_frame(request.request_id, request.ownership.clone(), response),
        events: Vec::new(),
    }
}

async fn cleanup_connections(
    sidecar: &mut NativeSidecar<LocalBridge>,
    active_connections: &BTreeSet<String>,
    active_sessions: &mut BTreeSet<SessionScope>,
) {
    for connection_id in active_connections {
        let _ = sidecar.remove_connection(connection_id).await;
    }
    untrack_disposed_sessions(&sidecar.take_disposed_sessions(), active_sessions);
}

fn track_session_state(
    payload: &ResponsePayload,
    active_sessions: &mut BTreeSet<SessionScope>,
    active_connections: &mut BTreeSet<String>,
) {
    match payload {
        ResponsePayload::AuthenticatedResponse(AuthenticatedResponse { connection_id, .. }) => {
            active_connections.insert(connection_id.clone());
        }
        ResponsePayload::SessionOpenedResponse(SessionOpenedResponse {
            session_id,
            owner_connection_id,
        }) => {
            active_sessions.insert(SessionScope {
                connection_id: owner_connection_id.clone(),
                session_id: session_id.clone(),
            });
        }
        _ => {}
    }
}

fn read_frame(
    codec: &WireFrameCodec,
    reader: &mut impl Read,
) -> Result<Option<ProtocolFrame>, Box<dyn Error>> {
    let mut prefix = [0u8; 4];
    match reader.read_exact(&mut prefix) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    }

    let declared_len = u32::from_be_bytes(prefix) as usize;
    if declared_len > codec.max_frame_bytes() {
        return Err(ProtocolCodecError::FrameTooLarge {
            size: declared_len,
            max: codec.max_frame_bytes(),
        }
        .into());
    }
    let total_len = prefix.len().saturating_add(declared_len);
    let mut bytes = Vec::with_capacity(total_len);
    bytes.extend_from_slice(&prefix);
    bytes.resize(total_len, 0);
    reader.read_exact(&mut bytes[prefix.len()..])?;

    Ok(Some(codec.decode(&bytes)?))
}

fn write_frame(
    codec: &WireFrameCodec,
    writer: &mut impl Write,
    frame: &ProtocolFrame,
) -> Result<(), Box<dyn Error>> {
    let bytes = codec.encode(frame)?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

fn frame_kind(frame: &ProtocolFrame) -> &'static str {
    match frame {
        ProtocolFrame::RequestFrame(_) => "request",
        ProtocolFrame::ResponseFrame(_) => "response",
        ProtocolFrame::EventFrame(_) => "event",
        ProtocolFrame::SidecarRequestFrame(_) => "sidecar_request",
        ProtocolFrame::SidecarResponseFrame(_) => "sidecar_response",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StdinFrameQueueError {
    Closed,
}

// Apply backpressure rather than killing the sidecar when the host writes
// faster than the event loop drains — the exact mirror of the stdout fix in
// `send_output_frame`. Booting a VM with an agent floods stdin (config,
// software projection, session setup) while the loop is busy; previously
// `try_send` turned that transient backlog into a "stdin frame queue exceeded
// 128 pending frames" error that exited the whole sidecar and made agent boots
// fail outright. Parking the dedicated reader thread stops further reads, the
// OS pipe buffer fills, and the host's writes block — natural, recoverable
// backpressure. It never deadlocks: the async loop drains the queue
// independently, and if the receiver is gone the send fails and the reader
// exits (same terminal path as before). Near-capacity telemetry still flows
// through the `sidecar_stdin_frames` gauge.
fn enqueue_stdin_frame(
    sender: &tokio::sync::mpsc::Sender<Result<Option<ProtocolFrame>, String>>,
    frame: Result<Option<ProtocolFrame>, String>,
) -> Result<(), StdinFrameQueueError> {
    sender
        .blocking_send(frame)
        .map_err(|_closed| StdinFrameQueueError::Closed)
}

fn flush_sidecar_requests(
    sidecar: &mut NativeSidecar<LocalBridge>,
    writer: &TrackedSyncSender<ProtocolFrame>,
) -> Result<(), Box<dyn Error>> {
    while let Some(request) = sidecar.pop_wire_sidecar_request()? {
        send_output_frame(writer, ProtocolFrame::SidecarRequestFrame(request))?;
    }
    Ok(())
}

fn send_output_frame(
    writer: &TrackedSyncSender<ProtocolFrame>,
    frame: ProtocolFrame,
) -> Result<(), io::Error> {
    // Apply backpressure rather than killing the sidecar when the host reads
    // stdout slowly. A full queue means the dedicated writer thread is blocked on
    // the stdout pipe (the host has not drained it yet) — a transient, recoverable
    // condition. Previously `try_send` turned that backlog into a `BrokenPipe`
    // error that propagated up and exited the whole sidecar process (code 1),
    // taking every session with it. A blocking `send` parks the producer until the
    // writer drains a slot, which transitively backpressures the V8 event bridge
    // and the guest. It never deadlocks: the writer thread runs independently, and
    // if it dies (real broken pipe) the receiver is dropped and `send` returns
    // `Disconnected`, which we still surface as a terminal `BrokenPipe`.
    writer.send(frame).map_err(|_disconnected| {
        io::Error::new(io::ErrorKind::BrokenPipe, "stdout writer disconnected")
    })
}

/// Emit a connection-scoped `StructuredEvent { name: "heartbeat" }` frame every
/// `interval` for as long as the stdout writer is alive. This is the host's
/// liveness signal: it resets the host's silence watchdog, so a host that sees
/// no frames at all for several intervals can conclude the sidecar process is
/// dead or wedged rather than merely busy. Runs on its own thread with a clone
/// of the outbound frame channel so beats are independent of the dispatch loop.
fn spawn_heartbeat_thread(write_tx: TrackedSyncSender<ProtocolFrame>, interval: Duration) {
    thread::spawn(move || loop {
        thread::sleep(interval);
        let frame = match crate::service::structured_event_frame(
            HEARTBEAT_CONNECTION_ID,
            "heartbeat",
            std::collections::HashMap::new(),
        ) {
            Ok(frame) => frame,
            Err(error) => {
                // Unreachable for a fixed name/empty detail; if it ever fires,
                // stop loudly instead of spinning on a broken encoder.
                tracing::error!(
                    target: "agentos_native_sidecar::stdio",
                    %error,
                    "failed to encode heartbeat frame; stopping heartbeat thread",
                );
                return;
            }
        };
        if send_output_frame(&write_tx, ProtocolFrame::EventFrame(frame)).is_err() {
            // Writer thread gone — the sidecar is shutting down. Normal exit.
            return;
        }
    });
}

fn default_compile_cache_root() -> PathBuf {
    // Stable across sidecar processes so V8 compile-cache (cachedData) survives a
    // fresh sidecar/VM and benefits cold starts. Previously keyed by PID, which
    // gave every process an empty cache — cold module imports never reused
    // compiled bytecode. Entries are namespaced+validated downstream by
    // `stable_compile_cache_namespace_hash` + V8's source/version checks, so a
    // shared root is safe; stale or mismatched entries are simply ignored.
    std::env::temp_dir().join("agentos-native-sidecar-compile-cache")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{AuthenticateRequest, KillProcessRequest};
    use crate::{ExtensionContext, ExtensionFuture, ExtensionInterruptResponse, ExtensionResponse};
    use std::io::Cursor;

    const TEST_EXTENSION_NAMESPACE: &str = "dev.rivet.secure-exec.test.blocking";

    #[test]
    fn heartbeat_thread_emits_periodic_structured_heartbeat_frames() {
        let (write_tx, write_rx) =
            tracked_sync_channel::<ProtocolFrame>(TrackedLimit::SidecarStdoutFrames, 16);
        spawn_heartbeat_thread(write_tx, Duration::from_millis(5));

        // Two beats prove the emitter is periodic, not one-shot.
        for beat in 0..2 {
            let frame = write_rx.recv().expect("heartbeat frame");
            let ProtocolFrame::EventFrame(event) = frame else {
                panic!("expected event frame for beat {beat}, got {frame:?}");
            };
            let event = crate::wire::event_frame_to_compat(event).expect("decode heartbeat frame");
            let crate::protocol::EventPayload::Structured(structured) = event.payload else {
                panic!("expected structured payload for beat {beat}");
            };
            assert_eq!(structured.name, "heartbeat");
        }
        // Dropping the receiver disconnects the channel; the emitter thread
        // observes the send failure and exits cleanly.
    }

    #[test]
    fn read_frame_rejects_oversized_prefix_before_allocating_payload() {
        let codec = WireFrameCodec::new(16);
        let mut reader = Cursor::new((32_u32).to_be_bytes().to_vec());

        let error = read_frame(&codec, &mut reader).expect_err("oversized frame should fail");
        let error = error
            .downcast::<ProtocolCodecError>()
            .expect("protocol codec error");
        assert!(matches!(
            *error,
            ProtocolCodecError::FrameTooLarge { size: 32, max: 16 }
        ));
    }

    #[test]
    fn stdio_work_queues_are_bounded() {
        let (event_ready_tx, _event_ready_rx) = channel::<()>(MAX_EVENT_READY_QUEUE);
        event_ready_tx
            .try_send(())
            .expect("event-ready queue should accept capacity");
        assert!(matches!(
            event_ready_tx.try_send(()),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_))
        ));
    }

    // Regression: a full stdin frame queue must apply backpressure (park the
    // reader thread until the async loop drains a slot), NOT tear the sidecar
    // down. Booting a VM with an agent floods stdin (config, software
    // projection, session setup) while the loop is busy; the old `try_send`
    // turned that backlog into a "stdin frame queue exceeded 128 pending
    // frames" error that exited the whole sidecar and failed the boot.
    #[test]
    fn stdin_frame_queue_applies_backpressure_instead_of_crashing() {
        const SENDS: usize = 8;
        let (stdin_tx, mut stdin_rx) = channel::<Result<Option<ProtocolFrame>, String>>(2);
        let producer = thread::spawn(move || {
            for _ in 0..SENDS {
                enqueue_stdin_frame(&stdin_tx, Ok(None))
                    .expect("a full queue must park the sender, not fail");
            }
        });
        let mut received = 0;
        while received < SENDS {
            if stdin_rx.blocking_recv().is_some() {
                received += 1;
            }
        }
        producer.join().expect("producer thread");

        // A dropped receiver (dead event loop) is the only terminal condition.
        let (closed_tx, closed_rx) = channel::<Result<Option<ProtocolFrame>, String>>(1);
        drop(closed_rx);
        assert!(matches!(
            enqueue_stdin_frame(&closed_tx, Ok(None)),
            Err(StdinFrameQueueError::Closed)
        ));
    }

    // Regression: a full stdout frame queue must apply backpressure (block the
    // producer until the writer drains a slot), NOT tear the sidecar down. The
    // old `try_send` turned a slow host reader into a `BrokenPipe` error that
    // propagated up and exited the whole sidecar process (code 1). Here a slow
    // drainer forces the queue past capacity; with backpressure every send
    // succeeds, and overflow only fails when the writer (receiver) is gone.
    #[test]
    fn stdout_frame_queue_applies_backpressure_instead_of_crashing() {
        let queue_frame = |request_id: RequestId| {
            ProtocolFrame::RequestFrame(request_frame(
                request_id,
                connection_ownership("conn-queue"),
                RequestPayload::AuthenticateRequest(AuthenticateRequest {
                    client_name: String::from("queue-test"),
                    auth_token: String::from("token"),
                    protocol_version: wire::PROTOCOL_VERSION,
                    bridge_version: agentos_bridge::bridge_contract().version,
                }),
            ))
        };

        // Small fixed capacity (independent of the production constant) with a
        // drainer slow enough that the queue fills and the producer is forced
        // onto the blocking path. The old try_send path errored on the
        // (capacity + 1)th frame; backpressure accepts all of them.
        let queue_cap = 8usize;
        let total_frames = queue_cap * 3;
        let (stdout_tx, stdout_rx) =
            tracked_sync_channel::<ProtocolFrame>(TrackedLimit::SidecarStdoutFrames, queue_cap);
        let drainer = std::thread::spawn(move || {
            let mut drained = 0usize;
            while stdout_rx.recv().is_ok() {
                drained += 1;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            drained
        });

        for request_id in 0..total_frames {
            send_output_frame(&stdout_tx, queue_frame(request_id as RequestId))
                .expect("backpressured stdout queue must accept frames, not crash");
        }
        drop(stdout_tx);
        let drained = drainer.join().expect("drainer thread panicked");
        assert_eq!(
            drained, total_frames,
            "every frame must survive the backpressured queue"
        );

        // When the writer (receiver) is gone, overflow is genuinely terminal and
        // still surfaces as a BrokenPipe error rather than blocking forever.
        let (closed_tx, closed_rx) =
            tracked_sync_channel::<ProtocolFrame>(TrackedLimit::SidecarStdoutFrames, queue_cap);
        drop(closed_rx);
        let error = send_output_frame(&closed_tx, queue_frame(0))
            .expect_err("send to a dropped writer must error");
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    // Regression (M5): the active-session set must shrink when a session is
    // disposed. `track_session_state` is insert-only, so the transport relies on
    // `untrack_disposed_sessions` draining the sidecar's disposed-session signal;
    // without it a long-lived connection's set grows per session forever and the
    // ~250us event pump iterates every dead entry.
    #[test]
    fn disposed_sessions_are_untracked_from_active_sessions() {
        let mut active_sessions = BTreeSet::<SessionScope>::new();
        let mut active_connections = BTreeSet::<String>::new();
        track_session_state(
            &ResponsePayload::SessionOpenedResponse(SessionOpenedResponse {
                session_id: String::from("session-1"),
                owner_connection_id: String::from("conn-1"),
            }),
            &mut active_sessions,
            &mut active_connections,
        );
        assert_eq!(
            active_sessions.len(),
            1,
            "opening a session should track it for the event pump"
        );

        untrack_disposed_sessions(
            &[(String::from("conn-1"), String::from("session-1"))],
            &mut active_sessions,
        );
        assert!(
            active_sessions.is_empty(),
            "a disposed session must be removed from the active-session set"
        );
    }

    #[test]
    fn read_frame_decodes_wire_authenticate_request() {
        let codec = WireFrameCodec::new(wire::DEFAULT_MAX_FRAME_BYTES);
        let frame = ProtocolFrame::RequestFrame(request_frame(
            1,
            connection_ownership("client-hint"),
            RequestPayload::AuthenticateRequest(AuthenticateRequest {
                client_name: "probe".to_string(),
                auth_token: "probe-token".to_string(),
                protocol_version: wire::PROTOCOL_VERSION,
                bridge_version: agentos_bridge::bridge_contract().version,
            }),
        ));
        let encoded = codec.encode(&frame).expect("encode wire frame");
        let mut reader = Cursor::new(encoded);

        let decoded = read_frame(&codec, &mut reader)
            .expect("decode bare frame")
            .expect("frame present");

        assert_eq!(decoded, frame);
    }

    #[test]
    fn extension_close_interrupts_matching_blocking_request() {
        let ownership = vm_ownership("conn-1", "session-1", "vm-1");
        let prompt = test_extension_request_frame(10, ownership.clone(), "prompt:ext-session-1");
        let close = ProtocolFrame::RequestFrame(test_extension_request_frame(
            11,
            ownership,
            "close:ext-session-1",
        ));

        let blocking_request = blocking_extension_request(&prompt);
        let interrupt = extension_interrupt_response(&blocking_request, &prompt, &close)
            .expect("close should interrupt prompt");

        assert_eq!(interrupt.interrupted_dispatch.response.request_id, 10);
        let ResponsePayload::ExtEnvelope(envelope) =
            interrupt.interrupted_dispatch.response.payload
        else {
            panic!("expected extension response");
        };
        assert_eq!(envelope.namespace, TEST_EXTENSION_NAMESPACE);
        assert_eq!(envelope.payload, b"prompt-cancelled:ext-session-1");
    }

    #[test]
    fn extension_cancel_interrupt_gets_synthetic_response() {
        let ownership = vm_ownership("conn-1", "session-1", "vm-1");
        let prompt = test_extension_request_frame(10, ownership.clone(), "prompt:ext-session-1");
        let cancel = ProtocolFrame::RequestFrame(test_extension_request_frame(
            11,
            ownership,
            "cancel:ext-session-1",
        ));

        let blocking_request = blocking_extension_request(&prompt);
        let interrupt = extension_interrupt_response(&blocking_request, &prompt, &cancel)
            .expect("cancel should interrupt prompt");
        let response = interrupt
            .interrupting_response
            .expect("cancel should get a response");

        assert_eq!(response.request_id, 11);
        let ResponsePayload::ExtEnvelope(envelope) = response.payload else {
            panic!("expected extension response");
        };
        assert_eq!(envelope.namespace, TEST_EXTENSION_NAMESPACE);
        assert_eq!(envelope.payload, b"cancelled:ext-session-1");
    }

    #[test]
    fn kill_process_interrupts_blocking_extension_request() {
        let ownership = vm_ownership("conn-1", "session-1", "vm-1");
        let prompt = test_extension_request_frame(10, ownership.clone(), "prompt:ext-session-1");
        let kill = ProtocolFrame::RequestFrame(request_frame(
            11,
            ownership,
            RequestPayload::KillProcessRequest(KillProcessRequest {
                process_id: "adapter-process".to_string(),
                signal: "SIGTERM".to_string(),
            }),
        ));

        let blocking_request = blocking_extension_request(&prompt);
        let interrupt = extension_interrupt_response(&blocking_request, &prompt, &kill)
            .expect("kill should interrupt prompt");

        assert_eq!(interrupt.interrupted_dispatch.response.request_id, 10);
        assert!(interrupt.interrupting_response.is_none());
    }

    fn test_extension_request_frame(
        request_id: RequestId,
        ownership: OwnershipScope,
        payload: &str,
    ) -> RequestFrame {
        request_frame(
            request_id,
            ownership,
            RequestPayload::ExtEnvelope(ExtEnvelope {
                namespace: TEST_EXTENSION_NAMESPACE.to_string(),
                payload: payload.as_bytes().to_vec(),
            }),
        )
    }

    fn blocking_extension_request(request: &RequestFrame) -> BlockingExtensionRequest {
        let RequestPayload::ExtEnvelope(envelope) = &request.payload else {
            panic!("expected extension request");
        };
        BlockingExtensionRequest {
            namespace: TEST_EXTENSION_NAMESPACE.to_string(),
            payload: envelope.payload.clone(),
            extension: Arc::new(TestBlockingInterruptExtension),
        }
    }

    struct TestBlockingInterruptExtension;

    impl Extension for TestBlockingInterruptExtension {
        fn namespace(&self) -> &str {
            TEST_EXTENSION_NAMESPACE
        }

        fn handle_request<'a>(
            &'a self,
            _ctx: ExtensionContext<'a>,
            _payload: Vec<u8>,
        ) -> ExtensionFuture<'a, ExtensionResponse> {
            Box::pin(async { Ok(ExtensionResponse::new(Vec::new())) })
        }

        fn is_blocking_request(&self, payload: &[u8]) -> bool {
            parse_test_payload(payload).is_some_and(|(kind, _session_id)| kind == "prompt")
        }

        fn interrupt_blocking_request(
            &self,
            blocking_payload: &[u8],
            interrupt: ExtensionInterruptRequest<'_>,
        ) -> Option<ExtensionInterruptResponse> {
            let (blocking_kind, blocking_session_id) = parse_test_payload(blocking_payload)?;
            if blocking_kind != "prompt" {
                return None;
            }

            let interrupted_response_payload =
                encode_test_response("prompt-cancelled", blocking_session_id);
            match interrupt {
                ExtensionInterruptRequest::KillProcess => Some(ExtensionInterruptResponse {
                    interrupted_response_payload,
                    interrupting_response_payload: None,
                }),
                ExtensionInterruptRequest::ExtensionPayload(payload) => {
                    let (interrupt_kind, interrupt_session_id) = parse_test_payload(payload)?;
                    match interrupt_kind {
                        "close" if interrupt_session_id == blocking_session_id => {
                            Some(ExtensionInterruptResponse {
                                interrupted_response_payload,
                                interrupting_response_payload: None,
                            })
                        }
                        "cancel" if interrupt_session_id == blocking_session_id => {
                            Some(ExtensionInterruptResponse {
                                interrupted_response_payload,
                                interrupting_response_payload: Some(encode_test_response(
                                    "cancelled",
                                    interrupt_session_id,
                                )),
                            })
                        }
                        "prompt" | "close" | "cancel" => None,
                        _ => None,
                    }
                }
            }
        }
    }

    fn parse_test_payload(payload: &[u8]) -> Option<(&str, &str)> {
        let payload = std::str::from_utf8(payload).ok()?;
        payload.split_once(':')
    }

    fn encode_test_response(kind: &str, session_id: &str) -> Vec<u8> {
        format!("{kind}:{session_id}").into_bytes()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LocalBridge {
    started_at: Instant,
    next_timer_id: usize,
    snapshots: BTreeMap<String, FilesystemSnapshot>,
}

impl Default for LocalBridge {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            next_timer_id: 0,
            snapshots: BTreeMap::new(),
        }
    }
}

impl BridgeTypes for LocalBridge {
    type Error = LocalBridgeError;
}

impl FilesystemBridge for LocalBridge {
    fn read_file(&mut self, request: ReadFileRequest) -> Result<Vec<u8>, Self::Error> {
        fs::read(Self::host_path(&request.path))
            .map_err(|error| LocalBridgeError::io("read", &request.path, error))
    }

    fn write_file(&mut self, request: WriteFileRequest) -> Result<(), Self::Error> {
        let host_path = Self::host_path(&request.path);
        if let Some(parent) = host_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LocalBridgeError::io("mkdir", &request.path, error))?;
        }
        fs::write(host_path, request.contents)
            .map_err(|error| LocalBridgeError::io("write", &request.path, error))
    }

    fn stat(&mut self, request: PathRequest) -> Result<FileMetadata, Self::Error> {
        fs::metadata(Self::host_path(&request.path))
            .map(Self::file_metadata)
            .map_err(|error| LocalBridgeError::io("stat", &request.path, error))
    }

    fn lstat(&mut self, request: PathRequest) -> Result<FileMetadata, Self::Error> {
        fs::symlink_metadata(Self::host_path(&request.path))
            .map(Self::file_metadata)
            .map_err(|error| LocalBridgeError::io("lstat", &request.path, error))
    }

    fn read_dir(&mut self, request: ReadDirRequest) -> Result<Vec<DirectoryEntry>, Self::Error> {
        let mut entries = fs::read_dir(Self::host_path(&request.path))
            .map_err(|error| LocalBridgeError::io("readdir", &request.path, error))?
            .map(|entry| {
                let entry =
                    entry.map_err(|error| LocalBridgeError::io("readdir", &request.path, error))?;
                let kind = entry
                    .file_type()
                    .map(Self::file_kind)
                    .map_err(|error| LocalBridgeError::io("readdir", &request.path, error))?;
                Ok(DirectoryEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    kind,
                })
            })
            .collect::<Result<Vec<_>, LocalBridgeError>>()?;
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    fn create_dir(&mut self, request: CreateDirRequest) -> Result<(), Self::Error> {
        let host_path = Self::host_path(&request.path);
        if request.recursive {
            fs::create_dir_all(host_path)
        } else {
            fs::create_dir(host_path)
        }
        .map_err(|error| LocalBridgeError::io("mkdir", &request.path, error))
    }

    fn remove_file(&mut self, request: PathRequest) -> Result<(), Self::Error> {
        fs::remove_file(Self::host_path(&request.path))
            .map_err(|error| LocalBridgeError::io("unlink", &request.path, error))
    }

    fn remove_dir(&mut self, request: PathRequest) -> Result<(), Self::Error> {
        fs::remove_dir(Self::host_path(&request.path))
            .map_err(|error| LocalBridgeError::io("rmdir", &request.path, error))
    }

    fn rename(&mut self, request: RenameRequest) -> Result<(), Self::Error> {
        let from_path = Self::host_path(&request.from_path);
        let to_path = Self::host_path(&request.to_path);
        if let Some(parent) = to_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LocalBridgeError::io("mkdir", &request.to_path, error))?;
        }
        fs::rename(from_path, to_path).map_err(|error| {
            LocalBridgeError::unsupported(format!(
                "rename {} -> {}: {}",
                request.from_path, request.to_path, error
            ))
        })
    }

    fn symlink(&mut self, request: SymlinkRequest) -> Result<(), Self::Error> {
        let link_path = Self::host_path(&request.link_path);
        if let Some(parent) = link_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LocalBridgeError::io("mkdir", &request.link_path, error))?;
        }
        create_symlink(&request.target_path, link_path)
            .map_err(|error| LocalBridgeError::io("symlink", &request.link_path, error))
    }

    fn read_link(&mut self, request: PathRequest) -> Result<String, Self::Error> {
        fs::read_link(Self::host_path(&request.path))
            .map(|target| target.to_string_lossy().into_owned())
            .map_err(|error| LocalBridgeError::io("readlink", &request.path, error))
    }

    fn chmod(&mut self, request: ChmodRequest) -> Result<(), Self::Error> {
        let permissions = fs::Permissions::from_mode(request.mode);
        fs::set_permissions(Self::host_path(&request.path), permissions)
            .map_err(|error| LocalBridgeError::io("chmod", &request.path, error))
    }

    fn truncate(&mut self, request: TruncateRequest) -> Result<(), Self::Error> {
        OpenOptions::new()
            .write(true)
            .create(false)
            .open(Self::host_path(&request.path))
            .and_then(|file| file.set_len(request.len))
            .map_err(|error| LocalBridgeError::io("truncate", &request.path, error))
    }

    fn exists(&mut self, request: PathRequest) -> Result<bool, Self::Error> {
        Ok(fs::symlink_metadata(Self::host_path(&request.path)).is_ok())
    }
}

impl PermissionBridge for LocalBridge {
    fn check_filesystem_access(
        &mut self,
        request: FilesystemPermissionRequest,
    ) -> Result<PermissionDecision, Self::Error> {
        Ok(PermissionDecision::deny(format!(
            "no static filesystem policy registered for {}:{}",
            request.vm_id, request.path
        )))
    }

    fn check_network_access(
        &mut self,
        request: NetworkPermissionRequest,
    ) -> Result<PermissionDecision, Self::Error> {
        Ok(PermissionDecision::deny(format!(
            "no static network policy registered for {}:{}",
            request.vm_id, request.resource
        )))
    }

    fn check_command_execution(
        &mut self,
        request: CommandPermissionRequest,
    ) -> Result<PermissionDecision, Self::Error> {
        Ok(PermissionDecision::deny(format!(
            "no static child_process policy registered for {}:{}",
            request.vm_id, request.command
        )))
    }

    fn check_environment_access(
        &mut self,
        request: EnvironmentPermissionRequest,
    ) -> Result<PermissionDecision, Self::Error> {
        Ok(PermissionDecision::deny(format!(
            "no static env policy registered for {}:{}",
            request.vm_id, request.key
        )))
    }
}

impl PersistenceBridge for LocalBridge {
    fn load_filesystem_state(
        &mut self,
        request: LoadFilesystemStateRequest,
    ) -> Result<Option<FilesystemSnapshot>, Self::Error> {
        Ok(self.snapshots.get(&request.vm_id).cloned())
    }

    fn flush_filesystem_state(
        &mut self,
        request: FlushFilesystemStateRequest,
    ) -> Result<(), Self::Error> {
        self.snapshots.insert(request.vm_id, request.snapshot);
        Ok(())
    }
}

impl ClockBridge for LocalBridge {
    fn wall_clock(&mut self, _request: ClockRequest) -> Result<SystemTime, Self::Error> {
        Ok(SystemTime::now())
    }

    fn monotonic_clock(&mut self, _request: ClockRequest) -> Result<Duration, Self::Error> {
        Ok(self.started_at.elapsed())
    }

    fn schedule_timer(
        &mut self,
        request: ScheduleTimerRequest,
    ) -> Result<ScheduledTimer, Self::Error> {
        self.next_timer_id += 1;
        Ok(ScheduledTimer {
            timer_id: format!("timer-{}", self.next_timer_id),
            delay: request.delay,
        })
    }
}

impl RandomBridge for LocalBridge {
    fn fill_random_bytes(&mut self, request: RandomBytesRequest) -> Result<Vec<u8>, Self::Error> {
        Ok(vec![0u8; request.len])
    }
}

impl EventBridge for LocalBridge {
    fn emit_structured_event(&mut self, _event: StructuredEventRecord) -> Result<(), Self::Error> {
        Ok(())
    }

    fn emit_diagnostic(&mut self, _event: DiagnosticRecord) -> Result<(), Self::Error> {
        Ok(())
    }

    fn emit_log(&mut self, _event: LogRecord) -> Result<(), Self::Error> {
        Ok(())
    }

    fn emit_lifecycle(&mut self, _event: LifecycleEventRecord) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ExecutionBridge for LocalBridge {
    fn create_javascript_context(
        &mut self,
        _request: CreateJavascriptContextRequest,
    ) -> Result<GuestContextHandle, Self::Error> {
        Err(LocalBridgeError::unsupported(
            "execution bridge is handled internally by the native sidecar",
        ))
    }

    fn create_wasm_context(
        &mut self,
        _request: CreateWasmContextRequest,
    ) -> Result<GuestContextHandle, Self::Error> {
        Err(LocalBridgeError::unsupported(
            "execution bridge is handled internally by the native sidecar",
        ))
    }

    fn start_execution(
        &mut self,
        _request: StartExecutionRequest,
    ) -> Result<StartedExecution, Self::Error> {
        Err(LocalBridgeError::unsupported(
            "execution bridge is handled internally by the native sidecar",
        ))
    }

    fn write_stdin(&mut self, _request: WriteExecutionStdinRequest) -> Result<(), Self::Error> {
        Err(LocalBridgeError::unsupported(
            "execution bridge is handled internally by the native sidecar",
        ))
    }

    fn close_stdin(&mut self, _request: ExecutionHandleRequest) -> Result<(), Self::Error> {
        Err(LocalBridgeError::unsupported(
            "execution bridge is handled internally by the native sidecar",
        ))
    }

    fn kill_execution(&mut self, _request: KillExecutionRequest) -> Result<(), Self::Error> {
        Err(LocalBridgeError::unsupported(
            "execution bridge is handled internally by the native sidecar",
        ))
    }

    fn poll_execution_event(
        &mut self,
        _request: PollExecutionEventRequest,
    ) -> Result<Option<ExecutionEvent>, Self::Error> {
        Ok(None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SessionScope {
    connection_id: String,
    session_id: String,
}

impl SessionScope {
    fn ownership_scope(&self) -> OwnershipScope {
        session_ownership(&self.connection_id, &self.session_id)
    }

    fn compat_ownership_scope(&self) -> crate::protocol::OwnershipScope {
        wire::ownership_scope_to_compat(self.ownership_scope())
    }
}

/// Live event sink backed by the outbound stdout channel. Writes each event as a
/// `ProtocolFrame::EventFrame` immediately, using the same blocking
/// backpressure semantics as the batch event path (`send_output_frame`): a full
/// queue parks the producer until the writer thread drains stdout rather than
/// tearing down the process.
struct FrameEventTransport {
    writer: TrackedSyncSender<ProtocolFrame>,
}

impl FrameEventTransport {
    fn new(writer: TrackedSyncSender<ProtocolFrame>) -> Self {
        Self { writer }
    }
}

impl EventSinkTransport for FrameEventTransport {
    fn emit_event(&self, event: crate::wire::EventFrame) -> Result<(), SidecarError> {
        send_output_frame(&self.writer, ProtocolFrame::EventFrame(event))
            .map_err(|error| SidecarError::Bridge(error.to_string()))
    }
}

struct FrameSidecarRequestTransport {
    writer: TrackedSyncSender<ProtocolFrame>,
    pending: Arc<Mutex<BTreeMap<RequestId, mpsc::SyncSender<SidecarResponseFrame>>>>,
}

impl FrameSidecarRequestTransport {
    fn new(writer: TrackedSyncSender<ProtocolFrame>) -> Self {
        Self {
            writer,
            pending: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    fn accept_response(&self, response: SidecarResponseFrame) -> bool {
        let sender = {
            let mut pending = match self.pending.lock() {
                Ok(pending) => pending,
                Err(_) => return false,
            };
            pending.remove(&response.request_id)
        };
        let Some(sender) = sender else {
            return false;
        };
        let _ = sender.send(response);
        true
    }
}

impl SidecarRequestTransport for FrameSidecarRequestTransport {
    fn send_request(
        &self,
        request: crate::protocol::SidecarRequestFrame,
        timeout: Duration,
    ) -> Result<crate::protocol::SidecarResponseFrame, SidecarError> {
        let request =
            wire::sidecar_request_frame_from_compat(request).map_err(wire_protocol_error)?;
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending
            .lock()
            .map_err(|_| {
                SidecarError::Bridge(String::from("sidecar callback waiter map lock poisoned"))
            })?
            .insert(request.request_id, sender);
        // Bound the request-frame WRITE by the caller's deadline. The shared
        // `send_output_frame` blocks (correct backpressure for the fire-and-forget
        // event/response paths), but this request path has a `timeout` that the
        // response wait below already honors — so a stalled host stdout must not
        // make the *send* block past it. Poll try_send until a slot frees or the
        // deadline passes.
        let write_deadline = Instant::now() + timeout;
        let mut frame = ProtocolFrame::SidecarRequestFrame(request.clone());
        let write_result = loop {
            match self.writer.try_send(frame) {
                Ok(()) => break Ok(()),
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    break Err(String::from("stdout writer disconnected"));
                }
                Err(mpsc::TrySendError::Full(returned)) => {
                    if Instant::now() >= write_deadline {
                        break Err(format!(
                            "timed out writing sidecar request frame after {}s",
                            timeout.as_secs()
                        ));
                    }
                    frame = returned;
                    thread::sleep(Duration::from_millis(1));
                }
            }
        };
        if let Err(message) = write_result {
            let _ = self
                .pending
                .lock()
                .map(|mut pending| pending.remove(&request.request_id));
            return Err(SidecarError::Io(format!(
                "failed to write sidecar request frame: {message}"
            )));
        }
        match receiver.recv_timeout(timeout) {
            Ok(response) => {
                wire::sidecar_response_frame_to_compat(response).map_err(wire_protocol_error)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = self
                    .pending
                    .lock()
                    .map(|mut pending| pending.remove(&request.request_id));
                Err(SidecarError::Io(format!(
                    "timed out waiting for sidecar response after {}s",
                    timeout.as_secs()
                )))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(SidecarError::Io(String::from(
                "sidecar response waiter disconnected",
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalBridgeError {
    message: String,
}

impl LocalBridgeError {
    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn io(operation: &str, path: &str, error: io::Error) -> Self {
        Self::unsupported(format!("{operation} {path}: {error}"))
    }
}

impl fmt::Display for LocalBridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for LocalBridgeError {}

impl LocalBridge {
    fn host_path(path: &str) -> PathBuf {
        let candidate = Path::new(path);
        if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(candidate)
        }
    }

    fn file_metadata(metadata: fs::Metadata) -> FileMetadata {
        FileMetadata {
            mode: metadata.permissions().mode(),
            size: metadata.size(),
            kind: Self::file_kind(metadata.file_type()),
        }
    }

    fn file_kind(file_type: fs::FileType) -> agentos_bridge::FileKind {
        if file_type.is_file() {
            agentos_bridge::FileKind::File
        } else if file_type.is_dir() {
            agentos_bridge::FileKind::Directory
        } else if file_type.is_symlink() {
            agentos_bridge::FileKind::SymbolicLink
        } else {
            agentos_bridge::FileKind::Other
        }
    }
}
