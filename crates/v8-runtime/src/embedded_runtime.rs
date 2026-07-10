use std::collections::HashMap;
use std::collections::HashSet;
use std::io::{self, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::Instant;

use crate::host_call::{
    record_sync_bridge_host_phase, record_sync_bridge_response_channel_send_start, CallIdRouter,
};
use crate::ipc_binary::BinaryFrame;
use crate::runtime_protocol::{
    validate_bridge_response_status, BridgeResponse, ModuleReaderHandle, RuntimeCommand,
    RuntimeEvent, SessionMessage, StreamEvent,
};
use crate::session::{RuntimeEventEnvelope, SessionCommand, SessionManager};
use crate::snapshot::SnapshotCache;
use crate::{bridge, isolate};

static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);
const SESSION_OUTPUT_CHANNEL_CAPACITY: usize = 1024;

pub struct EmbeddedV8Runtime {
    session_mgr: Arc<Mutex<SessionManager>>,
    session_outputs: Arc<Mutex<HashMap<String, SessionOutput>>>,
    snapshot_cache: Arc<SnapshotCache>,
    alive: Arc<AtomicBool>,
    dispatch_shutdown_tx: crossbeam_channel::Sender<()>,
    dispatch_thread: Mutex<Option<thread::JoinHandle<()>>>,
    next_output_generation: AtomicU64,
}

#[derive(Clone)]
struct SessionOutput {
    generation: u64,
    sender: mpsc::SyncSender<RuntimeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedV8SessionOutputRegistration {
    session_id: String,
    generation: u64,
}

impl EmbeddedV8Runtime {
    pub fn new(max_concurrency: Option<usize>) -> io::Result<Self> {
        bridge::init_codec();
        bridge::acquire_embedded_cbor_codec();
        isolate::init_v8_platform();

        // Keep bridge-only, agent-SDK, and wasm-runner userland variants warm
        // without immediately evicting each other.
        let snapshot_cache = Arc::new(SnapshotCache::new(8));
        let (event_tx, event_rx) = crossbeam_channel::bounded::<RuntimeEventEnvelope>(1024);
        let (dispatch_shutdown_tx, dispatch_shutdown_rx) = crossbeam_channel::bounded::<()>(1);
        let call_id_router: CallIdRouter = Arc::new(Mutex::new(HashMap::new()));
        let session_mgr = Arc::new(Mutex::new(SessionManager::new(
            max_concurrency.unwrap_or_else(default_max_concurrency),
            event_tx,
            call_id_router,
            Arc::clone(&snapshot_cache),
        )));
        let session_outputs = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let alive_for_thread = Arc::clone(&alive);
        let session_outputs_for_thread = Arc::clone(&session_outputs);
        let session_mgr_for_thread = Arc::clone(&session_mgr);

        let dispatch_thread = thread::Builder::new()
            .name(String::from("agentos-v8-runtime-dispatch"))
            .spawn(move || {
                loop {
                    crossbeam_channel::select! {
                        recv(event_rx) -> event => {
                            let Ok(event) = event else {
                                break;
                            };
                            route_outbound_event(
                                event,
                                &session_outputs_for_thread,
                                &session_mgr_for_thread,
                            );
                        }
                        recv(dispatch_shutdown_rx) -> _ => {
                            break;
                        }
                    }
                }
                alive_for_thread.store(false, Ordering::Release);
            })
            .inspect_err(|_| bridge::release_embedded_cbor_codec())?;

        Ok(Self {
            session_mgr,
            session_outputs,
            snapshot_cache,
            alive,
            dispatch_shutdown_tx,
            dispatch_thread: Mutex::new(Some(dispatch_thread)),
            next_output_generation: AtomicU64::new(1),
        })
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    pub fn snapshot_ready(&self, bridge_code: &str, userland_code: &str) -> bool {
        if userland_code.is_empty() {
            return true;
        }
        let Ok(flavor) = crate::snapshot::StdlibFlavor::from_environment() else {
            return false;
        };
        let bridge_code = crate::snapshot::bridge_code_for_flavor(flavor, bridge_code);
        self.snapshot_cache
            .try_get_with_flavor(flavor, &bridge_code, Some(userland_code))
            .is_some()
    }

    pub fn pre_warm_workers(
        &self,
        bridge_code: String,
        userland_code: String,
        heap_limit_mb: Option<u32>,
        count: usize,
    ) {
        self.session_mgr
            .lock()
            .expect("embedded runtime session manager lock poisoned")
            .pre_warm_workers(bridge_code, userland_code, heap_limit_mb, count);
    }

    pub fn register_session(&self, session_id: &str) -> io::Result<mpsc::Receiver<RuntimeEvent>> {
        self.register_session_with_output_registration(session_id)
            .map(|(receiver, _registration)| receiver)
    }

    pub fn register_session_with_output_registration(
        &self,
        session_id: &str,
    ) -> io::Result<(
        mpsc::Receiver<RuntimeEvent>,
        EmbeddedV8SessionOutputRegistration,
    )> {
        self.register_session_with_capacity(session_id, SESSION_OUTPUT_CHANNEL_CAPACITY)
    }

    fn register_session_with_capacity(
        &self,
        session_id: &str,
        capacity: usize,
    ) -> io::Result<(
        mpsc::Receiver<RuntimeEvent>,
        EmbeddedV8SessionOutputRegistration,
    )> {
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let mut outputs = self
            .session_outputs
            .lock()
            .expect("embedded runtime session outputs lock poisoned");
        if outputs.contains_key(session_id) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("session output {session_id} already exists"),
            ));
        }
        let generation = self.next_output_generation.fetch_add(1, Ordering::Relaxed);
        outputs.insert(session_id.to_owned(), SessionOutput { generation, sender });
        Ok((
            receiver,
            EmbeddedV8SessionOutputRegistration {
                session_id: session_id.to_owned(),
                generation,
            },
        ))
    }

    pub fn unregister_session(&self, session_id: &str) {
        self.session_outputs
            .lock()
            .expect("embedded runtime session outputs lock poisoned")
            .remove(session_id);
    }

    pub fn destroy_session_if_output_current(
        &self,
        registration: &EmbeddedV8SessionOutputRegistration,
    ) -> io::Result<bool> {
        if !remove_session_output_if_current(
            &self.session_outputs,
            &registration.session_id,
            registration.generation,
        ) {
            return Ok(false);
        }

        let shutdown = {
            let mut mgr = self
                .session_mgr
                .lock()
                .expect("session manager lock poisoned");
            mgr.begin_destroy_session_if_output_generation(
                &registration.session_id,
                registration.generation,
            )
            .map_err(other_io_error)?
        };
        match shutdown {
            Some(shutdown) => {
                shutdown.finish();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn session_handle(self: &Arc<Self>, session_id: String) -> EmbeddedV8SessionHandle {
        EmbeddedV8SessionHandle {
            session_id,
            runtime: Arc::clone(self),
        }
    }

    pub fn dispatch(&self, command: RuntimeCommand) -> io::Result<()> {
        match command {
            RuntimeCommand::CreateSession {
                session_id,
                heap_limit_mb,
                cpu_time_limit_ms,
                wall_clock_limit_ms,
                warm_hint,
            } => {
                let output_generation = self
                    .session_outputs
                    .lock()
                    .expect("embedded runtime session outputs lock poisoned")
                    .get(&session_id)
                    .map(|output| output.generation);
                let mut mgr = self
                    .session_mgr
                    .lock()
                    .expect("session manager lock poisoned");
                mgr.create_session_with_output_generation(
                    session_id,
                    heap_limit_mb,
                    cpu_time_limit_ms,
                    wall_clock_limit_ms,
                    output_generation,
                    warm_hint,
                )
                .map_err(other_io_error)
            }
            command => dispatch_runtime_command(&self.session_mgr, &self.snapshot_cache, command),
        }
    }

    pub fn session_count(&self) -> usize {
        self.session_mgr
            .lock()
            .expect("embedded runtime session manager lock poisoned")
            .session_count()
    }

    pub fn active_slot_count(&self) -> usize {
        self.session_mgr
            .lock()
            .expect("embedded runtime session manager lock poisoned")
            .active_slot_count()
    }
}

impl Drop for EmbeddedV8Runtime {
    fn drop(&mut self) {
        let session_handles = self
            .session_mgr
            .lock()
            .map(|mut mgr| mgr.take_session_shutdown_handles())
            .unwrap_or_default();
        for handle in session_handles {
            let _ = handle.join();
        }
        if let Ok(mut outputs) = self.session_outputs.lock() {
            outputs.clear();
        }
        let _ = self.dispatch_shutdown_tx.try_send(());
        if let Some(handle) = self.dispatch_thread.get_mut().ok().and_then(Option::take) {
            let _ = handle.join();
        }
        bridge::release_embedded_cbor_codec();
    }
}

pub struct EmbeddedV8SessionHandle {
    session_id: String,
    runtime: Arc<EmbeddedV8Runtime>,
}

impl EmbeddedV8SessionHandle {
    // In-process runtime protocol call; the arg list mirrors the Execute frame.
    #[allow(clippy::too_many_arguments)]
    pub fn execute(
        &self,
        mode: u8,
        file_path: String,
        bridge_code: String,
        post_restore_script: String,
        userland_code: String,
        high_resolution_time: bool,
        user_code: String,
        wasm_module_bytes: Option<Arc<Vec<u8>>>,
    ) -> io::Result<()> {
        validate_execute_mode(mode)?;
        self.runtime.dispatch(RuntimeCommand::SendToSession {
            session_id: self.session_id.clone(),
            message: SessionMessage::Execute {
                mode,
                file_path,
                bridge_code,
                post_restore_script,
                userland_code,
                high_resolution_time,
                user_code,
                wasm_module_bytes,
            },
        })
    }

    pub fn send_bridge_response(
        &self,
        call_id: u64,
        status: u8,
        payload: Vec<u8>,
    ) -> io::Result<()> {
        validate_bridge_response_status(status)?;
        self.runtime.dispatch(RuntimeCommand::SendToSession {
            session_id: self.session_id.clone(),
            message: SessionMessage::BridgeResponse(BridgeResponse {
                call_id,
                status,
                payload,
            }),
        })
    }

    pub fn send_stream_event(&self, event_type: &str, payload: Vec<u8>) -> io::Result<()> {
        self.runtime.dispatch(RuntimeCommand::SendToSession {
            session_id: self.session_id.clone(),
            message: SessionMessage::StreamEvent(StreamEvent {
                event_type: event_type.to_owned(),
                payload,
            }),
        })
    }

    /// Install a direct module-source reader on this session's thread so module
    /// loads read source directly instead of round-tripping the bridge. Routed
    /// through the dispatch thread (which owns the session manager).
    pub fn set_module_reader(
        &self,
        reader: Box<dyn crate::execution::GuestModuleReader>,
    ) -> io::Result<()> {
        self.runtime
            .dispatch(RuntimeCommand::SetSessionModuleReader {
                session_id: self.session_id.clone(),
                reader: ModuleReaderHandle::new(reader),
            })
    }

    pub fn terminate(&self) -> io::Result<()> {
        self.runtime.dispatch(RuntimeCommand::SendToSession {
            session_id: self.session_id.clone(),
            message: SessionMessage::TerminateExecution,
        })
    }

    pub fn destroy(&self) -> io::Result<()> {
        self.runtime.unregister_session(&self.session_id);
        self.runtime.dispatch(RuntimeCommand::DestroySession {
            session_id: self.session_id.clone(),
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

fn validate_execute_mode(mode: u8) -> io::Result<()> {
    if mode > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown Execute mode: {mode}"),
        ));
    }
    Ok(())
}

impl Clone for EmbeddedV8SessionHandle {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id.clone(),
            runtime: Arc::clone(&self.runtime),
        }
    }
}

pub fn shared_embedded_runtime() -> io::Result<Arc<EmbeddedV8Runtime>> {
    static SHARED_RUNTIME: OnceLock<Mutex<Weak<EmbeddedV8Runtime>>> = OnceLock::new();

    let shared_slot = SHARED_RUNTIME.get_or_init(|| Mutex::new(Weak::new()));
    let mut shared_guard = shared_slot
        .lock()
        .expect("shared embedded runtime init lock poisoned");
    if let Some(shared) = shared_guard.upgrade() {
        return Ok(shared);
    }

    let shared = Arc::new(EmbeddedV8Runtime::new(None)?);
    *shared_guard = Arc::downgrade(&shared);
    Ok(shared)
}

pub struct EmbeddedRuntimeHandle {
    alive: Arc<AtomicBool>,
    codec_released: AtomicBool,
    shutdown_stream: UnixStream,
    join_handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl EmbeddedRuntimeHandle {
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_stream.shutdown(Shutdown::Both);
        if let Ok(mut guard) = self.join_handle.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        self.release_codec();
    }

    fn release_codec(&self) {
        if !self.codec_released.swap(true, Ordering::AcqRel) {
            bridge::release_embedded_cbor_codec();
        }
    }
}

impl Drop for EmbeddedRuntimeHandle {
    fn drop(&mut self) {
        let _ = self.shutdown_stream.shutdown(Shutdown::Both);
        if let Some(handle) = self.join_handle.get_mut().ok().and_then(Option::take) {
            let _ = handle.join();
        }
        self.release_codec();
    }
}

pub fn spawn_embedded_runtime_ipc(
    max_concurrency: Option<usize>,
) -> io::Result<(UnixStream, EmbeddedRuntimeHandle)> {
    bridge::init_codec();
    bridge::acquire_embedded_cbor_codec();
    isolate::init_v8_platform();

    let (host_stream, runtime_stream) = UnixStream::pair()?;
    let shutdown_stream = host_stream.try_clone()?;
    let alive = Arc::new(AtomicBool::new(true));
    let alive_for_thread = Arc::clone(&alive);
    let max_concurrency = max_concurrency.unwrap_or_else(default_max_concurrency);

    let join_handle = thread::Builder::new()
        .name(String::from("agentos-v8-runtime"))
        .spawn(move || {
            run_embedded_runtime(runtime_stream, max_concurrency);
            alive_for_thread.store(false, Ordering::Release);
        })
        .inspect_err(|_| bridge::release_embedded_cbor_codec())?;

    Ok((
        host_stream,
        EmbeddedRuntimeHandle {
            alive,
            codec_released: AtomicBool::new(false),
            shutdown_stream,
            join_handle: Mutex::new(Some(join_handle)),
        },
    ))
}

fn default_max_concurrency() -> usize {
    thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
}

fn run_embedded_runtime(stream: UnixStream, max_concurrency: usize) {
    // Keep bridge-only, agent-SDK, and wasm-runner userland variants warm
    // without immediately evicting each other.
    let snapshot_cache = Arc::new(SnapshotCache::new(8));
    let writer_stream = match stream.try_clone() {
        Ok(writer_stream) => writer_stream,
        Err(error) => {
            eprintln!("embedded V8 runtime failed to clone stream: {error}");
            return;
        }
    };
    let (event_tx, event_rx) = crossbeam_channel::bounded::<RuntimeEventEnvelope>(1024);
    let call_id_router: CallIdRouter = Arc::new(Mutex::new(HashMap::new()));
    let connection_id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);

    let writer_handle = match thread::Builder::new()
        .name(format!("v8-ipc-writer-{connection_id}"))
        .spawn(move || ipc_writer_thread(event_rx, writer_stream))
    {
        Ok(handle) => handle,
        Err(error) => {
            eprintln!("embedded V8 runtime failed to spawn writer thread: {error}");
            return;
        }
    };

    let session_mgr = Arc::new(Mutex::new(SessionManager::new(
        max_concurrency,
        event_tx,
        call_id_router,
        Arc::clone(&snapshot_cache),
    )));

    handle_connection(stream, connection_id, session_mgr, snapshot_cache);
    let _ = writer_handle.join();
}

fn ipc_writer_thread(
    rx: crossbeam_channel::Receiver<RuntimeEventEnvelope>,
    mut writer: UnixStream,
) {
    while let Ok(envelope) = rx.recv() {
        let frame: BinaryFrame = envelope.event.into();
        let bytes = match crate::ipc_binary::frame_to_bytes(&frame) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("embedded V8 runtime writer encode error: {error}");
                break;
            }
        };
        if let Err(error) = writer.write_all(&bytes) {
            eprintln!("embedded V8 runtime writer error: {error}");
            break;
        }
    }
}

fn handle_connection(
    mut stream: UnixStream,
    connection_id: u64,
    session_mgr: Arc<Mutex<SessionManager>>,
    snapshot_cache: Arc<SnapshotCache>,
) {
    let mut session_ids = HashSet::new();

    loop {
        let frame = match crate::ipc_binary::read_frame(&mut stream) {
            Ok(frame) => frame,
            Err(ref error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => {
                eprintln!("embedded V8 runtime read error on connection {connection_id}: {error}");
                break;
            }
        };

        let command = match RuntimeCommand::try_from(frame) {
            Ok(command) => command,
            Err(error) => {
                eprintln!(
                    "embedded V8 runtime dispatch error on connection {connection_id}: {error}"
                );
                continue;
            }
        };

        if let RuntimeCommand::CreateSession { session_id, .. } = &command {
            session_ids.insert(session_id.clone());
        } else if let RuntimeCommand::DestroySession { session_id } = &command {
            session_ids.remove(session_id);
        }

        if let Err(error) = dispatch_runtime_command(&session_mgr, &snapshot_cache, command) {
            eprintln!("embedded V8 runtime dispatch error on connection {connection_id}: {error}");
        }
    }

    let shutdowns = {
        let mut mgr = session_mgr.lock().expect("session manager lock poisoned");
        mgr.begin_destroy_sessions(session_ids)
    };
    for shutdown in shutdowns {
        shutdown.finish();
    }
}

fn dispatch_runtime_command(
    session_mgr: &Arc<Mutex<SessionManager>>,
    snapshot_cache: &Arc<SnapshotCache>,
    command: RuntimeCommand,
) -> io::Result<()> {
    match command {
        RuntimeCommand::CreateSession {
            session_id,
            heap_limit_mb,
            cpu_time_limit_ms,
            wall_clock_limit_ms,
            warm_hint,
        } => {
            let mut mgr = session_mgr.lock().expect("session manager lock poisoned");
            mgr.create_session_with_output_generation(
                session_id,
                heap_limit_mb,
                cpu_time_limit_ms,
                wall_clock_limit_ms,
                None,
                warm_hint,
            )
            .map_err(other_io_error)
        }
        RuntimeCommand::DestroySession { session_id } => {
            let shutdown = {
                let mut mgr = session_mgr.lock().expect("session manager lock poisoned");
                mgr.begin_destroy_session(&session_id)
                    .map_err(other_io_error)?
            };
            shutdown.finish();
            Ok(())
        }
        RuntimeCommand::SendToSession {
            session_id,
            message,
        } => {
            // Resolve the sender and apply terminate side effects under the
            // lock, then send after releasing it so a full session command
            // channel cannot block the manager mutex.
            let is_bridge_response = matches!(&message, SessionMessage::BridgeResponse(_));
            let sender = {
                let mgr = session_mgr.lock().expect("session manager lock poisoned");
                let routed_session_id = match &message {
                    SessionMessage::BridgeResponse(response) => {
                        let phase_start = Instant::now();
                        let routed_session_id = mgr
                            .call_id_router()
                            .lock()
                            .expect("call_id router lock poisoned")
                            .remove(&response.call_id)
                            .unwrap_or(session_id);
                        record_sync_bridge_host_phase(
                            "sync_rpc_dispatch",
                            "dispatch_route_lookup",
                            phase_start.elapsed(),
                        );
                        routed_session_id
                    }
                    SessionMessage::InjectGlobals { .. }
                    | SessionMessage::Execute { .. }
                    | SessionMessage::StreamEvent(_)
                    | SessionMessage::TerminateExecution => session_id,
                };
                let phase_start = Instant::now();
                let sender = mgr
                    .session_command_sender(&routed_session_id, &message)
                    .map_err(other_io_error)?;
                if is_bridge_response {
                    record_sync_bridge_host_phase(
                        "sync_rpc_dispatch",
                        "dispatch_sender_lookup",
                        phase_start.elapsed(),
                    );
                }
                sender
            };
            if let SessionMessage::BridgeResponse(response) = &message {
                record_sync_bridge_response_channel_send_start(response.call_id);
            }
            let phase_start = Instant::now();
            let result = sender
                .send(SessionCommand::Message(message))
                .map_err(|e| other_io_error(format!("session thread disconnected: {}", e)));
            if is_bridge_response {
                record_sync_bridge_host_phase(
                    "sync_rpc_dispatch",
                    "dispatch_channel_send",
                    phase_start.elapsed(),
                );
            }
            result
        }
        RuntimeCommand::SetSessionModuleReader { session_id, reader } => {
            // Resolve the sender under the lock, release, then forward the live
            // reader as a SetModuleReader command to the session thread.
            let sender = {
                let mgr = session_mgr.lock().expect("session manager lock poisoned");
                mgr.session_sender(&session_id)
            };
            let sender = sender.map_err(other_io_error)?;
            match reader.take() {
                Some(reader) => sender
                    .send(SessionCommand::SetModuleReader(reader))
                    .map_err(|e| other_io_error(format!("session thread disconnected: {}", e))),
                None => Ok(()),
            }
        }
        RuntimeCommand::WarmSnapshot {
            bridge_code,
            userland_code,
        } => {
            let flavor =
                crate::snapshot::StdlibFlavor::from_environment().map_err(other_io_error)?;
            let bridge_code = crate::snapshot::bridge_code_for_flavor(flavor, &bridge_code);
            snapshot_cache
                .get_or_create_with_flavor(
                    flavor,
                    &bridge_code,
                    (!userland_code.is_empty()).then_some(userland_code.as_str()),
                )
                .map(|_| ())
                .map_err(other_io_error)
        }
    }
}

fn route_outbound_event(
    envelope: RuntimeEventEnvelope,
    session_outputs: &Arc<Mutex<HashMap<String, SessionOutput>>>,
    session_mgr: &Arc<Mutex<SessionManager>>,
) -> bool {
    let RuntimeEventEnvelope {
        output_generation,
        event,
    } = envelope;
    let session_id = event.session_id().to_owned();

    let output = session_outputs
        .lock()
        .expect("embedded runtime session outputs lock poisoned")
        .get(&session_id)
        .cloned();

    let Some(output) = output else {
        clear_dropped_bridge_call_route(&event, session_mgr);
        return false;
    };

    if output_generation != Some(output.generation) {
        clear_dropped_bridge_call_route(&event, session_mgr);
        return false;
    }

    match output.sender.try_send(event) {
        Ok(()) => {}
        Err(mpsc::TrySendError::Full(_)) | Err(mpsc::TrySendError::Disconnected(_)) => {
            if remove_session_output_if_current(session_outputs, &session_id, output.generation) {
                return session_mgr
                    .lock()
                    .expect("session manager lock poisoned")
                    .detach_session_if_output_generation(&session_id, output.generation)
                    .unwrap_or(false);
            }
        }
    }
    false
}

fn clear_dropped_bridge_call_route(event: &RuntimeEvent, session_mgr: &Arc<Mutex<SessionManager>>) {
    if let RuntimeEvent::BridgeCall { call_id, .. } = event {
        session_mgr
            .lock()
            .expect("session manager lock poisoned")
            .clear_call_route(*call_id);
    }
}

fn remove_session_output_if_current(
    session_outputs: &Arc<Mutex<HashMap<String, SessionOutput>>>,
    session_id: &str,
    generation: u64,
) -> bool {
    let mut outputs = session_outputs
        .lock()
        .expect("embedded runtime session outputs lock poisoned");
    if outputs
        .get(session_id)
        .is_some_and(|output| output.generation == generation)
    {
        outputs.remove(session_id);
        return true;
    }
    false
}

fn other_io_error(message: String) -> io::Error {
    io::Error::other(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_protocol::{BridgeResponse, RuntimeCommand, RuntimeEvent, SessionMessage};
    use std::time::Duration;

    static EMBEDDED_RUNTIME_CODEC_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn embedded_runtime_handle_reports_liveness_and_shutdown() {
        let _codec_guard = EMBEDDED_RUNTIME_CODEC_TEST_LOCK
            .lock()
            .expect("embedded runtime codec test lock poisoned");
        let (_stream, handle) =
            spawn_embedded_runtime_ipc(Some(1)).expect("spawn embedded runtime");
        assert!(
            handle.is_alive(),
            "embedded runtime should be alive after spawn"
        );
        handle.shutdown();
        assert!(
            !handle.is_alive(),
            "embedded runtime should report not alive after shutdown"
        );
    }

    #[test]
    fn embedded_runtime_session_shared_runtime_is_lazy() {
        let _codec_guard = EMBEDDED_RUNTIME_CODEC_TEST_LOCK
            .lock()
            .expect("embedded runtime codec test lock poisoned");
        let first = shared_embedded_runtime().expect("shared embedded runtime");
        let second = shared_embedded_runtime().expect("shared embedded runtime");
        assert!(
            Arc::ptr_eq(&first, &second),
            "shared_embedded_runtime() should reuse the same runtime instance"
        );
    }

    #[test]
    fn embedded_runtime_drop_releases_codec_after_destroying_sessions() {
        let _codec_guard = EMBEDDED_RUNTIME_CODEC_TEST_LOCK
            .lock()
            .expect("embedded runtime codec test lock poisoned");
        let codec_before = bridge::is_cbor_codec();
        let alive = {
            let runtime = EmbeddedV8Runtime::new(Some(1)).expect("embedded runtime");
            let alive = Arc::clone(&runtime.alive);
            assert!(
                bridge::is_cbor_codec(),
                "embedded runtime should enable the CBOR bridge codec while alive"
            );
            let (_receiver, _registration) = runtime
                .register_session_with_output_registration("drop-lifecycle")
                .expect("register session output");
            runtime
                .dispatch(RuntimeCommand::CreateSession {
                    session_id: "drop-lifecycle".into(),
                    heap_limit_mb: None,
                    cpu_time_limit_ms: None,
                    wall_clock_limit_ms: None,
                    warm_hint: None,
                })
                .expect("create session");
            assert_eq!(
                runtime.session_count(),
                1,
                "test should drop a runtime with a live session"
            );
            alive
        };

        assert!(
            !alive.load(Ordering::Acquire),
            "dropping embedded runtime should stop the dispatch thread"
        );
        assert_eq!(
            bridge::is_cbor_codec(),
            codec_before,
            "dropping embedded runtime should restore the prior codec state"
        );
    }

    #[test]
    fn embedded_runtime_stream_bridge_response_routing_prefers_call_id_router() {
        let snapshot_cache = Arc::new(SnapshotCache::new(1));
        let (event_tx, _event_rx) = crossbeam_channel::unbounded::<RuntimeEventEnvelope>();
        let call_id_router: CallIdRouter = Arc::new(Mutex::new(HashMap::new()));
        let session_mgr = Arc::new(Mutex::new(SessionManager::new(
            1,
            event_tx,
            Arc::clone(&call_id_router),
            Arc::clone(&snapshot_cache),
        )));

        {
            let mut mgr = session_mgr.lock().expect("session manager");
            mgr.create_session("stream-target".into(), None, None, None)
                .expect("create target session");
        }
        call_id_router
            .lock()
            .expect("call_id router")
            .insert(41, "stream-target".into());

        dispatch_runtime_command(
            &session_mgr,
            &snapshot_cache,
            RuntimeCommand::SendToSession {
                session_id: "wrong-session".into(),
                message: SessionMessage::BridgeResponse(BridgeResponse {
                    call_id: 41,
                    status: 0,
                    payload: vec![0xAB],
                }),
            },
        )
        .expect("bridge response should route via call_id table");

        assert!(
            call_id_router
                .lock()
                .expect("call_id router")
                .get(&41)
                .is_none(),
            "bridge response routing should consume the call_id entry"
        );

        session_mgr
            .lock()
            .expect("session manager")
            .destroy_session("stream-target")
            .expect("destroy target session");
    }

    #[test]
    fn embedded_runtime_session_handle_rejects_unknown_bridge_response_status() {
        let _codec_guard = EMBEDDED_RUNTIME_CODEC_TEST_LOCK
            .lock()
            .expect("embedded runtime codec test lock poisoned");
        let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1)).expect("embedded runtime"));
        let handle = runtime.session_handle("missing-session".into());

        let err = handle
            .send_bridge_response(1, 3, Vec::new())
            .expect_err("unknown bridge response status should be rejected");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("unknown BridgeResponse status"));
    }

    #[test]
    fn embedded_runtime_stream_events_preserve_order_per_session() {
        let (sender, receiver) = mpsc::sync_channel(SESSION_OUTPUT_CHANNEL_CAPACITY);
        let session_outputs = Arc::new(Mutex::new(HashMap::from([(
            String::from("stream-order"),
            SessionOutput {
                generation: 1,
                sender,
            },
        )])));
        let session_mgr = test_session_manager();

        route_outbound_event(
            runtime_envelope(
                1,
                RuntimeEvent::Log {
                    session_id: "stream-order".into(),
                    channel: 0,
                    message: "first".into(),
                },
            ),
            &session_outputs,
            &session_mgr,
        );
        route_outbound_event(
            runtime_envelope(
                1,
                RuntimeEvent::StreamCallback {
                    session_id: "stream-order".into(),
                    callback_type: "stdin".into(),
                    payload: vec![1, 2, 3],
                },
            ),
            &session_outputs,
            &session_mgr,
        );

        let first = receiver
            .recv_timeout(Duration::from_millis(100))
            .expect("first event");
        let second = receiver
            .recv_timeout(Duration::from_millis(100))
            .expect("second event");

        assert!(matches!(
            first,
            RuntimeEvent::Log { ref message, .. } if message == "first"
        ));
        assert!(matches!(
            second,
            RuntimeEvent::StreamCallback { ref callback_type, ref payload, .. }
                if callback_type == "stdin" && payload == &vec![1, 2, 3]
        ));
    }

    #[test]
    fn embedded_runtime_stream_termination_race_drops_late_events_after_receiver_close() {
        let (sender, receiver) = mpsc::sync_channel(SESSION_OUTPUT_CHANNEL_CAPACITY);
        let session_outputs = Arc::new(Mutex::new(HashMap::from([(
            String::from("stream-race"),
            SessionOutput {
                generation: 1,
                sender,
            },
        )])));
        let session_mgr = test_session_manager();
        drop(receiver);

        route_outbound_event(
            runtime_envelope(
                1,
                RuntimeEvent::ExecutionResult {
                    session_id: "stream-race".into(),
                    exit_code: 0,
                    exports: None,
                    error: None,
                },
            ),
            &session_outputs,
            &session_mgr,
        );

        assert!(
            session_outputs
                .lock()
                .expect("session outputs")
                .get("stream-race")
                .is_none(),
            "late events should drop stale receiver registrations during teardown races"
        );
    }

    #[test]
    fn embedded_runtime_stream_backpressure_drops_full_session_output() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let session_outputs = Arc::new(Mutex::new(HashMap::from([(
            String::from("stream-full"),
            SessionOutput {
                generation: 1,
                sender,
            },
        )])));
        let session_mgr = test_session_manager_with_session("stream-full");

        route_outbound_event(
            runtime_envelope(
                1,
                RuntimeEvent::Log {
                    session_id: "stream-full".into(),
                    channel: 0,
                    message: "first".into(),
                },
            ),
            &session_outputs,
            &session_mgr,
        );
        let cleaned_up = route_outbound_event(
            runtime_envelope(
                1,
                RuntimeEvent::Log {
                    session_id: "stream-full".into(),
                    channel: 0,
                    message: "second".into(),
                },
            ),
            &session_outputs,
            &session_mgr,
        );
        assert!(cleaned_up, "full session output should detach the session");

        let first = receiver
            .recv_timeout(Duration::from_millis(100))
            .expect("first event");
        assert!(matches!(
            first,
            RuntimeEvent::Log { ref message, .. } if message == "first"
        ));
        assert!(
            receiver.recv_timeout(Duration::from_millis(20)).is_err(),
            "full session output should drop the overflowing event"
        );
        assert!(
            session_outputs
                .lock()
                .expect("session outputs")
                .get("stream-full")
                .is_none(),
            "full session output should remove the stale registration"
        );
        assert_eq!(
            session_mgr.lock().expect("session manager").session_count(),
            0,
            "full session output should destroy the runtime session"
        );
    }

    #[test]
    fn embedded_runtime_drops_stale_generation_events_for_reused_session_id() {
        let (sender, receiver) = mpsc::sync_channel(SESSION_OUTPUT_CHANNEL_CAPACITY);
        let session_outputs = Arc::new(Mutex::new(HashMap::from([(
            String::from("stream-reused"),
            SessionOutput {
                generation: 2,
                sender,
            },
        )])));
        let session_mgr = test_session_manager_with_generation("stream-reused", 2);
        session_mgr
            .lock()
            .expect("session manager")
            .call_id_router()
            .lock()
            .expect("call_id router")
            .insert(99, "stream-reused".into());

        let routed = route_outbound_event(
            runtime_envelope(
                1,
                RuntimeEvent::BridgeCall {
                    session_id: "stream-reused".into(),
                    call_id: 99,
                    method: "_stale".into(),
                    payload: Vec::new(),
                },
            ),
            &session_outputs,
            &session_mgr,
        );

        assert!(!routed, "stale generation event should not trigger cleanup");
        assert!(
            receiver.recv_timeout(Duration::from_millis(20)).is_err(),
            "stale generation event should not reach reused session output"
        );
        assert_eq!(
            session_mgr.lock().expect("session manager").session_count(),
            1,
            "stale generation event must leave reused session alive"
        );
        assert!(
            session_mgr
                .lock()
                .expect("session manager")
                .call_id_router()
                .lock()
                .expect("call_id router")
                .get(&99)
                .is_none(),
            "stale bridge calls should clear their call route"
        );
    }

    #[test]
    fn embedded_runtime_clears_bridge_route_when_output_is_missing() {
        let session_outputs = Arc::new(Mutex::new(HashMap::new()));
        let session_mgr = test_session_manager();
        session_mgr
            .lock()
            .expect("session manager")
            .call_id_router()
            .lock()
            .expect("call_id router")
            .insert(123, "stream-detached".into());

        let routed = route_outbound_event(
            runtime_envelope(
                1,
                RuntimeEvent::BridgeCall {
                    session_id: "stream-detached".into(),
                    call_id: 123,
                    method: "_detached".into(),
                    payload: Vec::new(),
                },
            ),
            &session_outputs,
            &session_mgr,
        );

        assert!(!routed, "missing output should not route the bridge call");
        assert!(
            session_mgr
                .lock()
                .expect("session manager")
                .call_id_router()
                .lock()
                .expect("call_id router")
                .get(&123)
                .is_none(),
            "bridge calls dropped with no output should clear their call route"
        );
    }

    #[test]
    fn embedded_runtime_stale_output_registration_cannot_destroy_reused_session_id() {
        let _codec_guard = EMBEDDED_RUNTIME_CODEC_TEST_LOCK
            .lock()
            .expect("embedded runtime codec test lock poisoned");
        let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1)).expect("embedded runtime"));
        let session_id = "stream-generation-reuse";
        let (_first_receiver, first_registration) = runtime
            .register_session_with_capacity(session_id, 1)
            .expect("register first session output");
        runtime
            .dispatch(RuntimeCommand::CreateSession {
                session_id: session_id.into(),
                heap_limit_mb: None,
                cpu_time_limit_ms: None,
                wall_clock_limit_ms: None,
                warm_hint: None,
            })
            .expect("create first session");
        runtime
            .session_handle(session_id.into())
            .destroy()
            .expect("destroy first session");

        let (_second_receiver, _second_registration) = runtime
            .register_session_with_capacity(session_id, 1)
            .expect("register reused session output");
        runtime
            .dispatch(RuntimeCommand::CreateSession {
                session_id: session_id.into(),
                heap_limit_mb: None,
                cpu_time_limit_ms: None,
                wall_clock_limit_ms: None,
                warm_hint: None,
            })
            .expect("create reused session");

        assert!(
            !runtime
                .destroy_session_if_output_current(&first_registration)
                .expect("stale destroy should be ignored"),
            "stale registration should not match the reused session output"
        );
        assert_eq!(
            runtime.session_count(),
            1,
            "stale registration must not destroy the reused session"
        );

        runtime
            .session_handle(session_id.into())
            .destroy()
            .expect("destroy reused session");
    }

    #[test]
    fn session_cleanup_generation_guard_does_not_destroy_reused_session_id() {
        let session_mgr = test_session_manager();
        {
            let mut mgr = session_mgr.lock().expect("session manager");
            mgr.create_session_with_output_generation(
                "reused".into(),
                None,
                None,
                None,
                Some(1),
                None,
            )
            .expect("create first session");
            mgr.destroy_session("reused")
                .expect("destroy first session");
            mgr.create_session_with_output_generation(
                "reused".into(),
                None,
                None,
                None,
                Some(2),
                None,
            )
            .expect("create reused session");

            assert!(
                !mgr.destroy_session_if_output_generation("reused", 1)
                    .expect("stale generation destroy should be ignored"),
                "stale cleanup generation should not match reused session"
            );
            assert_eq!(
                mgr.session_count(),
                1,
                "stale cleanup generation must leave reused session alive"
            );
            mgr.destroy_session("reused")
                .expect("destroy reused session");
        }
    }

    fn test_session_manager() -> Arc<Mutex<SessionManager>> {
        let (event_tx, _event_rx) = crossbeam_channel::bounded::<RuntimeEventEnvelope>(1);
        Arc::new(Mutex::new(SessionManager::new(
            1,
            event_tx,
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(SnapshotCache::new(1)),
        )))
    }

    fn runtime_envelope(output_generation: u64, event: RuntimeEvent) -> RuntimeEventEnvelope {
        RuntimeEventEnvelope {
            output_generation: Some(output_generation),
            event,
        }
    }

    fn test_session_manager_with_session(session_id: &str) -> Arc<Mutex<SessionManager>> {
        test_session_manager_with_generation(session_id, 1)
    }

    fn test_session_manager_with_generation(
        session_id: &str,
        output_generation: u64,
    ) -> Arc<Mutex<SessionManager>> {
        let session_mgr = test_session_manager();
        session_mgr
            .lock()
            .expect("session manager")
            .create_session_with_output_generation(
                session_id.into(),
                None,
                None,
                None,
                Some(output_generation),
                None,
            )
            .expect("create test session");
        session_mgr
    }
}
