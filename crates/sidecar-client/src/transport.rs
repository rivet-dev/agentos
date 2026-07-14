//! `SidecarTransport`: spawns a native sidecar binary and speaks the existing framed
//! BARE protocol over its stdio.
//!
//! This mirrors the TypeScript `Sidecar`. Generated wire payloads are the native
//! transport path.
//!
//! Request-id direction is load-bearing: host-initiated `Request`/`Response` frames use positive ids
//! allocated by this transport, while sidecar-initiated `SidecarRequest`/`SidecarResponse` callbacks
//! echo the id allocated by the sidecar.

use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

use scc::HashMap as SccHashMap;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, watch};

use crate::wire::{self, WireFrameCodec};
use crate::TransportError;

/// Count backstop for the shared decoded-event log. The byte bound below is the primary memory
/// envelope; this separately bounds a flood of tiny events.
const EVENT_CHANNEL_CAPACITY: usize = 4096;

/// Conservative non-payload allocation allowance for one decoded event and its queue entry.
const EVENT_ENTRY_OVERHEAD_BYTES: usize = 512;

/// Maximum serialized bytes retained by the shared decoded-event log. One negotiated maximum-size
/// frame plus its length prefix and decoded-entry overhead must fit; older events are evicted before
/// this bound can be exceeded by a second frame.
const DEFAULT_EVENT_CHANNEL_MAX_RETAINED_BYTES: usize =
    wire::DEFAULT_MAX_FRAME_BYTES + std::mem::size_of::<u32>() + EVENT_ENTRY_OVERHEAD_BYTES;

fn event_log_byte_limit(max_frame_bytes: usize) -> usize {
    max_frame_bytes
        .saturating_add(std::mem::size_of::<u32>())
        .saturating_add(EVENT_ENTRY_OVERHEAD_BYTES)
}

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum OwnershipKey {
    Connection(String),
    Session(String, String),
    Vm(String, String, String),
}

impl From<&wire::OwnershipScope> for OwnershipKey {
    fn from(ownership: &wire::OwnershipScope) -> Self {
        match ownership {
            wire::OwnershipScope::ConnectionOwnership(value) => {
                Self::Connection(value.connection_id.clone())
            }
            wire::OwnershipScope::SessionOwnership(value) => {
                Self::Session(value.connection_id.clone(), value.session_id.clone())
            }
            wire::OwnershipScope::VmOwnership(value) => Self::Vm(
                value.connection_id.clone(),
                value.session_id.clone(),
                value.vm_id.clone(),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum WireEventRoute {
    Process {
        ownership: OwnershipKey,
        process_id: String,
    },
    Control(OwnershipKey),
}

/// Typed failure returned when a bounded wire-event subscription cannot deliver a contiguous
/// stream. Callers must surface `Lagged` rather than silently waiting for a terminal event that may
/// have been evicted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WireEventRecvError {
    #[error("wire event stream lagged and skipped {skipped} event(s)")]
    Lagged { skipped: u64 },
    #[error("wire event stream closed")]
    Closed,
}

#[derive(Debug)]
struct RetainedWireEvent {
    sequence: u64,
    route_sequence: u64,
    retained_bytes: usize,
    route: WireEventRoute,
    event: SharedWireEvent,
}

#[derive(Debug)]
struct WireEventLogState {
    entries: VecDeque<RetainedWireEvent>,
    next_sequence: u64,
    retained_bytes: usize,
    active_routes: HashMap<WireEventRoute, usize>,
    next_route_sequence: HashMap<WireEventRoute, u64>,
    near_byte_limit_warned: bool,
    near_count_limit_warned: bool,
    closed: bool,
}

#[derive(Debug)]
struct WireEventLog {
    state: parking_lot::Mutex<WireEventLogState>,
    retained_byte_limit: AtomicUsize,
    signal_tx: watch::Sender<u64>,
    signal_revision: AtomicU64,
}

impl WireEventLog {
    fn new() -> Self {
        let (signal_tx, _) = watch::channel(0);
        Self {
            state: parking_lot::Mutex::new(WireEventLogState {
                entries: VecDeque::new(),
                next_sequence: 0,
                retained_bytes: 0,
                active_routes: HashMap::new(),
                next_route_sequence: HashMap::new(),
                near_byte_limit_warned: false,
                near_count_limit_warned: false,
                closed: false,
            }),
            retained_byte_limit: AtomicUsize::new(DEFAULT_EVENT_CHANNEL_MAX_RETAINED_BYTES),
            signal_tx,
            signal_revision: AtomicU64::new(0),
        }
    }

    fn subscribe(self: &Arc<Self>, route: WireEventRoute) -> WireEventSubscription {
        let mut state = self.state.lock();
        let next_sequence = state.next_sequence;
        let next_route_sequence = state.next_route_sequence.get(&route).copied().unwrap_or(0);
        *state.active_routes.entry(route.clone()).or_insert(0) += 1;
        drop(state);
        WireEventSubscription {
            log: Arc::clone(self),
            next_sequence,
            route: Some(route),
            next_route_sequence,
            signal_rx: self.signal_tx.subscribe(),
        }
    }

    fn subscribe_provisional(self: &Arc<Self>) -> WireEventSubscription {
        let next_sequence = self.state.lock().next_sequence;
        WireEventSubscription {
            log: Arc::clone(self),
            next_sequence,
            route: None,
            next_route_sequence: 0,
            signal_rx: self.signal_tx.subscribe(),
        }
    }

    fn bind_route(&self, subscription: &mut WireEventSubscription, route: WireEventRoute) {
        debug_assert!(subscription.route.is_none());
        let mut state = self.state.lock();
        subscription.next_sequence = state.next_sequence;
        subscription.next_route_sequence =
            state.next_route_sequence.get(&route).copied().unwrap_or(0);
        *state.active_routes.entry(route.clone()).or_insert(0) += 1;
        subscription.route = Some(route);
        drop(state);
        self.signal();
    }

    fn publish(&self, event: wire::EventFrame, retained_bytes: usize, route: WireEventRoute) {
        let byte_limit = self.retained_byte_limit.load(Ordering::Acquire);
        let mut state = self.state.lock();
        if state.closed || !state.active_routes.contains_key(&route) {
            return;
        }
        let sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.saturating_add(1);
        let route_sequence = *state.next_route_sequence.get(&route).unwrap_or(&0);
        state
            .next_route_sequence
            .insert(route.clone(), route_sequence.saturating_add(1));
        let retained_bytes = retained_bytes.max(EVENT_ENTRY_OVERHEAD_BYTES);
        state.retained_bytes = state.retained_bytes.saturating_add(retained_bytes);
        state.entries.push_back(RetainedWireEvent {
            sequence,
            route_sequence,
            retained_bytes,
            route,
            event: Arc::new(event),
        });

        if !state.near_byte_limit_warned
            && state.retained_bytes.saturating_mul(100) >= byte_limit.saturating_mul(80)
        {
            state.near_byte_limit_warned = true;
            tracing::warn!(
                retained_bytes = state.retained_bytes,
                byte_limit,
                "wire event retention is nearing its negotiated byte limit"
            );
        }
        if !state.near_count_limit_warned
            && state.entries.len().saturating_mul(100) >= EVENT_CHANNEL_CAPACITY.saturating_mul(80)
        {
            state.near_count_limit_warned = true;
            tracing::warn!(
                retained_events = state.entries.len(),
                count_limit = EVENT_CHANNEL_CAPACITY,
                "wire event retention is nearing its count limit"
            );
        }

        let mut evicted = 0u64;
        while !state.entries.is_empty()
            && (state.entries.len() > EVENT_CHANNEL_CAPACITY || state.retained_bytes > byte_limit)
        {
            if let Some(removed) = state.entries.pop_front() {
                state.retained_bytes = state.retained_bytes.saturating_sub(removed.retained_bytes);
                evicted = evicted.saturating_add(1);
            }
        }
        if evicted > 0 {
            tracing::warn!(
                evicted,
                retained_events = state.entries.len(),
                retained_bytes = state.retained_bytes,
                byte_limit,
                count_limit = EVENT_CHANNEL_CAPACITY,
                "bounded wire event log evicted events; lagging subscribers will receive a typed error"
            );
        }
        if state.retained_bytes.saturating_mul(100) < byte_limit.saturating_mul(70) {
            state.near_byte_limit_warned = false;
        }
        if state.entries.len().saturating_mul(100) < EVENT_CHANNEL_CAPACITY.saturating_mul(70) {
            state.near_count_limit_warned = false;
        }
        drop(state);
        self.signal();
    }

    fn set_max_frame_bytes(&self, max_frame_bytes: usize) {
        let byte_limit = event_log_byte_limit(max_frame_bytes);
        self.retained_byte_limit
            .store(byte_limit, Ordering::Release);
        let mut state = self.state.lock();
        let mut evicted = 0u64;
        while !state.entries.is_empty()
            && (state.entries.len() > EVENT_CHANNEL_CAPACITY || state.retained_bytes > byte_limit)
        {
            if let Some(removed) = state.entries.pop_front() {
                state.retained_bytes = state.retained_bytes.saturating_sub(removed.retained_bytes);
                evicted = evicted.saturating_add(1);
            }
        }
        if evicted > 0 {
            tracing::warn!(
                evicted,
                retained_events = state.entries.len(),
                retained_bytes = state.retained_bytes,
                byte_limit,
                count_limit = EVENT_CHANNEL_CAPACITY,
                "lower negotiated frame limit evicted wire events; lagging subscribers will receive a typed error"
            );
        }
        drop(state);
        self.signal();
    }

    fn close(&self) {
        self.state.lock().closed = true;
        self.signal();
    }

    fn unsubscribe(&self, route: &WireEventRoute) {
        let mut state = self.state.lock();
        let Some(count) = state.active_routes.get_mut(route) else {
            return;
        };
        *count -= 1;
        if *count > 0 {
            return;
        }
        state.active_routes.remove(route);
        state.next_route_sequence.remove(route);
        let mut retained_bytes = state.retained_bytes;
        state.entries.retain(|entry| {
            if &entry.route == route {
                retained_bytes = retained_bytes.saturating_sub(entry.retained_bytes);
                false
            } else {
                true
            }
        });
        state.retained_bytes = retained_bytes;
        let byte_limit = self.retained_byte_limit.load(Ordering::Acquire);
        if state.retained_bytes.saturating_mul(100) < byte_limit.saturating_mul(70) {
            state.near_byte_limit_warned = false;
        }
        if state.entries.len().saturating_mul(100) < EVENT_CHANNEL_CAPACITY.saturating_mul(70) {
            state.near_count_limit_warned = false;
        }
    }

    fn signal(&self) {
        let revision = self
            .signal_revision
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        self.signal_tx.send_replace(revision);
    }

    #[cfg(test)]
    fn retained_usage(&self) -> (usize, usize) {
        let state = self.state.lock();
        (state.entries.len(), state.retained_bytes)
    }
}

/// A cursor into the transport's single byte- and count-bounded decoded-event log. An optional full
/// ownership scope is applied before delivery so two VMs may safely reuse the same process id.
pub struct WireEventSubscription {
    log: Arc<WireEventLog>,
    next_sequence: u64,
    route: Option<WireEventRoute>,
    next_route_sequence: u64,
    signal_rx: watch::Receiver<u64>,
}

struct PendingWireRequest {
    tx: oneshot::Sender<PendingWireResponse>,
    process_events: Option<WireEventSubscription>,
    /// One bounded, request-scoped action that must happen after decoding the response but before
    /// the waiter wakes or the reader can dispatch a following event frame. Kept wire-generic so
    /// product clients can atomically bind their own host correlation without teaching this crate
    /// about extension protocols such as ACP.
    response_hook: Option<WireResponseHook>,
}

struct PendingWireResponse {
    payload: wire::ResponsePayload,
    process_events: Option<WireEventSubscription>,
    cancel_cleanup: Option<CancelledProcessCleanup>,
    response_hook_error: Option<TransportError>,
}

type WireResponseHook =
    Box<dyn FnOnce(&wire::ResponsePayload) -> Result<(), TransportError> + Send + Sync + 'static>;

struct CancelledProcessCleanup {
    details: Option<CancelledProcessCleanupDetails>,
}

struct CancelledProcessCleanupDetails {
    transport: Weak<SidecarTransport>,
    ownership: wire::OwnershipScope,
    process_id: String,
}

impl CancelledProcessCleanup {
    fn new(
        transport: &Arc<SidecarTransport>,
        ownership: wire::OwnershipScope,
        process_id: String,
    ) -> Self {
        Self {
            details: Some(CancelledProcessCleanupDetails {
                transport: Arc::downgrade(transport),
                ownership,
                process_id,
            }),
        }
    }

    fn disarm(&mut self) {
        self.details = None;
    }
}

impl Drop for CancelledProcessCleanup {
    fn drop(&mut self) {
        let Some(details) = self.details.take() else {
            return;
        };
        let Some(transport) = details.transport.upgrade() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::error!(
                process_id = details.process_id,
                "cannot schedule cleanup for cancelled Execute outside a Tokio runtime"
            );
            return;
        };
        runtime.spawn(async move {
            let response = transport
                .request_wire(
                    details.ownership,
                    wire::RequestPayload::KillProcessRequest(wire::KillProcessRequest {
                        process_id: details.process_id.clone(),
                        signal: String::from("SIGKILL"),
                    }),
                )
                .await;
            match response {
                Ok(wire::ResponsePayload::ProcessKilledResponse(killed))
                    if killed.process_id == details.process_id => {}
                Ok(wire::ResponsePayload::RejectedResponse(rejected)) => tracing::error!(
                    code = rejected.code,
                    message = rejected.message,
                    process_id = details.process_id,
                    "sidecar rejected cleanup after Execute cancellation"
                ),
                Ok(other) => tracing::error!(
                    ?other,
                    process_id = details.process_id,
                    "unexpected cleanup response after Execute cancellation"
                ),
                Err(error) => tracing::error!(
                    ?error,
                    process_id = details.process_id,
                    "failed to kill process after its Execute caller was cancelled"
                ),
            }
        });
    }
}

impl WireEventSubscription {
    pub async fn recv(&mut self) -> Result<SharedWireEvent, WireEventRecvError> {
        loop {
            {
                let state = self.log.state.lock();
                let Some(route) = self.route.as_ref() else {
                    drop(state);
                    if self.signal_rx.changed().await.is_err() {
                        return Err(WireEventRecvError::Closed);
                    }
                    continue;
                };
                let first_route_sequence = state
                    .entries
                    .iter()
                    .find(|entry| &entry.route == route)
                    .map(|entry| entry.route_sequence)
                    .unwrap_or_else(|| {
                        state
                            .next_route_sequence
                            .get(route)
                            .copied()
                            .unwrap_or(self.next_route_sequence)
                    });
                if self.next_route_sequence < first_route_sequence {
                    let skipped = first_route_sequence - self.next_route_sequence;
                    self.next_route_sequence = first_route_sequence;
                    return Err(WireEventRecvError::Lagged { skipped });
                }
                let first_sequence = state
                    .entries
                    .front()
                    .map(|entry| entry.sequence)
                    .unwrap_or(state.next_sequence);
                if self.next_sequence < first_sequence {
                    self.next_sequence = first_sequence;
                }

                for entry in &state.entries {
                    if entry.sequence < self.next_sequence {
                        continue;
                    }
                    self.next_sequence = entry.sequence.saturating_add(1);
                    if &entry.route == route {
                        self.next_route_sequence = entry.route_sequence.saturating_add(1);
                        return Ok(Arc::clone(&entry.event));
                    }
                }
                self.next_sequence = state.next_sequence;

                if state.closed {
                    return Err(WireEventRecvError::Closed);
                }
            }
            if self.signal_rx.changed().await.is_err() {
                return Err(WireEventRecvError::Closed);
            }
        }
    }
}

impl Drop for WireEventSubscription {
    fn drop(&mut self) {
        if let Some(route) = self.route.take() {
            self.log.unsubscribe(&route);
        }
    }
}

/// Owns the spawned sidecar child, the framed BARE stdio I/O tasks, the pending-response map, the
/// event fan-out, and the callback dispatch table.
pub struct SidecarTransport {
    /// The spawned sidecar process (stdout/stdin taken by the I/O tasks; kept for kill on drop).
    child: parking_lot::Mutex<Option<Child>>,
    /// Pending host-initiated requests, keyed by positive `RequestId`.
    pending: SccHashMap<wire::RequestId, PendingWireRequest>,
    pending_request_lock: parking_lot::Mutex<()>,
    /// Host request-id counter (positive, starts at 1).
    request_counter: AtomicI64,
    /// Negotiated max frame size.
    max_frame_bytes: AtomicUsize,
    /// Process output/terminal events and VM control events use separate bounded logs so process
    /// traffic cannot evict ACP/cron state transitions. Each log is bounded by bytes and count.
    process_event_log: Arc<WireEventLog>,
    control_event_log: Arc<WireEventLog>,
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
        let process_event_log = Arc::new(WireEventLog::new());
        let control_event_log = Arc::new(WireEventLog::new());

        let transport = Arc::new(Self {
            child: parking_lot::Mutex::new(Some(child)),
            pending: SccHashMap::new(),
            pending_request_lock: parking_lot::Mutex::new(()),
            request_counter: AtomicI64::new(1),
            max_frame_bytes: AtomicUsize::new(wire::DEFAULT_MAX_FRAME_BYTES),
            process_event_log,
            control_event_log,
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
        Ok(self
            .request_wire_with_frame_limit(ownership, payload, None, false, None)
            .await?
            .payload)
    }

    /// Issue a request and run one synchronous, request-scoped hook in the reader immediately after
    /// its response is decoded. The hook completes before the response waiter is woken and before a
    /// later event frame can be dispatched. Pending requests (and therefore captured hooks) remain
    /// bounded by [`PENDING_REQUEST_LIMIT`]. Once the request is successfully enqueued, its hook is
    /// intentionally retained even if the waiter is cancelled: the sidecar may still commit the
    /// operation, and the hook must install host correlation for that authoritative response. The
    /// pending tombstone is removed by the response, transport failure, or silence watchdog.
    ///
    /// The hook receives only generated wire types; extension decoding and correlation remain the
    /// caller's responsibility. A hook must perform bounded in-memory work and must not block.
    pub async fn request_wire_with_response_hook<F>(
        &self,
        ownership: wire::OwnershipScope,
        payload: wire::RequestPayload,
        response_hook: F,
    ) -> Result<wire::ResponsePayload, TransportError>
    where
        F: FnOnce(&wire::ResponsePayload) -> Result<(), TransportError> + Send + Sync + 'static,
    {
        Ok(self
            .request_wire_with_frame_limit(
                ownership,
                payload,
                None,
                false,
                Some(Box::new(response_hook)),
            )
            .await?
            .payload)
    }

    /// Issue a host request using generated wire protocol types with a caller-specific frame limit.
    pub async fn request_wire_bounded(
        &self,
        ownership: wire::OwnershipScope,
        payload: wire::RequestPayload,
        max_frame_bytes: usize,
    ) -> Result<wire::ResponsePayload, TransportError> {
        Ok(self
            .request_wire_with_frame_limit(ownership, payload, Some(max_frame_bytes), false, None)
            .await?
            .payload)
    }

    /// Issue an Execute request while atomically binding its exact process event route before the
    /// response waiter is woken. The sidecar writes the response before subsequent events, so the
    /// reader installs `(full ownership, process_id)` routing without a client-generated id or a
    /// post-response loss window.
    pub async fn request_wire_with_process_events(
        &self,
        ownership: wire::OwnershipScope,
        payload: wire::RequestPayload,
    ) -> Result<(wire::ResponsePayload, Option<WireEventSubscription>), TransportError> {
        if !matches!(payload, wire::RequestPayload::ExecuteRequest(_)) {
            return Err(TransportError::Sidecar(String::from(
                "process event routing is valid only for Execute requests",
            )));
        }
        let response = self
            .request_wire_with_frame_limit(ownership, payload, None, true, None)
            .await?;
        Ok((response.payload, response.process_events))
    }

    async fn request_wire_with_frame_limit(
        &self,
        ownership: wire::OwnershipScope,
        payload: wire::RequestPayload,
        max_frame_bytes: Option<usize>,
        subscribe_process_events: bool,
        response_hook: Option<WireResponseHook>,
    ) -> Result<PendingWireResponse, TransportError> {
        let retain_response_hook = response_hook.is_some();
        let request_id = self.next_request_id();
        let frame = wire::ProtocolFrame::RequestFrame(wire::RequestFrame {
            schema: wire::protocol_schema(),
            request_id,
            ownership,
            payload,
        });
        let bytes = self.encode_wire_frame(&frame, max_frame_bytes)?;

        let (tx, rx) = oneshot::channel();
        let process_events =
            subscribe_process_events.then(|| self.process_event_log.subscribe_provisional());
        self.register_pending_with_hook(request_id, tx, process_events, response_hook)?;
        let mut pending_guard = PendingRequestGuard::new(self, request_id, true);

        if self.request_writer_tx.send(bytes).await.is_err() {
            self.pending.remove(&request_id);
            return Err(TransportError::Sidecar(
                "sidecar transport closed".to_string(),
            ));
        }
        if subscribe_process_events || retain_response_hook {
            // Execute needs its process-id binding even when its caller is cancelled. Resource-
            // establishing response hooks have the same requirement: after the request is
            // successfully enqueued, retain this bounded pending tombstone so the reader still
            // installs host correlation for an authoritative sidecar success.
            pending_guard.disarm();
        }

        let mut response = rx
            .await
            .map_err(|_| TransportError::Sidecar("sidecar transport disconnected".to_string()))?;
        if let Some(error) = response.response_hook_error.take() {
            return Err(error);
        }
        if let Some(cleanup) = response.cancel_cleanup.as_mut() {
            cleanup.disarm();
        }
        Ok(response)
    }

    #[cfg(test)]
    fn subscribe_process_events_for(
        &self,
        ownership: wire::OwnershipScope,
        process_id: impl Into<String>,
    ) -> WireEventSubscription {
        self.process_event_log.subscribe(WireEventRoute::Process {
            ownership: OwnershipKey::from(&ownership),
            process_id: process_id.into(),
        })
    }

    /// Subscribe to non-process VM events for one exact connection/session/VM scope. Keeping this
    /// separate prevents high-volume stdout from starving ACP and durable cron delivery.
    pub fn subscribe_control_events_for(
        &self,
        ownership: wire::OwnershipScope,
    ) -> WireEventSubscription {
        self.control_event_log
            .subscribe(WireEventRoute::Control(OwnershipKey::from(&ownership)))
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
        let previous = self.max_frame_bytes.load(Ordering::SeqCst);
        if max_frame_bytes > previous {
            self.process_event_log.set_max_frame_bytes(max_frame_bytes);
            self.control_event_log.set_max_frame_bytes(max_frame_bytes);
            self.max_frame_bytes
                .store(max_frame_bytes, Ordering::SeqCst);
        } else {
            self.max_frame_bytes
                .store(max_frame_bytes, Ordering::SeqCst);
            self.process_event_log.set_max_frame_bytes(max_frame_bytes);
            self.control_event_log.set_max_frame_bytes(max_frame_bytes);
        }
    }

    /// Kill the child sidecar process if this transport still owns one.
    pub fn kill_child(&self) {
        if let Some(mut child) = self.child.lock().take() {
            if let Err(error) = child.start_kill() {
                tracing::error!(?error, "failed to kill child sidecar process");
            }
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
    #[cfg(test)]
    async fn handle_wire_frame(self: &Arc<Self>, frame: wire::ProtocolFrame) {
        self.handle_wire_frame_sized(frame, EVENT_ENTRY_OVERHEAD_BYTES)
            .await;
    }

    async fn handle_wire_frame_sized(
        self: &Arc<Self>,
        frame: wire::ProtocolFrame,
        retained_bytes: usize,
    ) {
        match frame {
            wire::ProtocolFrame::ResponseFrame(response) => {
                match self.pending.remove(&response.request_id) {
                    Some((_, mut pending)) => {
                        if let (
                            Some(subscription),
                            wire::ResponsePayload::ProcessStartedResponse(started),
                        ) = (pending.process_events.as_mut(), &response.payload)
                        {
                            self.process_event_log.bind_route(
                                subscription,
                                WireEventRoute::Process {
                                    ownership: OwnershipKey::from(&response.ownership),
                                    process_id: started.process_id.clone(),
                                },
                            );
                        }
                        let cancel_cleanup = match &response.payload {
                            wire::ResponsePayload::ProcessStartedResponse(started) => {
                                Some(CancelledProcessCleanup::new(
                                    self,
                                    response.ownership.clone(),
                                    started.process_id.clone(),
                                ))
                            }
                            _ => None,
                        };
                        let response_hook_error = pending.response_hook.take().and_then(|hook| {
                            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                hook(&response.payload)
                            })) {
                                Ok(Ok(())) => None,
                                Ok(Err(error)) => Some(error),
                                Err(_) => Some(TransportError::Sidecar(String::from(
                                    "sidecar response hook panicked",
                                ))),
                            }
                        });
                        let delivered = PendingWireResponse {
                            payload: response.payload,
                            process_events: pending.process_events,
                            cancel_cleanup,
                            response_hook_error,
                        };
                        if let Err(delivered) = pending.tx.send(delivered) {
                            if let Some(error) = delivered.response_hook_error {
                                tracing::error!(
                                    ?error,
                                    request_id = response.request_id,
                                    "sidecar response hook failed after its request waiter was cancelled"
                                );
                            }
                        }
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
                let ownership = OwnershipKey::from(&event.ownership);
                let (process_event, route) = match &event.payload {
                    wire::EventPayload::ProcessOutputEvent(output) => (
                        true,
                        WireEventRoute::Process {
                            ownership,
                            process_id: output.process_id.clone(),
                        },
                    ),
                    wire::EventPayload::ProcessExitedEvent(exited) => (
                        true,
                        WireEventRoute::Process {
                            ownership,
                            process_id: exited.process_id.clone(),
                        },
                    ),
                    wire::EventPayload::CronDispatchEvent(_)
                    | wire::EventPayload::VmLifecycleEvent(_)
                    | wire::EventPayload::StructuredEvent(_)
                    | wire::EventPayload::ExtEnvelope(_) => {
                        (false, WireEventRoute::Control(ownership))
                    }
                };
                if process_event {
                    self.process_event_log.publish(event, retained_bytes, route);
                } else {
                    self.control_event_log.publish(event, retained_bytes, route);
                }
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
        self.process_event_log.close();
        self.control_event_log.close();
    }

    #[cfg(test)]
    fn register_pending_request(
        &self,
        request_id: wire::RequestId,
        tx: oneshot::Sender<PendingWireResponse>,
    ) -> Result<(), TransportError> {
        self.register_pending(request_id, tx, None)
    }

    #[cfg(test)]
    fn register_pending(
        &self,
        request_id: wire::RequestId,
        tx: oneshot::Sender<PendingWireResponse>,
        process_events: Option<WireEventSubscription>,
    ) -> Result<(), TransportError> {
        self.register_pending_with_hook(request_id, tx, process_events, None)
    }

    fn register_pending_with_hook(
        &self,
        request_id: wire::RequestId,
        tx: oneshot::Sender<PendingWireResponse>,
        process_events: Option<WireEventSubscription>,
        response_hook: Option<WireResponseHook>,
    ) -> Result<(), TransportError> {
        let _guard = self.pending_request_lock.lock();
        if pending_request_count(self) >= PENDING_REQUEST_LIMIT {
            return Err(TransportError::Sidecar(format!(
                "sidecar pending request limit exceeded: at most {PENDING_REQUEST_LIMIT} requests can be in flight"
            )));
        }
        let _ = self.pending.insert(
            request_id,
            PendingWireRequest {
                tx,
                process_events,
                response_hook,
            },
        );
        Ok(())
    }
}

struct PendingRequestGuard<'a> {
    transport: &'a SidecarTransport,
    request_id: wire::RequestId,
    remove_on_drop: bool,
}

impl<'a> PendingRequestGuard<'a> {
    fn new(
        transport: &'a SidecarTransport,
        request_id: wire::RequestId,
        remove_on_drop: bool,
    ) -> Self {
        Self {
            transport,
            request_id,
            remove_on_drop,
        }
    }

    fn disarm(&mut self) {
        self.remove_on_drop = false;
    }
}

impl Drop for PendingRequestGuard<'_> {
    fn drop(&mut self) {
        if self.remove_on_drop {
            let _ = self.transport.pending.remove(&self.request_id);
        }
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
            Ok(frame) => {
                transport
                    .handle_wire_frame_sized(
                        frame,
                        frame_bytes.len().saturating_add(EVENT_ENTRY_OVERHEAD_BYTES),
                    )
                    .await
            }
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
        SidecarTransport {
            child: parking_lot::Mutex::new(None),
            pending: SccHashMap::new(),
            pending_request_lock: parking_lot::Mutex::new(()),
            request_counter: AtomicI64::new(1),
            max_frame_bytes: AtomicUsize::new(wire::DEFAULT_MAX_FRAME_BYTES),
            process_event_log: Arc::new(WireEventLog::new()),
            control_event_log: Arc::new(WireEventLog::new()),
            callbacks: SccHashMap::new(),
            request_writer_tx,
            control_writer_tx,
            last_inbound_at: parking_lot::Mutex::new(std::time::Instant::now()),
        }
    }

    fn test_vm_ownership(vm_id: &str) -> wire::OwnershipScope {
        wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: "conn-1".to_string(),
            session_id: "session-1".to_string(),
            vm_id: vm_id.to_string(),
        })
    }

    fn process_output_event(
        ownership: wire::OwnershipScope,
        process_id: &str,
        chunk: &[u8],
    ) -> wire::ProtocolFrame {
        wire::ProtocolFrame::EventFrame(wire::EventFrame {
            schema: wire::protocol_schema(),
            ownership,
            payload: wire::EventPayload::ProcessOutputEvent(wire::ProcessOutputEvent {
                process_id: process_id.to_owned(),
                channel: wire::StreamChannel::Stdout,
                chunk: chunk.to_vec(),
            }),
        })
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
        let mut wire_events =
            transport.subscribe_process_events_for(test_vm_ownership("vm-1"), "proc-1");
        let mut second_subscriber =
            transport.subscribe_process_events_for(test_vm_ownership("vm-1"), "proc-1");

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
        let mut first = transport.subscribe_process_events_for(test_vm_ownership("vm-1"), "proc-1");
        let mut second =
            transport.subscribe_process_events_for(test_vm_ownership("vm-1"), "proc-1");
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
    async fn process_event_log_is_byte_bounded_and_reports_exact_lag() {
        let transport = Arc::new(test_transport());
        let ownership = test_vm_ownership("vm-byte-bound");
        let mut events =
            transport.subscribe_process_events_for(ownership.clone(), "proc-byte-bound");
        let retained_size = DEFAULT_EVENT_CHANNEL_MAX_RETAINED_BYTES / 2 + 1;

        for index in 0..3 {
            transport
                .handle_wire_frame_sized(
                    process_output_event(ownership.clone(), "proc-byte-bound", &[index as u8]),
                    retained_size,
                )
                .await;
        }

        let (retained_events, retained_bytes) = transport.process_event_log.retained_usage();
        assert_eq!(retained_events, 1);
        assert!(retained_bytes <= DEFAULT_EVENT_CHANNEL_MAX_RETAINED_BYTES);
        assert_eq!(
            events.recv().await,
            Err(WireEventRecvError::Lagged { skipped: 2 })
        );
        let newest = events.recv().await.expect("newest retained event");
        assert!(matches!(
            &newest.payload,
            wire::EventPayload::ProcessOutputEvent(wire::ProcessOutputEvent {
                process_id,
                ..
            }) if process_id == "proc-byte-bound"
        ));
    }

    #[tokio::test]
    async fn process_event_log_count_backstop_reports_exact_lag() {
        let transport = Arc::new(test_transport());
        let ownership = test_vm_ownership("vm-count-bound");
        let mut events =
            transport.subscribe_process_events_for(ownership.clone(), "proc-count-bound");

        for _ in 0..=EVENT_CHANNEL_CAPACITY {
            transport
                .handle_wire_frame(process_output_event(
                    ownership.clone(),
                    "proc-count-bound",
                    b"x",
                ))
                .await;
        }

        assert_eq!(
            events.recv().await,
            Err(WireEventRecvError::Lagged { skipped: 1 })
        );
    }

    #[tokio::test]
    async fn route_local_cursor_lags_only_the_slow_subscriber() {
        let transport = Arc::new(test_transport());
        transport.set_max_frame_bytes(1024);
        let ownership = test_vm_ownership("vm-fast-slow");
        let mut fast = transport.subscribe_process_events_for(ownership.clone(), "proc-route");
        let mut slow = transport.subscribe_process_events_for(ownership.clone(), "proc-route");

        transport
            .handle_wire_frame_sized(
                process_output_event(ownership.clone(), "proc-route", b"first"),
                800,
            )
            .await;
        let first = fast.recv().await.expect("fast first event");
        assert!(matches!(
            &first.payload,
            wire::EventPayload::ProcessOutputEvent(event) if event.chunk == b"first"
        ));

        transport
            .handle_wire_frame_sized(
                process_output_event(ownership, "proc-route", b"second"),
                800,
            )
            .await;

        let second = fast.recv().await.expect("fast second event without lag");
        assert!(matches!(
            &second.payload,
            wire::EventPayload::ProcessOutputEvent(event) if event.chunk == b"second"
        ));
        assert_eq!(
            slow.recv().await,
            Err(WireEventRecvError::Lagged { skipped: 1 })
        );
        let second = slow.recv().await.expect("slow retained event after lag");
        assert!(matches!(
            &second.payload,
            wire::EventPayload::ProcessOutputEvent(event) if event.chunk == b"second"
        ));
    }

    #[tokio::test]
    async fn evictions_are_isolated_by_full_owner_and_process_id() {
        let transport = Arc::new(test_transport());
        transport.set_max_frame_bytes(1024);
        let owner_a = test_vm_ownership("vm-route-a");
        let owner_b = test_vm_ownership("vm-route-b");
        let mut route_a = transport.subscribe_process_events_for(owner_a.clone(), "proc-shared");
        let mut route_b = transport.subscribe_process_events_for(owner_b.clone(), "proc-shared");
        let mut sibling = transport.subscribe_process_events_for(owner_a.clone(), "proc-sibling");

        for chunk in [b"b-one".as_slice(), b"b-two".as_slice()] {
            transport
                .handle_wire_frame_sized(
                    process_output_event(owner_b.clone(), "proc-shared", chunk),
                    800,
                )
                .await;
        }
        transport
            .handle_wire_frame_sized(
                process_output_event(owner_a.clone(), "proc-shared", b"a-one"),
                700,
            )
            .await;

        assert!(
            route_a.recv().await.is_ok(),
            "cross-owner eviction lagged A"
        );
        assert_eq!(
            route_b.recv().await,
            Err(WireEventRecvError::Lagged { skipped: 1 })
        );
        assert!(
            route_b.recv().await.is_ok(),
            "B must retain its newest event"
        );

        for chunk in [b"s-one".as_slice(), b"s-two".as_slice()] {
            transport
                .handle_wire_frame_sized(
                    process_output_event(owner_a.clone(), "proc-sibling", chunk),
                    800,
                )
                .await;
        }
        transport
            .handle_wire_frame_sized(process_output_event(owner_a, "proc-shared", b"a-two"), 700)
            .await;

        assert!(
            route_a.recv().await.is_ok(),
            "sibling-process eviction lagged A"
        );
        assert_eq!(
            sibling.recv().await,
            Err(WireEventRecvError::Lagged { skipped: 1 })
        );
        assert!(
            sibling.recv().await.is_ok(),
            "sibling must retain its newest event"
        );
    }

    #[tokio::test]
    async fn dropping_last_route_subscriber_releases_retained_events() {
        let transport = Arc::new(test_transport());
        let ownership = test_vm_ownership("vm-drop-route");
        let first = transport.subscribe_process_events_for(ownership.clone(), "proc-drop");
        let second = transport.subscribe_process_events_for(ownership.clone(), "proc-drop");
        transport
            .handle_wire_frame(process_output_event(
                ownership.clone(),
                "proc-drop",
                b"retained",
            ))
            .await;
        assert_eq!(transport.process_event_log.retained_usage().0, 1);
        drop(first);
        assert_eq!(transport.process_event_log.retained_usage().0, 1);
        drop(second);
        assert_eq!(transport.process_event_log.retained_usage(), (0, 0));

        transport
            .handle_wire_frame(process_output_event(ownership, "proc-drop", b"inactive"))
            .await;
        assert_eq!(transport.process_event_log.retained_usage(), (0, 0));
    }

    #[tokio::test]
    async fn negotiated_frame_limit_controls_event_retention_and_lowering_reports_lag() {
        let transport = Arc::new(test_transport());
        let raised = wire::DEFAULT_MAX_FRAME_BYTES.saturating_mul(2);
        transport.set_max_frame_bytes(raised);
        assert_eq!(
            transport
                .process_event_log
                .retained_byte_limit
                .load(Ordering::Acquire),
            event_log_byte_limit(raised)
        );
        let ownership = test_vm_ownership("vm-negotiated-bound");
        let mut events =
            transport.subscribe_process_events_for(ownership.clone(), "proc-negotiated");
        let retained_size = DEFAULT_EVENT_CHANNEL_MAX_RETAINED_BYTES / 2 + 1;
        for chunk in [b"one".as_slice(), b"two".as_slice()] {
            transport
                .handle_wire_frame_sized(
                    process_output_event(ownership.clone(), "proc-negotiated", chunk),
                    retained_size,
                )
                .await;
        }
        assert_eq!(transport.process_event_log.retained_usage().0, 2);

        transport.set_max_frame_bytes(wire::DEFAULT_MAX_FRAME_BYTES);
        assert_eq!(
            events.recv().await,
            Err(WireEventRecvError::Lagged { skipped: 1 })
        );
        assert!(events.recv().await.is_ok(), "newest event remains retained");
    }

    #[tokio::test]
    async fn process_route_is_bound_before_started_response_is_observed() {
        let transport = Arc::new(test_transport());
        let ownership = test_vm_ownership("vm-atomic-route");
        let (tx, rx) = oneshot::channel();
        let provisional = transport.process_event_log.subscribe_provisional();
        transport
            .register_pending(7, tx, Some(provisional))
            .expect("register Execute response");

        transport
            .handle_wire_frame(wire::ProtocolFrame::ResponseFrame(wire::ResponseFrame {
                schema: wire::protocol_schema(),
                request_id: 7,
                ownership: ownership.clone(),
                payload: wire::ResponsePayload::ProcessStartedResponse(
                    wire::ProcessStartedResponse {
                        process_id: String::from("proc-atomic"),
                        pid: Some(42),
                    },
                ),
            }))
            .await;
        transport
            .handle_wire_frame(process_output_event(ownership, "proc-atomic", b"immediate"))
            .await;

        let mut response = rx.await.expect("started response");
        response
            .cancel_cleanup
            .as_mut()
            .expect("armed cancellation cleanup")
            .disarm();
        let mut events = response.process_events.expect("bound process route");
        let event = events.recv().await.expect("immediate output");
        assert!(matches!(
            &event.payload,
            wire::EventPayload::ProcessOutputEvent(event) if event.chunk == b"immediate"
        ));
    }

    #[tokio::test]
    async fn response_hook_runs_before_waiter_wakes_and_following_event_dispatches() {
        let transport = Arc::new(test_transport());
        let ownership = test_vm_ownership("vm-response-hook");
        let mut events = transport.subscribe_control_events_for(ownership.clone());
        let hook_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (tx, rx) = oneshot::channel();
        transport
            .register_pending_with_hook(
                8,
                tx,
                None,
                Some(Box::new({
                    let hook_ran = hook_ran.clone();
                    move |_| {
                        hook_ran.store(true, Ordering::SeqCst);
                        Ok(())
                    }
                })),
            )
            .expect("register response hook");

        transport
            .handle_wire_frame(wire::ProtocolFrame::ResponseFrame(wire::ResponseFrame {
                schema: wire::protocol_schema(),
                request_id: 8,
                ownership: ownership.clone(),
                payload: wire::ResponsePayload::ExtEnvelope(wire::ExtEnvelope {
                    namespace: String::from("test.response-hook"),
                    payload: vec![1],
                }),
            }))
            .await;
        assert!(
            hook_ran.load(Ordering::SeqCst),
            "response hook runs before its waiter can observe the response"
        );
        let response = rx.await.expect("response waiter wakes");
        assert!(response.response_hook_error.is_none());

        transport
            .handle_wire_frame(wire::ProtocolFrame::EventFrame(wire::EventFrame {
                schema: wire::protocol_schema(),
                ownership,
                payload: wire::EventPayload::StructuredEvent(wire::StructuredEvent {
                    name: String::from("after-hook"),
                    detail: std::collections::HashMap::new(),
                }),
            }))
            .await;
        assert!(hook_ran.load(Ordering::SeqCst));
        assert!(matches!(
            &events.recv().await.expect("following event" ).payload,
            wire::EventPayload::StructuredEvent(event) if event.name == "after-hook"
        ));
    }

    #[tokio::test]
    async fn cancelled_response_hook_request_still_binds_before_following_event() {
        let (request_writer_tx, mut request_writer_rx) =
            mpsc::channel(REQUEST_FRAME_QUEUE_CAPACITY);
        let (control_writer_tx, _control_writer_rx) = mpsc::channel(CONTROL_FRAME_QUEUE_CAPACITY);
        let transport = Arc::new(SidecarTransport {
            child: parking_lot::Mutex::new(None),
            pending: SccHashMap::new(),
            pending_request_lock: parking_lot::Mutex::new(()),
            request_counter: AtomicI64::new(100),
            max_frame_bytes: AtomicUsize::new(wire::DEFAULT_MAX_FRAME_BYTES),
            process_event_log: Arc::new(WireEventLog::new()),
            control_event_log: Arc::new(WireEventLog::new()),
            callbacks: SccHashMap::new(),
            request_writer_tx,
            control_writer_tx,
            last_inbound_at: parking_lot::Mutex::new(std::time::Instant::now()),
        });
        let ownership = test_vm_ownership("vm-atomic-hook");
        let mut events = transport.subscribe_control_events_for(ownership.clone());
        let route_installed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task = tokio::spawn({
            let transport = transport.clone();
            let ownership = ownership.clone();
            let route_installed = route_installed.clone();
            async move {
                transport
                    .request_wire_with_response_hook(
                        ownership,
                        wire::RequestPayload::ProvidedCommandsRequest,
                        move |_| {
                            route_installed.store(true, Ordering::SeqCst);
                            Ok(())
                        },
                    )
                    .await
            }
        });

        request_writer_rx
            .recv()
            .await
            .expect("request was successfully enqueued");
        task.abort();
        let _ = task.await;
        assert_eq!(
            pending_request_count(&transport),
            1,
            "post-enqueue cancellation must retain the bounded response hook tombstone"
        );

        transport
            .handle_wire_frame(wire::ProtocolFrame::ResponseFrame(wire::ResponseFrame {
                schema: wire::protocol_schema(),
                request_id: 100,
                ownership: ownership.clone(),
                payload: wire::ResponsePayload::ExtEnvelope(wire::ExtEnvelope {
                    namespace: String::from("test.cancelled-response-hook"),
                    payload: vec![1],
                }),
            }))
            .await;
        transport
            .handle_wire_frame(wire::ProtocolFrame::EventFrame(wire::EventFrame {
                schema: wire::protocol_schema(),
                ownership,
                payload: wire::EventPayload::StructuredEvent(wire::StructuredEvent {
                    name: String::from("immediate-after-response"),
                    detail: std::collections::HashMap::new(),
                }),
            }))
            .await;

        assert!(
            route_installed.load(Ordering::SeqCst),
            "the cancelled request's hook must run before the next event is dispatched"
        );
        assert_eq!(pending_request_count(&transport), 0);
        let event = events
            .recv()
            .await
            .expect("following event remains observable");
        assert!(matches!(
            &event.payload,
            wire::EventPayload::StructuredEvent(event)
                if event.name == "immediate-after-response"
        ));
    }

    #[tokio::test]
    async fn transport_close_wakes_event_waiters() {
        let transport = Arc::new(test_transport());
        let mut events =
            transport.subscribe_process_events_for(test_vm_ownership("vm-close"), "proc-close");
        let waiter = tokio::spawn(async move { events.recv().await });
        tokio::task::yield_now().await;
        transport.fail_all_pending();
        assert_eq!(
            waiter.await.expect("waiter task"),
            Err(WireEventRecvError::Closed)
        );
    }

    #[tokio::test]
    async fn process_event_subscription_filters_the_full_ownership_scope() {
        let transport = Arc::new(test_transport());
        let expected = test_vm_ownership("vm-shared-name");
        let wrong_connection = wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: "conn-other".to_owned(),
            session_id: "session-1".to_owned(),
            vm_id: "vm-shared-name".to_owned(),
        });
        let wrong_session = wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: "conn-1".to_owned(),
            session_id: "session-other".to_owned(),
            vm_id: "vm-shared-name".to_owned(),
        });
        let mut events = transport.subscribe_process_events_for(expected.clone(), "same-process");

        for ownership in [wrong_connection, wrong_session] {
            transport
                .handle_wire_frame(process_output_event(ownership, "same-process", b"wrong"))
                .await;
        }
        transport
            .handle_wire_frame(process_output_event(
                expected.clone(),
                "same-process",
                b"right",
            ))
            .await;

        let event = events.recv().await.expect("exact-owner event");
        assert_eq!(event.ownership, expected);
        assert!(matches!(
            &event.payload,
            wire::EventPayload::ProcessOutputEvent(wire::ProcessOutputEvent { chunk, .. })
                if chunk == b"right"
        ));
    }

    #[tokio::test]
    async fn process_flood_cannot_evict_control_events() {
        let transport = Arc::new(test_transport());
        let ownership = test_vm_ownership("vm-control-isolation");
        let mut control = transport.subscribe_control_events_for(ownership.clone());
        let _process = transport.subscribe_process_events_for(ownership.clone(), "proc-flood");
        transport
            .handle_wire_frame(wire::ProtocolFrame::EventFrame(wire::EventFrame {
                schema: wire::protocol_schema(),
                ownership: ownership.clone(),
                payload: wire::EventPayload::StructuredEvent(wire::StructuredEvent {
                    name: "control".to_owned(),
                    detail: std::collections::HashMap::new(),
                }),
            }))
            .await;

        for _ in 0..3 {
            transport
                .handle_wire_frame_sized(
                    process_output_event(ownership.clone(), "proc-flood", b"x"),
                    DEFAULT_EVENT_CHANNEL_MAX_RETAINED_BYTES / 2 + 1,
                )
                .await;
        }

        let event = control.recv().await.expect("control event");
        assert!(matches!(
            &event.payload,
            wire::EventPayload::StructuredEvent(wire::StructuredEvent { name, .. })
                if name == "control"
        ));
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
        assert!(rx.await.is_err(), "watchdog should drop the pending sender");
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
        let mut wire_events = transport.subscribe_control_events_for(
            wire::OwnershipScope::ConnectionOwnership(wire::ConnectionOwnership {
                connection_id: "conn-1".to_string(),
            }),
        );

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
            tokio::time::timeout(std::time::Duration::from_millis(10), wire_events.recv())
                .await
                .is_err(),
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
            let _guard = PendingRequestGuard::new(&transport, 1, true);
            assert_eq!(pending_request_count(&transport), 1);
        }

        assert_eq!(pending_request_count(&transport), 0);
    }

    #[tokio::test]
    async fn cancelled_execute_retains_tombstone_and_kills_started_process() {
        let (request_writer_tx, mut request_writer_rx) =
            mpsc::channel(REQUEST_FRAME_QUEUE_CAPACITY);
        let (control_writer_tx, _control_writer_rx) = mpsc::channel(CONTROL_FRAME_QUEUE_CAPACITY);
        let transport = Arc::new(SidecarTransport {
            child: parking_lot::Mutex::new(None),
            pending: SccHashMap::new(),
            pending_request_lock: parking_lot::Mutex::new(()),
            request_counter: AtomicI64::new(100),
            max_frame_bytes: AtomicUsize::new(wire::DEFAULT_MAX_FRAME_BYTES),
            process_event_log: Arc::new(WireEventLog::new()),
            control_event_log: Arc::new(WireEventLog::new()),
            callbacks: SccHashMap::new(),
            request_writer_tx,
            control_writer_tx,
            last_inbound_at: parking_lot::Mutex::new(std::time::Instant::now()),
        });
        let ownership = test_vm_ownership("vm-cancelled-execute");
        let (tx, rx) = oneshot::channel();
        transport
            .register_pending(
                7,
                tx,
                Some(transport.process_event_log.subscribe_provisional()),
            )
            .expect("register Execute");
        {
            let _guard = PendingRequestGuard::new(&transport, 7, false);
        }
        assert_eq!(
            pending_request_count(&transport),
            1,
            "cancelled Execute retains its response tombstone"
        );

        transport
            .handle_wire_frame(wire::ProtocolFrame::ResponseFrame(wire::ResponseFrame {
                schema: wire::protocol_schema(),
                request_id: 7,
                ownership: ownership.clone(),
                payload: wire::ResponsePayload::ProcessStartedResponse(
                    wire::ProcessStartedResponse {
                        process_id: String::from("proc-cancelled"),
                        pid: Some(77),
                    },
                ),
            }))
            .await;
        drop(rx);

        let kill_bytes =
            tokio::time::timeout(std::time::Duration::from_secs(1), request_writer_rx.recv())
                .await
                .expect("cleanup request timeout")
                .expect("cleanup request frame");
        let kill = WireFrameCodec::default()
            .decode(&kill_bytes)
            .expect("decode cleanup request");
        let wire::ProtocolFrame::RequestFrame(kill) = kill else {
            panic!("cleanup must be a request frame");
        };
        assert_eq!(kill.ownership, ownership);
        assert!(matches!(
            kill.payload,
            wire::RequestPayload::KillProcessRequest(wire::KillProcessRequest {
                process_id,
                signal,
            }) if process_id == "proc-cancelled" && signal == "SIGKILL"
        ));

        transport
            .handle_wire_frame(wire::ProtocolFrame::ResponseFrame(wire::ResponseFrame {
                schema: wire::protocol_schema(),
                request_id: kill.request_id,
                ownership,
                payload: wire::ResponsePayload::ProcessKilledResponse(
                    wire::ProcessKilledResponse {
                        process_id: String::from("proc-cancelled"),
                    },
                ),
            }))
            .await;
        tokio::task::yield_now().await;
        assert_eq!(pending_request_count(&transport), 0);
    }

    #[tokio::test]
    async fn execute_cancelled_before_enqueue_removes_pending_slot() {
        let (request_writer_tx, _request_writer_rx) = mpsc::channel(REQUEST_FRAME_QUEUE_CAPACITY);
        for _ in 0..REQUEST_FRAME_QUEUE_CAPACITY {
            request_writer_tx
                .send(vec![0])
                .await
                .expect("fill request queue");
        }
        let (control_writer_tx, _control_writer_rx) = mpsc::channel(CONTROL_FRAME_QUEUE_CAPACITY);
        let transport = Arc::new(SidecarTransport {
            child: parking_lot::Mutex::new(None),
            pending: SccHashMap::new(),
            pending_request_lock: parking_lot::Mutex::new(()),
            request_counter: AtomicI64::new(1),
            max_frame_bytes: AtomicUsize::new(wire::DEFAULT_MAX_FRAME_BYTES),
            process_event_log: Arc::new(WireEventLog::new()),
            control_event_log: Arc::new(WireEventLog::new()),
            callbacks: SccHashMap::new(),
            request_writer_tx,
            control_writer_tx,
            last_inbound_at: parking_lot::Mutex::new(std::time::Instant::now()),
        });
        let request_transport = Arc::clone(&transport);
        let task = tokio::spawn(async move {
            request_transport
                .request_wire_with_process_events(
                    test_vm_ownership("vm-cancel-before-enqueue"),
                    wire::RequestPayload::ExecuteRequest(wire::ExecuteRequest {
                        process_id: None,
                        command: Some(String::from("true")),
                        shell_command: None,
                        runtime: None,
                        entrypoint: None,
                        args: Vec::new(),
                        env: None,
                        cwd: None,
                        wasm_permission_tier: None,
                        pty: None,
                        keep_stdin_open: None,
                        timeout_ms: None,
                        capture_output: None,
                    }),
                )
                .await
        });
        for _ in 0..100 {
            if pending_request_count(&transport) == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(pending_request_count(&transport), 1);
        task.abort();
        assert!(task.await.is_err(), "cancelled request task must abort");
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
