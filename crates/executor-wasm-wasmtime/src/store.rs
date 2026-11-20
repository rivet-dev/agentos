//! Per-execution Store state backed by agentOS host capabilities.

use super::diagnostics::ExecutionDiagnostics;
use super::engine::{WasmtimeEngineHandle, WasmtimeEngineProfile};
use super::lifecycle::QueuedWasmtimeEvent;
use super::limits;
use super::threads::ThreadGroup;
use super::worker::WorkerIpcClient;
use crate::backend::{
    direct_host_reply_channel, HostCallIdentity, HostCallReply, HostServiceError,
};
use crate::host::{HostOperation, HostProcessContext, ProcessHostCapabilitySet};
use agentos_driver_tokio::accounting::{Reservation, ResourceClass, ResourceLedger};
use agentos_driver_tokio::DriverHandle;
use agentos_executor_wasm_abi::{
    guest_visible_wasm_env, StartWasmExecutionRequest, WasmExecutionEvent,
};
use flume::Sender;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::Notify;
use wasmtime::{Store, StoreLimits, UpdateDeadline};

pub const DEFAULT_MAX_HOST_REPLY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct PendingExecReplacement {
    pub module: Arc<wasmtime::Module>,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
}

/// One generation-bound direct waiter namespace shared by module loading and
/// every import issued by a Store. It owns no sidecar or kernel state.
#[derive(Clone)]
pub struct WasmtimeHostClient {
    process: HostProcessContext,
    host: Option<ProcessHostCapabilitySet>,
    worker: Option<WorkerIpcClient>,
    next_call_id: Arc<AtomicU64>,
    max_host_reply_bytes: usize,
    cancelled: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
    signal_pending: Arc<AtomicBool>,
    resources: Arc<ResourceLedger>,
    events: Sender<QueuedWasmtimeEvent>,
    event_notify: Option<Arc<Notify>>,
    diagnostics: Option<Arc<ExecutionDiagnostics>>,
    active_cpu_origin_ns: Arc<OnceLock<u64>>,
}

impl WasmtimeHostClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: ProcessHostCapabilitySet,
        max_host_reply_bytes: usize,
        cancelled: Arc<AtomicBool>,
        cancel_notify: Arc<Notify>,
        signal_pending: Arc<AtomicBool>,
        resources: Arc<ResourceLedger>,
        events: Sender<QueuedWasmtimeEvent>,
        event_notify: Option<Arc<Notify>>,
    ) -> Self {
        Self {
            process: host.process(),
            host: Some(host),
            worker: None,
            next_call_id: Arc::new(AtomicU64::new(1)),
            max_host_reply_bytes,
            cancelled,
            cancel_notify,
            signal_pending,
            resources,
            events,
            event_notify,
            diagnostics: None,
            active_cpu_origin_ns: Arc::new(OnceLock::new()),
        }
    }

    pub(super) fn new_worker(
        worker: WorkerIpcClient,
        max_host_reply_bytes: usize,
        cancelled: Arc<AtomicBool>,
        cancel_notify: Arc<Notify>,
        signal_pending: Arc<AtomicBool>,
        resources: Arc<ResourceLedger>,
        events: Sender<QueuedWasmtimeEvent>,
    ) -> Self {
        Self {
            process: worker.process(),
            host: None,
            worker: Some(worker),
            next_call_id: Arc::new(AtomicU64::new(1)),
            max_host_reply_bytes,
            cancelled,
            cancel_notify,
            signal_pending,
            resources,
            events,
            event_notify: None,
            diagnostics: None,
            active_cpu_origin_ns: Arc::new(OnceLock::new()),
        }
    }

    pub fn with_diagnostics(mut self, diagnostics: Arc<ExecutionDiagnostics>) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    /// Threaded execution owns an isolated worker process. Its process CPU
    /// clock includes every pthread, including threads between epoch checks.
    /// Direct execution shares a process with other VMs and uses its own thread.
    pub(super) fn cpu_time_ns(&self) -> Result<u64, HostServiceError> {
        cpu_time_ns(self.worker.is_some())
    }

    pub(super) fn check_active_cpu_budget(
        &self,
        limit_ms: Option<u32>,
    ) -> Result<(), HostServiceError> {
        if let (Some(limit_ms), Some(origin)) = (limit_ms, self.active_cpu_origin_ns.get()) {
            let observed_ns = self.cpu_time_ns()?.saturating_sub(*origin);
            let limit_ns = u64::from(limit_ms) * 1_000_000;
            if observed_ns >= limit_ns {
                return Err(active_cpu_limit_error(limit_ns, observed_ns));
            }
        }
        Ok(())
    }

    pub fn process(&self) -> HostProcessContext {
        self.process
    }

    pub(super) fn max_host_reply_bytes(&self) -> usize {
        self.max_host_reply_bytes
    }

    pub fn canceled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn signal_pending(&self) -> bool {
        self.worker
            .as_ref()
            .map(WorkerIpcClient::signal_pending)
            .unwrap_or_else(|| self.signal_pending.load(Ordering::Acquire))
    }

    pub async fn submit(
        &self,
        operation: HostOperation,
        retained_request_bytes: usize,
    ) -> Result<HostCallReply, HostServiceError> {
        if let Some(diagnostics) = self.diagnostics.as_ref() {
            diagnostics.first_host_call();
        }
        if self.canceled() {
            return Err(HostServiceError::new(
                "ECANCELED",
                "Wasmtime execution was canceled before host-call admission",
            ));
        }
        if let Some(worker) = self.worker.as_ref() {
            return worker.submit(operation).await;
        }
        let call_id = self
            .next_call_id
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| {
                HostServiceError::new(
                    "EOVERFLOW",
                    "Wasmtime host-call identity space is exhausted",
                )
            })?;
        let process = self.process;
        let identity = HostCallIdentity {
            generation: process.generation,
            pid: process.pid,
            call_id,
        };
        let host = self.host.as_ref().ok_or_else(|| {
            HostServiceError::new("EIO", "direct Wasmtime host capability is unavailable")
        })?;
        let admission = host.admit_request(retained_request_bytes)?;
        let (reply, receiver) = direct_host_reply_channel(identity, self.max_host_reply_bytes)?;
        host.submit(operation, reply, admission)?;
        tokio::select! {
            reply = receiver => reply,
            () = self.cancel_notify.notified() => Err(HostServiceError::new(
                "ECANCELED",
                "Wasmtime execution was canceled while awaiting a host operation",
            )),
        }
    }

    /// Route an owned native ABI request through the shared compatibility
    /// decoder and then the common typed host-operation dispatcher. The reply
    /// capability is direct and call-specific; this path never uses V8's
    /// synchronous line protocol or scans an event stream for a response.
    pub async fn submit_adapter_call(
        &self,
        method: String,
        args: Vec<Value>,
        raw_bytes_args: HashMap<usize, Vec<u8>>,
    ) -> Result<HostCallReply, HostServiceError> {
        if let Some(diagnostics) = self.diagnostics.as_ref() {
            diagnostics.first_host_call();
            diagnostics.first_guest_host_call();
            if matches!(method.as_str(), "__kernel_stdio_write" | "process.fd_write")
                && args
                    .first()
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|fd| matches!(fd, 1 | 2))
            {
                diagnostics.first_output();
            }
        }
        if self.canceled() {
            return Err(HostServiceError::new(
                "ECANCELED",
                "Wasmtime execution was canceled before adapter-call admission",
            ));
        }
        if let Some(worker) = self.worker.as_ref() {
            return worker
                .submit_adapter_call(method, args, raw_bytes_args)
                .await;
        }
        let call_id = self
            .next_call_id
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| {
                HostServiceError::new(
                    "EOVERFLOW",
                    "Wasmtime host-call identity space is exhausted",
                )
            })?;
        let process = self.process;
        let identity = HostCallIdentity {
            generation: process.generation,
            pid: process.pid,
            call_id,
        };
        let (reply, receiver) = direct_host_reply_channel(identity, self.max_host_reply_bytes)?;
        let request = agentos_executor_contract::HostRpcRequest {
            id: call_id,
            method,
            args,
            raw_bytes_args,
        };
        let retained_event_bytes = request
            .method
            .len()
            .checked_add(
                serde_json::to_vec(&request.args)
                    .map_err(|error| {
                        HostServiceError::new(
                            "ERR_AGENTOS_WASMTIME_HOST_CALL_ENCODING",
                            error.to_string(),
                        )
                    })?
                    .len(),
            )
            .and_then(|total| {
                request
                    .raw_bytes_args
                    .values()
                    .try_fold(total, |total, bytes| total.checked_add(bytes.len()))
            })
            .ok_or_else(|| {
                HostServiceError::new(
                    "EOVERFLOW",
                    "Wasmtime host-call retained byte accounting overflowed",
                )
            })?;
        let event = QueuedWasmtimeEvent::new(
            &self.resources,
            Ok(WasmExecutionEvent::HostCall { request, reply }),
            retained_event_bytes,
        )?;
        tokio::select! {
            result = self.events.send_async(event) => result.map_err(|_| HostServiceError::new(
                "EPIPE",
                "Wasmtime host-call event receiver was dropped",
            ))?,
            () = self.cancel_notify.notified() => return Err(HostServiceError::new(
                "ECANCELED",
                "Wasmtime execution was canceled while admitting an adapter call",
            )),
        }
        if let Some(notify) = self.event_notify.as_ref() {
            notify.notify_one();
        }
        tokio::select! {
            reply = receiver => reply,
            () = self.cancel_notify.notified() => Err(HostServiceError::new(
                "ECANCELED",
                "Wasmtime execution was canceled while awaiting an adapter call",
            )),
        }
    }

    pub async fn publish_stderr(&self, bytes: Vec<u8>) -> Result<(), HostServiceError> {
        if let Some(worker) = self.worker.as_ref() {
            return worker.publish_stderr(bytes).await;
        }
        let retained_bytes = bytes.len();
        let event = QueuedWasmtimeEvent::new(
            &self.resources,
            Ok(WasmExecutionEvent::Stderr(bytes)),
            retained_bytes,
        )?;
        tokio::select! {
            result = self.events.send_async(event) => {
                result.map_err(|_| HostServiceError::new(
                    "EPIPE",
                    "Wasmtime stderr event receiver was dropped",
                ))?;
            }
            () = self.cancel_notify.notified() => return Err(HostServiceError::new(
                "ECANCELED",
                "Wasmtime execution was canceled while publishing stderr",
            )),
        }
        if let Some(notify) = self.event_notify.as_ref() {
            notify.notify_one();
        }
        Ok(())
    }

    pub(super) fn worker_client(&self) -> Option<&WorkerIpcClient> {
        self.worker.as_ref()
    }

    pub(super) fn report_thread_group_exit(&self, code: i32) -> Result<(), HostServiceError> {
        if let Some(worker) = self.worker.as_ref() {
            worker.report_group_exit(code)?;
        }
        Ok(())
    }

    pub(super) fn report_thread_group_failure(&self, error: HostServiceError) {
        if let Some(worker) = self.worker.as_ref() {
            if let Err(report_error) = worker.report_group_failure(error) {
                eprintln!(
                    "{}: failed to report pthread group failure to parent: {}",
                    report_error.code, report_error.message
                );
            }
        }
    }
}

pub struct WasmtimeStoreState {
    pub host: WasmtimeHostClient,
    pub engine: Arc<WasmtimeEngineHandle>,
    pub argv: Vec<Vec<u8>>,
    pub env: Vec<Vec<u8>>,
    pub virtual_pid: u32,
    pub thread_id: i32,
    pub thread_group: Option<Arc<ThreadGroup>>,
    pub limits: StoreLimits,
    pub exit_code: Option<i32>,
    pub exec_replaced: bool,
    pub pending_exec_replacement: Option<PendingExecReplacement>,
    pub max_module_file_bytes: usize,
    pub max_blocking_read_ms: u64,
    pub max_spawn_file_actions: usize,
    pub max_spawn_file_action_bytes: usize,
    pub warned_fixed_limits: HashSet<&'static str>,
    pub pending_open_mode: Option<u32>,
    pub pending_open_direct: bool,
    pub(super) deterministic_fuel_enabled: bool,
    active_cpu_limit_ns: Option<u64>,
    active_cpu_started_ns: u64,
    paused: Arc<AtomicBool>,
    pause_notify: Arc<Notify>,
    _async_stack_reservation: Option<Reservation>,
    _guest_memory_reservation: Option<Reservation>,
}

impl std::fmt::Debug for WasmtimeStoreState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WasmtimeStoreState")
            .field("process", &self.host.process())
            .field("argv_count", &self.argv.len())
            .field("env_count", &self.env.len())
            .field("exit_code", &self.exit_code)
            .finish_non_exhaustive()
    }
}

impl WasmtimeStoreState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime: &DriverHandle,
        host: WasmtimeHostClient,
        engine: Arc<WasmtimeEngineHandle>,
        request: &StartWasmExecutionRequest,
        profile: WasmtimeEngineProfile,
        active_cpu_started_ns: u64,
        paused: Arc<AtomicBool>,
        pause_notify: Arc<Notify>,
        environment_is_guest_visible: bool,
        group_memory_pre_reserved: bool,
        thread_group: Option<Arc<ThreadGroup>>,
        thread_id: i32,
    ) -> Result<Self, HostServiceError> {
        let async_stack_bytes = profile.async_stack_bytes()?;
        let async_stack_reservation = if group_memory_pre_reserved {
            None
        } else {
            Some(
                runtime
                    .resources()
                    .reserve(ResourceClass::WasmMemoryBytes, async_stack_bytes)
                    .map_err(|error| {
                        HostServiceError::new(
                            "ERR_AGENTOS_WASMTIME_ASYNC_STACK_LIMIT",
                            error.to_string(),
                        )
                        .with_details(serde_json::json!({
                            "limitName": "limits.resources.maxWasmStackBytes",
                            "observed": async_stack_bytes,
                        }))
                    })?,
            )
        };
        let linear_memory_bytes = limits::max_memory_bytes(&request.limits)?;
        let aggregate_memory_bytes = limits::aggregate_store_memory_bytes(&request.limits)?;
        let guest_memory_reservation = if group_memory_pre_reserved {
            None
        } else {
            Some(
                runtime
                    .resources()
                    .reserve(ResourceClass::WasmMemoryBytes, aggregate_memory_bytes)
                    .map_err(|error| {
                        HostServiceError::new(
                            "ERR_AGENTOS_WASMTIME_AGGREGATE_MEMORY_LIMIT",
                            error.to_string(),
                        )
                        .with_details(serde_json::json!({
                            "limitName": "limits.resources.maxWasmMemoryBytes",
                            "observed": aggregate_memory_bytes,
                            "linearMemoryBytes": linear_memory_bytes,
                            "exceptionGcHeapBytes": linear_memory_bytes,
                            "tableAccountingBytes": limits::DEFAULT_TABLE_ACCOUNTING_BYTES,
                            "resource": "wasmtimeGuestMemory",
                        }))
                    })?,
            )
        };
        let argv = nul_terminated_strings(request.argv.iter().map(String::as_str), "argv")?;
        let guest_env = store_guest_environment(&request.env, environment_is_guest_visible);
        let env = nul_terminated_strings(
            guest_env
                .iter()
                .map(|(key, value)| format!("{key}={value}")),
            "environment",
        )?;
        let virtual_pid = request
            .guest_runtime
            .virtual_pid
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or_else(|| host.process().pid);
        let active_cpu_started_ns = *host
            .active_cpu_origin_ns
            .get_or_init(|| active_cpu_started_ns);
        Ok(Self {
            host,
            engine,
            argv,
            env,
            virtual_pid,
            thread_id,
            thread_group,
            limits: limits::store_limits(&request.limits)?,
            exit_code: None,
            exec_replaced: false,
            pending_exec_replacement: None,
            max_module_file_bytes: request
                .limits
                .max_module_file_bytes
                .map(usize::try_from)
                .transpose()
                .map_err(|_| {
                    HostServiceError::new("EFBIG", "module byte limit does not fit this platform")
                })?
                .unwrap_or(256 * 1024 * 1024),
            max_blocking_read_ms: request.limits.max_blocking_read_ms.unwrap_or(30_000),
            max_spawn_file_actions: request
                .limits
                .max_spawn_file_actions
                .map(usize::try_from)
                .transpose()
                .map_err(|_| {
                    HostServiceError::new(
                        "E2BIG",
                        "spawn file-action count limit does not fit this platform",
                    )
                })?
                .unwrap_or(4096),
            max_spawn_file_action_bytes: request
                .limits
                .max_spawn_file_action_bytes
                .map(usize::try_from)
                .transpose()
                .map_err(|_| {
                    HostServiceError::new(
                        "E2BIG",
                        "spawn file-action byte limit does not fit this platform",
                    )
                })?
                .unwrap_or(1024 * 1024),
            warned_fixed_limits: HashSet::new(),
            pending_open_mode: None,
            pending_open_direct: false,
            deterministic_fuel_enabled: request.limits.deterministic_fuel.is_some(),
            active_cpu_limit_ns: request
                .limits
                .active_cpu_time_limit_ms
                .map(|value| u64::from(value).saturating_mul(1_000_000)),
            active_cpu_started_ns,
            paused,
            pause_notify,
            _async_stack_reservation: async_stack_reservation,
            _guest_memory_reservation: guest_memory_reservation,
        })
    }

    pub fn canceled(&self) -> bool {
        self.host.canceled()
            || self
                .thread_group
                .as_ref()
                .is_some_and(|group| group.is_shutting_down())
    }

    pub(super) fn has_active_cpu_limit(&self) -> bool {
        self.active_cpu_limit_ns.is_some()
    }

    pub(super) fn remaining_active_cpu_ms(&self) -> Result<Option<u32>, HostServiceError> {
        self.active_cpu_limit_ns
            .map(|limit| {
                let observed = self
                    .host
                    .cpu_time_ns()?
                    .saturating_sub(self.active_cpu_started_ns);
                let remaining_ms = limit.saturating_sub(observed) / 1_000_000;
                if remaining_ms == 0 {
                    return Err(active_cpu_limit_error(limit, observed));
                }
                Ok(remaining_ms.min(u32::MAX as u64) as u32)
            })
            .transpose()
    }

    pub(super) fn check_active_cpu_budget(&self) -> Result<(), HostServiceError> {
        if self.active_cpu_limit_ns.is_none() {
            return Ok(());
        }
        self.check_active_cpu_budget_at(self.host.cpu_time_ns()?)
    }

    fn check_active_cpu_budget_at(&self, now_ns: u64) -> Result<(), HostServiceError> {
        if let Some(limit_ns) = self.active_cpu_limit_ns {
            let observed_ns = now_ns.saturating_sub(self.active_cpu_started_ns);
            if observed_ns >= limit_ns {
                return Err(active_cpu_limit_error(limit_ns, observed_ns));
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn active_cpu_exhausted_at(&self, now_ns: u64) -> bool {
        self.check_active_cpu_budget_at(now_ns).is_err()
    }

    fn pause_waiter(&self) -> Option<(Arc<AtomicBool>, Arc<Notify>)> {
        self.paused
            .load(Ordering::Acquire)
            .then(|| (Arc::clone(&self.paused), Arc::clone(&self.pause_notify)))
    }
}

fn store_guest_environment(
    environment: &BTreeMap<String, String>,
    environment_is_guest_visible: bool,
) -> BTreeMap<String, String> {
    if environment_is_guest_visible {
        environment.clone()
    } else {
        guest_visible_wasm_env(environment)
    }
}

pub fn max_host_reply_bytes(
    request: &StartWasmExecutionRequest,
) -> Result<usize, HostServiceError> {
    request
        .limits
        .max_sync_rpc_response_line_bytes
        .map(usize::try_from)
        .transpose()
        .map_err(|_| {
            HostServiceError::new(
                "ERR_AGENTOS_WASMTIME_LIMIT_CONFIG",
                "limits.reactor.maxBridgeResponseBytes does not fit this platform",
            )
        })
        .map(|value| value.unwrap_or(DEFAULT_MAX_HOST_REPLY_BYTES))
}

#[allow(clippy::too_many_arguments)]
pub fn create_store(
    engine: Arc<WasmtimeEngineHandle>,
    runtime: &DriverHandle,
    host: WasmtimeHostClient,
    request: &StartWasmExecutionRequest,
    profile: WasmtimeEngineProfile,
    active_cpu_started_ns: u64,
    paused: Arc<AtomicBool>,
    pause_notify: Arc<Notify>,
    environment_is_guest_visible: bool,
    group_memory_pre_reserved: bool,
    thread_group: Option<Arc<ThreadGroup>>,
    thread_id: i32,
) -> Result<Store<WasmtimeStoreState>, HostServiceError> {
    let deterministic_fuel = request.limits.deterministic_fuel;
    let mut store = Store::new(
        engine.engine(),
        WasmtimeStoreState::new(
            runtime,
            host,
            Arc::clone(&engine),
            request,
            profile,
            active_cpu_started_ns,
            paused,
            pause_notify,
            environment_is_guest_visible,
            group_memory_pre_reserved,
            thread_group,
            thread_id,
        )?,
    );
    store.limiter(|state| &mut state.limits);
    // Fuel instrumentation is Engine-wide, so ordinary execution uses an
    // uninstrumented exact Engine profile and remains epoch-managed. Only an
    // explicit deterministic budget selects the fuel-enabled profile and
    // configures its Store counter.
    if let Some(deterministic_fuel) = deterministic_fuel {
        store.set_fuel(deterministic_fuel).map_err(|error| {
            eprintln!("ERR_AGENTOS_WASMTIME_FUEL_CONFIG: private Store diagnostic: {error:#}");
            HostServiceError::new(
                "ERR_AGENTOS_WASMTIME_FUEL_CONFIG",
                "failed to configure the deterministic execution budget",
            )
        })?;
    }
    store.set_epoch_deadline(1);
    store.epoch_deadline_callback(|context| {
        if context.data().canceled() {
            return Err(wasmtime::format_err!(
                "ERR_AGENTOS_WASMTIME_CANCELED: execution canceled"
            ));
        }
        context
            .data()
            .check_active_cpu_budget()
            .map_err(wasmtime::Error::new)?;
        if let Some((paused, notify)) = context.data().pause_waiter() {
            return Ok(UpdateDeadline::YieldCustom(
                1,
                Box::pin(async move {
                    loop {
                        let notified = notify.notified();
                        if !paused.load(Ordering::Acquire) {
                            break;
                        }
                        notified.await;
                    }
                }),
            ));
        }
        Ok(UpdateDeadline::Yield(1))
    });
    Ok(store)
}

pub(super) fn active_cpu_limit_error(limit_ns: u64, observed_ns: u64) -> HostServiceError {
    HostServiceError::new(
        "ERR_AGENTOS_WASMTIME_ACTIVE_CPU_LIMIT",
        "Wasmtime execution exhausted its aggregate active CPU budget; raise limits.wasm.activeCpuTimeLimitMs",
    )
    .with_details(serde_json::json!({ "limitName": "limits.wasm.activeCpuTimeLimitMs", "limit": limit_ns / 1_000_000, "observed": observed_ns / 1_000_000 }))
}

#[cfg(unix)]
fn cpu_time_ns(process_wide: bool) -> Result<u64, HostServiceError> {
    let clock = if process_wide {
        nix::time::ClockId::CLOCK_PROCESS_CPUTIME_ID
    } else {
        nix::time::ClockId::CLOCK_THREAD_CPUTIME_ID
    };
    let value = nix::time::clock_gettime(clock).map_err(|error| {
        eprintln!("ERR_AGENTOS_WASMTIME_CPU_CLOCK: private clock diagnostic: {error}");
        HostServiceError::new(
            "ERR_AGENTOS_WASMTIME_CPU_CLOCK",
            "failed to read execution CPU clock for limits.wasm.activeCpuTimeLimitMs",
        )
        .with_details(serde_json::json!({ "limitName": "limits.wasm.activeCpuTimeLimitMs" }))
    })?;
    u64::try_from(value.tv_sec())
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000_000_000))
        .and_then(|seconds| {
            u64::try_from(value.tv_nsec())
                .ok()
                .and_then(|nanos| seconds.checked_add(nanos))
        })
        .ok_or_else(|| {
            HostServiceError::new(
                "ERR_AGENTOS_WASMTIME_CPU_CLOCK",
                "execution CPU clock for limits.wasm.activeCpuTimeLimitMs is outside its supported range",
            ).with_details(serde_json::json!({ "limitName": "limits.wasm.activeCpuTimeLimitMs" }))
        })
}

#[cfg(not(unix))]
fn cpu_time_ns(_process_wide: bool) -> Result<u64, HostServiceError> {
    Err(HostServiceError::new(
        "ERR_AGENTOS_WASMTIME_CPU_CLOCK",
        "execution CPU clock for limits.wasm.activeCpuTimeLimitMs is unsupported on this platform",
    )
    .with_details(serde_json::json!({ "limitName": "limits.wasm.activeCpuTimeLimitMs" })))
}

#[cfg(test)]
fn thread_cpu_time_ns() -> u64 {
    cpu_time_ns(false).expect("test thread CPU clock")
}

fn nul_terminated_strings<I, S>(
    values: I,
    field: &'static str,
) -> Result<Vec<Vec<u8>>, HostServiceError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    values
        .into_iter()
        .map(|value| {
            let value = value.as_ref();
            if value.as_bytes().contains(&0) {
                return Err(HostServiceError::new(
                    "EINVAL",
                    format!("WebAssembly {field} value contains an interior NUL"),
                ));
            }
            let mut bytes = Vec::with_capacity(value.len().saturating_add(1));
            bytes.extend_from_slice(value.as_bytes());
            bytes.push(0);
            Ok(bytes)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{bounded_execution_event_channel, PayloadLimit};
    use agentos_driver_tokio::accounting::{ResourceLedger, ResourceLimit};
    use agentos_driver_tokio::{DriverConfig, TokioDriver};
    use agentos_executor_contract::GuestRuntimeConfig;
    use agentos_executor_wasm_abi::{WasmExecutionLimits, WasmPermissionTier};
    use std::path::PathBuf;

    #[test]
    fn store_reservations_are_released_on_teardown() {
        let runtime = TokioDriver::process(&DriverConfig::default()).expect("test runtime");
        let process = crate::host::HostProcessContext {
            generation: 97,
            pid: 41,
        };
        let resources = Arc::new(ResourceLedger::child(
            "wasmtime-store-teardown-test",
            [(
                ResourceClass::WasmMemoryBytes,
                ResourceLimit::new(1024 * 1024 * 1024, "runtime.resources.maxWasmMemoryBytes"),
            )],
            Arc::clone(runtime.handle().resources()),
        ));
        let scoped = runtime
            .handle()
            .scoped_for_vm(Arc::clone(&resources), process.generation);
        let (submission, _host_events) = bounded_execution_event_channel(
            process,
            4,
            PayloadLimit::new("limits.process.pendingEventBytes", 1024).expect("event byte limit"),
            Arc::new(|| {}),
        )
        .expect("host event channel");
        let (events, _event_receiver) = flume::bounded(4);
        let host = WasmtimeHostClient::new(
            ProcessHostCapabilitySet::from_event_submission(submission),
            DEFAULT_MAX_HOST_REPLY_BYTES,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Notify::new()),
            Arc::new(AtomicBool::new(false)),
            Arc::clone(&resources),
            events,
            None,
        );
        let request = StartWasmExecutionRequest {
            vm_id: String::from("vm-store-teardown"),
            context_id: String::from("ctx-store-teardown"),
            managed_kernel_host: true,
            argv: vec![String::from("/test.wasm")],
            env: BTreeMap::new(),
            cwd: PathBuf::from("/"),
            permission_tier: WasmPermissionTier::Full,
            limits: WasmExecutionLimits::default(),
            guest_runtime: GuestRuntimeConfig::default(),
        };
        let profile =
            WasmtimeEngineProfile::new(request.limits.max_stack_bytes).expect("engine profile");
        let engine = super::super::engine::WasmtimeEngineRegistry::new(1)
            .get_or_create(profile)
            .expect("engine");
        let expected = profile
            .async_stack_bytes()
            .expect("async stack")
            .saturating_add(
                limits::aggregate_store_memory_bytes(&request.limits)
                    .expect("aggregate store bytes"),
            );

        let state = WasmtimeStoreState::new(
            &scoped,
            host,
            engine,
            &request,
            profile,
            thread_cpu_time_ns(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(Notify::new()),
            false,
            false,
            None,
            0,
        )
        .expect("store state");
        assert_eq!(
            resources.usage(ResourceClass::WasmMemoryBytes).used,
            expected
        );
        drop(state);
        assert_eq!(resources.usage(ResourceClass::WasmMemoryBytes).used, 0);
        assert!(resources.integrity_ok());
    }

    #[test]
    fn threaded_stores_share_cpu_budget_and_main_exec_does_not_reset_it() {
        let runtime = TokioDriver::process(&DriverConfig::default())
            .expect("runtime")
            .handle();
        let process = crate::host::HostProcessContext {
            generation: 777,
            pid: 42,
        };
        let (submission, _host_events) = bounded_execution_event_channel(
            process,
            4,
            PayloadLimit::new("limits.process.pendingEventBytes", 1024).unwrap(),
            Arc::new(|| {}),
        )
        .unwrap();
        let (events, _receiver) = flume::bounded(4);
        let host = WasmtimeHostClient::new(
            ProcessHostCapabilitySet::from_event_submission(submission),
            DEFAULT_MAX_HOST_REPLY_BYTES,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Notify::new()),
            Arc::new(AtomicBool::new(false)),
            Arc::clone(runtime.resources()),
            events,
            None,
        );
        let request = StartWasmExecutionRequest {
            vm_id: "shared-cpu".into(),
            context_id: "shared-cpu".into(),
            managed_kernel_host: true,
            argv: vec!["/threads.wasm".into()],
            env: BTreeMap::new(),
            cwd: "/".into(),
            permission_tier: WasmPermissionTier::Full,
            limits: WasmExecutionLimits {
                active_cpu_time_limit_ms: Some(10),
                max_threads: Some(2),
                ..Default::default()
            },
            guest_runtime: GuestRuntimeConfig::default(),
        };
        let profile = WasmtimeEngineProfile::new_threaded(None).unwrap();
        let engine = super::super::engine::WasmtimeEngineRegistry::process()
            .get_or_create(profile)
            .unwrap();
        // Future synthetic clock values keep this deterministic and avoid CPU saturation.
        let origin = thread_cpu_time_ns().saturating_add(1_000_000_000);
        let make_state = |thread_id, started| {
            WasmtimeStoreState::new(
                &runtime,
                host.clone(),
                Arc::clone(&engine),
                &request,
                profile,
                started,
                Arc::new(AtomicBool::new(false)),
                Arc::new(Notify::new()),
                false,
                true,
                None,
                thread_id,
            )
            .unwrap()
        };
        let main = make_state(0, origin);
        let secondary = make_state(1, origin + 6_000_000);
        assert!(!main.active_cpu_exhausted_at(origin + 6_000_000));
        // Both threads are below 10ms individually, but the process has spent 12ms.
        assert!(
            secondary.active_cpu_exhausted_at(origin + 12_000_000),
            "secondary Store must not receive a fresh execution CPU allowance"
        );
        assert!(main.active_cpu_exhausted_at(origin + 12_000_000));
        drop(main);
        let mut replacement_main = make_state(0, origin + 12_000_000);
        assert!(
            replacement_main.active_cpu_exhausted_at(origin + 12_000_000),
            "replacing the main Store must retain process CPU usage"
        );
        replacement_main.active_cpu_limit_ns = Some(0);
        let exhausted = replacement_main
            .check_active_cpu_budget_at(origin)
            .unwrap_err();
        assert_eq!(exhausted.code, "ERR_AGENTOS_WASMTIME_ACTIVE_CPU_LIMIT");
        assert_eq!(exhausted.details.unwrap()["limit"], 0);
        // The exec syscall queries this before sending CommitExec to the host.
        assert_eq!(
            replacement_main.remaining_active_cpu_ms().unwrap_err().code,
            "ERR_AGENTOS_WASMTIME_ACTIVE_CPU_LIMIT"
        );
    }

    #[test]
    fn exec_replacement_does_not_refilter_guest_constructed_environment() {
        let environment = BTreeMap::from([
            ("AGENTOS_EXEC_MARK".to_owned(), "from-exec".to_owned()),
            ("VISIBLE".to_owned(), "yes".to_owned()),
        ]);

        let initial = store_guest_environment(&environment, false);
        assert_eq!(initial.get("VISIBLE").map(String::as_str), Some("yes"));
        assert!(!initial.contains_key("AGENTOS_EXEC_MARK"));

        let replacement = store_guest_environment(&environment, true);
        assert_eq!(
            replacement.get("AGENTOS_EXEC_MARK").map(String::as_str),
            Some("from-exec")
        );
    }
}
