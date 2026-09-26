//! Explicit WASI-threads group for the `wasmtime-threads` backend.
//!
//! Linux/POSIX semantics remain in the kernel and owned libc. This module
//! only owns engine objects: one imported shared memory and one Store/Instance
//! per native guest thread.

use super::engine::{WasmtimeEngineHandle, WasmtimeEngineProfile};
use super::linker;
use super::store::{self, PendingExecReplacement, WasmtimeHostClient};
use crate::backend::HostServiceError;
use agentos_driver_tokio::DriverHandle;
use agentos_executor_wasm_abi::StartWasmExecutionRequest;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use wasmtime::{ExternType, Module, SharedMemory};

const MAX_WASI_THREAD_ID: i32 = 0x1fff_ffff;

#[derive(Debug)]
struct ThreadGroupState {
    next_tid: i32,
    active: usize,
    reserved_native_threads: usize,
    shutting_down: bool,
    first_failure: Option<HostServiceError>,
    process_exit_code: Option<i32>,
    exec_replacement: Option<Option<PendingExecReplacement>>,
    handles: Vec<std::thread::JoinHandle<()>>,
}

pub enum ThreadGroupCompletion {
    Exit(i32),
    Exec(Option<PendingExecReplacement>),
}

pub struct ThreadGroup {
    engine: Arc<WasmtimeEngineHandle>,
    module: Arc<Module>,
    runtime: DriverHandle,
    host: WasmtimeHostClient,
    request: StartWasmExecutionRequest,
    profile: WasmtimeEngineProfile,
    paused: Arc<std::sync::atomic::AtomicBool>,
    pause_notify: Arc<Notify>,
    environment_is_guest_visible: bool,
    memory: SharedMemory,
    maximum_threads: usize,
    debug: bool,
    state: Mutex<ThreadGroupState>,
    failure_notify: Notify,
    shutdown_notify: Notify,
    completion_notify: Notify,
}

impl std::fmt::Debug for ThreadGroup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ThreadGroup")
            .field("process", &self.host.process())
            .field("maximum_threads", &self.maximum_threads)
            .field("memory_pages", &self.memory.size())
            .finish_non_exhaustive()
    }
}

impl ThreadGroup {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        engine: Arc<WasmtimeEngineHandle>,
        module: Arc<Module>,
        runtime: DriverHandle,
        host: WasmtimeHostClient,
        request: StartWasmExecutionRequest,
        profile: WasmtimeEngineProfile,
        paused: Arc<std::sync::atomic::AtomicBool>,
        pause_notify: Arc<Notify>,
        environment_is_guest_visible: bool,
    ) -> Result<Arc<Self>, HostServiceError> {
        let memory_type = module
            .imports()
            .find_map(|import| {
                (import.module() == "env" && import.name() == "memory").then(|| import.ty())
            })
            .and_then(|ty| match ty {
                ExternType::Memory(memory) => Some(memory),
                _ => None,
            })
            .ok_or_else(|| {
                HostServiceError::new(
                    "ERR_AGENTOS_WASM_THREADS_MEMORY_IMPORT",
                    "threaded WebAssembly must import shared memory as env.memory",
                )
            })?;
        if !memory_type.is_shared() {
            return Err(HostServiceError::new(
                "ERR_AGENTOS_WASM_THREADS_MEMORY_NOT_SHARED",
                "threaded WebAssembly env.memory must use the shared-memory type",
            ));
        }
        let maximum_pages = memory_type.maximum().ok_or_else(|| {
            HostServiceError::new(
                "ERR_AGENTOS_WASM_THREADS_MEMORY_UNBOUNDED",
                "threaded WebAssembly shared memory must declare a maximum",
            )
        })?;
        let maximum_bytes = maximum_pages
            .checked_mul(memory_type.page_size())
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| {
                HostServiceError::new(
                    "ERR_AGENTOS_WASM_THREADS_MEMORY_LIMIT",
                    "threaded WebAssembly shared-memory maximum does not fit this platform",
                )
            })?;
        let configured_maximum = super::limits::max_memory_bytes(&request.limits)?;
        if maximum_bytes > configured_maximum {
            return Err(HostServiceError::new(
                "ERR_AGENTOS_WASM_THREADS_MEMORY_LIMIT",
                "threaded WebAssembly shared-memory maximum exceeds the configured limit",
            )
            .with_details(serde_json::json!({
                "limitName": "limits.resources.maxWasmMemoryBytes",
                "limit": configured_maximum,
                "observed": maximum_bytes,
            })));
        }
        let memory = SharedMemory::new(engine.engine(), memory_type).map_err(|error| {
            eprintln!(
                "ERR_AGENTOS_WASM_THREADS_MEMORY_CREATE: private shared-memory diagnostic: {error:#}"
            );
            HostServiceError::new(
                "ERR_AGENTOS_WASM_THREADS_MEMORY_CREATE",
                "failed to allocate the threaded WebAssembly shared memory",
            )
        })?;
        Ok(Arc::new(Self {
            engine,
            module,
            runtime,
            host,
            maximum_threads: request.limits.max_threads.unwrap_or(16).max(1),
            debug: request
                .env
                .get("AGENTOS_WASM_THREAD_DEBUG")
                .is_some_and(|value| value == "1"),
            request,
            profile,
            paused,
            pause_notify,
            environment_is_guest_visible,
            memory,
            state: Mutex::new(ThreadGroupState {
                next_tid: 1,
                active: 1,
                reserved_native_threads: 0,
                shutting_down: false,
                first_failure: None,
                process_exit_code: None,
                exec_replacement: None,
                handles: Vec::new(),
            }),
            failure_notify: Notify::new(),
            shutdown_notify: Notify::new(),
            completion_notify: Notify::new(),
        }))
    }

    pub fn memory(&self) -> &SharedMemory {
        &self.memory
    }

    /// WASI threads returns a positive TID on success and a negative value on
    /// failure. wasi-libc translates every negative result to `EAGAIN`.
    pub fn spawn(self: &Arc<Self>, start_arg: i32) -> i32 {
        let tid = {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => {
                    eprintln!(
                        "ERR_AGENTOS_WASM_THREAD_GROUP_POISONED: cannot admit another pthread"
                    );
                    return -1;
                }
            };
            // Reap completed native workers before admitting another one.
            // Keep reservations until the handle is reaped: completion can race
            // handle insertion, so active Store count alone cannot bound this Vec.
            let mut index = 0;
            while index < state.handles.len() {
                if state.handles[index].is_finished() {
                    let handle = state.handles.swap_remove(index);
                    state.reserved_native_threads = state.reserved_native_threads.saturating_sub(1);
                    if handle.join().is_err() {
                        let error = HostServiceError::new(
                            "ERR_AGENTOS_WASM_THREAD_PANIC",
                            "a threaded WebAssembly native worker panicked",
                        );
                        eprintln!("{}: {}", error.code, error.message);
                        state.first_failure.get_or_insert(error);
                        self.failure_notify.notify_one();
                        return -1;
                    }
                } else {
                    index += 1;
                }
            }
            if state.shutting_down
                || state.active >= self.maximum_threads
                || state.reserved_native_threads >= self.maximum_threads.saturating_sub(1)
            {
                return -1;
            }
            let tid = state.next_tid;
            if !(1..=MAX_WASI_THREAD_ID).contains(&tid) {
                return -1;
            }
            state.next_tid = tid.saturating_add(1);
            state.active += 1;
            state.reserved_native_threads += 1;
            tid
        };

        let group = Arc::clone(self);
        if self.debug {
            eprintln!("AGENTOS_WASM_THREAD_DEBUG spawn tid={tid} arg={start_arg}");
        }
        // AGENTOS_THREAD_SITE: admitted-threaded-wasmtime-guest
        let handle = match std::thread::Builder::new()
            .name(format!("agentos-wasm-pthread-{tid}"))
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    group
                        .runtime
                        .tokio_handle()
                        .block_on(group.run_secondary(tid, start_arg))
                }))
                .unwrap_or_else(|_| {
                    Err(HostServiceError::new(
                        "ERR_AGENTOS_WASM_THREAD_PANIC",
                        "a threaded WebAssembly native worker panicked",
                    ))
                });
                group.finish_secondary(result);
            }) {
            Ok(handle) => handle,
            Err(error) => {
                eprintln!("ERR_AGENTOS_WASM_THREAD_SPAWN: native worker spawn failed: {error}");
                match self.state.lock() {
                    Ok(mut state) => {
                        state.active = state.active.saturating_sub(1);
                        state.reserved_native_threads =
                            state.reserved_native_threads.saturating_sub(1);
                    }
                    Err(_) => eprintln!(
                        "ERR_AGENTOS_WASM_THREAD_GROUP_POISONED: native spawn rollback failed"
                    ),
                }
                return -1;
            }
        };
        match self.state.lock() {
            Ok(mut state) => state.handles.push(handle),
            Err(_) => {
                // The spawned thread still owns its complete execution state;
                // dropping the handle detaches it, so mark the process group
                // failed and rely on the outer killable worker boundary.
                eprintln!("ERR_AGENTOS_WASM_THREAD_GROUP_POISONED: lost pthread join handle");
            }
        }
        tid
    }

    async fn run_secondary(
        self: &Arc<Self>,
        tid: i32,
        start_arg: i32,
    ) -> Result<(), HostServiceError> {
        if self.debug {
            eprintln!("AGENTOS_WASM_THREAD_DEBUG start tid={tid} arg={start_arg}");
        }
        let thread_id = u32::try_from(tid).map_err(|_| {
            HostServiceError::new(
                "ERR_AGENTOS_WASM_THREAD_ID",
                "WASI thread id does not fit the kernel signal-thread namespace",
            )
        })?;
        self.host
            .submit(
                crate::host::HostOperation::Signal(crate::host::SignalOperation::RegisterThread {
                    thread_id,
                    inherit_from: 0,
                }),
                std::mem::size_of::<u32>() * 2,
            )
            .await?;
        let result = tokio::select! {
            biased;
            () = self.wait_for_shutdown() => Ok(()),
            result = self.run_registered_secondary(tid, start_arg) => result,
        };
        let unregister = self
            .host
            .submit(
                crate::host::HostOperation::Signal(
                    crate::host::SignalOperation::UnregisterThread { thread_id },
                ),
                std::mem::size_of::<u32>(),
            )
            .await;
        // Exec has already removed the old image's signal-thread records.
        // Unregistering an already removed thread is completed cleanup.
        let unregister = match unregister {
            Err(error) if error.code == "ESRCH" => {
                Ok(crate::backend::HostCallReply::Json(serde_json::Value::Null))
            }
            other => other,
        };
        match (result, unregister) {
            (Err(error), Err(unregister)) => {
                eprintln!(
                    "{}: pthread failed; signal-thread teardown also failed: {}",
                    error.code, unregister
                );
                Err(error)
            }
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(_)) => Ok(()),
        }
    }

    async fn run_registered_secondary(
        self: &Arc<Self>,
        tid: i32,
        start_arg: i32,
    ) -> Result<(), HostServiceError> {
        let mut store = store::create_store(
            Arc::clone(&self.engine),
            &self.runtime,
            self.host.clone(),
            &self.request,
            self.profile,
            store::thread_cpu_time_ns(),
            Arc::clone(&self.paused),
            Arc::clone(&self.pause_notify),
            self.environment_is_guest_visible,
            true,
            Some(Arc::clone(self)),
            tid,
        )?;
        let mut linker =
            linker::build_linker(self.engine.engine(), self.request.permission_tier, true)
                .map_err(|error| {
                    eprintln!(
                        "ERR_AGENTOS_WASM_THREAD_LINKER: private linker diagnostic: {error:#}"
                    );
                    HostServiceError::new(
                        "ERR_AGENTOS_WASM_THREAD_LINKER",
                        "failed to construct the threaded WebAssembly linker",
                    )
                })?;
        linker
            .define(&store, "env", "memory", self.memory.clone())
            .map_err(|error| {
                eprintln!(
                    "ERR_AGENTOS_WASM_THREAD_MEMORY_LINK: private linker diagnostic: {error:#}"
                );
                HostServiceError::new(
                    "ERR_AGENTOS_WASM_THREAD_MEMORY_LINK",
                    "failed to link the threaded WebAssembly shared memory",
                )
            })?;
        let instance = linker
            .instantiate_async(&mut store, &self.module)
            .await
            .map_err(|error| {
                super::error::normalize("ERR_AGENTOS_WASM_THREAD_INSTANTIATE", &error, false)
            })?;
        linker::initialize_inherited_signal_mask(&mut store, &instance).await?;
        let start = instance
            .get_typed_func::<(i32, i32), ()>(&mut store, "wasi_thread_start")
            .map_err(|error| {
                eprintln!(
                    "ERR_AGENTOS_WASM_THREAD_ENTRYPOINT: private entrypoint diagnostic: {error:#}"
                );
                HostServiceError::new(
                    "ERR_AGENTOS_WASM_THREAD_ENTRYPOINT",
                    "threaded WebAssembly does not export a valid wasi_thread_start function",
                )
            })?;
        match start.call_async(&mut store, (tid, start_arg)).await {
            Ok(()) => Ok(()),
            Err(_) if store.data().exit_code.is_some() || store.data().exec_replaced => Ok(()),
            Err(error) => Err(super::error::normalize(
                "ERR_AGENTOS_WASM_THREAD_TRAP",
                &error,
                store.data().canceled(),
            )),
        }
    }

    fn finish_secondary(&self, result: Result<(), HostServiceError>) {
        if self.debug {
            eprintln!(
                "AGENTOS_WASM_THREAD_DEBUG finish result={}",
                if result.is_ok() { "ok" } else { "error" }
            );
        }
        let failure = result.err();
        let Ok(mut state) = self.state.lock() else {
            eprintln!("ERR_AGENTOS_WASM_THREAD_GROUP_POISONED: pthread completion was lost");
            return;
        };
        state.active = state.active.saturating_sub(1);
        if let Some(error) = failure.as_ref() {
            eprintln!(
                "{}: pthread execution failed: {}",
                error.code, error.message
            );
            state.first_failure.get_or_insert_with(|| error.clone());
            self.failure_notify.notify_one();
        }
        drop(state);
        self.completion_notify.notify_one();
        if let Some(error) = failure {
            self.host.report_thread_group_failure(error);
        }
    }

    /// Return the process-wide exit requested by any Store.
    pub fn process_exit_code(&self) -> Result<Option<i32>, HostServiceError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| group_poisoned())?
            .process_exit_code)
    }

    pub fn request_process_exit(&self, code: i32) -> Result<(), HostServiceError> {
        let mut state = self.state.lock().map_err(|_| group_poisoned())?;
        if state.process_exit_code.is_none() {
            self.host.report_thread_group_exit(code)?;
            state.process_exit_code = Some(code);
        }
        self.failure_notify.notify_one();
        Ok(())
    }

    /// Wake the main Store when a secondary exits, replaces the image, or traps.
    pub async fn wait_for_completion(&self) -> Result<ThreadGroupCompletion, HostServiceError> {
        loop {
            let notified = self.failure_notify.notified();
            match self.state.lock() {
                Ok(mut state) => {
                    if let Some(code) = state.process_exit_code {
                        return Ok(ThreadGroupCompletion::Exit(code));
                    }
                    if let Some(replacement) = state.exec_replacement.take() {
                        return Ok(ThreadGroupCompletion::Exec(replacement));
                    }
                    if let Some(error) = state.first_failure.as_ref() {
                        return Err(error.clone());
                    }
                }
                Err(_) => return Err(group_poisoned()),
            }
            notified.await;
        }
    }

    pub fn request_exec(
        &self,
        replacement: Option<PendingExecReplacement>,
    ) -> Result<(), HostServiceError> {
        self.state
            .lock()
            .map_err(|_| group_poisoned())?
            .exec_replacement = Some(replacement);
        self.failure_notify.notify_one();
        Ok(())
    }

    pub fn take_exec(&self) -> Result<Option<Option<PendingExecReplacement>>, HostServiceError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| group_poisoned())?
            .exec_replacement
            .take())
    }

    pub fn is_shutting_down(&self) -> bool {
        match self.state.lock() {
            Ok(state) => state.shutting_down,
            Err(_) => {
                eprintln!("ERR_AGENTOS_WASM_THREAD_GROUP_POISONED: stopping guest execution");
                true
            }
        }
    }

    async fn wait_for_shutdown(&self) {
        loop {
            let notified = self.shutdown_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_shutting_down() {
                return;
            }
            notified.await;
        }
    }

    /// Exec reuses the worker process, so every old Store must stop before a
    /// replacement can run. Wake host-call waiters and interrupt CPU work via
    /// the epoch callback, then wait for signal-thread cleanup to finish.
    pub async fn retire_image(&self) -> Result<(), HostServiceError> {
        self.state
            .lock()
            .map_err(|_| group_poisoned())?
            .shutting_down = true;
        self.shutdown_notify.notify_waiters();
        loop {
            let notified = self.completion_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.state.lock().map_err(|_| group_poisoned())?.active <= 1 {
                break;
            }
            notified.await;
        }
        self.settle_main()
    }

    /// Mark the group closed when the process main Store exits. Linux process
    /// exit does not join detached pthreads: the enclosing worker process is
    /// the teardown unit and its exit terminates every remaining native guest
    /// thread. Finished JoinHandles are reaped here; unfinished handles are
    /// deliberately detached immediately before the worker itself exits.
    pub fn settle_main(&self) -> Result<(), HostServiceError> {
        let mut state = self.state.lock().map_err(|_| group_poisoned())?;
        state.shutting_down = true;
        let handles = std::mem::take(&mut state.handles);
        let failure = state.first_failure.take();
        drop(state);
        for handle in handles {
            if handle.is_finished() && handle.join().is_err() {
                return Err(HostServiceError::new(
                    "ERR_AGENTOS_WASM_THREAD_PANIC",
                    "a threaded WebAssembly native worker panicked",
                ));
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

fn group_poisoned() -> HostServiceError {
    HostServiceError::new(
        "ERR_AGENTOS_WASM_THREAD_GROUP_POISONED",
        "threaded WebAssembly group state is poisoned",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{bounded_execution_event_channel, ExecutionEvent, PayloadLimit};
    use crate::host::{HostOperation, HostProcessContext, ProcessHostCapabilitySet};
    use agentos_driver_tokio::{DriverConfig, TokioDriver};
    use agentos_executor_contract::GuestRuntimeConfig;
    use agentos_executor_wasm_abi::{WasmExecutionLimits, WasmPermissionTier};
    use std::collections::BTreeMap;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    #[test]
    fn sequential_thread_churn_does_not_retain_historical_native_handles() {
        let runtime = TokioDriver::process(&DriverConfig::default())
            .unwrap()
            .handle();
        let profile = WasmtimeEngineProfile::new_threaded(None).unwrap();
        let engine = super::super::engine::WasmtimeEngineRegistry::process()
            .get_or_create(profile)
            .unwrap();
        let module = Arc::new(
            Module::new(
                engine.engine(),
                wat::parse_str(
                    r#"(module
            (import "env" "memory" (memory 1 1 shared))
            (func (export "wasi_thread_start") (param i32 i32)))"#,
                )
                .unwrap(),
            )
            .unwrap(),
        );
        let (submission, events) = bounded_execution_event_channel(
            HostProcessContext {
                generation: 1,
                pid: 42,
            },
            8,
            PayloadLimit::new("limits.process.pendingEventBytes", 65536).unwrap(),
            Arc::new(|| {}),
        )
        .unwrap();
        let (event_sender, _event_receiver) = flume::bounded(8);
        let host = WasmtimeHostClient::new(
            ProcessHostCapabilitySet::from_event_submission(submission),
            65536,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Notify::new()),
            Arc::new(AtomicBool::new(false)),
            Arc::clone(runtime.resources()),
            event_sender,
            None,
        );
        let request = StartWasmExecutionRequest {
            vm_id: "thread-churn".into(),
            context_id: "thread-churn".into(),
            managed_kernel_host: true,
            argv: vec!["/threads.wasm".into()],
            env: BTreeMap::new(),
            cwd: "/".into(),
            permission_tier: WasmPermissionTier::Full,
            limits: WasmExecutionLimits {
                max_threads: Some(2),
                max_memory_bytes: Some(65536),
                ..Default::default()
            },
            guest_runtime: GuestRuntimeConfig::default(),
        };
        let group = ThreadGroup::new(
            engine,
            module,
            runtime,
            host,
            request,
            profile,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Notify::new()),
            false,
        )
        .unwrap();
        for _ in 0..8 {
            assert!(group.spawn(0) > 0);
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                while let Some(event) = events.try_recv().unwrap() {
                    let ExecutionEvent::HostCall { operation, reply } = event else {
                        panic!("unexpected event");
                    };
                    assert!(matches!(operation, HostOperation::Signal(_)));
                    reply
                        .succeed_json(serde_json::json!({"signals":[]}))
                        .unwrap();
                }
                let complete = {
                    let state = group.state.lock().unwrap();
                    assert!(state.first_failure.is_none(), "{:?}", state.first_failure);
                    state.active == 1 && state.handles.iter().all(|handle| handle.is_finished())
                };
                if complete {
                    break;
                }
                assert!(Instant::now() < deadline, "secondary did not finish");
                std::thread::yield_now();
            }
            assert!(
                group.state.lock().unwrap().handles.len() <= 1,
                "maxThreads=2 must not retain historical secondary thread handles"
            );
        }
        group.settle_main().unwrap();
    }
}
