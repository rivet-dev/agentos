//! `SidecarTransport`: spawns the native `agent-os-sidecar` binary and speaks the existing framed
//! BARE protocol over its stdio.
//!
//! This mirrors the TypeScript `NativeSidecarProcessClient`. It REUSES `agent_os_sidecar::protocol`
//! and defines NO wire types. Framing: 4-byte big-endian length prefix via
//! [`protocol::NativeFrameCodec`], payload codec pinned to [`protocol::NativePayloadCodec::Bare`].
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

use agent_os_sidecar::protocol::{
    self, DEFAULT_MAX_FRAME_BYTES, EventPayload, NativeFrameCodec, NativePayloadCodec,
    OwnershipScope, ProtocolFrame, RequestFrame, RequestPayload, ResponsePayload,
    SidecarRequestFrame, SidecarRequestPayload, SidecarResponseFrame, SidecarResponsePayload,
};

use crate::error::ClientError;

/// Broadcast capacity for the structured/lifecycle/process event fan-out.
const EVENT_CHANNEL_CAPACITY: usize = 4096;

/// Maximum outbound frames buffered while the writer task drains to sidecar stdin.
const REQUEST_FRAME_QUEUE_CAPACITY: usize = 4096;

/// Maximum callback/control response frames buffered ahead of regular host requests.
const CONTROL_FRAME_QUEUE_CAPACITY: usize = 1024;

/// Maximum in-flight host-initiated sidecar requests per transport.
const PENDING_REQUEST_LIMIT: usize = 4096;

/// Env var that overrides the sidecar binary path. Defaults to `agent-os-sidecar` on `PATH`. Tests
/// point this at the freshly built binary.
const SIDECAR_BIN_ENV: &str = "AGENT_OS_SIDECAR_BIN";

/// A registered callback that answers a sidecar-initiated request.
pub(crate) type SidecarCallback = Arc<
    dyn Fn(
            SidecarRequestPayload,
            OwnershipScope,
        )
            -> futures::future::BoxFuture<'static, Result<SidecarResponsePayload, ClientError>>
        + Send
        + Sync,
>;

/// Owns the spawned sidecar child, the framed BARE stdio I/O tasks, the pending-response map, the
/// event fan-out, and the callback dispatch table.
pub struct SidecarTransport {
    /// The spawned sidecar process (stdout/stdin taken by the I/O tasks; kept for kill on drop).
    pub(crate) child: parking_lot::Mutex<Option<Child>>,
    /// Pending host-initiated requests, keyed by positive `RequestId`.
    pub(crate) pending: SccHashMap<protocol::RequestId, oneshot::Sender<ResponsePayload>>,
    pub(crate) pending_request_lock: parking_lot::Mutex<()>,
    /// Host request-id counter (positive, starts at 1).
    pub(crate) request_counter: AtomicI64,
    /// Negotiated max frame size.
    pub(crate) max_frame_bytes: AtomicUsize,
    /// Structured-event fan-out for `Event` frames.
    pub(crate) event_tx: broadcast::Sender<(OwnershipScope, EventPayload)>,
    /// Registered host callbacks for `SidecarRequest` frames (tools, permissions, ACP, JS-bridge).
    pub(crate) callbacks: SccHashMap<&'static str, SidecarCallback>,
    /// Outbound host request frames drained by the writer task into the child's stdin.
    pub(crate) request_writer_tx: mpsc::Sender<Vec<u8>>,
    /// Outbound callback/control response frames. The writer drains this before regular requests.
    pub(crate) control_writer_tx: mpsc::Sender<Vec<u8>>,
}

impl SidecarTransport {
    /// Spawn the native `agent-os-sidecar` binary and start the stdio I/O tasks.
    ///
    /// Does NOT run the handshake; `AgentOs::create` drives Authenticate -> OpenSession -> CreateVm ->
    /// ConfigureVm using [`request`](Self::request) once the transport is live.
    pub(crate) async fn spawn(binary_path: Option<String>) -> Result<Arc<Self>, ClientError> {
        // Prefer the typed path threaded from `AgentOsConfig` (resolved from the
        // npm package on the TypeScript side), mirroring how rivetkit threads
        // `engine_binary_path` into `Command::new`. The `AGENT_OS_SIDECAR_BIN`
        // env var stays only as a debug/override fallback.
        let bin = binary_path
            .or_else(|| std::env::var(SIDECAR_BIN_ENV).ok())
            .unwrap_or_else(|| "agent-os-sidecar".to_string());
        let mut child = Command::new(&bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                ClientError::Sidecar(format!("failed to spawn sidecar '{bin}': {error}"))
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ClientError::Sidecar("sidecar stdin was not piped".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ClientError::Sidecar("sidecar stdout was not piped".to_string()))?;

        let (request_writer_tx, request_writer_rx) = mpsc::channel(REQUEST_FRAME_QUEUE_CAPACITY);
        let (control_writer_tx, control_writer_rx) = mpsc::channel(CONTROL_FRAME_QUEUE_CAPACITY);
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        let transport = Arc::new(Self {
            child: parking_lot::Mutex::new(Some(child)),
            pending: SccHashMap::new(),
            pending_request_lock: parking_lot::Mutex::new(()),
            request_counter: AtomicI64::new(1),
            max_frame_bytes: AtomicUsize::new(DEFAULT_MAX_FRAME_BYTES),
            event_tx,
            callbacks: SccHashMap::new(),
            request_writer_tx,
            control_writer_tx,
        });

        tokio::spawn(run_writer(stdin, control_writer_rx, request_writer_rx));
        tokio::spawn(run_reader(Arc::downgrade(&transport), stdout));

        Ok(transport)
    }

    /// Allocate the next positive host request id.
    pub(crate) fn next_request_id(&self) -> protocol::RequestId {
        self.request_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Issue a host request and await its response payload.
    pub(crate) async fn request(
        &self,
        ownership: OwnershipScope,
        payload: RequestPayload,
    ) -> Result<ResponsePayload, ClientError> {
        self.request_with_frame_limit(ownership, payload, None)
            .await
    }

    /// Issue a host request using a caller-specific frame limit no larger than the negotiated
    /// transport limit. This is used by fully buffered APIs that need a stricter per-operation cap.
    pub(crate) async fn request_bounded(
        &self,
        ownership: OwnershipScope,
        payload: RequestPayload,
        max_frame_bytes: usize,
    ) -> Result<ResponsePayload, ClientError> {
        self.request_with_frame_limit(ownership, payload, Some(max_frame_bytes))
            .await
    }

    async fn request_with_frame_limit(
        &self,
        ownership: OwnershipScope,
        payload: RequestPayload,
        max_frame_bytes: Option<usize>,
    ) -> Result<ResponsePayload, ClientError> {
        let request_id = self.next_request_id();
        let frame = ProtocolFrame::Request(RequestFrame::new(request_id, ownership, payload));
        let bytes = self.encode_frame(&frame, max_frame_bytes)?;

        let (tx, rx) = oneshot::channel();
        self.register_pending_request(request_id, tx)?;
        let _pending_guard = PendingRequestGuard::new(self, request_id);

        if self.request_writer_tx.send(bytes).await.is_err() {
            self.pending.remove(&request_id);
            return Err(ClientError::Sidecar("sidecar transport closed".to_string()));
        }

        rx.await
            .map_err(|_| ClientError::Sidecar("sidecar transport disconnected".to_string()))
    }

    /// Subscribe to structured/lifecycle/process events.
    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<(OwnershipScope, EventPayload)> {
        self.event_tx.subscribe()
    }

    /// Register a callback that answers a class of sidecar-initiated requests.
    pub(crate) fn register_callback(&self, key: &'static str, callback: SidecarCallback) {
        let _ = self.callbacks.insert(key, callback);
    }

    fn encode_frame(
        &self,
        frame: &ProtocolFrame,
        max_frame_bytes: Option<usize>,
    ) -> Result<Vec<u8>, ClientError> {
        let transport_limit = self.max_frame_bytes.load(Ordering::Relaxed);
        let max_frame_bytes = max_frame_bytes
            .map(|limit| limit.min(transport_limit))
            .unwrap_or(transport_limit);
        let codec = NativeFrameCodec::with_payload_codec(max_frame_bytes, NativePayloadCodec::Bare);
        Ok(codec.encode(frame)?)
    }

    /// Route a decoded inbound frame. Host transports only legitimately receive `Response`, `Event`,
    /// and `SidecarRequest` frames.
    async fn handle_frame(self: &Arc<Self>, frame: ProtocolFrame) {
        match frame {
            ProtocolFrame::Response(response) => match self.pending.remove(&response.request_id) {
                Some((_, tx)) => {
                    let _ = tx.send(response.payload);
                }
                None => {
                    tracing::warn!(
                        request_id = response.request_id,
                        "response for unknown request id"
                    )
                }
            },
            ProtocolFrame::Event(event) => {
                let _ = self.event_tx.send((event.ownership, event.payload));
            }
            ProtocolFrame::SidecarRequest(request) => self.dispatch_sidecar_request(request).await,
            ProtocolFrame::SidecarResponse(_) | ProtocolFrame::Request(_) => {
                tracing::warn!("unexpected inbound frame on host transport")
            }
        }
    }

    /// Dispatch a sidecar-initiated request to its registered callback. The callback runs in a
    /// spawned task so long-running host callbacks (tool execution, permission prompts) cannot stall
    /// the reader loop, which must keep draining responses for any requests the callback itself
    /// issues through this transport.
    async fn dispatch_sidecar_request(self: &Arc<Self>, frame: SidecarRequestFrame) {
        let key = sidecar_request_key(&frame.payload);
        let callback = self.callbacks.read(&key, |_, value| value.clone());
        match callback {
            Some(callback) => {
                let transport = Arc::downgrade(self);
                tokio::spawn(async move {
                    match callback(frame.payload, frame.ownership.clone()).await {
                        Ok(payload) => {
                            let response =
                                ProtocolFrame::SidecarResponse(SidecarResponseFrame::new(
                                    frame.request_id,
                                    frame.ownership,
                                    payload,
                                ));
                            // If the transport is gone, the child is being killed; drop the reply.
                            let Some(transport) = transport.upgrade() else {
                                return;
                            };
                            if let Ok(bytes) = transport.encode_frame(&response, None) {
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

    /// Reject every in-flight request after the transport disconnects (dropping the senders makes
    /// each `request` await resolve to a disconnect error).
    fn fail_all_pending(&self) {
        self.pending.clear();
    }

    fn register_pending_request(
        &self,
        request_id: protocol::RequestId,
        tx: oneshot::Sender<ResponsePayload>,
    ) -> Result<(), ClientError> {
        let _guard = self.pending_request_lock.lock();
        if pending_request_count(self) >= PENDING_REQUEST_LIMIT {
            return Err(ClientError::Sidecar(format!(
                "sidecar pending request limit exceeded: at most {PENDING_REQUEST_LIMIT} requests can be in flight"
            )));
        }
        let _ = self.pending.insert(request_id, tx);
        Ok(())
    }
}

struct PendingRequestGuard<'a> {
    transport: &'a SidecarTransport,
    request_id: protocol::RequestId,
}

impl<'a> PendingRequestGuard<'a> {
    fn new(transport: &'a SidecarTransport, request_id: protocol::RequestId) -> Self {
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
fn sidecar_request_key(payload: &SidecarRequestPayload) -> &'static str {
    match payload {
        SidecarRequestPayload::ToolInvocation(_) => "tool_invocation",
        SidecarRequestPayload::PermissionRequest(_) => "permission_request",
        SidecarRequestPayload::AcpRequest(_) => "acp_request",
        SidecarRequestPayload::JsBridgeCall(_) => "js_bridge_call",
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

        let codec = NativeFrameCodec::with_payload_codec(max_frame_bytes, NativePayloadCodec::Bare);
        match codec.decode(&frame_bytes) {
            Ok(frame) => transport.handle_frame(frame).await,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_transport() -> SidecarTransport {
        let (request_writer_tx, _request_writer_rx) = mpsc::channel(REQUEST_FRAME_QUEUE_CAPACITY);
        let (control_writer_tx, _control_writer_rx) = mpsc::channel(CONTROL_FRAME_QUEUE_CAPACITY);
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        SidecarTransport {
            child: parking_lot::Mutex::new(None),
            pending: SccHashMap::new(),
            pending_request_lock: parking_lot::Mutex::new(()),
            request_counter: AtomicI64::new(1),
            max_frame_bytes: AtomicUsize::new(DEFAULT_MAX_FRAME_BYTES),
            event_tx,
            callbacks: SccHashMap::new(),
            request_writer_tx,
            control_writer_tx,
        }
    }

    #[test]
    fn frame_length_limit_rejects_oversized_declared_length() {
        assert!(!frame_length_exceeds_limit(1024, 1024));
        assert!(frame_length_exceeds_limit(1025, 1024));
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
        for request_id in 1..=PENDING_REQUEST_LIMIT as protocol::RequestId {
            let (tx, _rx) = oneshot::channel();
            transport
                .register_pending_request(request_id, tx)
                .expect("register pending request");
        }
        let (tx, _rx) = oneshot::channel();
        let error = transport
            .register_pending_request((PENDING_REQUEST_LIMIT + 1) as protocol::RequestId, tx)
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
