//! `SidecarTransport`: spawns the native `agent-os-sidecar` binary and speaks the existing framed
//! BARE protocol over its stdio.
//!
//! This mirrors the TypeScript `NativeSidecarProcessClient`. It REUSES `agent_os_sidecar::protocol`
//! and defines NO wire types. Framing: 4-byte big-endian length prefix via
//! [`protocol::NativeFrameCodec`], payload codec pinned to [`protocol::NativePayloadCodec::Bare`].
//!
//! Request-id direction is load-bearing: host-initiated `Request`/`Response` frames use POSITIVE ids
//! (counter starts at 1, increments); sidecar-initiated `SidecarRequest`/`SidecarResponse` callbacks
//! use NEGATIVE ids (counter starts at -1, decrements).

use std::process::Stdio;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

use scc::HashMap as SccHashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{broadcast, mpsc, oneshot};

use agent_os_sidecar::protocol::{
    self, EventPayload, NativeFrameCodec, NativePayloadCodec, OwnershipScope, ProtocolFrame,
    RequestFrame, RequestPayload, ResponsePayload, SidecarRequestFrame, SidecarRequestPayload,
    SidecarResponseFrame, SidecarResponsePayload, DEFAULT_MAX_FRAME_BYTES,
};

use crate::error::ClientError;

/// Broadcast capacity for the structured/lifecycle/process event fan-out.
const EVENT_CHANNEL_CAPACITY: usize = 4096;

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
    /// Host request-id counter (positive, starts at 1).
    pub(crate) request_counter: AtomicI64,
    /// Sidecar callback request-id counter (negative, starts at -1).
    pub(crate) sidecar_request_counter: AtomicI64,
    /// Negotiated max frame size.
    pub(crate) max_frame_bytes: AtomicUsize,
    /// Structured-event fan-out for `Event` frames.
    pub(crate) event_tx: broadcast::Sender<(OwnershipScope, EventPayload)>,
    /// Registered host callbacks for `SidecarRequest` frames (tools, permissions, ACP, JS-bridge).
    pub(crate) callbacks: SccHashMap<&'static str, SidecarCallback>,
    /// Outbound framed-bytes channel drained by the writer task into the child's stdin.
    pub(crate) writer_tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl SidecarTransport {
    /// Spawn the native `agent-os-sidecar` binary and start the stdio I/O tasks.
    ///
    /// Does NOT run the handshake; `AgentOs::create` drives Authenticate -> OpenSession -> CreateVm ->
    /// ConfigureVm using [`request`](Self::request) once the transport is live.
    pub(crate) async fn spawn() -> Result<Arc<Self>, ClientError> {
        let bin = std::env::var(SIDECAR_BIN_ENV).unwrap_or_else(|_| "agent-os-sidecar".to_string());
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

        let (writer_tx, writer_rx) = mpsc::unbounded_channel();
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        let transport = Arc::new(Self {
            child: parking_lot::Mutex::new(Some(child)),
            pending: SccHashMap::new(),
            request_counter: AtomicI64::new(1),
            sidecar_request_counter: AtomicI64::new(-1),
            max_frame_bytes: AtomicUsize::new(DEFAULT_MAX_FRAME_BYTES),
            event_tx,
            callbacks: SccHashMap::new(),
            writer_tx,
        });

        tokio::spawn(run_writer(stdin, writer_rx));
        tokio::spawn(run_reader(Arc::downgrade(&transport), stdout));

        Ok(transport)
    }

    /// Allocate the next positive host request id.
    pub(crate) fn next_request_id(&self) -> protocol::RequestId {
        self.request_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Allocate the next negative sidecar-callback request id.
    pub(crate) fn next_sidecar_request_id(&self) -> protocol::RequestId {
        self.sidecar_request_counter.fetch_sub(1, Ordering::SeqCst)
    }

    /// Issue a host request and await its response payload.
    pub(crate) async fn request(
        &self,
        ownership: OwnershipScope,
        payload: RequestPayload,
    ) -> Result<ResponsePayload, ClientError> {
        let request_id = self.next_request_id();
        let frame = ProtocolFrame::Request(RequestFrame::new(request_id, ownership, payload));
        let bytes = self.encode_frame(&frame)?;

        let (tx, rx) = oneshot::channel();
        let _ = self.pending.insert(request_id, tx);

        if self.writer_tx.send(bytes).is_err() {
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

    fn encode_frame(&self, frame: &ProtocolFrame) -> Result<Vec<u8>, ClientError> {
        let codec = NativeFrameCodec::with_payload_codec(
            self.max_frame_bytes.load(Ordering::Relaxed),
            NativePayloadCodec::Bare,
        );
        Ok(codec.encode(frame)?)
    }

    /// Route a decoded inbound frame. Host transports only legitimately receive `Response`, `Event`,
    /// and `SidecarRequest` frames.
    async fn handle_frame(&self, frame: ProtocolFrame) {
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

    async fn dispatch_sidecar_request(&self, frame: SidecarRequestFrame) {
        let key = sidecar_request_key(&frame.payload);
        let callback = self.callbacks.read(&key, |_, value| value.clone());
        match callback {
            Some(callback) => match callback(frame.payload, frame.ownership.clone()).await {
                Ok(payload) => {
                    let response = ProtocolFrame::SidecarResponse(SidecarResponseFrame::new(
                        frame.request_id,
                        frame.ownership,
                        payload,
                    ));
                    if let Ok(bytes) = self.encode_frame(&response) {
                        let _ = self.writer_tx.send(bytes);
                    }
                }
                Err(error) => tracing::warn!(?error, key, "sidecar callback failed"),
            },
            None => tracing::warn!(key, "no callback registered for sidecar request"),
        }
    }

    /// Reject every in-flight request after the transport disconnects (dropping the senders makes
    /// each `request` await resolve to a disconnect error).
    fn fail_all_pending(&self) {
        self.pending.clear();
    }
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

/// Drain the outbound channel into the child's stdin. Exits when the channel closes (transport
/// dropped) or a write fails (child gone).
async fn run_writer(mut stdin: ChildStdin, mut writer_rx: mpsc::UnboundedReceiver<Vec<u8>>) {
    while let Some(bytes) = writer_rx.recv().await {
        if stdin.write_all(&bytes).await.is_err() {
            break;
        }
        if stdin.flush().await.is_err() {
            break;
        }
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

        let mut frame_bytes = vec![0u8; 4 + length];
        frame_bytes[..4].copy_from_slice(&length_buf);
        if stdout.read_exact(&mut frame_bytes[4..]).await.is_err() {
            break;
        }

        let Some(transport) = transport.upgrade() else {
            break;
        };
        let codec = NativeFrameCodec::with_payload_codec(
            transport.max_frame_bytes.load(Ordering::Relaxed),
            NativePayloadCodec::Bare,
        );
        match codec.decode(&frame_bytes) {
            Ok(frame) => transport.handle_frame(frame).await,
            Err(error) => tracing::warn!(?error, "failed to decode sidecar frame"),
        }
    }

    if let Some(transport) = transport.upgrade() {
        transport.fail_all_pending();
    }
}
