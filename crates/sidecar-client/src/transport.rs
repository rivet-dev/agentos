//! `SidecarTransport`: spawns a native sidecar binary and speaks the existing framed
//! BARE protocol over its stdio.
//!
//! This mirrors the TypeScript `Sidecar`. Generated wire payloads are the native
//! transport path.
//!
//! Request-id direction is load-bearing: host-initiated `Request`/`Response` frames use positive ids
//! allocated by this transport, while sidecar-initiated `SidecarRequest`/`SidecarResponse` callbacks
//! echo the id allocated by the sidecar.

use std::process::Stdio;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

use scc::HashMap as SccHashMap;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::wire::{self, WireFrameCodec};
use crate::TransportError;

/// Broadcast capacity for the structured/lifecycle/process event fan-out.
const EVENT_CHANNEL_CAPACITY: usize = 4096;

/// Maximum outbound frames buffered while the writer task drains to sidecar stdin.
const REQUEST_FRAME_QUEUE_CAPACITY: usize = 4096;

/// Maximum callback/control response frames buffered ahead of regular host requests.
const CONTROL_FRAME_QUEUE_CAPACITY: usize = 1024;

/// Maximum in-flight host-initiated sidecar requests per transport.
const PENDING_REQUEST_LIMIT: usize = 4096;

/// Env var that overrides the sidecar binary path. Defaults to `agentos-native-sidecar` on `PATH`.
/// Product clients can pass an explicit binary path to [`SidecarTransport::spawn`].
const SIDECAR_BIN_ENV: &str = "AGENTOS_SIDECAR_BIN";

/// How long the host tolerates TOTAL inbound silence (no responses, events, sidecar requests, or
/// heartbeats) before declaring the sidecar dead. The sidecar heartbeats every 10s from a dedicated
/// thread even while busy, so this allows two missed beats plus margin; it bounds "sidecar is dead
/// or wedged", never "this request is slow" — individual requests have no deadline of their own.
/// Fixed protocol constant paired with the sidecar heartbeat cadence; mirrors the TS client.
const SIDECAR_SILENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// A registered callback that answers a sidecar-initiated request using generated wire types.
pub type WireSidecarCallback = Arc<
    dyn Fn(
            wire::SidecarRequestPayload,
            wire::OwnershipScope,
        ) -> futures::future::BoxFuture<
            'static,
            Result<wire::SidecarResponsePayload, TransportError>,
        > + Send
        + Sync,
>;

/// A decoded sidecar event shared across all transport subscribers.
///
/// The transport has several independent event consumers. Keeping the decoded frame behind an
/// [`Arc`] makes broadcast fan-out clone only this pointer instead of deep-cloning payload buffers
/// such as captured process stdout/stderr for every subscriber.
pub type SharedWireEvent = Arc<wire::EventFrame>;

/// Owns the spawned sidecar child, the framed BARE stdio I/O tasks, the pending-response map, the
/// event fan-out, and the callback dispatch table.
pub struct SidecarTransport {
    /// The spawned sidecar process (stdout/stdin taken by the I/O tasks; kept for kill on drop).
    child: parking_lot::Mutex<Option<Child>>,
    /// Pending host-initiated requests, keyed by positive `RequestId`.
    pending: SccHashMap<wire::RequestId, oneshot::Sender<wire::ResponsePayload>>,
    pending_request_lock: parking_lot::Mutex<()>,
    /// Host request-id counter (positive, starts at 1).
    request_counter: AtomicI64,
    /// Negotiated max frame size.
    max_frame_bytes: AtomicUsize,
    /// Structured-event fan-out for `Event` frames.
    event_tx: broadcast::Sender<SharedWireEvent>,
    /// Registered host callbacks for `SidecarRequest` frames.
    callbacks: SccHashMap<&'static str, WireSidecarCallback>,
    /// Outbound host request frames drained by the writer task into the child's stdin.
    request_writer_tx: mpsc::Sender<Vec<u8>>,
    /// Outbound callback/control response frames. The writer drains this before regular requests.
    control_writer_tx: mpsc::Sender<Vec<u8>>,
    /// When the reader last received any inbound frame; the silence watchdog reads it.
    last_inbound_at: parking_lot::Mutex<std::time::Instant>,
}

impl SidecarTransport {
    /// Spawn the native sidecar binary and start the stdio I/O tasks.
    ///
    /// Does NOT run the handshake. Product clients drive Authenticate and any follow-up setup using
    /// [`request_wire`](Self::request_wire) once the transport is live.
    pub async fn spawn(binary_path: Option<String>) -> Result<Arc<Self>, TransportError> {
        let bin = resolve_sidecar_binary_path(binary_path);
        let mut child = Command::new(&bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                TransportError::Sidecar(format!("failed to spawn sidecar '{bin}': {error}"))
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TransportError::Sidecar("sidecar stdin was not piped".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TransportError::Sidecar("sidecar stdout was not piped".to_string()))?;

        let (request_writer_tx, request_writer_rx) = mpsc::channel(REQUEST_FRAME_QUEUE_CAPACITY);
        let (control_writer_tx, control_writer_rx) = mpsc::channel(CONTROL_FRAME_QUEUE_CAPACITY);
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        let transport = Arc::new(Self {
            child: parking_lot::Mutex::new(Some(child)),
            pending: SccHashMap::new(),
            pending_request_lock: parking_lot::Mutex::new(()),
            request_counter: AtomicI64::new(1),
            max_frame_bytes: AtomicUsize::new(wire::DEFAULT_MAX_FRAME_BYTES),
            event_tx,
            callbacks: SccHashMap::new(),
            request_writer_tx,
            control_writer_tx,
            last_inbound_at: parking_lot::Mutex::new(std::time::Instant::now()),
        });

        tokio::spawn(run_writer(stdin, control_writer_rx, request_writer_rx));
        tokio::spawn(run_reader(Arc::downgrade(&transport), stdout));
        tokio::spawn(run_silence_watchdog(
            Arc::downgrade(&transport),
            SIDECAR_SILENCE_TIMEOUT,
        ));

        Ok(transport)
    }

    /// Allocate the next positive host request id.
    pub fn next_request_id(&self) -> wire::RequestId {
        self.request_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Issue a host request using generated wire protocol types and await a generated response.
    pub async fn request_wire(
        &self,
        ownership: wire::OwnershipScope,
        payload: wire::RequestPayload,
    ) -> Result<wire::ResponsePayload, TransportError> {
        self.request_wire_with_frame_limit(ownership, payload, None)
            .await
    }

    /// Issue a host request using generated wire protocol types with a caller-specific frame limit.
    pub async fn request_wire_bounded(
        &self,
        ownership: wire::OwnershipScope,
        payload: wire::RequestPayload,
        max_frame_bytes: usize,
    ) -> Result<wire::ResponsePayload, TransportError> {
        self.request_wire_with_frame_limit(ownership, payload, Some(max_frame_bytes))
            .await
    }

    async fn request_wire_with_frame_limit(
        &self,
        ownership: wire::OwnershipScope,
        payload: wire::RequestPayload,
        max_frame_bytes: Option<usize>,
    ) -> Result<wire::ResponsePayload, TransportError> {
        let request_id = self.next_request_id();
        let frame = wire::ProtocolFrame::RequestFrame(wire::RequestFrame {
            schema: wire::protocol_schema(),
            request_id,
            ownership,
            payload,
        });
        let bytes = self.encode_wire_frame(&frame, max_frame_bytes)?;

        let (tx, rx) = oneshot::channel();
        self.register_pending_request(request_id, tx)?;
        let _pending_guard = PendingRequestGuard::new(self, request_id);

        if self.request_writer_tx.send(bytes).await.is_err() {
            self.pending.remove(&request_id);
            return Err(TransportError::Sidecar(
                "sidecar transport closed".to_string(),
            ));
        }

        rx.await
            .map_err(|_| TransportError::Sidecar("sidecar transport disconnected".to_string()))
    }

    /// Subscribe to structured/lifecycle/process events using generated wire protocol types.
    pub fn subscribe_wire_events(&self) -> broadcast::Receiver<SharedWireEvent> {
        self.event_tx.subscribe()
    }

    /// Register a callback that answers a class of sidecar-initiated requests using generated wire
    /// protocol types.
    pub fn register_wire_callback(&self, key: &'static str, callback: WireSidecarCallback) {
        let _ = self.callbacks.insert(key, callback);
    }

    /// Return the currently negotiated max frame size.
    pub fn max_frame_bytes(&self) -> usize {
        self.max_frame_bytes.load(Ordering::Relaxed)
    }

    /// Update the negotiated max frame size after authentication.
    pub fn set_max_frame_bytes(&self, max_frame_bytes: usize) {
        self.max_frame_bytes
            .store(max_frame_bytes, Ordering::SeqCst);
    }

    /// Kill the child sidecar process if this transport still owns one.
    pub fn kill_child(&self) {
        if let Some(mut child) = self.child.lock().take() {
            let _ = child.start_kill();
        }
    }

    fn encode_wire_frame(
        &self,
        frame: &wire::ProtocolFrame,
        max_frame_bytes: Option<usize>,
    ) -> Result<Vec<u8>, TransportError> {
        let transport_limit = self.max_frame_bytes.load(Ordering::Relaxed);
        let max_frame_bytes = max_frame_bytes
            .map(|limit| limit.min(transport_limit))
            .unwrap_or(transport_limit);
        let codec = WireFrameCodec::new(max_frame_bytes);
        Ok(codec.encode(frame)?)
    }

    /// Route a decoded inbound frame. Host transports only legitimately receive `Response`, `Event`,
    /// and `SidecarRequest` frames.
    async fn handle_wire_frame(self: &Arc<Self>, frame: wire::ProtocolFrame) {
        match frame {
            wire::ProtocolFrame::ResponseFrame(response) => {
                match self.pending.remove(&response.request_id) {
                    Some((_, tx)) => {
                        let _ = tx.send(response.payload);
                    }
                    None => {
                        tracing::warn!(
                            request_id = response.request_id,
                            "response for unknown request id"
                        )
                    }
                }
            }
            wire::ProtocolFrame::EventFrame(event) => {
                // Transport-level liveness beats from the sidecar. Their arrival
                // already reset the silence watchdog in the reader; they carry no
                // meaning for event subscribers, so drop them here (mirrors the
                // TS client's heartbeat swallow).
                if matches!(
                    &event.payload,
                    wire::EventPayload::StructuredEvent(structured)
                        if structured.name == "heartbeat"
                ) {
                    return;
                }
                let _ = self.event_tx.send(Arc::new(event));
            }
            wire::ProtocolFrame::SidecarRequestFrame(request) => {
                self.dispatch_sidecar_request(request).await
            }
            wire::ProtocolFrame::SidecarResponseFrame(_) | wire::ProtocolFrame::RequestFrame(_) => {
                tracing::warn!("unexpected inbound frame on host transport")
            }
        }
    }

    /// Dispatch a sidecar-initiated request to its registered callback. The callback runs in a
    /// spawned task so long-running host callbacks (tool execution, permission prompts) cannot stall
    /// the reader loop, which must keep draining responses for any requests the callback itself
    /// issues through this transport.
    async fn dispatch_sidecar_request(self: &Arc<Self>, frame: wire::SidecarRequestFrame) {
        let key = sidecar_request_key(&frame.payload);
        let callback = self.callbacks.read(&key, |_, value| value.clone());
        match callback {
            Some(callback) => {
                let transport = Arc::downgrade(self);
                tokio::spawn(async move {
                    match callback(frame.payload, frame.ownership.clone()).await {
                        Ok(payload) => {
                            let response = wire::ProtocolFrame::SidecarResponseFrame(
                                wire::SidecarResponseFrame {
                                    schema: wire::protocol_schema(),
                                    request_id: frame.request_id,
                                    ownership: frame.ownership,
                                    payload,
                                },
                            );
                            // If the transport is gone, the child is being killed; drop the reply.
                            let Some(transport) = transport.upgrade() else {
                                return;
                            };
                            if let Ok(bytes) = transport.encode_wire_frame(&response, None) {
                                let _ = transport.control_writer_tx.send(bytes).await;
                            }
                        }
                        Err(error) => tracing::warn!(?error, key, "sidecar callback failed"),
                    }
                });
            }
            None => tracing::warn!(key, "no callback registered for sidecar request"),
        }
    }

    /// Reject every in-flight request after the transport disconnects.
    fn fail_all_pending(&self) {
        self.pending.clear();
    }

    fn register_pending_request(
        &self,
        request_id: wire::RequestId,
        tx: oneshot::Sender<wire::ResponsePayload>,
    ) -> Result<(), TransportError> {
        let _guard = self.pending_request_lock.lock();
        if pending_request_count(self) >= PENDING_REQUEST_LIMIT {
            return Err(TransportError::Sidecar(format!(
                "sidecar pending request limit exceeded: at most {PENDING_REQUEST_LIMIT} requests can be in flight"
            )));
        }
        let _ = self.pending.insert(request_id, tx);
        Ok(())
    }
}

struct PendingRequestGuard<'a> {
    transport: &'a SidecarTransport,
    request_id: wire::RequestId,
}

impl<'a> PendingRequestGuard<'a> {
    fn new(transport: &'a SidecarTransport, request_id: wire::RequestId) -> Self {
        Self {
            transport,
            request_id,
        }
    }
}

impl Drop for PendingRequestGuard<'_> {
    fn drop(&mut self) {
        let _ = self.transport.pending.remove(&self.request_id);
    }
}

fn pending_request_count(transport: &SidecarTransport) -> usize {
    let mut count = 0;
    transport.pending.scan(|_, _| {
        count += 1;
    });
    count
}

/// Map a sidecar-request payload to the callback registry key.
fn sidecar_request_key(payload: &wire::SidecarRequestPayload) -> &'static str {
    match payload {
        wire::SidecarRequestPayload::HostCallbackRequest(_) => "host_callback",
        wire::SidecarRequestPayload::JsBridgeCallRequest(_) => "js_bridge_call",
        wire::SidecarRequestPayload::ExtEnvelope(_) => "ext",
    }
}

/// Drain outbound channels into the child's stdin. Control responses are preferred so a full request
/// queue cannot starve sidecar-request replies.
async fn run_writer<W>(
    mut stdin: W,
    mut control_rx: mpsc::Receiver<Vec<u8>>,
    mut request_rx: mpsc::Receiver<Vec<u8>>,
) where
    W: AsyncWrite + Unpin,
{
    let mut prefer_control = true;
    loop {
        let (bytes, wrote_control) = if prefer_control {
            tokio::select! {
                biased;
                bytes = control_rx.recv() => match bytes {
                    Some(bytes) => (bytes, true),
                    None => match request_rx.recv().await {
                        Some(bytes) => (bytes, false),
                        None => break,
                    },
                },
                bytes = request_rx.recv() => match bytes {
                    Some(bytes) => (bytes, false),
                    None => match control_rx.recv().await {
                        Some(bytes) => (bytes, true),
                        None => break,
                    },
                },
            }
        } else {
            tokio::select! {
                biased;
                bytes = request_rx.recv() => match bytes {
                    Some(bytes) => (bytes, false),
                    None => match control_rx.recv().await {
                        Some(bytes) => (bytes, true),
                        None => break,
                    },
                },
                bytes = control_rx.recv() => match bytes {
                    Some(bytes) => (bytes, true),
                    None => match request_rx.recv().await {
                        Some(bytes) => (bytes, false),
                        None => break,
                    },
                },
            }
        };
        if stdin.write_all(&bytes).await.is_err() {
            break;
        }
        if stdin.flush().await.is_err() {
            break;
        }
        prefer_control = !wrote_control;
    }
}

/// Read length-prefixed BARE frames from the child's stdout and route them. Holds a `Weak` so the
/// transport can drop (and `kill_on_drop` the child) independently; exits on EOF/read error or once
/// the transport is gone.
async fn run_reader(transport: Weak<SidecarTransport>, mut stdout: ChildStdout) {
    loop {
        let mut length_buf = [0u8; 4];
        if stdout.read_exact(&mut length_buf).await.is_err() {
            break;
        }
        let length = u32::from_be_bytes(length_buf) as usize;

        let Some(transport) = transport.upgrade() else {
            break;
        };
        let max_frame_bytes = transport.max_frame_bytes.load(Ordering::Relaxed);
        if frame_length_exceeds_limit(length, max_frame_bytes) {
            tracing::warn!(
                size = length,
                max = max_frame_bytes,
                "sidecar frame exceeds negotiated limit"
            );
            break;
        }

        let mut frame_bytes = vec![0u8; 4 + length];
        frame_bytes[..4].copy_from_slice(&length_buf);
        if stdout.read_exact(&mut frame_bytes[4..]).await.is_err() {
            break;
        }
        // Any complete inbound frame proves the sidecar is alive; the silence
        // watchdog measures from here.
        *transport.last_inbound_at.lock() = std::time::Instant::now();

        let codec = WireFrameCodec::new(max_frame_bytes);
        match codec.decode(&frame_bytes) {
            Ok(frame) => transport.handle_wire_frame(frame).await,
            Err(error) => tracing::warn!(?error, "failed to decode sidecar frame"),
        }
    }

    if let Some(transport) = transport.upgrade() {
        transport.fail_all_pending();
    }
}

fn frame_length_exceeds_limit(length: usize, max_frame_bytes: usize) -> bool {
    length > max_frame_bytes
}

/// Kill the sidecar and fail all in-flight requests once the transport has seen no inbound frames
/// (not even heartbeats) for `timeout`. A silent sidecar is dead or wedged, not busy: a busy
/// sidecar still heartbeats every 10s from a dedicated thread. Exits when the transport drops.
async fn run_silence_watchdog(transport: Weak<SidecarTransport>, timeout: std::time::Duration) {
    let check_interval = (timeout / 4).min(std::time::Duration::from_secs(1));
    loop {
        tokio::time::sleep(check_interval).await;
        let Some(transport) = transport.upgrade() else {
            return;
        };
        let silence = transport.last_inbound_at.lock().elapsed();
        if silence < timeout {
            continue;
        }
        tracing::error!(
            silence_ms = silence.as_millis() as u64,
            "sidecar unresponsive: no protocol frames or heartbeats; killing sidecar",
        );
        transport.kill_child();
        transport.fail_all_pending();
        return;
    }
}

fn resolve_sidecar_binary_path(binary_path: Option<String>) -> String {
    binary_path
        .or_else(|| std::env::var(SIDECAR_BIN_ENV).ok())
        .unwrap_or_else(|| "agentos-native-sidecar".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn test_transport() -> SidecarTransport {
        let (request_writer_tx, _request_writer_rx) = mpsc::channel(REQUEST_FRAME_QUEUE_CAPACITY);
        let (control_writer_tx, _control_writer_rx) = mpsc::channel(CONTROL_FRAME_QUEUE_CAPACITY);
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        SidecarTransport {
            child: parking_lot::Mutex::new(None),
            pending: SccHashMap::new(),
            pending_request_lock: parking_lot::Mutex::new(()),
            request_counter: AtomicI64::new(1),
            max_frame_bytes: AtomicUsize::new(wire::DEFAULT_MAX_FRAME_BYTES),
            event_tx,
            callbacks: SccHashMap::new(),
            request_writer_tx,
            control_writer_tx,
            last_inbound_at: parking_lot::Mutex::new(std::time::Instant::now()),
        }
    }

    #[test]
    fn binary_path_prefers_explicit_path_over_env() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous = std::env::var(SIDECAR_BIN_ENV).ok();
        std::env::set_var(SIDECAR_BIN_ENV, "/tmp/from-env");

        assert_eq!(
            resolve_sidecar_binary_path(Some("/tmp/from-config".to_string())),
            "/tmp/from-config"
        );

        restore_env(SIDECAR_BIN_ENV, previous);
    }

    #[test]
    fn binary_path_uses_secure_exec_env_fallback() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous = std::env::var(SIDECAR_BIN_ENV).ok();
        std::env::set_var(SIDECAR_BIN_ENV, "/tmp/agentos-native-sidecar");

        assert_eq!(
            resolve_sidecar_binary_path(None),
            "/tmp/agentos-native-sidecar"
        );

        restore_env(SIDECAR_BIN_ENV, previous);
    }

    #[test]
    fn binary_path_defaults_to_agentos_native_sidecar() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous = std::env::var(SIDECAR_BIN_ENV).ok();
        std::env::remove_var(SIDECAR_BIN_ENV);

        assert_eq!(resolve_sidecar_binary_path(None), "agentos-native-sidecar");

        restore_env(SIDECAR_BIN_ENV, previous);
    }

    fn restore_env(key: &str, value: Option<String>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn frame_length_limit_rejects_oversized_declared_length() {
        assert!(!frame_length_exceeds_limit(1024, 1024));
        assert!(frame_length_exceeds_limit(1025, 1024));
    }

    #[test]
    fn transport_encodes_requests_with_generated_wire_codec() {
        let transport = test_transport();
        let frame = wire::ProtocolFrame::RequestFrame(wire::RequestFrame {
            schema: wire::protocol_schema(),
            request_id: 7,
            ownership: wire::OwnershipScope::ConnectionOwnership(wire::ConnectionOwnership {
                connection_id: "conn-1".to_string(),
            }),
            payload: wire::RequestPayload::AuthenticateRequest(wire::AuthenticateRequest {
                client_name: "transport-test".to_string(),
                auth_token: "token".to_string(),
                protocol_version: wire::PROTOCOL_VERSION,
                bridge_version: 1,
            }),
        });

        let encoded = transport
            .encode_wire_frame(&frame, None)
            .expect("encode transport frame");
        let decoded = WireFrameCodec::default()
            .decode(&encoded)
            .expect("decode generated wire frame");

        assert!(matches!(
            decoded,
            wire::ProtocolFrame::RequestFrame(wire::RequestFrame {
                payload: wire::RequestPayload::AuthenticateRequest(_),
                ..
            })
        ));
    }

    #[tokio::test]
    async fn transport_fans_out_generated_wire_events() {
        let transport = Arc::new(test_transport());
        let mut wire_events = transport.subscribe_wire_events();
        let mut second_subscriber = transport.subscribe_wire_events();

        transport
            .handle_wire_frame(wire::ProtocolFrame::EventFrame(wire::EventFrame {
                schema: wire::protocol_schema(),
                ownership: wire::OwnershipScope::VmOwnership(wire::VmOwnership {
                    connection_id: "conn-1".to_string(),
                    session_id: "session-1".to_string(),
                    vm_id: "vm-1".to_string(),
                }),
                payload: wire::EventPayload::ProcessOutputEvent(wire::ProcessOutputEvent {
                    process_id: "proc-1".to_string(),
                    channel: wire::StreamChannel::Stdout,
                    chunk: b"hello".to_vec(),
                }),
            }))
            .await;

        let event = wire_events.recv().await.expect("wire event");
        let second_event = second_subscriber.recv().await.expect("second wire event");
        assert!(
            Arc::ptr_eq(&event, &second_event),
            "fan-out subscribers must share one decoded event frame"
        );
        assert!(matches!(
            &event.ownership,
            wire::OwnershipScope::VmOwnership(wire::VmOwnership {
                connection_id,
                session_id,
                vm_id,
            }) if connection_id == "conn-1" && session_id == "session-1" && vm_id == "vm-1"
        ));
        assert!(matches!(
            &event.payload,
            wire::EventPayload::ProcessOutputEvent(wire::ProcessOutputEvent {
                process_id,
                channel: wire::StreamChannel::Stdout,
                chunk,
            }) if process_id == "proc-1" && chunk.as_slice() == b"hello"
        ));
    }

    #[tokio::test]
    async fn transport_shares_terminal_capture_buffers_across_subscribers() {
        let transport = Arc::new(test_transport());
        let mut first = transport.subscribe_wire_events();
        let mut second = transport.subscribe_wire_events();
        let stdout = vec![b'o'; 64 * 1024];
        let stderr = vec![b'e'; 64 * 1024];

        transport
            .handle_wire_frame(wire::ProtocolFrame::EventFrame(wire::EventFrame {
                schema: wire::protocol_schema(),
                ownership: wire::OwnershipScope::VmOwnership(wire::VmOwnership {
                    connection_id: "conn-1".to_string(),
                    session_id: "session-1".to_string(),
                    vm_id: "vm-1".to_string(),
                }),
                payload: wire::EventPayload::ProcessExitedEvent(wire::ProcessExitedEvent {
                    process_id: "proc-1".to_string(),
                    exit_code: 0,
                    stdout: Some(stdout),
                    stderr: Some(stderr),
                    error: None,
                }),
            }))
            .await;

        let first = first.recv().await.expect("first terminal event");
        let second = second.recv().await.expect("second terminal event");
        assert!(
            Arc::ptr_eq(&first, &second),
            "terminal capture buffers must not be deep-cloned by broadcast fan-out"
        );
    }

    #[tokio::test]
    async fn silence_watchdog_fails_pending_requests_after_sustained_silence() {
        let transport = Arc::new(test_transport());
        let (tx, rx) = oneshot::channel();
        transport
            .register_pending_request(1, tx)
            .expect("register pending request");

        tokio::spawn(run_silence_watchdog(
            Arc::downgrade(&transport),
            std::time::Duration::from_millis(40),
        ));

        // No inbound activity at all: the watchdog must reject the pending
        // request (dropped sender -> disconnected error at the caller).
        rx.await
            .expect_err("watchdog should drop the pending sender");
        assert_eq!(pending_request_count(&transport), 0);
    }

    #[tokio::test]
    async fn silence_watchdog_stays_quiet_while_frames_arrive() {
        let transport = Arc::new(test_transport());
        let (tx, mut rx) = oneshot::channel();
        transport
            .register_pending_request(1, tx)
            .expect("register pending request");

        tokio::spawn(run_silence_watchdog(
            Arc::downgrade(&transport),
            std::time::Duration::from_millis(120),
        ));

        // Simulate steady inbound activity (what the reader does per frame)
        // for well past the silence window; the watchdog must not fire.
        for _ in 0..6 {
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            *transport.last_inbound_at.lock() = std::time::Instant::now();
            assert!(
                rx.try_recv().is_err(),
                "pending request must remain registered while frames arrive"
            );
        }
        assert_eq!(pending_request_count(&transport), 1);
    }

    #[tokio::test]
    async fn heartbeat_events_are_swallowed_before_the_event_fanout() {
        let transport = Arc::new(test_transport());
        let mut wire_events = transport.subscribe_wire_events();

        transport
            .handle_wire_frame(wire::ProtocolFrame::EventFrame(wire::EventFrame {
                schema: wire::protocol_schema(),
                ownership: wire::OwnershipScope::ConnectionOwnership(wire::ConnectionOwnership {
                    connection_id: "sidecar-transport".to_string(),
                }),
                payload: wire::EventPayload::StructuredEvent(wire::StructuredEvent {
                    name: "heartbeat".to_string(),
                    detail: std::collections::HashMap::new(),
                }),
            }))
            .await;
        // A non-heartbeat structured event still fans out, proving the filter
        // is name-scoped rather than dropping all structured events.
        transport
            .handle_wire_frame(wire::ProtocolFrame::EventFrame(wire::EventFrame {
                schema: wire::protocol_schema(),
                ownership: wire::OwnershipScope::ConnectionOwnership(wire::ConnectionOwnership {
                    connection_id: "conn-1".to_string(),
                }),
                payload: wire::EventPayload::StructuredEvent(wire::StructuredEvent {
                    name: "limit_warning".to_string(),
                    detail: std::collections::HashMap::new(),
                }),
            }))
            .await;

        let event = wire_events.recv().await.expect("structured event");
        let payload = &event.payload;
        assert!(matches!(
            payload,
            wire::EventPayload::StructuredEvent(wire::StructuredEvent { name, .. })
                if name == "limit_warning"
        ));
        assert!(
            wire_events.try_recv().is_err(),
            "heartbeat must not fan out"
        );
    }

    #[test]
    fn pending_request_guard_removes_registered_slot_on_drop() {
        let transport = test_transport();
        let (tx, _rx) = oneshot::channel();
        transport
            .register_pending_request(1, tx)
            .expect("register pending request");

        {
            let _guard = PendingRequestGuard::new(&transport, 1);
            assert_eq!(pending_request_count(&transport), 1);
        }

        assert_eq!(pending_request_count(&transport), 0);
    }

    #[test]
    fn pending_request_limit_rejects_full_transport() {
        let transport = test_transport();
        for request_id in 1..=PENDING_REQUEST_LIMIT as wire::RequestId {
            let (tx, _rx) = oneshot::channel();
            transport
                .register_pending_request(request_id, tx)
                .expect("register pending request");
        }
        let (tx, _rx) = oneshot::channel();
        let error = transport
            .register_pending_request((PENDING_REQUEST_LIMIT + 1) as wire::RequestId, tx)
            .expect_err("full pending map should reject");

        assert!(
            error
                .to_string()
                .contains("sidecar pending request limit exceeded"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn writer_prioritizes_control_frames_over_request_backlog() {
        let (client, mut server) = tokio::io::duplex(64);
        let (control_tx, control_rx) = mpsc::channel(CONTROL_FRAME_QUEUE_CAPACITY);
        let (request_tx, request_rx) = mpsc::channel(REQUEST_FRAME_QUEUE_CAPACITY);
        request_tx
            .send(vec![b'r'])
            .await
            .expect("send request frame");
        control_tx
            .send(vec![b'c'])
            .await
            .expect("send control frame");
        drop(control_tx);
        drop(request_tx);

        let writer = tokio::spawn(run_writer(client, control_rx, request_rx));
        let mut first = [0u8; 1];
        server
            .read_exact(&mut first)
            .await
            .expect("read first byte");
        writer.await.expect("writer task");

        assert_eq!(first, [b'c']);
    }

    #[tokio::test]
    async fn writer_alternates_when_control_and_request_are_ready() {
        let (client, mut server) = tokio::io::duplex(64);
        let (control_tx, control_rx) = mpsc::channel(CONTROL_FRAME_QUEUE_CAPACITY);
        let (request_tx, request_rx) = mpsc::channel(REQUEST_FRAME_QUEUE_CAPACITY);
        control_tx.send(vec![b'c']).await.expect("control one");
        control_tx.send(vec![b'C']).await.expect("control two");
        request_tx.send(vec![b'r']).await.expect("request one");
        request_tx.send(vec![b'R']).await.expect("request two");
        drop(control_tx);
        drop(request_tx);

        let writer = tokio::spawn(run_writer(client, control_rx, request_rx));
        let mut output = [0u8; 4];
        server.read_exact(&mut output).await.expect("read output");
        writer.await.expect("writer task");

        assert_eq!(output, [b'c', b'r', b'C', b'R']);
    }
}
