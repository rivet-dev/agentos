//! Per-execution Store state backed by agentOS host capabilities.

use super::engine::{WasmtimeEngineHandle, WasmtimeEngineProfile};
use super::limits;
use super::threads::ThreadGroup;
use crate::backend::HostServiceError;
use agentos_driver_tokio::accounting::{Reservation, ResourceClass};
use agentos_driver_tokio::DriverHandle;
use agentos_executor_wasm_abi::StartWasmExecutionRequest;
#[cfg(test)]
use agentos_executor_wasm_host::store_guest_environment;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;
use wasmtime::{Store, StoreLimits, UpdateDeadline};

pub use agentos_executor_wasm_host::{active_cpu_limit_error, max_host_reply_bytes};
#[cfg(test)]
pub use agentos_executor_wasm_host::DEFAULT_MAX_HOST_REPLY_BYTES;

/// The shared host-import channel; Wasmtime-specific only in name.
pub type WasmtimeHostClient = agentos_executor_wasm_host::WasmHostClient;
pub type PendingExecReplacement =
    agentos_executor_wasm_host::PendingExecReplacement<Arc<wasmtime::Module>>;
pub type HostState = agentos_executor_wasm_host::HostState<Arc<wasmtime::Module>>;

/// Wasmtime Store data: the shared host-import state plus the Wasmtime-only
/// parts (engine, limiter, thread group, pause, and resource reservations).
/// Dereferences to [`HostState`], so host-import fields read as before.
pub struct WasmtimeStoreState {
    common: HostState,
    pub engine: Arc<WasmtimeEngineHandle>,
    pub thread_group: Option<Arc<ThreadGroup>>,
    pub limits: StoreLimits,
    paused: Arc<AtomicBool>,
    pause_notify: Arc<Notify>,
    _async_stack_reservation: Option<Reservation>,
    _guest_memory_reservation: Option<Reservation>,
}

impl Deref for WasmtimeStoreState {
    type Target = HostState;

    fn deref(&self) -> &HostState {
        &self.common
    }
}

impl DerefMut for WasmtimeStoreState {
    fn deref_mut(&mut self) -> &mut HostState {
        &mut self.common
    }
}

impl std::fmt::Debug for WasmtimeStoreState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.common.fmt(formatter)
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
        let threaded = matches!(
            profile.feature_profile,
            crate::WasmtimeFeatureProfile::AgentOsOwnedWasiV1Threads
        );
        let hooks = thread_group
            .clone()
            .map(|group| group as Arc<dyn agentos_executor_wasm_host::ThreadGroupHooks<Arc<wasmtime::Module>>>);
        let common = HostState::new(
            host,
            request,
            active_cpu_started_ns,
            environment_is_guest_visible,
            hooks,
            thread_id,
            threaded,
        )?;
        Ok(Self {
            common,
            engine,
            thread_group,
            limits: limits::store_limits(&request.limits)?,
            paused,
            pause_notify,
            _async_stack_reservation: async_stack_reservation,
            _guest_memory_reservation: guest_memory_reservation,
        })
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

#[cfg(test)]
fn thread_cpu_time_ns() -> u64 {
    agentos_executor_wasm_host::cpu_time_ns(false).expect("test thread CPU clock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::ProcessHostCapabilitySet;
    use std::collections::BTreeMap;
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
        replacement_main.set_active_cpu_limit_ns(Some(0));
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
