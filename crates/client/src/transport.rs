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

use std::sync::atomic::{AtomicI64, AtomicUsize};
use std::sync::Arc;

use scc::HashMap as SccHashMap;
use tokio::process::Child;
use tokio::sync::{broadcast, oneshot};

use agent_os_sidecar::protocol::{
    self, EventPayload, OwnershipScope, RequestPayload, ResponsePayload, SidecarRequestPayload,
    SidecarResponsePayload,
};

use crate::error::ClientError;

/// A registered callback that answers a sidecar-initiated request.
pub(crate) type SidecarCallback = Arc<
    dyn Fn(
            SidecarRequestPayload,
            OwnershipScope,
        ) -> futures::future::BoxFuture<'static, Result<SidecarResponsePayload, ClientError>>
        + Send
        + Sync,
>;

/// Owns the spawned sidecar child, the framed BARE stdio I/O tasks, the pending-response map, the
/// event fan-out, and the callback dispatch table.
pub struct SidecarTransport {
    /// The spawned sidecar process.
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
}

impl SidecarTransport {
    /// Spawn the native `agent-os-sidecar` binary and start the stdio I/O tasks.
    ///
    /// TODO(parity: spawn child, start reader/writer tasks, demux frames, run handshake).
    pub(crate) async fn spawn() -> Result<Arc<Self>, ClientError> {
        todo!("parity: SidecarTransport::spawn")
    }

    /// Allocate the next positive host request id.
    pub(crate) fn next_request_id(&self) -> protocol::RequestId {
        self.request_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Allocate the next negative sidecar-callback request id.
    pub(crate) fn next_sidecar_request_id(&self) -> protocol::RequestId {
        self.sidecar_request_counter
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Issue a host request and await its response payload.
    ///
    /// TODO(parity: encode RequestFrame, register pending slot, write, await oneshot).
    pub(crate) async fn request(
        &self,
        _ownership: OwnershipScope,
        _payload: RequestPayload,
    ) -> Result<ResponsePayload, ClientError> {
        todo!("parity: SidecarTransport::request")
    }

    /// Subscribe to structured/lifecycle/process events.
    pub(crate) fn subscribe_events(
        &self,
    ) -> broadcast::Receiver<(OwnershipScope, EventPayload)> {
        self.event_tx.subscribe()
    }
}
