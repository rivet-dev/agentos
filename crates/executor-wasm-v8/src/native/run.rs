//! The guest thread: one isolate, one image at a time, exec replacements in
//! place.

use super::block_on::block_on;
use super::error::{classify_trap, heap_limit_error, instantiation_error, wall_clock_error};
use super::guest::{import_object, v8_string, Bindings, GuestState, Unwind};
use super::module::{admit_image, NativeImage};
use super::Control;
use agentos_driver_tokio::accounting::ResourceLedger;
use agentos_driver_tokio::{DriverHandle, TaskClass};
use agentos_executor_contract::backend::HostServiceError;
use agentos_executor_contract::host::{
    FilesystemOperation, HostOperation, ProcessHostCapabilitySet,
};
use agentos_executor_v8_runtime::isolate;
use agentos_executor_v8_runtime::timeout::{current_thread_cpu_clock, ThreadCpuClock};
use agentos_executor_wasm_abi::{StartWasmExecutionRequest, WasmExecutionEvent};
use agentos_executor_wasm_host::image::load_module;
use agentos_executor_wasm_host::{
    active_cpu_limit_error, imports, max_host_reply_bytes, ExecutionDiagnostics, HostState,
    QueuedWasmEvent, WasmHostClient,
};
use flume::Sender;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;

#[allow(clippy::too_many_arguments)]
pub(super) fn guest_thread(
    mut guest_isolate: isolate::GuestIsolate,
    module_path: String,
    module_image: Option<super::PreloadedImage>,
    request: StartWasmExecutionRequest,
    runtime: DriverHandle,
    control: Arc<Control>,
    events: Sender<QueuedWasmEvent>,
    event_notify: Option<Arc<Notify>>,
) {
    let resources = Arc::clone(runtime.resources());
    let result = {
        let _runtime = runtime.tokio_handle().enter();
        run(
            &mut guest_isolate,
            module_path,
            module_image,
            request,
            &runtime,
            &control,
            events.clone(),
            event_notify.clone(),
        )
    };
    publish_result(&events, event_notify.as_ref(), &resources, result);
}

// One call site hands over the whole launch; a struct would only rename it.
#[allow(clippy::too_many_arguments)]
fn run(
    guest_isolate: &mut isolate::GuestIsolate,
    module_path: String,
    module_image: Option<super::PreloadedImage>,
    request: StartWasmExecutionRequest,
    runtime: &DriverHandle,
    control: &Arc<Control>,
    events: Sender<QueuedWasmEvent>,
    event_notify: Option<Arc<Notify>>,
) -> Result<i32, HostServiceError> {
    let diagnostics = Arc::new(
        ExecutionDiagnostics::new(
            request
                .env
                .get("AGENTOS_WASM_WARMUP_DEBUG")
                .is_some_and(|value| value == "1"),
        )
        .for_backend("v8"),
    );
    diagnostics.phase("launch", control.spawned_at.elapsed());
    let started = Instant::now();
    block_on(wait_until_started(control))?;
    diagnostics.phase("startWait", started.elapsed());
    let started = Instant::now();
    let host = block_on(wait_for_host(control))?;
    diagnostics.phase("hostWait", started.elapsed());
    let host = WasmHostClient::new(
        host,
        max_host_reply_bytes(&request)?,
        Arc::clone(&control.cancelled),
        Arc::clone(&control.cancel_notify),
        Arc::clone(&control.signal_pending),
        Arc::clone(runtime.resources()),
        events,
        event_notify,
    )
    .with_diagnostics(Arc::clone(&diagnostics));
    // The kernel owns the WASI capability roots and descriptor numbers.
    let started = Instant::now();
    block_on(host.submit(
        HostOperation::Filesystem(FilesystemOperation::CanonicalPreopens),
        0,
    ))?;
    diagnostics.phase("canonicalPreopens", started.elapsed());
    let started = Instant::now();
    // The vm hands over the image it loaded at spawn; otherwise (a trusted
    // initial image the vm did not stage) read it through the kernel.
    let (bytes, digest) = match module_image {
        Some(image) => (image.bytes, Some(image.digest)),
        None => (
            Arc::from(block_on(load_module(&host, &module_path, &request))?),
            None,
        ),
    };
    diagnostics.phase("moduleRead", started.elapsed());

    control
        .isolate
        .lock()
        .map_err(|_| {
            HostServiceError::new(
                "ERR_AGENTOS_WASM_CONTROL_POISONED",
                "execution control state is poisoned",
            )
        })?
        .replace(guest_isolate.thread_safe_handle());
    // A cancel that raced isolate creation must still stop the guest.
    if control.cancelled.load(Ordering::Acquire) {
        guest_isolate.terminate_execution();
    }
    let _watchdog = Watchdog::start(runtime, control, &request)?;
    let result = run_images(guest_isolate, bytes, digest, request, &host, &diagnostics);
    if let Ok(mut slot) = control.isolate.lock() {
        slot.take();
    }
    if diagnostics.enabled() {
        let reason = if result.is_ok() {
            "completed"
        } else {
            "failed"
        };
        if let Some(line) = diagnostics.line(reason, &module_path) {
            if let Err(error) = block_on(host.publish_stderr(line)) {
                eprintln!(
                    "ERR_AGENTOS_WASM_DIAGNOSTICS_PUBLISH: failed to publish phase diagnostics: {}: {}",
                    error.code, error.message
                );
            }
        }
    }
    if let Some(abort) = control.take_abort() {
        return Err(abort);
    }
    if control.cancelled.load(Ordering::Acquire) && result.is_err() {
        return Err(HostServiceError::new(
            "ECANCELED",
            "WebAssembly execution was canceled",
        ));
    }
    result
}

fn run_images(
    guest_isolate: &mut v8::Isolate,
    bytes: Arc<[u8]>,
    digest: Option<[u8; 32]>,
    request: StartWasmExecutionRequest,
    host: &WasmHostClient,
    diagnostics: &ExecutionDiagnostics,
) -> Result<i32, HostServiceError> {
    // One process image may replace itself repeatedly; every image shares
    // this active-CPU origin so exec cannot reset its budget.
    let active_cpu_started_ns = host.cpu_time_ns()?;
    let mut request = request;
    let mut environment_is_guest_visible = false;
    let started = Instant::now();
    let mut image = {
        v8::scope!(let scope, &mut *guest_isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        let admitted = admit_image(
            scope,
            &bytes,
            digest,
            &request,
            Some(request.permission_tier),
        )?;
        if !admitted.has_valid_start {
            return Err(HostServiceError::new(
                "ERR_AGENTOS_WASM_ENTRYPOINT",
                "WebAssembly module does not export a valid _start function",
            ));
        }
        diagnostics.module(bytes.len(), admitted.cache_hit);
        admitted.image
    };
    diagnostics.phase("moduleCompile", started.elapsed());
    drop(bytes);
    loop {
        // A cancel between images must not let the next one start.
        if host.canceled() {
            return Err(terminated());
        }
        let common = HostState::new(
            host.clone(),
            &request,
            active_cpu_started_ns,
            environment_is_guest_visible,
            None,
            0,
            false,
        )?;
        let state = Box::into_raw(Box::new(GuestState {
            common,
            request: request.clone(),
            memory: None,
            signal_trampoline: None,
            unwind: None,
        }));
        let mut bindings = Bindings::default();
        let outcome = run_image(
            guest_isolate,
            &image,
            &request,
            state,
            &mut bindings,
            diagnostics,
        );
        guest_isolate.cancel_terminate_execution();
        drop(bindings);
        // SAFETY: `state` came from `Box::into_raw` above, and the image has
        // made its last call, so no callback can still reach it.
        let mut state = unsafe { Box::from_raw(state) };
        // The heap limit wins over exit and exec: the termination it scheduled
        // may land after the guest already exited or replaced its image, and
        // continuing would carry the raised limit into the next image.
        if isolate::take_heap_limit_exceeded(guest_isolate) {
            return Err(heap_limit_error());
        }
        match outcome {
            Ok(()) => return Ok(state.common.exit_code.unwrap_or(0)),
            Err(failure) => {
                if let Some(code) = state.common.exit_code {
                    return Ok(code);
                }
                if let Some(replacement) = state.common.pending_exec_replacement.take() {
                    request.permission_tier = replacement.permission_tier?;
                    request.argv = replacement.argv;
                    request.env = replacement.env;
                    environment_is_guest_visible = true;
                    image = replacement.module;
                    require_tier(&image, request.permission_tier)?;
                    continue;
                }
                if state.common.exec_replaced {
                    return Err(HostServiceError::new(
                        "ERR_AGENTOS_EXEC_REPLACED",
                        "the kernel committed a replacement process image",
                    ));
                }
                match state.unwind.take() {
                    Some(Unwind::Failed(error)) => return Err(error),
                    Some(Unwind::Exit | Unwind::ExecReplaced) | None => {}
                }
                return Err(failure);
            }
        }
    }
}

/// An exec replacement was admitted for the tier the kernel then committed.
fn require_tier(
    image: &NativeImage,
    tier: agentos_executor_wasm_abi::WasmPermissionTier,
) -> Result<(), HostServiceError> {
    use agentos_executor_wasm_abi::WasmPermissionTier as Tier;
    let allowed = match tier {
        Tier::Isolated => image.tiers.isolated,
        Tier::ReadOnly => image.tiers.read_only,
        Tier::ReadWrite => image.tiers.read_write,
        Tier::Full => image.tiers.full,
    };
    if allowed {
        Ok(())
    } else {
        Err(HostServiceError::new(
            "ERR_AGENTOS_WASM_UNSUPPORTED_IMPORT",
            "replacement imports are not available at the committed permission tier",
        ))
    }
}

/// Instantiate `image` in a fresh context and run `_start`. `Err` carries the
/// classification of a trap or termination; the caller first consults the
/// host state for exit, exec, and host failures, which take precedence.
fn run_image(
    guest_isolate: &mut v8::Isolate,
    image: &NativeImage,
    request: &StartWasmExecutionRequest,
    state: *mut GuestState,
    bindings: &mut Bindings,
    diagnostics: &ExecutionDiagnostics,
) -> Result<(), HostServiceError> {
    let started = Instant::now();
    v8::scope!(let scope, guest_isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let tc, scope);

    let imports_object = import_object(tc, image, request.permission_tier, state, bindings)?;
    let module = v8::WasmModuleObject::from_compiled_module(tc, &image.compiled)
        .ok_or_else(instantiation_error)?;
    let instance_constructor = webassembly_member(tc, context, "Instance")?;
    // SAFETY: `state` is live for this image; see `GuestState`.
    unsafe { (*state).common.check_active_cpu_budget()? };
    let Some(instance) =
        instance_constructor.new_instance(tc, &[module.into(), imports_object.into()])
    else {
        if tc.has_terminated() {
            return Err(terminated());
        }
        let diagnostic = exception_text(tc);
        eprintln!("ERR_AGENTOS_WASM_INSTANTIATION: private instantiation diagnostic: {diagnostic}");
        if diagnostic.contains("Out of memory") || diagnostic.contains("could not allocate memory")
        {
            return Err(HostServiceError::new(
                "ERR_AGENTOS_WASM_MEMORY_LIMIT",
                "WebAssembly initial memory exceeds the configured linear-memory limit",
            )
            .with_details(serde_json::json!({
                "limitName": "limits.resources.maxWasmMemoryBytes",
            })));
        }
        return Err(instantiation_error());
    };
    let exports_key = v8_string(tc, "exports")?;
    let exports = instance
        .get(tc, exports_key.into())
        .and_then(|exports| v8::Local::<v8::Object>::try_from(exports).ok())
        .ok_or_else(instantiation_error)?;

    // SAFETY: as above; no other reference to the state is live here.
    unsafe {
        let memory_key = v8_string(tc, "memory")?;
        (*state).memory = exports
            .get(tc, memory_key.into())
            .and_then(|memory| v8::Local::<v8::WasmMemoryObject>::try_from(memory).ok())
            .map(|memory| v8::Global::new(tc, memory));
        if image.has_signal_trampoline {
            (*state).signal_trampoline = Some(v8::Global::new(
                tc,
                export_function(tc, exports, "__wasi_signal_trampoline")?,
            ));
        }
        (*state).common.check_active_cpu_budget()?;
    }

    diagnostics.phase("Instance", started.elapsed());
    let started = Instant::now();
    initialize_inherited_signal_mask(tc, image, exports, state)?;
    diagnostics.phase("signalMaskInit", started.elapsed());

    let start = export_function(tc, exports, "_start")?;
    let receiver = v8::undefined(tc).into();
    let started = Instant::now();
    let returned = start.call(tc, receiver, &[]).is_some();
    diagnostics.phase("wasi.start", started.elapsed());
    if returned {
        // SAFETY: as above.
        unsafe { (*state).common.check_active_cpu_budget()? };
        return Ok(());
    }
    if tc.has_terminated() {
        return Err(terminated());
    }
    Err(classify_trap(&exception_text(tc)))
}

fn initialize_inherited_signal_mask(
    tc: &mut v8::PinScope<'_, '_>,
    image: &NativeImage,
    exports: v8::Local<'_, v8::Object>,
    state: *mut GuestState,
) -> Result<(), HostServiceError> {
    // SAFETY: as in `run_image`.
    let host = unsafe { (*state).common.host.clone() };
    let mask = block_on(imports::inherited_signal_mask(&host, 0, false))?;
    let Some((low, high)) = mask else {
        return Ok(());
    };
    let setter_error = || {
        HostServiceError::new(
            "ERR_AGENTOS_WASM_INITIAL_SIGNAL_MASK",
            "WebAssembly module cannot initialize its inherited signal mask",
        )
    };
    if !image.has_initial_sigmask_setter {
        return Err(setter_error());
    }
    let setter = export_function(tc, exports, "__agentos_set_initial_sigmask")
        .map_err(|_| setter_error())?;
    let receiver = v8::undefined(tc).into();
    let low = v8::Integer::new(tc, low).into();
    let high = v8::Integer::new(tc, high).into();
    let status = setter
        .call(tc, receiver, &[low, high])
        .and_then(|status| status.int32_value(tc));
    // SAFETY: as in `run_image`.
    unsafe { (*state).common.check_active_cpu_budget()? };
    match status {
        Some(0) => Ok(()),
        Some(errno) => Err(HostServiceError::new(
            "ERR_AGENTOS_WASM_INITIAL_SIGNAL_MASK",
            format!("inherited signal-mask initialization failed with errno {errno}"),
        )),
        None => Err(HostServiceError::new(
            "ERR_AGENTOS_WASM_INITIAL_SIGNAL_MASK",
            "inherited signal-mask initialization trapped",
        )),
    }
}

fn webassembly_member<'s>(
    scope: &v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    name: &str,
) -> Result<v8::Local<'s, v8::Function>, HostServiceError> {
    let global = context.global(scope);
    let namespace_key = v8_string(scope, "WebAssembly")?;
    let namespace = global
        .get(scope, namespace_key.into())
        .and_then(|namespace| v8::Local::<v8::Object>::try_from(namespace).ok())
        .ok_or_else(instantiation_error)?;
    let key = v8_string(scope, name)?;
    namespace
        .get(scope, key.into())
        .and_then(|member| v8::Local::<v8::Function>::try_from(member).ok())
        .ok_or_else(instantiation_error)
}

fn export_function<'s>(
    scope: &v8::PinScope<'s, '_>,
    exports: v8::Local<'s, v8::Object>,
    name: &str,
) -> Result<v8::Local<'s, v8::Function>, HostServiceError> {
    let key = v8_string(scope, name)?;
    exports
        .get(scope, key.into())
        .and_then(|export| v8::Local::<v8::Function>::try_from(export).ok())
        .ok_or_else(|| {
            HostServiceError::new(
                "ERR_AGENTOS_WASM_ENTRYPOINT",
                "WebAssembly module does not export a valid _start function",
            )
        })
}

fn exception_text(tc: &v8::PinnedRef<'_, v8::TryCatch<'_, '_, v8::HandleScope<'_>>>) -> String {
    tc.exception()
        .map(|exception| exception.to_rust_string_lossy(tc))
        .unwrap_or_default()
}

fn terminated() -> HostServiceError {
    HostServiceError::new("ECANCELED", "WebAssembly execution was canceled")
}

async fn wait_until_started(control: &Control) -> Result<(), HostServiceError> {
    loop {
        let notified = control.start_notify.notified();
        if control.cancelled.load(Ordering::Acquire) {
            return Err(HostServiceError::new(
                "ECANCELED",
                "WebAssembly execution was canceled before start",
            ));
        }
        if control.started.load(Ordering::Acquire) {
            return Ok(());
        }
        notified.await;
    }
}

async fn wait_for_host(control: &Control) -> Result<ProcessHostCapabilitySet, HostServiceError> {
    loop {
        let notified = control.host_notify.notified();
        let host = control
            .host
            .lock()
            .map_err(|_| {
                HostServiceError::new(
                    "ERR_AGENTOS_WASM_HOST_LATCH_POISONED",
                    "host capability latch is poisoned",
                )
            })?
            .clone();
        if let Some(host) = host {
            return Ok(host);
        }
        if control.cancelled.load(Ordering::Acquire) {
            return Err(HostServiceError::new(
                "ECANCELED",
                "WebAssembly execution was canceled before host binding",
            ));
        }
        notified.await;
    }
}

/// Enforces the wall-clock and active-CPU limits while guest code runs
/// without making host calls. Host calls check the CPU budget themselves.
struct Watchdog(Option<tokio::task::JoinHandle<()>>);

impl Watchdog {
    /// Arm the configured limits. A limit that cannot be enforced fails the
    /// execution instead of letting the guest run unbounded.
    fn start(
        runtime: &DriverHandle,
        control: &Arc<Control>,
        request: &StartWasmExecutionRequest,
    ) -> Result<Self, HostServiceError> {
        let wall_limit_ms = request.limits.wall_clock_limit_ms;
        let cpu = match request.limits.active_cpu_time_limit_ms {
            Some(limit) => Some((
                limit,
                current_thread_cpu_clock().ok_or_else(|| {
                    HostServiceError::new(
                        "ERR_AGENTOS_WASM_CPU_CLOCK",
                        "this platform exposes no per-thread CPU clock to enforce limits.wasm.activeCpuTimeLimitMs",
                    )
                })?,
            )),
            None => None,
        };
        if wall_limit_ms.is_none() && cpu.is_none() {
            return Ok(Self(None));
        }
        let control = Arc::clone(control);
        let task = runtime
            .spawn(TaskClass::Timer, async move {
                watch(control, wall_limit_ms, cpu).await;
            })
            .map_err(|error| {
                HostServiceError::new(
                    "ERR_AGENTOS_WASM_LIMIT_WATCHDOG",
                    format!("could not start the WebAssembly limit watchdog: {error}"),
                )
            })?;
        Ok(Self(Some(task)))
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}

async fn watch(
    control: Arc<Control>,
    wall_limit_ms: Option<u64>,
    cpu: Option<(u32, ThreadCpuClock)>,
) {
    let started = tokio::time::Instant::now();
    let cpu_baseline = cpu.and_then(|(_, clock)| clock.elapsed_ms()).unwrap_or(0);
    loop {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if let Some(limit) = wall_limit_ms {
            if elapsed_ms >= limit {
                control.abort(wall_clock_error(limit));
                return;
            }
        }
        let mut next = wall_limit_ms.map(|limit| limit.saturating_sub(elapsed_ms));
        if let Some((limit_ms, clock)) = cpu {
            let used = clock
                .elapsed_ms()
                .unwrap_or(cpu_baseline)
                .saturating_sub(cpu_baseline);
            let limit = u64::from(limit_ms);
            if used >= limit {
                control.abort(active_cpu_limit_error(
                    limit.saturating_mul(1_000_000),
                    used.saturating_mul(1_000_000),
                ));
                return;
            }
            // A thread's CPU clock advances no faster than wall time, so the
            // budget cannot run out before this much time passes.
            let remaining = limit - used;
            next = Some(next.map_or(remaining, |next| next.min(remaining)));
        }
        let delay = next.unwrap_or(1_000).max(10);
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
}

fn publish_result(
    sender: &Sender<QueuedWasmEvent>,
    notify: Option<&Arc<Notify>>,
    resources: &Arc<ResourceLedger>,
    result: Result<i32, HostServiceError>,
) {
    match result {
        Ok(code) => publish(
            sender,
            notify,
            resources,
            WasmExecutionEvent::Exited(code),
            0,
        ),
        // Exec replacement and sidecar-directed cancellation have their
        // authoritative lifecycle event published by the controller.
        Err(error)
            if matches!(
                error.code.as_str(),
                "ERR_AGENTOS_EXEC_REPLACED" | "ECANCELED"
            ) => {}
        Err(error) => {
            let message = format!("{}: {}\n", error.code, error.message).into_bytes();
            let retained = message.len();
            publish(
                sender,
                notify,
                resources,
                WasmExecutionEvent::Stderr(message),
                retained,
            );
            publish(sender, notify, resources, WasmExecutionEvent::Exited(1), 0);
        }
    }
}

fn publish(
    sender: &Sender<QueuedWasmEvent>,
    notify: Option<&Arc<Notify>>,
    resources: &Arc<ResourceLedger>,
    event: WasmExecutionEvent,
    retained_bytes: usize,
) {
    let event = match QueuedWasmEvent::new(resources, Ok(event), retained_bytes) {
        Ok(event) => event,
        Err(error) => {
            eprintln!("{}: {}", error.code, error.message);
            return;
        }
    };
    if sender.send(event).is_err() {
        eprintln!("ERR_AGENTOS_WASM_EVENT_CHANNEL: execution event receiver was dropped");
        return;
    }
    if let Some(notify) = notify {
        notify.notify_one();
    }
}
