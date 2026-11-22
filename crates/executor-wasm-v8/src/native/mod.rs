//! Standalone WebAssembly on V8 with native Rust host imports.
//!
//! The guest module is instantiated from Rust in its own isolate, and every
//! host import is a Rust callback running the same handlers as the Wasmtime
//! executor (`agentos-executor-wasm-host`). No JavaScript runs in the guest
//! path, and host requests reach the kernel through the same typed host
//! capabilities and `HostCall` events Wasmtime uses.

mod block_on;
mod error;
mod guest;
mod module;
mod pool;
mod run;

use agentos_driver_tokio::DriverHandle;
use agentos_executor_contract::backend::{
    ExecutionWakeIdentity, HostServiceError, PublishedSignalCheckpoint,
};
use agentos_executor_contract::host::ProcessHostCapabilitySet;
use agentos_executor_wasm_abi::{
    StartWasmExecutionRequest, WasmExecutionError, WasmExecutionEvent,
};
use agentos_executor_wasm_host::QueuedWasmEvent;
use flume::Receiver;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

/// An image the vm loaded and checked at spawn, handed to the execution so
/// it is neither read nor hashed again.
#[derive(Clone, Debug)]
pub struct PreloadedImage {
    bytes: Arc<[u8]>,
    digest: [u8; 32],
}

/// Check an image at spawn time against the locked proposal profile, so a
/// malformed module fails the spawn as `execve` would. Shares the digest cache
/// the execution uses, so an image is validated once per process.
pub fn preflight_native_image(bytes: Vec<u8>) -> Result<PreloadedImage, HostServiceError> {
    let digest = module::validate_profile(&bytes)?;
    Ok(PreloadedImage {
        bytes: Arc::from(bytes),
        digest,
    })
}

/// State shared between the execution handle, the guest thread, and its
/// watchdog.
pub(crate) struct Control {
    cancelled: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
    signal_pending: Arc<AtomicBool>,
    started: AtomicBool,
    start_notify: Notify,
    host: Mutex<Option<ProcessHostCapabilitySet>>,
    host_notify: Notify,
    signal_checkpoints: Mutex<VecDeque<(ExecutionWakeIdentity, PublishedSignalCheckpoint)>>,
    max_signal_checkpoints: usize,
    /// Set by the guest thread once its isolate exists.
    isolate: Mutex<Option<v8::IsolateHandle>>,
    /// A limit or cancellation that ended the guest from outside it.
    abort: Mutex<Option<HostServiceError>>,
    paused: Mutex<bool>,
    pause_changed: Condvar,
    /// When the execution was requested, for launch diagnostics.
    spawned_at: std::time::Instant,
}

impl Control {
    fn new(started: bool, max_signal_checkpoints: usize) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            cancel_notify: Arc::new(Notify::new()),
            signal_pending: Arc::new(AtomicBool::new(false)),
            started: AtomicBool::new(started),
            start_notify: Notify::new(),
            host: Mutex::new(None),
            host_notify: Notify::new(),
            signal_checkpoints: Mutex::new(VecDeque::new()),
            max_signal_checkpoints: max_signal_checkpoints.max(1),
            isolate: Mutex::new(None),
            abort: Mutex::new(None),
            paused: Mutex::new(false),
            pause_changed: Condvar::new(),
            spawned_at: std::time::Instant::now(),
        }
    }

    fn checkpoints(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, VecDeque<(ExecutionWakeIdentity, PublishedSignalCheckpoint)>>,
        HostServiceError,
    > {
        self.signal_checkpoints.lock().map_err(|_| {
            HostServiceError::new(
                "EIO",
                "ERR_AGENTOS_WASM_SIGNAL_INBOX_POISONED: signal checkpoint state is poisoned",
            )
        })
    }

    fn publish_signal(
        &self,
        identity: ExecutionWakeIdentity,
        delivery: PublishedSignalCheckpoint,
    ) -> Result<(), HostServiceError> {
        let mut checkpoints = self.checkpoints()?;
        if checkpoints.len() >= self.max_signal_checkpoints {
            return Err(HostServiceError::new(
                "ERR_AGENTOS_WASMTIME_SIGNAL_CHECKPOINT_LIMIT",
                "WebAssembly signal checkpoint inbox is full",
            )
            .with_details(serde_json::json!({
                "limitName": "limits.process.pendingEventCount",
                "limit": self.max_signal_checkpoints,
            })));
        }
        if checkpoints.len().saturating_add(1) * 5 >= self.max_signal_checkpoints * 4 {
            eprintln!(
                "WARN_AGENTOS_WASMTIME_SIGNAL_CHECKPOINT_LIMIT: signal checkpoint inbox is at {}/{} entries",
                checkpoints.len().saturating_add(1),
                self.max_signal_checkpoints
            );
        }
        checkpoints.push_back((identity, delivery));
        self.signal_pending.store(true, Ordering::Release);
        Ok(())
    }

    fn take_signal(
        &self,
        identity: ExecutionWakeIdentity,
        thread_id: Option<u32>,
    ) -> Result<Option<PublishedSignalCheckpoint>, HostServiceError> {
        let mut checkpoints = self.checkpoints()?;
        let Some(position) = checkpoints.iter().position(|(pending, delivery)| {
            *pending == identity
                && thread_id.is_none_or(|thread_id| delivery.thread_id == thread_id)
        }) else {
            return Ok(None);
        };
        let delivery = checkpoints.remove(position).map(|(_, delivery)| delivery);
        self.signal_pending
            .store(!checkpoints.is_empty(), Ordering::Release);
        Ok(delivery)
    }

    fn discard_signals(&self, identity: ExecutionWakeIdentity) -> Result<(), HostServiceError> {
        let mut checkpoints = self.checkpoints()?;
        checkpoints.retain(|(pending, _)| *pending != identity);
        self.signal_pending
            .store(!checkpoints.is_empty(), Ordering::Release);
        Ok(())
    }

    fn terminate_guest(&self) {
        if let Ok(isolate) = self.isolate.lock() {
            if let Some(isolate) = isolate.as_ref() {
                isolate.terminate_execution();
            }
        }
    }

    /// End the guest from outside: record why (the first reason wins), wake
    /// any host wait, and terminate guest code.
    pub(crate) fn abort(&self, reason: HostServiceError) {
        if let Ok(mut abort) = self.abort.lock() {
            abort.get_or_insert(reason);
        }
        self.interrupt();
    }

    fn interrupt(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.started.store(true, Ordering::Release);
        self.start_notify.notify_waiters();
        self.host_notify.notify_waiters();
        self.cancel_notify.notify_waiters();
        self.set_paused(false);
        self.terminate_guest();
    }

    fn take_abort(&self) -> Option<HostServiceError> {
        self.abort.lock().ok().and_then(|mut abort| abort.take())
    }

    fn set_paused(&self, paused: bool) {
        let Ok(mut state) = self.paused.lock() else {
            eprintln!("ERR_AGENTOS_WASM_PAUSE_POISONED: pause state is poisoned");
            return;
        };
        *state = paused && !self.cancelled.load(Ordering::Acquire);
        self.pause_changed.notify_all();
    }

    /// Block the calling (guest) thread while the execution is paused.
    fn wait_while_paused(&self) {
        let Ok(mut paused) = self.paused.lock() else {
            return;
        };
        while *paused {
            paused = match self.pause_changed.wait(paused) {
                Ok(paused) => paused,
                Err(_) => return,
            };
        }
    }
}

/// Runs on the guest thread between guest instructions, at V8's request.
unsafe extern "C" fn pause_interrupt(
    _isolate: v8::UnsafeRawIsolatePtr,
    data: *mut std::ffi::c_void,
) {
    // SAFETY: `data` is an `Arc<Control>` leaked by `set_paused` for exactly
    // this callback.
    let control = unsafe { Arc::from_raw(data as *const Control) };
    control.wait_while_paused();
}

/// A standalone WebAssembly execution on V8 with native host imports.
pub struct NativeWasmV8Execution {
    execution_id: String,
    events: Receiver<QueuedWasmEvent>,
    control: Arc<Control>,
    worker_done: Receiver<()>,
    teardown_timeout: Duration,
    prepared: bool,
}

impl std::fmt::Debug for NativeWasmV8Execution {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeWasmV8Execution")
            .field("execution_id", &self.execution_id)
            .field("prepared", &self.prepared)
            .finish_non_exhaustive()
    }
}

impl NativeWasmV8Execution {
    pub fn spawn(
        execution_id: String,
        module_path: String,
        module_image: Option<PreloadedImage>,
        request: StartWasmExecutionRequest,
        runtime: DriverHandle,
        event_notify: Option<Arc<Notify>>,
        defer_execute: bool,
    ) -> Result<Self, WasmExecutionError> {
        crate::reject_v8_deterministic_fuel(&request)?;
        crate::resolve_wasm_stack_limit_bytes(&request)?;
        let permit = runtime
            .vm_executor_admission()
            .try_acquire()
            .map_err(|error| {
                WasmExecutionError::Host(
                    HostServiceError::new("ERR_AGENTOS_VM_EXECUTOR_LIMIT", error.to_string())
                        .with_details(serde_json::json!({
                            "limitName": "runtime.maxActiveVmExecutors",
                            "limit": runtime.max_active_vm_executors(),
                        })),
                )
            })?;
        let pending_count = request.limits.pending_event_count.unwrap_or(64).max(1);
        let (event_sender, events) = flume::bounded(pending_count);
        let (done_sender, worker_done) = flume::bounded(1);
        let control = Arc::new(Control::new(!defer_execute, pending_count));
        let worker_control = Arc::clone(&control);
        let worker_runtime = runtime.clone();
        let heap_limit_mb = request
            .limits
            .runner_heap_limit_mb
            .unwrap_or(crate::DEFAULT_WASM_ISOLATE_HEAP_LIMIT_MB);
        pool::launch(
            format!("agentos-v8-wasm-{execution_id}"),
            heap_limit_mb,
            Box::new(move |guest_isolate| {
                let _permit = permit;
                run::guest_thread(
                    guest_isolate,
                    module_path,
                    module_image,
                    request,
                    worker_runtime,
                    worker_control,
                    event_sender,
                    event_notify,
                );
                if done_sender.send(()).is_err() {
                    eprintln!(
                        "ERR_AGENTOS_WASM_TEARDOWN_CHANNEL: guest completion receiver was dropped"
                    );
                }
            }),
        )
        .map_err(WasmExecutionError::Spawn)?;
        Ok(Self {
            execution_id,
            events,
            control,
            worker_done,
            teardown_timeout: runtime.vm_executor_teardown_timeout(),
            prepared: defer_execute,
        })
    }

    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }

    pub fn configure_host_services(&self, host: ProcessHostCapabilitySet) {
        match self.control.host.lock() {
            Ok(mut slot) if slot.is_none() => {
                *slot = Some(host);
                self.control.host_notify.notify_waiters();
            }
            Ok(_) => eprintln!(
                "ERR_AGENTOS_WASM_HOST_ALREADY_BOUND: ignored duplicate host capability binding"
            ),
            Err(_) => {
                eprintln!("ERR_AGENTOS_WASM_HOST_LATCH_POISONED: failed to bind host capabilities")
            }
        }
    }

    pub fn is_prepared_for_start(&self) -> bool {
        self.prepared && !self.control.started.load(Ordering::Acquire)
    }

    pub fn start_prepared(&mut self) -> Result<(), WasmExecutionError> {
        if !self.prepared {
            return Err(WasmExecutionError::Host(HostServiceError::new(
                "ERR_AGENTOS_EXECUTION_NOT_PREPARED",
                "WebAssembly execution was not created as a prepared image",
            )));
        }
        if self.control.started.swap(true, Ordering::AcqRel) {
            return Err(WasmExecutionError::Host(HostServiceError::new(
                "EALREADY",
                "prepared WebAssembly execution has already started",
            )));
        }
        self.prepared = false;
        self.control.start_notify.notify_waiters();
        Ok(())
    }

    pub fn terminate(&self) {
        self.control.interrupt();
    }

    pub fn set_paused(&self, paused: bool) {
        self.control.set_paused(paused);
        if !paused {
            return;
        }
        let Ok(isolate) = self.control.isolate.lock() else {
            return;
        };
        if let Some(isolate) = isolate.as_ref() {
            let data = Arc::into_raw(Arc::clone(&self.control)) as *mut std::ffi::c_void;
            if !isolate.request_interrupt(pause_interrupt, data) {
                // The isolate is gone; reclaim the reference the callback
                // would have released.
                // SAFETY: `data` came from `Arc::into_raw` just above and was
                // not handed to V8.
                drop(unsafe { Arc::from_raw(data as *const Control) });
            }
        }
    }

    pub fn deliver_signal_checkpoint(
        &self,
        identity: ExecutionWakeIdentity,
        signal: i32,
        delivery_token: u64,
        flags: u32,
        thread_id: u32,
    ) -> Result<(), HostServiceError> {
        self.control.publish_signal(
            identity,
            PublishedSignalCheckpoint {
                signal,
                delivery_token,
                flags,
                thread_id,
            },
        )
    }

    pub fn take_signal_checkpoint(
        &self,
        identity: ExecutionWakeIdentity,
    ) -> Result<Option<PublishedSignalCheckpoint>, HostServiceError> {
        self.control.take_signal(identity, None)
    }

    pub fn take_signal_checkpoint_for_thread(
        &self,
        identity: ExecutionWakeIdentity,
        thread_id: u32,
    ) -> Result<Option<PublishedSignalCheckpoint>, HostServiceError> {
        self.control.take_signal(identity, Some(thread_id))
    }

    pub fn discard_signal_checkpoints(
        &self,
        identity: ExecutionWakeIdentity,
    ) -> Result<(), HostServiceError> {
        self.control.discard_signals(identity)
    }

    pub async fn poll_event(
        &self,
        timeout: Duration,
    ) -> Result<Option<WasmExecutionEvent>, WasmExecutionError> {
        match tokio::time::timeout(timeout, self.events.recv_async()).await {
            Ok(Ok(event)) => event.into_event().map(Some),
            Ok(Err(_)) => Err(WasmExecutionError::EventChannelClosed),
            Err(_) => Ok(None),
        }
    }

    pub fn has_pending_events(&self) -> bool {
        !self.events.is_empty()
    }

    pub fn try_poll_event(&self) -> Result<Option<WasmExecutionEvent>, WasmExecutionError> {
        match self.events.try_recv() {
            Ok(event) => event.into_event().map(Some),
            Err(flume::TryRecvError::Empty) => Ok(None),
            Err(flume::TryRecvError::Disconnected) => Err(WasmExecutionError::EventChannelClosed),
        }
    }

    pub fn poll_event_blocking(
        &self,
        timeout: Duration,
    ) -> Result<Option<WasmExecutionEvent>, WasmExecutionError> {
        match self.events.recv_timeout(timeout) {
            Ok(event) => event.into_event().map(Some),
            Err(flume::RecvTimeoutError::Timeout) => Ok(None),
            Err(flume::RecvTimeoutError::Disconnected) => {
                Err(WasmExecutionError::EventChannelClosed)
            }
        }
    }

    pub fn next_event_blocking(&self) -> Result<WasmExecutionEvent, WasmExecutionError> {
        self.events
            .recv()
            .map_err(|_| WasmExecutionError::EventChannelClosed)?
            .into_event()
    }

    /// Wait for the guest thread to finish. A pooled thread is not ours to
    /// join, so completion is its last message; a dropped channel means the
    /// guest panicked.
    fn join_worker(&self) {
        match self.worker_done.recv_timeout(self.teardown_timeout) {
            Ok(()) => {}
            Err(flume::RecvTimeoutError::Disconnected) => {
                eprintln!("ERR_AGENTOS_WASM_WORKER_PANIC: guest executor panicked");
            }
            Err(flume::RecvTimeoutError::Timeout) => eprintln!(
                "ERR_AGENTOS_WASM_TEARDOWN_TIMEOUT: guest executor did not stop within {} ms",
                self.teardown_timeout.as_millis()
            ),
        }
    }
}

impl Drop for NativeWasmV8Execution {
    fn drop(&mut self) {
        self.terminate();
        self.join_worker();
    }
}
