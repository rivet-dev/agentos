//! Bounded worker management for pthread-enabled WebAssembly instances.
//!
//! A Node VM has one root session isolate for all Node and user JavaScript.
//! Concurrent WASM pthreads run only `wasi_thread_start` in internal V8 worker
//! isolates which share the root instance's compiled module and linear-memory
//! backing store. This manager owns the host threads, V8 termination handles,
//! accounting, panic capture, and the all-worker teardown barrier. It does not
//! expose a JavaScript Worker API or create another WebAssembly engine.

use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use v8::{ValueDeserializerHelper, ValueSerializerHelper};

pub const DEFAULT_MAX_RUNTIME_THREADS: usize = 8;
pub const DEFAULT_RUNTIME_THREAD_WARNING_AT: usize = 7;
pub const DEFAULT_WASM_WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;
pub const DEFAULT_WASM_WORKER_TEARDOWN_GRACE: Duration = Duration::from_secs(5);
pub const MAX_RUNTIME_THREADS_FIELD: &str = "limits.nodeRuntime.maxThreads";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmWorkerLimits {
    /// Includes the root session thread.
    pub max_runtime_threads: usize,
    /// Includes the root session thread.
    pub warn_at_runtime_threads: usize,
    pub worker_stack_bytes: usize,
    pub teardown_grace: Duration,
}

impl Default for WasmWorkerLimits {
    fn default() -> Self {
        Self {
            max_runtime_threads: DEFAULT_MAX_RUNTIME_THREADS,
            warn_at_runtime_threads: DEFAULT_RUNTIME_THREAD_WARNING_AT,
            worker_stack_bytes: DEFAULT_WASM_WORKER_STACK_BYTES,
            teardown_grace: DEFAULT_WASM_WORKER_TEARDOWN_GRACE,
        }
    }
}

impl WasmWorkerLimits {
    fn validate(self) -> Result<Self, WasmWorkerError> {
        if self.max_runtime_threads == 0 {
            return Err(WasmWorkerError::InvalidLimit {
                field: MAX_RUNTIME_THREADS_FIELD,
                configured: 0,
                reason: "must include at least the root runtime thread",
            });
        }
        if self.warn_at_runtime_threads == 0
            || self.warn_at_runtime_threads > self.max_runtime_threads
        {
            return Err(WasmWorkerError::InvalidLimit {
                field: "limits.nodeRuntime.threadWarningAt",
                configured: self.warn_at_runtime_threads,
                reason: "must be between one and maxThreads",
            });
        }
        if self.worker_stack_bytes == 0 {
            return Err(WasmWorkerError::InvalidLimit {
                field: "limits.nodeRuntime.workerStackBytes",
                configured: 0,
                reason: "must be greater than zero",
            });
        }
        if self.teardown_grace.is_zero() {
            return Err(WasmWorkerError::InvalidLimit {
                field: "limits.nodeRuntime.maxTeardownGraceMs",
                configured: 0,
                reason: "must be greater than zero",
            });
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum WasmWorkerError {
    InvalidLimit {
        field: &'static str,
        configured: usize,
        reason: &'static str,
    },
    LimitExceeded {
        field: &'static str,
        configured: usize,
    },
    Terminating,
    Spawn(String),
    WorkerFailed {
        tid: i32,
        message: String,
    },
    WorkerPanicked {
        tid: i32,
        message: String,
    },
    TeardownDeadline {
        field: &'static str,
        configured_ms: u128,
        remaining_workers: usize,
    },
}

impl fmt::Display for WasmWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit {
                field,
                configured,
                reason,
            } => write!(formatter, "invalid {field}={configured}: {reason}"),
            Self::LimitExceeded { field, configured } => write!(
                formatter,
                "runtime thread limit {field}={configured} is exhausted; raise {field} to allow more threads"
            ),
            Self::Terminating => formatter.write_str("Node WASM worker manager is terminating"),
            Self::Spawn(message) => write!(formatter, "failed to spawn Node WASM worker: {message}"),
            Self::WorkerFailed { tid, message } => {
                write!(formatter, "Node WASM worker {tid} failed: {message}")
            }
            Self::WorkerPanicked { tid, message } => {
                write!(formatter, "Node WASM worker {tid} panicked: {message}")
            }
            Self::TeardownDeadline {
                field,
                configured_ms,
                remaining_workers,
            } => write!(
                formatter,
                "Node WASM teardown exceeded {field}={configured_ms}ms with {remaining_workers} worker(s) still live"
            ),
        }
    }
}

impl std::error::Error for WasmWorkerError {}

/// Per-worker cancellation state shared with the concrete V8 executor.
///
/// The executor installs its thread-safe V8 isolate handle before entering
/// guest code. A teardown request racing that installation is replayed
/// immediately, closing the spawn-versus-shutdown lost-wake window.
pub struct WasmWorkerControl {
    termination_requested: AtomicBool,
    isolate_handle: Mutex<Option<v8::IsolateHandle>>,
}

impl WasmWorkerControl {
    fn new() -> Self {
        Self {
            termination_requested: AtomicBool::new(false),
            isolate_handle: Mutex::new(None),
        }
    }

    pub fn install_isolate_handle(&self, handle: v8::IsolateHandle) {
        let terminate_now = self.termination_requested.load(Ordering::Acquire);
        *self
            .isolate_handle
            .lock()
            .expect("Node WASM worker isolate handle lock poisoned") = Some(handle.clone());
        if terminate_now {
            handle.terminate_execution();
        }
    }

    pub fn clear_isolate_handle(&self) {
        self.isolate_handle
            .lock()
            .expect("Node WASM worker isolate handle lock poisoned")
            .take();
    }

    pub fn termination_requested(&self) -> bool {
        self.termination_requested.load(Ordering::Acquire)
    }

    fn request_termination(&self) {
        self.termination_requested.store(true, Ordering::Release);
        if let Some(handle) = self
            .isolate_handle
            .lock()
            .expect("Node WASM worker isolate handle lock poisoned")
            .as_ref()
        {
            handle.terminate_execution();
        }
    }
}

/// Supplies one internal V8 worker instance. Implementations must instantiate
/// the already-compiled module with the shared memory, expose only the shared
/// POSIX provider and thread-safe Node-API subset, and call only
/// `wasi_thread_start(tid, start_arg)`.
pub trait WasmWorkerExecutor: Send + Sync + 'static {
    fn execute(
        &self,
        control: Arc<WasmWorkerControl>,
        tid: i32,
        start_arg: i32,
    ) -> Result<(), String>;

    /// Wake any provider-owned futex, poll, or syscall wait. V8 termination
    /// interrupts compute; the provider must use this hook for blocking imports.
    fn cancel_blocking_imports(&self) {}
}

struct SharedMemorySerializer {
    stores: Arc<Mutex<Vec<v8::SharedRef<v8::BackingStore>>>>,
}

impl v8::ValueSerializerImpl for SharedMemorySerializer {
    fn throw_data_clone_error<'s>(
        &self,
        scope: &mut v8::HandleScope<'s>,
        message: v8::Local<'s, v8::String>,
    ) {
        let exception = v8::Exception::error(scope, message);
        scope.throw_exception(exception);
    }

    fn get_shared_array_buffer_id<'s>(
        &self,
        _scope: &mut v8::HandleScope<'s>,
        buffer: v8::Local<'s, v8::SharedArrayBuffer>,
    ) -> Option<u32> {
        let mut stores = self
            .stores
            .lock()
            .expect("Node WASM shared backing-store lock poisoned");
        let id = u32::try_from(stores.len()).ok()?;
        stores.push(buffer.get_backing_store());
        Some(id)
    }
}

struct SharedMemoryDeserializer {
    stores: Arc<Vec<v8::SharedRef<v8::BackingStore>>>,
}

impl v8::ValueDeserializerImpl for SharedMemoryDeserializer {
    fn get_shared_array_buffer_from_id<'s>(
        &self,
        scope: &mut v8::HandleScope<'s>,
        transfer_id: u32,
    ) -> Option<v8::Local<'s, v8::SharedArrayBuffer>> {
        let store = self.stores.get(transfer_id as usize)?;
        Some(v8::SharedArrayBuffer::with_backing_store(scope, store))
    }
}

/// Concrete executor for internal V8 WASM-worker isolates.
///
/// `capture` must run in the root session isolate after it compiles the module
/// and creates the shared memory. The trusted bootstrap source receives only
/// `(module, memory, tid, startArg)` and must instantiate closure-private import
/// objects and invoke `wasi_thread_start`; it is runtime code, never guest input.
pub struct V8SharedWasmWorkerExecutor {
    module: Arc<v8::CompiledWasmModule>,
    memory_wire: Arc<Vec<u8>>,
    backing_stores: Arc<Vec<v8::SharedRef<v8::BackingStore>>>,
    bootstrap_source: &'static str,
    heap_limit_mb: Option<u32>,
}

impl V8SharedWasmWorkerExecutor {
    pub fn capture<'s>(
        scope: &mut v8::HandleScope<'s>,
        module: v8::Local<'s, v8::WasmModuleObject>,
        memory: v8::Local<'s, v8::WasmMemoryObject>,
        bootstrap_source: &'static str,
        heap_limit_mb: Option<u32>,
    ) -> Result<Self, String> {
        let stores = Arc::new(Mutex::new(Vec::new()));
        let serializer = v8::ValueSerializer::new(
            scope,
            Box::new(SharedMemorySerializer {
                stores: Arc::clone(&stores),
            }),
        );
        serializer.write_header();
        let context = scope.get_current_context();
        if serializer.write_value(context, memory.into()) != Some(true) {
            return Err("failed to serialize Node WASM shared memory".to_owned());
        }
        let memory_wire = Arc::new(serializer.release());
        let backing_stores = Arc::new(
            stores
                .lock()
                .expect("Node WASM shared backing-store lock poisoned")
                .clone(),
        );
        if backing_stores.is_empty() {
            return Err("Node WASM memory did not expose a shared backing store".to_owned());
        }
        Ok(Self {
            module: Arc::new(module.get_compiled_module()),
            memory_wire,
            backing_stores,
            bootstrap_source,
            heap_limit_mb,
        })
    }
}

impl WasmWorkerExecutor for V8SharedWasmWorkerExecutor {
    fn execute(
        &self,
        control: Arc<WasmWorkerControl>,
        tid: i32,
        start_arg: i32,
    ) -> Result<(), String> {
        let mut worker_isolate = crate::isolate::create_isolate(self.heap_limit_mb);
        control.install_isolate_handle(worker_isolate.thread_safe_handle());
        let context = crate::isolate::create_context(&mut worker_isolate);
        let result = (|| {
            let scope = &mut v8::HandleScope::new(&mut worker_isolate);
            let context = v8::Local::new(scope, &context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let module = v8::WasmModuleObject::from_compiled_module(scope, &self.module)
                .ok_or_else(|| "worker failed to recreate compiled Node WASM module".to_owned())?;
            let deserializer = v8::ValueDeserializer::new(
                scope,
                Box::new(SharedMemoryDeserializer {
                    stores: Arc::clone(&self.backing_stores),
                }),
                &self.memory_wire,
            );
            if deserializer.read_header(context) != Some(true) {
                return Err("worker failed to read Node WASM memory clone header".to_owned());
            }
            let memory = deserializer
                .read_value(context)
                .ok_or_else(|| "worker failed to clone Node WebAssembly.Memory".to_owned())?;
            if !memory.is_wasm_memory_object() {
                return Err("worker memory clone is not WebAssembly.Memory".to_owned());
            }
            let source = v8::String::new(scope, self.bootstrap_source)
                .ok_or_else(|| "worker bootstrap source exceeds V8 string limits".to_owned())?;
            let script = v8::Script::compile(scope, source, None)
                .ok_or_else(|| "worker failed to compile trusted bootstrap".to_owned())?;
            let function = script
                .run(scope)
                .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
                .ok_or_else(|| "worker bootstrap is not callable".to_owned())?;
            let undefined = v8::undefined(scope).into();
            let tid_value = v8::Integer::new(scope, tid).into();
            let arg_value = v8::Integer::new(scope, start_arg).into();
            if function
                .call(
                    scope,
                    undefined,
                    &[module.into(), memory, tid_value, arg_value],
                )
                .is_none()
            {
                if control.termination_requested() {
                    return Ok(());
                }
                return Err("worker wasi_thread_start threw".to_owned());
            }
            Ok(())
        })();
        crate::isolate::drop_isolate(Some(worker_isolate));
        result
    }
}

struct WorkerRecord {
    tid: i32,
    control: Arc<WasmWorkerControl>,
    join: thread::JoinHandle<Result<(), WasmWorkerError>>,
}

struct ManagerState {
    terminating: bool,
    workers: Vec<WorkerRecord>,
}

pub struct WasmWorkerManager {
    limits: WasmWorkerLimits,
    executor: Arc<dyn WasmWorkerExecutor>,
    next_tid: AtomicI32,
    spawned_workers: AtomicUsize,
    warning_emitted: AtomicBool,
    state: Mutex<ManagerState>,
    completed_tx: mpsc::SyncSender<i32>,
    completed_rx: Mutex<mpsc::Receiver<i32>>,
}

impl WasmWorkerManager {
    pub fn new(
        limits: WasmWorkerLimits,
        executor: Arc<dyn WasmWorkerExecutor>,
    ) -> Result<Self, WasmWorkerError> {
        let limits = limits.validate()?;
        // At most maxThreads-1 workers can complete without being reaped.
        let capacity = limits.max_runtime_threads.saturating_sub(1).max(1);
        let (completed_tx, completed_rx) = mpsc::sync_channel(capacity);
        Ok(Self {
            limits,
            executor,
            next_tid: AtomicI32::new(1),
            spawned_workers: AtomicUsize::new(0),
            warning_emitted: AtomicBool::new(false),
            state: Mutex::new(ManagerState {
                terminating: false,
                workers: Vec::new(),
            }),
            completed_tx,
            completed_rx: Mutex::new(completed_rx),
        })
    }

    pub fn limits(&self) -> WasmWorkerLimits {
        self.limits
    }

    /// Active runtime threads including the root session thread.
    pub fn active_runtime_threads(&self) -> usize {
        self.drain_completion_notifications();
        let mut state = self
            .state
            .lock()
            .expect("Node WASM worker manager lock poisoned");
        let _ = Self::reap_finished_locked(&mut state);
        1 + state.workers.len()
    }

    pub fn spawned_worker_count(&self) -> usize {
        self.spawned_workers.load(Ordering::Acquire)
    }

    pub fn spawn(&self, start_arg: i32) -> Result<i32, WasmWorkerError> {
        self.drain_completion_notifications();
        let mut state = self
            .state
            .lock()
            .expect("Node WASM worker manager lock poisoned");
        Self::reap_finished_locked(&mut state)?;
        if state.terminating {
            return Err(WasmWorkerError::Terminating);
        }
        let active_threads = 1 + state.workers.len();
        if active_threads >= self.limits.max_runtime_threads {
            return Err(WasmWorkerError::LimitExceeded {
                field: MAX_RUNTIME_THREADS_FIELD,
                configured: self.limits.max_runtime_threads,
            });
        }

        let tid = self.next_tid.fetch_add(1, Ordering::Relaxed);
        if tid <= 0 {
            return Err(WasmWorkerError::Spawn(
                "thread id space is exhausted".to_owned(),
            ));
        }
        let control = Arc::new(WasmWorkerControl::new());
        let worker_control = Arc::clone(&control);
        let executor = Arc::clone(&self.executor);
        let completed = self.completed_tx.clone();
        let (start_tx, start_rx) = mpsc::sync_channel::<()>(0);
        let join = thread::Builder::new()
            .name(format!("agentos-node-wasm-worker-{tid}"))
            .stack_size(self.limits.worker_stack_bytes)
            .spawn(move || {
                if start_rx.recv().is_err() {
                    return Err(WasmWorkerError::Terminating);
                }
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    executor.execute(Arc::clone(&worker_control), tid, start_arg)
                }));
                worker_control.clear_isolate_handle();
                let result = match outcome {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(message)) => Err(WasmWorkerError::WorkerFailed { tid, message }),
                    Err(payload) => Err(WasmWorkerError::WorkerPanicked {
                        tid,
                        message: panic_message(payload),
                    }),
                };
                // The channel is sized for every concurrently live worker, so
                // completion publication cannot block behind the root thread.
                let _ = completed.send(tid);
                result
            })
            .map_err(|error| WasmWorkerError::Spawn(error.to_string()))?;
        state.workers.push(WorkerRecord { tid, control, join });
        let runtime_threads = 1 + state.workers.len();
        if runtime_threads >= self.limits.warn_at_runtime_threads
            && self
                .warning_emitted
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            eprintln!(
                "agentos-v8-runtime: Node WASM runtime thread usage is near its configured limit: {MAX_RUNTIME_THREADS_FIELD}={} active={runtime_threads}",
                self.limits.max_runtime_threads,
            );
        }
        // Publish the worker record before letting the thread enter V8 so a
        // concurrent teardown always owns a termination handle or pending slot.
        start_tx.send(()).map_err(|_| {
            WasmWorkerError::Spawn(
                "worker exited before its accounting record was published".to_owned(),
            )
        })?;
        self.spawned_workers.fetch_add(1, Ordering::Release);
        Ok(tid)
    }

    pub fn shutdown(&self) -> Result<(), WasmWorkerError> {
        {
            let mut state = self
                .state
                .lock()
                .expect("Node WASM worker manager lock poisoned");
            state.terminating = true;
            for worker in &state.workers {
                worker.control.request_termination();
            }
        }
        self.executor.cancel_blocking_imports();

        let started = Instant::now();
        let mut first_worker_error = None;
        loop {
            self.drain_completion_notifications();
            {
                let mut state = self
                    .state
                    .lock()
                    .expect("Node WASM worker manager lock poisoned");
                if let Err(error) = Self::reap_finished_locked(&mut state) {
                    first_worker_error.get_or_insert(error);
                }
                if state.workers.is_empty() {
                    return match first_worker_error {
                        Some(error) => Err(error),
                        None => Ok(()),
                    };
                }
            }

            let elapsed = started.elapsed();
            let Some(remaining) = self.limits.teardown_grace.checked_sub(elapsed) else {
                let remaining_workers = self
                    .state
                    .lock()
                    .expect("Node WASM worker manager lock poisoned")
                    .workers
                    .len();
                return Err(WasmWorkerError::TeardownDeadline {
                    field: "limits.nodeRuntime.maxTeardownGraceMs",
                    configured_ms: self.limits.teardown_grace.as_millis(),
                    remaining_workers,
                });
            };
            let wait = remaining.min(Duration::from_millis(25));
            let _ = self
                .completed_rx
                .lock()
                .expect("Node WASM worker completion receiver lock poisoned")
                .recv_timeout(wait);
        }
    }

    fn drain_completion_notifications(&self) {
        let receiver = self
            .completed_rx
            .lock()
            .expect("Node WASM worker completion receiver lock poisoned");
        while receiver.try_recv().is_ok() {}
    }

    fn reap_finished_locked(state: &mut ManagerState) -> Result<(), WasmWorkerError> {
        let mut first_error = None;
        let mut index = 0;
        while index < state.workers.len() {
            if !state.workers[index].join.is_finished() {
                index += 1;
                continue;
            }
            let worker = state.workers.swap_remove(index);
            let tid = worker.tid;
            match worker.join.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    first_error.get_or_insert(error);
                }
                Err(payload) => {
                    first_error.get_or_insert(WasmWorkerError::WorkerPanicked {
                        tid,
                        message: panic_message(payload),
                    });
                }
            };
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct BlockingExecutor {
        entered: AtomicUsize,
    }

    impl WasmWorkerExecutor for BlockingExecutor {
        fn execute(
            &self,
            control: Arc<WasmWorkerControl>,
            _tid: i32,
            _start_arg: i32,
        ) -> Result<(), String> {
            self.entered.fetch_add(1, Ordering::Relaxed);
            while !control.termination_requested() {
                thread::yield_now();
            }
            Ok(())
        }
    }

    #[test]
    fn enforces_thread_cap_and_joins_every_worker_on_shutdown() {
        let executor = Arc::new(BlockingExecutor {
            entered: AtomicUsize::new(0),
        });
        let manager = WasmWorkerManager::new(
            WasmWorkerLimits {
                max_runtime_threads: 3,
                warn_at_runtime_threads: 2,
                teardown_grace: Duration::from_secs(1),
                ..WasmWorkerLimits::default()
            },
            executor.clone(),
        )
        .expect("valid worker manager");

        manager.spawn(11).expect("first worker");
        manager.spawn(22).expect("second worker");
        assert_eq!(
            manager.spawn(33),
            Err(WasmWorkerError::LimitExceeded {
                field: MAX_RUNTIME_THREADS_FIELD,
                configured: 3,
            })
        );
        while executor.entered.load(Ordering::Relaxed) != 2 {
            thread::yield_now();
        }
        manager.shutdown().expect("bounded worker shutdown");
        assert_eq!(manager.active_runtime_threads(), 1);
        assert_eq!(manager.spawn(44), Err(WasmWorkerError::Terminating));
    }

    struct ImmediateExecutor;

    impl WasmWorkerExecutor for ImmediateExecutor {
        fn execute(
            &self,
            _control: Arc<WasmWorkerControl>,
            _tid: i32,
            _start_arg: i32,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn reaps_completion_notifications_across_repeated_worker_churn() {
        let manager = WasmWorkerManager::new(
            WasmWorkerLimits {
                max_runtime_threads: 2,
                warn_at_runtime_threads: 2,
                ..WasmWorkerLimits::default()
            },
            Arc::new(ImmediateExecutor),
        )
        .expect("valid worker manager");

        for start_arg in 0..100 {
            manager.spawn(start_arg).expect("worker should start");
            while manager.active_runtime_threads() != 1 {
                thread::yield_now();
            }
        }
        assert_eq!(manager.spawned_worker_count(), 100);
        manager
            .shutdown()
            .expect("completed workers should be reaped");
    }

    struct PanicExecutor;

    impl WasmWorkerExecutor for PanicExecutor {
        fn execute(
            &self,
            _control: Arc<WasmWorkerControl>,
            _tid: i32,
            _start_arg: i32,
        ) -> Result<(), String> {
            panic!("injected worker panic")
        }
    }

    #[test]
    fn captures_worker_panics_and_still_completes_teardown_barrier() {
        let manager = WasmWorkerManager::new(WasmWorkerLimits::default(), Arc::new(PanicExecutor))
            .expect("valid worker manager");
        manager.spawn(0).expect("worker should start");
        let error = manager.shutdown().expect_err("panic must propagate");
        assert!(matches!(
            error,
            WasmWorkerError::WorkerPanicked { tid: 1, ref message }
                if message == "injected worker panic"
        ));
        assert_eq!(manager.active_runtime_threads(), 1);
    }

    struct SlowExecutor;

    impl WasmWorkerExecutor for SlowExecutor {
        fn execute(
            &self,
            _control: Arc<WasmWorkerControl>,
            _tid: i32,
            _start_arg: i32,
        ) -> Result<(), String> {
            thread::sleep(Duration::from_millis(40));
            Ok(())
        }
    }

    #[test]
    fn reports_teardown_deadline_without_losing_worker_ownership() {
        let manager = WasmWorkerManager::new(
            WasmWorkerLimits {
                teardown_grace: Duration::from_millis(1),
                ..WasmWorkerLimits::default()
            },
            Arc::new(SlowExecutor),
        )
        .expect("valid worker manager");
        manager.spawn(0).expect("worker should start");
        let error = manager
            .shutdown()
            .expect_err("injected slow worker must exceed the first deadline");
        assert!(matches!(
            error,
            WasmWorkerError::TeardownDeadline {
                configured_ms: 1,
                remaining_workers: 1,
                ..
            }
        ));

        thread::sleep(Duration::from_millis(50));
        manager
            .shutdown()
            .expect("manager must retain and later join the slow worker");
        assert_eq!(manager.active_runtime_threads(), 1);
    }
}
