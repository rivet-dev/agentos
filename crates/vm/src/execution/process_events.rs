use super::*;
use crate::protocol::ExecutionStreamChannel;
use crate::state::{DeferredRpcError, ManagedHostNetDescriptionRegistry};

fn rollback_new_deferred_connect_resources(
    process: &mut ActiveProcess,
    kernel: &mut SidecarKernel,
    kernel_readiness: &KernelSocketReadinessRegistry,
    unix_addresses: &GuestUnixAddressRegistry,
    previous_tcp_ids: &BTreeSet<String>,
    previous_unix_ids: &BTreeSet<String>,
) {
    let new_tcp_ids = process
        .tcp_sockets
        .keys()
        .filter(|socket_id| !previous_tcp_ids.contains(*socket_id))
        .cloned()
        .collect::<Vec<_>>();
    for socket_id in new_tcp_ids {
        if let Some(socket) = process.tcp_sockets.remove(&socket_id) {
            release_tcp_socket_handle(process, &socket_id, socket, kernel, kernel_readiness);
        }
    }
    let new_unix_ids = process
        .unix_sockets
        .keys()
        .filter(|socket_id| !previous_unix_ids.contains(*socket_id))
        .cloned()
        .collect::<Vec<_>>();
    for socket_id in new_unix_ids {
        if let Some(socket) = process.unix_sockets.remove(&socket_id) {
            release_unix_socket_handle(process, &socket_id, socket, unix_addresses);
        }
    }
}

pub(super) fn settle_host_call_completion_for_process(
    generation: u64,
    kernel: &mut SidecarKernel,
    kernel_readiness: &KernelSocketReadinessRegistry,
    unix_addresses: &GuestUnixAddressRegistry,
    managed_descriptions: &ManagedHostNetDescriptionRegistry,
    process: &mut ActiveProcess,
    completion: crate::state::HostCallCompletion,
) -> Result<(), VmError> {
    let identity = completion.reply.identity();
    if identity.generation != generation || identity.pid != process.kernel_pid {
        return completion
            .reply
            .fail(HostServiceError::new(
                "ESTALE",
                "host completion identity does not name the active process",
            ))
            .map_err(VmError::from);
    }
    let previous_tcp_ids = process.tcp_sockets.keys().cloned().collect::<BTreeSet<_>>();
    let previous_unix_ids = process
        .unix_sockets
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let request_id = completion.reply.identity().call_id;
    let connected = process.pending_net_connects.remove(&request_id);
    let managed_description_id = process
        .pending_managed_host_net_connects
        .remove(&request_id);
    let completion_result = match (completion.result, connected) {
        (Ok(_), Some(connected)) => finalize_net_connect(process, kernel_readiness, connected)
            .map_err(|error| crate::state::DeferredRpcError::from(host_service_error(&error))),
        (result @ Err(_), Some(connected)) => {
            match restore_pending_bound_unix_connect(process, &connected) {
                Ok(()) => result,
                Err(error) => Err(crate::state::DeferredRpcError::from(host_service_error(
                    &error,
                ))),
            }
        }
        (result, None) => result,
    };
    let result = match completion_result {
        Ok(value) => {
            if let Some(description_id) = managed_description_id {
                let update = (|| -> Result<(), VmError> {
                    let socket_id = value
                        .get("socketId")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            VmError::host("EIO", "managed connect completion omitted socket id")
                        })?
                        .to_owned();
                    let local_address =
                        crate::execution::host_dispatch::managed_socket_address_from_info(
                            &value, false,
                        )?;
                    let peer_address =
                        crate::execution::host_dispatch::managed_socket_address_from_info(
                            &value, true,
                        )?;
                    let mut descriptions = managed_descriptions.lock().map_err(|_| {
                        VmError::host("EIO", "managed description registry lock poisoned")
                    })?;
                    let description = descriptions.get_mut(&description_id).ok_or_else(|| {
                        VmError::host(
                            "ESTALE",
                            "managed connect description disappeared before completion",
                        )
                    })?;
                    let route = if description.domain == crate::executor::host::SocketDomain::Unix {
                        crate::state::ManagedHostNetRoute::UnixSocket(socket_id)
                    } else {
                        crate::state::ManagedHostNetRoute::TcpSocket(socket_id)
                    };
                    description.routes.insert(process.kernel_pid, route);
                    description.local_address = local_address;
                    description.peer_address = peer_address;
                    Ok(())
                })();
                if let Err(error) = update {
                    rollback_new_deferred_connect_resources(
                        process,
                        kernel,
                        kernel_readiness,
                        unix_addresses,
                        &previous_tcp_ids,
                        &previous_unix_ids,
                    );
                    return completion
                        .reply
                        .fail(host_service_error(&error))
                        .map_err(VmError::from);
                }
            }
            completion.reply.succeed(HostCallReply::Json(value))
        }
        Err(error) => completion.reply.fail(HostServiceError {
            code: error.code,
            message: error.message,
            details: error.details,
        }),
    };
    result.map_err(VmError::from)
}

pub(crate) fn internal_event_reply(event: &ActiveExecutionEvent) -> Option<DirectHostReplyHandle> {
    match event {
        ActiveExecutionEvent::Common(ExecutionEvent::HostCall { reply, .. }) => Some(reply.clone()),
        ActiveExecutionEvent::HostRpcRequest(request) => Some(request.reply.clone()),
        ActiveExecutionEvent::HostCallCompletion(completion) => Some(completion.reply.clone()),
        ActiveExecutionEvent::ManagedStreamReadRecheck(pending) => Some(pending.reply.clone()),
        ActiveExecutionEvent::ManagedUdpPollRecheck(pending) => Some(pending.reply.clone()),
        _ => None,
    }
}

fn reject_internal_event(event: ActiveExecutionEvent, error: &VmError) {
    if let Some(reply) = internal_event_reply(&event) {
        if let Err(reply_error) = reply.fail(host_service_error(error)) {
            tracing::error!(%reply_error, "failed to reject internal process event");
        }
    }
}

pub struct ProcessEventPumpTurn {
    pub emitted_any: bool,
    pub host_services: Vec<OwnedHostEventService>,
    pub child_bridge_services: Vec<OwnedChildBridgeEventService>,
}

/// An already claimed internal event retains its byte reservation until the
/// bounded supervisor finishes servicing it.
pub struct OwnedHostEventService {
    pub(crate) ownership: OwnershipScope,
    pub(crate) vm_id: String,
    pub(crate) process_id: String,
    pub(crate) child_path: Vec<String>,
    pub(crate) vm: crate::state::VmHandle,
    pub(crate) event: ActiveExecutionEvent,
    pub(crate) reservation: Option<PendingExecutionEventReservation>,
}

impl OwnedHostEventService {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        ownership: OwnershipScope,
        vm_id: String,
        process_id: String,
        child_path: Vec<String>,
        vm: crate::state::VmHandle,
        event: ActiveExecutionEvent,
        reservation: Option<PendingExecutionEventReservation>,
    ) -> Self {
        Self {
            ownership,
            vm_id,
            process_id,
            child_path,
            vm,
            event,
            reservation,
        }
    }
}

pub(super) struct HostFunctionProcessEventRequest {
    pub(super) runtime_context: agentos_driver_tokio::DriverHandle,
    pub(super) sidecar_requests: SharedSidecarRequestClient,
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) vm_id: String,
    pub(super) host_function_resolution: HostFunctionCommandResolution,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) paused: Arc<AtomicBool>,
    pub(super) pause_notify: Arc<tokio::sync::Notify>,
    pub(super) pending_events: Arc<Mutex<VecDeque<ActiveExecutionEvent>>>,
    pub(super) event_overflow_reason: Arc<Mutex<Option<HostServiceError>>>,
    pub(super) pending_event_bytes: Arc<AtomicUsize>,
    pub(super) pending_event_count_limit: Arc<AtomicUsize>,
    pub(super) pending_event_bytes_limit: Arc<AtomicUsize>,
    pub(super) vm_pending_event_bytes_budget: Arc<VmPendingByteBudget>,
    pub(super) event_notify: Arc<tokio::sync::Notify>,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::execution) fn enqueue_deferred_host_service_completion<B>(
    sidecar: &VmManager<B>,
    vm_id: &str,
    process_id: &str,
    runtime: agentos_driver_tokio::DriverHandle,
    reply: DirectHostReplyHandle,
    operation: &str,
    receiver: tokio::sync::oneshot::Receiver<Result<Value, DeferredRpcError>>,
    timeout: Option<Duration>,
    task_class: agentos_driver_tokio::TaskClass,
) -> Result<(), VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let task_reply = reply.clone();
    let method = operation.to_owned();
    let vm = sidecar
        .vms
        .get(vm_id)
        .expect("validated deferred-service VM remains registered");
    let connection_id = vm.connection_id.clone();
    let session_id = vm.session_id.clone();
    let sender = sidecar.process_event_sender.clone();
    let event_notify = Arc::clone(&sidecar.process_event_notify);
    let envelope_vm_id = vm_id.to_owned();
    let envelope_process_id = process_id.to_owned();
    if let Err(error) = runtime.spawn(task_class, async move {
        let receive = async {
            receiver.await.unwrap_or_else(|_| {
                Err(DeferredRpcError {
                    code: "ERR_AGENTOS_DEFERRED_RPC_RESPONSE_CHANNEL_CLOSED".to_owned(),
                    message: format!("deferred host-service response channel closed for {method}"),
                    details: None,
                })
            })
        };
        let result = match timeout {
            Some(timeout) => {
                match crate::execution::operation_deadline_timeout(&method, timeout, receive).await {
                    Ok(result) => result,
                    Err(_) => Err(DeferredRpcError {
                        code: "ETIMEDOUT".to_owned(),
                        message: format!(
                            "{method} exceeded limits.reactor.operationDeadlineMs ({} ms)",
                            timeout.as_millis()
                        ),
                        details: None,
                    }),
                }
            }
            None => receive.await,
        };
        let envelope = ProcessEventEnvelope {
            connection_id,
            session_id,
            vm_id: envelope_vm_id,
            process_id: envelope_process_id,
            child_path: Vec::new(),
            event: ActiveExecutionEvent::HostCallCompletion(
                crate::state::HostCallCompletion {
                    reply: task_reply,
                    result,
                },
            ),
        };
        if let Err(error) = sender.send(envelope).await {
            if let ActiveExecutionEvent::HostCallCompletion(completion) = error.0.event {
                if let Err(reply_error) = completion.reply.fail(HostServiceError::new(
                    "ECANCELED",
                    "deferred host-service completion lane closed",
                )) {
                    eprintln!(
                        "ERR_AGENTOS_HOST_REPLY_SETTLEMENT: failed to cancel deferred host-service completion after lane closure: {reply_error}"
                    );
                }
            }
            eprintln!(
                "ERR_AGENTOS_PROCESS_EVENT_CHANNEL_CLOSED: deferred host-service completion could not be delivered"
            );
        } else {
            event_notify.notify_one();
        }
    }) {
        reply
            .fail(host_service_error(&VmError::from(error)))
            .map_err(VmError::from)?;
    }
    Ok(())
}

// The producer owns these independent atomics/queues; keeping them explicit
// avoids introducing another partially initialized shared-state wrapper.
#[allow(clippy::too_many_arguments)]
pub(crate) fn send_host_function_process_event(
    cancelled: &AtomicBool,
    pending_events: &Arc<Mutex<VecDeque<ActiveExecutionEvent>>>,
    event_overflow_reason: &Mutex<Option<HostServiceError>>,
    pending_event_bytes: &AtomicUsize,
    pending_event_count_limit: &AtomicUsize,
    pending_event_bytes_limit: &AtomicUsize,
    vm_pending_event_bytes_budget: &VmPendingByteBudget,
    event: ActiveExecutionEvent,
) -> bool {
    let mut pending_events = pending_events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cancelled.load(Ordering::Acquire) {
        return false;
    }
    let count_limit = pending_event_count_limit.load(Ordering::Acquire);
    let event_bytes = event.retained_bytes();
    let bytes = pending_event_bytes.load(Ordering::Acquire);
    if pending_events.len() >= count_limit {
        let mut reason = event_overflow_reason
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reason.get_or_insert_with(|| {
            HostServiceError::new(
                "ERR_AGENTOS_RESOURCE_LIMIT",
                format!(
                    "process execution event queue exceeded {count_limit} events \
                     (limits.process.pendingEventCount); raise limits.process.pendingEventCount"
                ),
            )
            .with_details(json!({
                "limitName": "limits.process.pendingEventCount",
                "limit": count_limit,
                "observed": pending_events.len().saturating_add(1),
            }))
        });
        return false;
    }
    let byte_limit = pending_event_bytes_limit.load(Ordering::Acquire);
    if bytes.saturating_add(event_bytes) > byte_limit {
        let mut reason = event_overflow_reason
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reason.get_or_insert_with(|| {
            HostServiceError::new(
                "ERR_AGENTOS_RESOURCE_LIMIT",
                format!(
                    "process execution event queue exceeded {byte_limit} bytes \
                     (limits.process.pendingEventBytes); raise limits.process.pendingEventBytes"
                ),
            )
            .with_details(json!({
                "limitName": "limits.process.pendingEventBytes",
                "limit": byte_limit,
                "observed": bytes.saturating_add(event_bytes),
            }))
        });
        return false;
    }
    if !vm_pending_event_bytes_budget.try_reserve(event_bytes) {
        let limit = vm_pending_event_bytes_budget.limit();
        let observed = vm_pending_event_bytes_budget
            .used()
            .saturating_add(event_bytes);
        let mut reason = event_overflow_reason
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reason.get_or_insert_with(|| {
            HostServiceError::new(
                "ERR_AGENTOS_RESOURCE_LIMIT",
                format!(
                    "VM process execution event queues exceeded {limit} bytes \
                     (limits.process.pendingEventBytes); raise limits.process.pendingEventBytes"
                ),
            )
            .with_details(json!({
                "limitName": "limits.process.pendingEventBytes",
                "limit": limit,
                "observed": observed,
            }))
        });
        return false;
    }
    pending_events.push_back(event);
    pending_event_bytes.fetch_add(event_bytes, Ordering::AcqRel);
    true
}

#[allow(clippy::too_many_arguments)]
fn send_host_function_process_event_and_notify(
    cancelled: &AtomicBool,
    pending_events: &Arc<Mutex<VecDeque<ActiveExecutionEvent>>>,
    event_overflow_reason: &Mutex<Option<HostServiceError>>,
    pending_event_bytes: &AtomicUsize,
    pending_event_count_limit: &AtomicUsize,
    pending_event_bytes_limit: &AtomicUsize,
    vm_pending_event_bytes_budget: &VmPendingByteBudget,
    event_notify: &tokio::sync::Notify,
    event: ActiveExecutionEvent,
) -> bool {
    let sent = send_host_function_process_event(
        cancelled,
        pending_events,
        event_overflow_reason,
        pending_event_bytes,
        pending_event_count_limit,
        pending_event_bytes_limit,
        vm_pending_event_bytes_budget,
        event,
    );
    if sent {
        event_notify.notify_one();
    }
    sent
}

pub(super) fn spawn_host_function_process_events(request: HostFunctionProcessEventRequest) {
    // A STOP acknowledged before producer admission must prevent the trusted
    // callback from starting. Resume wakes this one bounded gate task. A
    // callback already in flight may finish, but its events remain hidden by
    // the paused adapter poll gate until CONT.
    if request.paused.load(Ordering::Acquire) {
        let runtime = request.runtime_context.clone();
        let paused = Arc::clone(&request.paused);
        let pause_notify = Arc::clone(&request.pause_notify);
        let cancelled = Arc::clone(&request.cancelled);
        let failure_reason = Arc::clone(&request.event_overflow_reason);
        let failure_notify = Arc::clone(&request.event_notify);
        if let Err(error) = runtime.spawn(agentos_driver_tokio::TaskClass::Vm, async move {
            loop {
                let notified = pause_notify.notified();
                if cancelled.load(Ordering::Acquire) {
                    return;
                }
                if !paused.load(Ordering::Acquire) {
                    break;
                }
                notified.await;
            }
            spawn_host_function_process_events(request);
        }) {
            let mut reason = failure_reason
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            reason.get_or_insert_with(|| {
                HostServiceError::new(
                    "ERR_AGENTOS_HOST_FUNCTION_PAUSE_GATE",
                    format!("failed to schedule paused host_function producer gate: {error}"),
                )
            });
            failure_notify.notify_one();
        }
        return;
    }
    let HostFunctionProcessEventRequest {
        runtime_context,
        sidecar_requests,
        connection_id,
        session_id,
        vm_id,
        host_function_resolution,
        cancelled,
        paused: _,
        pause_notify: _,
        pending_events,
        event_overflow_reason,
        pending_event_bytes,
        pending_event_count_limit,
        pending_event_bytes_limit,
        vm_pending_event_bytes_budget,
        event_notify,
    } = request;
    let failure_cancelled = Arc::clone(&cancelled);
    let failure_events = Arc::clone(&pending_events);
    let failure_overflow_reason = Arc::clone(&event_overflow_reason);
    let failure_event_bytes = Arc::clone(&pending_event_bytes);
    let failure_event_count_limit = Arc::clone(&pending_event_count_limit);
    let failure_event_bytes_limit = Arc::clone(&pending_event_bytes_limit);
    let failure_vm_event_bytes_budget = Arc::clone(&vm_pending_event_bytes_budget);
    let failure_notify = Arc::clone(&event_notify);
    let submit_result =
        runtime_context
            .blocking()
            .submit(HOST_FUNCTION_CALL_BLOCKING_JOB_BYTES, move || {
                let enqueue = |event| {
                    send_host_function_process_event_and_notify(
                        &cancelled,
                        &pending_events,
                        &event_overflow_reason,
                        &pending_event_bytes,
                        &pending_event_count_limit,
                        &pending_event_bytes_limit,
                        &vm_pending_event_bytes_budget,
                        &event_notify,
                        event,
                    )
                };
                match host_function_resolution {
                    HostFunctionCommandResolution::Failure(message) => {
                        let output_enqueued = enqueue(ActiveExecutionEvent::Stderr(
                            format_host_function_failure_output(
                            &message,
                            ),
                        ));
                        if !output_enqueued && !cancelled.load(Ordering::Acquire) {
                            eprintln!(
                                "ERR_AGENTOS_HOST_FUNCTION_EVENT_DELIVERY: failed to enqueue host_function failure output; queue limit state retains the typed failure"
                            );
                        } else if output_enqueued
                            && !enqueue(ActiveExecutionEvent::Exited(1))
                            && !cancelled.load(Ordering::Acquire)
                        {
                            eprintln!(
                                "ERR_AGENTOS_HOST_FUNCTION_EVENT_DELIVERY: failed to enqueue host_function failure exit event; queue limit state retains the typed failure"
                            );
                        }
                    }
                    HostFunctionCommandResolution::Invoke { request, timeout } => {
                        let response = sidecar_requests.invoke(
                            OwnershipScope::vm(connection_id, session_id, vm_id),
                            SidecarRequestPayload::HostCallback(request),
                            timeout,
                        );
                        if cancelled.load(Ordering::Acquire) {
                            return;
                        }
                        let (output, exit_code, stdout) = match response {
                            Ok(crate::protocol::SidecarResponsePayload::HostCallbackResult(
                                result,
                            )) => {
                                if let Some(value) = result.result {
                                    let value: serde_json::Value = serde_json::from_str(&value)
                                        .unwrap_or(serde_json::Value::String(value));
                                    let output = serde_json::to_vec(&json!({
                                        "ok": true,
                                        "result": value,
                                    }))
                                    .unwrap_or_else(|error| {
                                        format_host_function_failure_output(&format!(
                                            "failed to serialize host function result: {error}"
                                        ))
                                    });
                                    (output, 0, true)
                                } else {
                                    let message = result.error.unwrap_or_else(|| {
                                        String::from("host function invocation returned no result")
                                    });
                                    (format_host_function_failure_output(&message), 1, false)
                                }
                            }
                            Ok(_) => (
                                format_host_function_failure_output(
                                    "unexpected sidecar host function response",
                                ),
                                1,
                                false,
                            ),
                            Err(error) => (
                                format_host_function_failure_output(&error.to_string()),
                                1,
                                false,
                            ),
                        };
                        let output_event = if stdout {
                            ActiveExecutionEvent::Stdout(output)
                        } else {
                            ActiveExecutionEvent::Stderr(output)
                        };
                        let output_enqueued = enqueue(output_event);
                        if !output_enqueued && !cancelled.load(Ordering::Acquire) {
                            eprintln!(
                                "ERR_AGENTOS_HOST_FUNCTION_EVENT_DELIVERY: failed to enqueue host_function result output; queue limit state retains the typed failure"
                            );
                        } else if output_enqueued
                            && !enqueue(ActiveExecutionEvent::Exited(exit_code))
                            && !cancelled.load(Ordering::Acquire)
                        {
                            eprintln!(
                                "ERR_AGENTOS_HOST_FUNCTION_EVENT_DELIVERY: failed to enqueue host_function exit event; queue limit state retains the typed failure"
                            );
                        }
                    }
                }
            });
    if let Err(error) = submit_result {
        let enqueue_failure = |event| {
            send_host_function_process_event_and_notify(
                &failure_cancelled,
                &failure_events,
                &failure_overflow_reason,
                &failure_event_bytes,
                &failure_event_count_limit,
                &failure_event_bytes_limit,
                &failure_vm_event_bytes_budget,
                &failure_notify,
                event,
            )
        };
        let output_enqueued = enqueue_failure(ActiveExecutionEvent::Stderr(
            format_host_function_failure_output(&error.to_string()),
        ));
        if !output_enqueued && !failure_cancelled.load(Ordering::Acquire) {
            eprintln!(
                "ERR_AGENTOS_HOST_FUNCTION_EVENT_DELIVERY: failed to enqueue blocking-admission failure output; queue limit state retains the typed failure"
            );
        } else if output_enqueued
            && !enqueue_failure(ActiveExecutionEvent::Exited(1))
            && !failure_cancelled.load(Ordering::Acquire)
        {
            eprintln!(
                "ERR_AGENTOS_HOST_FUNCTION_EVENT_DELIVERY: failed to enqueue blocking-admission exit event; queue limit state retains the typed failure"
            );
        }
    }
}

static SYNC_RPC_STATS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::BTreeMap<String, u64>>,
> = std::sync::OnceLock::new();

#[derive(Default)]
struct ExecutePhaseStats {
    calls: u64,
    total_ns: u128,
    max_ns: u128,
}

static EXECUTE_PHASES: OnceLock<Mutex<BTreeMap<String, ExecutePhaseStats>>> = OnceLock::new();
static EXECUTE_LIFETIMES: OnceLock<Mutex<BTreeMap<String, Instant>>> = OnceLock::new();
static EXECUTE_EXIT_EVENT_QUEUED: OnceLock<Mutex<BTreeMap<String, Instant>>> = OnceLock::new();

fn execute_phases_enabled() -> bool {
    std::env::var("AGENTOS_EXECUTE_PHASES").as_deref() == Ok("1")
}

fn execute_phase_key(vm_id: &str, process_id: &str) -> String {
    format!("{vm_id}/{process_id}")
}

pub(crate) fn record_execute_phase(stage: &str, elapsed: Duration) {
    if !execute_phases_enabled() {
        return;
    }
    let phases = EXECUTE_PHASES.get_or_init(|| Mutex::new(BTreeMap::new()));
    let Ok(mut phases) = phases.lock() else {
        eprintln!("ERR_AGENTOS_DIAGNOSTIC_STATE: execute-phase statistics lock is poisoned");
        return;
    };
    let stats = phases.entry(stage.to_string()).or_default();
    stats.calls += 1;
    let elapsed_ns = elapsed.as_nanos();
    stats.total_ns += elapsed_ns;
    stats.max_ns = stats.max_ns.max(elapsed_ns);

    let Some(path) = std::env::var_os("AGENTOS_EXECUTE_PHASES_FILE") else {
        return;
    };
    let mut output = String::new();
    for (stage, stats) in phases.iter() {
        let total_us = stats.total_ns / 1_000;
        let avg_us = if stats.calls == 0 {
            0
        } else {
            total_us / u128::from(stats.calls)
        };
        let max_us = stats.max_ns / 1_000;
        output.push_str(&format!(
            "stage={stage} calls={} total_us={total_us} avg_us={avg_us} max_us={max_us}\n",
            stats.calls
        ));
    }
    if let Err(error) = fs::write(&path, output) {
        eprintln!(
            "ERR_AGENTOS_DIAGNOSTIC_WRITE: failed to write process execute-phase statistics to {}: {error}",
            path.to_string_lossy()
        );
    }
}

pub(super) fn mark_execute_response_ready(vm_id: &str, process_id: &str) {
    if !execute_phases_enabled() {
        return;
    }
    let lifetimes = EXECUTE_LIFETIMES.get_or_init(|| Mutex::new(BTreeMap::new()));
    match lifetimes.lock() {
        Ok(mut lifetimes) => {
            lifetimes.insert(execute_phase_key(vm_id, process_id), Instant::now());
        }
        Err(_) => {
            eprintln!("ERR_AGENTOS_DIAGNOSTIC_STATE: execute-lifetime lock is poisoned");
        }
    }
}

pub(crate) fn mark_execute_exit_event_queued(vm_id: &str, process_id: &str) {
    if !execute_phases_enabled() {
        return;
    }
    let queued = EXECUTE_EXIT_EVENT_QUEUED.get_or_init(|| Mutex::new(BTreeMap::new()));
    match queued.lock() {
        Ok(mut queued) => {
            let key = execute_phase_key(vm_id, process_id);
            if let std::collections::btree_map::Entry::Vacant(entry) = queued.entry(key) {
                record_execute_response_to_exit_milestone(
                    "execute_response_to_exit_event_queued",
                    vm_id,
                    process_id,
                );
                entry.insert(Instant::now());
            }
        }
        Err(_) => {
            eprintln!("ERR_AGENTOS_DIAGNOSTIC_STATE: execute-exit queue timing lock is poisoned");
        }
    }
}

pub(crate) fn record_execute_exit_event_queue_wait(stage: &str, vm_id: &str, process_id: &str) {
    if !execute_phases_enabled() {
        return;
    }
    let Some(queued) = EXECUTE_EXIT_EVENT_QUEUED.get() else {
        return;
    };
    let Ok(mut queued) = queued.lock() else {
        eprintln!("ERR_AGENTOS_DIAGNOSTIC_STATE: execute-exit queue timing lock is poisoned");
        return;
    };
    if let Some(started) = queued.remove(&execute_phase_key(vm_id, process_id)) {
        record_execute_phase(stage, started.elapsed());
    }
}

pub(crate) fn record_execute_response_to_exit_milestone(
    stage: &str,
    vm_id: &str,
    process_id: &str,
) {
    if !execute_phases_enabled() {
        return;
    }
    let Some(lifetimes) = EXECUTE_LIFETIMES.get() else {
        return;
    };
    let Ok(lifetimes) = lifetimes.lock() else {
        eprintln!("ERR_AGENTOS_DIAGNOSTIC_STATE: execute-lifetime lock is poisoned");
        return;
    };
    if let Some(started) = lifetimes.get(&execute_phase_key(vm_id, process_id)) {
        record_execute_phase(stage, started.elapsed());
    }
}

fn record_execute_response_to_exit(vm_id: &str, process_id: &str) {
    if !execute_phases_enabled() {
        return;
    }
    let Some(lifetimes) = EXECUTE_LIFETIMES.get() else {
        return;
    };
    let Ok(mut lifetimes) = lifetimes.lock() else {
        eprintln!("ERR_AGENTOS_DIAGNOSTIC_STATE: execute-lifetime lock is poisoned");
        return;
    };
    if let Some(started) = lifetimes.remove(&execute_phase_key(vm_id, process_id)) {
        record_execute_phase("execute_response_to_exit_event", started.elapsed());
    }
}

pub(super) fn sync_rpc_trace_enabled() -> bool {
    std::env::var("AGENTOS_SYNC_RPC_TRACE").as_deref() == Ok("1")
}

pub(super) fn record_sync_rpc(method: &str) {
    let stats =
        SYNC_RPC_STATS.get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()));
    let Ok(mut map) = stats.lock() else {
        eprintln!("ERR_AGENTOS_DIAGNOSTIC_STATE: sync-RPC statistics lock is poisoned");
        return;
    };
    *map.entry(method.to_string()).or_insert(0) += 1;
    let total: u64 = map.values().sum();
    if total == 1 || total.is_multiple_of(50) {
        let mut top: Vec<(&String, &u64)> = map.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1));
        let breakdown = top
            .iter()
            .take(8)
            .map(|(m, c)| format!("{m}={c}"))
            .collect::<Vec<_>>()
            .join(" ");
        tracing::info!(target: "agentos_vm::perf", total, %breakdown, "sync_rpc count");
    }
}

impl<B> VmManager<B>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    /// Prepare only after the supervisor owns the VM operation permit. All
    /// suspension retains owned capabilities, never the central manager.
    pub(crate) fn prepare_owned_host_event_service(
        &mut self,
        target: OwnedHostEventService,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), VmError>> + 'static>> {
        let OwnedHostEventService {
            vm_id,
            process_id,
            child_path,
            vm,
            event,
            reservation,
            ..
        } = target;
        let future: std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), VmError>>>> =
            match event {
                ActiveExecutionEvent::Common(ExecutionEvent::HostCall { operation, reply }) => {
                    if child_path.is_empty() {
                        self.prepare_owned_root_host_call(&vm_id, &process_id, vm, operation, reply)
                    } else {
                        self.prepare_owned_descendant_host_call(
                            &vm_id,
                            &process_id,
                            &child_path,
                            vm,
                            operation,
                            reply,
                        )
                    }
                }
                ActiveExecutionEvent::HostRpcRequest(request) => {
                    self.prepare_owned_host_rpc(&vm_id, &process_id, &child_path, vm, request)
                }
                event => {
                    let result = (|| -> Result<(), VmError> {
                        match event {
                            ActiveExecutionEvent::HostCallCompletion(completion) => {
                                vm.try_command("settle owned host completion", |state| {
                                    let readiness = Arc::clone(&state.kernel_socket_readiness);
                                    let unix_addresses = Arc::clone(&state.unix_address_registry);
                                    let descriptions =
                                        Arc::clone(&state.managed_host_net_descriptions);
                                    let Some(root) = state.active_processes.get_mut(&process_id)
                                    else {
                                        return completion
                                            .reply
                                            .fail(HostServiceError::new(
                                                "ESTALE",
                                                "host completion root was reaped",
                                            ))
                                            .map_err(VmError::from);
                                    };
                                    let Some(process) =
                                        Self::active_process_by_owned_path_mut(root, &child_path)
                                    else {
                                        return completion
                                            .reply
                                            .fail(HostServiceError::new(
                                                "ESTALE",
                                                "host completion descendant was reaped",
                                            ))
                                            .map_err(VmError::from);
                                    };
                                    settle_host_call_completion_for_process(
                                        state.generation,
                                        &mut state.kernel,
                                        &readiness,
                                        &unix_addresses,
                                        &descriptions,
                                        process,
                                        completion,
                                    )
                                })
                            }
                            ActiveExecutionEvent::ManagedStreamReadRecheck(pending) => {
                                dispatch_claimed_context_stream_read(
                                    self,
                                    &vm_id,
                                    &process_id,
                                    *pending,
                                )
                            }
                            ActiveExecutionEvent::ManagedUdpPollRecheck(pending) => {
                                dispatch_claimed_context_udp_poll(
                                    self,
                                    &vm_id,
                                    &process_id,
                                    *pending,
                                )
                            }
                            ActiveExecutionEvent::SignalState {
                                signal,
                                registration,
                            } => vm.try_read("apply owned signal registration", |state| {
                                let root = state
                                    .active_processes
                                    .get(&process_id)
                                    .ok_or_else(|| missing_process_error(&vm_id, &process_id))?;
                                let path =
                                    child_path.iter().map(String::as_str).collect::<Vec<_>>();
                                let process = Self::active_process_by_path(root, &path)
                                    .ok_or_else(|| missing_process_error(&vm_id, &process_id))?;
                                apply_kernel_signal_registration(process, signal, &registration)
                            })?,
                            ActiveExecutionEvent::DeferredPosixPollWake => {
                                if child_path.is_empty() {
                                    self.recheck_root_deferred_operations(&vm_id, &process_id, true)
                                } else {
                                    let path =
                                        child_path.iter().map(String::as_str).collect::<Vec<_>>();
                                    self.service_descendant_guest_wait(
                                        &vm_id,
                                        &process_id,
                                        &path,
                                        None,
                                    )?;
                                    self.service_descendant_kernel_poll(
                                        &vm_id,
                                        &process_id,
                                        &path,
                                        None,
                                    )?;
                                    self.service_descendant_kernel_read(
                                        &vm_id,
                                        &process_id,
                                        &path,
                                        None,
                                    )
                                }
                            }
                            ActiveExecutionEvent::Common(ExecutionEvent::Warning(error)) => {
                                tracing::warn!(%error, "executor warning");
                                Ok(())
                            }
                            event => Err(VmError::InvalidState(format!(
                                "ERR_AGENTOS_PUBLIC_EVENT_ON_INTERNAL_SERVICE: {event:?}"
                            ))),
                        }
                    })();
                    Box::pin(async move { result })
                }
            };
        Box::pin(async move {
            let _reservation = reservation;
            future.await
        })
    }

    /// Move a thread-safe runtime completion back into the exact LocalSet-owned
    /// process queue. Public stdout/stderr/exit envelopes continue to the
    /// broker unchanged.
    fn route_received_internal_process_event(
        &mut self,
        envelope: ProcessEventEnvelope,
    ) -> Result<Option<ProcessEventEnvelope>, VmError> {
        if !Self::internal_execution_event(&envelope.event) {
            return Ok(Some(envelope));
        }
        self.validate_process_event_envelope_locator(&envelope)?;
        let target_label = if envelope.child_path.is_empty() {
            envelope.process_id.clone()
        } else {
            format!("{}/{}", envelope.process_id, envelope.child_path.join("/"))
        };
        let Some(mut vm) = self.vms.get_mut(&envelope.vm_id) else {
            tracing::debug!(
                vm_id = envelope.vm_id,
                process_id = target_label,
                "ERR_AGENTOS_STALE_PROCESS_EVENT: runtime completion targeted a disposed VM"
            );
            reject_internal_event(
                envelope.event,
                &VmError::host("ESTALE", "internal process event target no longer exists"),
            );
            return Ok(None);
        };
        if vm.connection_id != envelope.connection_id || vm.session_id != envelope.session_id {
            return Err(VmError::InvalidState(format!(
                "ERR_AGENTOS_PROCESS_EVENT_SCOPE_MISMATCH: runtime completion for VM {} carried connection/session {}/{}, expected {}/{}",
                envelope.vm_id,
                envelope.connection_id,
                envelope.session_id,
                vm.connection_id,
                vm.session_id
            )));
        }
        let Some(root) = vm.active_processes.get_mut(&envelope.process_id) else {
            tracing::debug!(
                vm_id = envelope.vm_id,
                process_id = target_label,
                "ERR_AGENTOS_STALE_PROCESS_EVENT: runtime completion targeted a reaped root process"
            );
            reject_internal_event(
                envelope.event,
                &VmError::host("ESTALE", "internal process event target no longer exists"),
            );
            return Ok(None);
        };
        let Some(process) = Self::active_process_by_owned_path_mut(root, &envelope.child_path)
        else {
            tracing::debug!(
                vm_id = envelope.vm_id,
                process_id = target_label,
                "ERR_AGENTOS_STALE_PROCESS_EVENT: runtime completion targeted a reaped descendant"
            );
            reject_internal_event(
                envelope.event,
                &VmError::host("ESTALE", "internal process event target no longer exists"),
            );
            return Ok(None);
        };
        match process.try_queue_pending_execution_envelope(envelope) {
            Ok(()) => Ok(None),
            Err((error, envelope)) => {
                reject_internal_event(envelope.event, &error);
                Err(error)
            }
        }
    }

    /// Transfer channel events one at a time so an admission error cannot drop
    /// later envelopes that were already removed from the bounded channel.
    pub(crate) fn drain_runtime_process_event_channel_nowait(&mut self) -> Result<bool, VmError> {
        let transfer_limit = self.config.runtime.protocol.max_process_events.max(1);
        let mut transferred = 0usize;
        while transferred < transfer_limit {
            let envelope = if let Some(envelope) = self.deferred_process_event_envelope.take() {
                self.observe_pending_process_event_depth();
                envelope
            } else {
                if self.pending_process_event_capacity() == 0 {
                    break;
                }
                let next = {
                    let receiver = self.process_event_receiver.as_mut().ok_or_else(|| {
                        VmError::InvalidState(String::from("process event receiver unavailable"))
                    })?;
                    receiver.try_recv()
                };
                match next {
                    Ok(envelope) => envelope,
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                }
            };
            transferred = transferred.saturating_add(1);
            if let Some(envelope) = self.route_received_internal_process_event(envelope)? {
                self.validate_process_event_envelope_locator(&envelope)?;
                let event_byte_limit = self
                    .vms
                    .get(&envelope.vm_id)
                    .map(|vm| vm.limits.process.pending_event_bytes)
                    .unwrap_or(crate::core::limits::DEFAULT_PROCESS_PENDING_EVENT_BYTES);
                if envelope.retained_bytes() > event_byte_limit {
                    return Err(VmError::InvalidState(format!(
                        "ERR_AGENTOS_PROCESS_EVENT_BYTES_LIMIT: process event for VM {} retains {} bytes, exceeding limits.process.pendingEventBytes ({event_byte_limit}); raise limits.process.pendingEventBytes",
                        envelope.vm_id,
                        envelope.retained_bytes()
                    )));
                }
                if let Err((error, envelope)) = self.try_queue_pending_process_event(envelope) {
                    debug_assert!(self.deferred_process_event_envelope.is_none());
                    self.deferred_process_event_envelope = Some(envelope);
                    // Retaining a blocked envelope is not forward progress:
                    // public pollers must wait for capacity rather than spin.
                    transferred = transferred.saturating_sub(1);
                    self.observe_pending_process_event_depth();
                    tracing::debug!(
                        %error,
                        "process-event receiver paused at temporary public-queue capacity"
                    );
                    break;
                }
            }
        }
        let has_more = self
            .process_event_receiver
            .as_ref()
            .is_some_and(|receiver| !receiver.is_empty());
        if has_more && self.deferred_process_event_envelope.is_none() {
            self.process_event_notify.notify_one();
        }
        Ok(transferred > 0)
    }

    /// Perform one bounded, non-suspending process-event turn. Runtime-owned
    /// queues remain durable; this command only transfers events that are
    /// already ready and never waits for a producer.
    pub fn pump_process_events_nowait(
        &mut self,
        ownership: &OwnershipScope,
        max_service_claims: usize,
    ) -> Result<ProcessEventPumpTurn, VmError> {
        let mut emitted_any = self.poll_in_process_event_services_nowait();
        let mut host_services = Vec::new();
        let mut child_bridge_services = Vec::new();
        let mut root_source_remains = false;
        self.expire_public_execution_deadlines()?;

        if self.drain_runtime_process_event_channel_nowait()? {
            emitted_any = true;
        }

        for vm_id in self.vm_ids_for_scope(ownership)? {
            let work_limit = self.config.runtime.fairness.vm_quantum_operations;
            let Some((connection_id, session_id, process_ids)) = self.vms.get(&vm_id).map(|vm| {
                vm.kernel.reap_due_zombies();
                (
                    vm.connection_id.clone(),
                    vm.session_id.clone(),
                    vm.active_processes.keys().cloned().collect::<Vec<_>>(),
                )
            }) else {
                continue;
            };
            let mut work = 0usize;
            for process_id in process_ids {
                if host_services
                    .len()
                    .saturating_add(child_bridge_services.len())
                    >= max_service_claims
                {
                    self.process_event_notify.notify_one();
                    break;
                }
                if work >= work_limit {
                    self.process_event_notify.notify_one();
                    break;
                }
                if self
                    .vms
                    .get(&vm_id)
                    .is_some_and(|vm| vm.detached_child_processes.contains(&process_id))
                {
                    continue;
                }
                self.recheck_root_deferred_operations(&vm_id, &process_id, false)?;
                // This is a hot poll path. Keep the event inline instead of
                // allocating once per process just to shrink the enum.
                #[allow(clippy::large_enum_variant)]
                enum PollResult {
                    Event(Option<PolledExecutionEvent>),
                    RecoverClosed,
                }
                let polled = {
                    let Some(mut vm) = self.vms.get_mut(&vm_id) else {
                        continue;
                    };
                    let Some(process) = vm.active_processes.get_mut(&process_id) else {
                        continue;
                    };
                    if let Some(event) = process.lease_pending_execution_event() {
                        PollResult::Event(Some(event))
                    } else {
                        match process.try_poll_execution_event() {
                            Ok(event) => PollResult::Event(event),
                            Err(VmError::ExecutionEventChannelClosed { .. }) => {
                                PollResult::RecoverClosed
                            }
                            Err(error) => return Err(error),
                        }
                    }
                };
                let event = match polled {
                    PollResult::Event(event) => event,
                    PollResult::RecoverClosed => self
                        .recover_closed_root_runtime_process_event(&vm_id, &process_id)?
                        .map(PolledExecutionEvent::unreserved),
                };
                let Some(event) = event else { continue };
                root_source_remains |= self.vms.get(&vm_id).is_some_and(|vm| {
                    vm.active_processes.get(&process_id).is_some_and(|process| {
                        !process.pending_execution_events.is_empty()
                            || process.execution.has_pending_events()
                    })
                });
                if Self::internal_execution_event(event.event()) {
                    let PolledExecutionEvent { event, reservation } = event;
                    if let Some(vm) = self.vms.handle(&vm_id) {
                        host_services.push(OwnedHostEventService::new(
                            OwnershipScope::vm(&connection_id, &session_id, &vm_id),
                            vm_id.clone(),
                            process_id,
                            Vec::new(),
                            vm,
                            event,
                            reservation,
                        ));
                    }

                    work = work.saturating_add(1);
                    continue;
                }
                let PolledExecutionEvent { event, reservation } = event;
                let envelope = ProcessEventEnvelope {
                    connection_id: connection_id.clone(),
                    session_id: session_id.clone(),
                    vm_id: vm_id.clone(),
                    child_path: Vec::new(),
                    process_id,
                    event,
                };
                if let Err(error) = self.check_pending_process_event_capacity(&envelope) {
                    let Some(mut vm) = self.vms.get_mut(&vm_id) else {
                        return Err(error);
                    };
                    if let Some(process) = vm.active_processes.get_mut(&envelope.process_id) {
                        process.requeue_pending_execution_event(PolledExecutionEvent {
                            event: envelope.event,
                            reservation,
                        })?;
                    }
                    return Err(error);
                }
                self.queue_pending_process_event(envelope)?;
                drop(reservation);
                emitted_any = true;
                work = work.saturating_add(1);
            }
            if self.pump_child_process_events_nowait(
                &vm_id,
                &mut host_services,
                &mut child_bridge_services,
                max_service_claims,
            )? {
                emitted_any = true;
            }
            if self.pump_detached_child_process_events_nowait(
                &vm_id,
                &mut host_services,
                &mut child_bridge_services,
                max_service_claims,
            )? {
                emitted_any = true;
            }
            let root_ids = self
                .vms
                .get(&vm_id)
                .map(|vm| vm.active_processes.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            for root_id in root_ids {
                self.recheck_root_deferred_operations(&vm_id, &root_id, true)?;
            }
        }
        if self.route_claimed_pending_process_events()? > 0 {
            emitted_any = true;
        }
        let service_claims = host_services
            .len()
            .saturating_add(child_bridge_services.len());
        if max_service_claims > 0 && service_claims >= max_service_claims {
            // `Notify` coalesces producer edges. If this turn consumes the one
            // stored permit while filling the owned-service staging capacity,
            // source queues may still contain work but no producer will emit a
            // second edge. Preserve one continuation permit; an empty follow-up
            // turn does not rearm and therefore cannot hot-spin.
            self.process_event_notify.notify_one();
        }
        if root_source_remains {
            // A root process is probed once per bounded coordinator turn. Its
            // runtime producer uses a coalesced Notify, so one producer edge
            // can represent an arbitrary number of already-durable stdout,
            // stderr, exit, or internal events. Preserve one continuation
            // edge whenever the non-consuming post-claim probe still observes
            // source work. An exactly drained turn does not rearm and therefore
            // cannot hot-spin.
            self.process_event_notify.notify_one();
        }
        self.rearm_kernel_reaper_task()?;
        Ok(ProcessEventPumpTurn {
            emitted_any,
            host_services,
            child_bridge_services,
        })
    }

    /// Apply one already-polled public process event without suspending the
    /// protocol coordinator. Internal RPC events are never valid broker
    /// payloads; they stay on the owned VM event-service path.
    fn record_ordinary_process_output(
        &mut self,
        vm_id: &str,
        process_id: &str,
        channel: StreamChannel,
        chunk: &[u8],
    ) -> Option<(u64, u64)> {
        if let Some(mut vm) = self.vms.get_mut(vm_id) {
            return vm.record_process_output(process_id, channel, chunk);
        }
        None
    }

    fn record_ordinary_process_exit(&mut self, vm_id: &str, process_id: &str, exit_code: i32) {
        if let Some(mut vm) = self.vms.get_mut(vm_id) {
            vm.record_process_exit(process_id, exit_code);
        }
    }

    pub(crate) fn handle_public_execution_event_nowait(
        &mut self,
        vm_id: &str,
        process_id: &str,
        event: ActiveExecutionEvent,
    ) -> Result<Option<EventFrame>, VmError> {
        let event = match event {
            ActiveExecutionEvent::Common(ExecutionEvent::RuntimeFault(fault)) => {
                let fault = fault.into_error();
                let kernel_fault =
                    agentos_vm_kernel::process_runtime::ProcessRuntimeFault::try_new(
                        fault.code.clone(),
                        fault.message.clone(),
                        fault.details.clone(),
                    )
                    .map_err(|error| VmError::host(error.code(), error.message()))?;
                let Some(mut vm) = self.vms.get_mut(vm_id) else {
                    log_stale_process_event(
                        &self.bridge,
                        vm_id,
                        process_id,
                        "runtime fault dispatch",
                    );
                    return Ok(None);
                };
                let Some(process) = vm.active_processes.get_mut(process_id) else {
                    log_stale_process_event(
                        &self.bridge,
                        vm_id,
                        process_id,
                        "runtime fault dispatch",
                    );
                    return Ok(None);
                };
                process.kernel_handle.finish_runtime_fault(kernel_fault);
                tracing::error!(
                    vm_id,
                    process_id,
                    code = %fault.code,
                    message = %fault.message,
                    details = ?fault.details,
                    "executor reported a typed runtime fault"
                );
                ActiveExecutionEvent::Exited(1)
            }
            ActiveExecutionEvent::Common(ExecutionEvent::Exited(exit)) => {
                let exit_code = match exit {
                    crate::executor::backend::ExecutionExit::Exited(code) => code,
                    crate::executor::backend::ExecutionExit::Signaled { signal, .. } => {
                        128_i32.saturating_add(signal)
                    }
                };
                ActiveExecutionEvent::Exited(exit_code)
            }
            ActiveExecutionEvent::Common(ExecutionEvent::Output { stream, bytes }) => {
                match stream {
                    crate::executor::backend::OutputStream::Stdout => {
                        ActiveExecutionEvent::Stdout(bytes.into_vec())
                    }
                    crate::executor::backend::OutputStream::Stderr => {
                        ActiveExecutionEvent::Stderr(bytes.into_vec())
                    }
                }
            }
            event => event,
        };
        let Some((connection_id, session_id, active)) = self.vms.get(vm_id).map(|vm| {
            (
                vm.connection_id.clone(),
                vm.session_id.clone(),
                vm.active_processes.contains_key(process_id),
            )
        }) else {
            log_stale_process_event(&self.bridge, vm_id, process_id, "public event dispatch");
            return Ok(None);
        };
        if !active {
            log_stale_process_event(&self.bridge, vm_id, process_id, "public event dispatch");
            return Ok(None);
        }
        let ownership = OwnershipScope::vm(&connection_id, &session_id, vm_id);
        let public_execution = self.is_public_execution_process(vm_id, process_id);

        if self.capture_extension_process_output_event(vm_id, process_id, &event) {
            return Ok(None);
        }

        match event {
            ActiveExecutionEvent::Stdout(chunk) if public_execution => Ok(self
                .record_public_execution_output(
                    vm_id,
                    process_id,
                    ExecutionStreamChannel::Stdout,
                    chunk,
                )
                .map(|payload| EventFrame::new(ownership, payload))),
            ActiveExecutionEvent::Stderr(chunk) if public_execution => Ok(self
                .record_public_execution_output(
                    vm_id,
                    process_id,
                    ExecutionStreamChannel::Stderr,
                    chunk,
                )
                .map(|payload| EventFrame::new(ownership, payload))),
            ActiveExecutionEvent::Stdout(chunk) => {
                let replay_identity = self.record_ordinary_process_output(
                    vm_id,
                    process_id,
                    StreamChannel::Stdout,
                    &chunk,
                );
                Ok(Some(EventFrame::new(
                    ownership,
                    EventPayload::ProcessOutput(ProcessOutputEvent {
                        process_id: process_id.to_owned(),
                        channel: StreamChannel::Stdout,
                        chunk,
                        sequence: replay_identity.map(|identity| identity.0),
                        timestamp_ms: replay_identity.map(|identity| identity.1),
                    }),
                )))
            }
            ActiveExecutionEvent::Stderr(chunk) => {
                let replay_identity = self.record_ordinary_process_output(
                    vm_id,
                    process_id,
                    StreamChannel::Stderr,
                    &chunk,
                );
                Ok(Some(EventFrame::new(
                    ownership,
                    EventPayload::ProcessOutput(ProcessOutputEvent {
                        process_id: process_id.to_owned(),
                        channel: StreamChannel::Stderr,
                        chunk,
                        sequence: replay_identity.map(|identity| identity.0),
                        timestamp_ms: replay_identity.map(|identity| identity.1),
                    }),
                )))
            }
            ActiveExecutionEvent::Exited(exit_code) => {
                self.record_ordinary_process_exit(vm_id, process_id, exit_code);
                record_execute_response_to_exit_milestone(
                    "execute_response_to_exit_event_handle",
                    vm_id,
                    process_id,
                );
                record_execute_response_to_exit(vm_id, process_id);
                let park_resident = public_execution
                    && self.should_park_public_execution_process(vm_id, process_id);
                let became_idle = if park_resident {
                    false
                } else {
                    self.finish_active_process_exit(vm_id, process_id, exit_code)?
                        .unwrap_or(false)
                };
                if became_idle || (park_resident && !self.has_running_nonresident_processes(vm_id))
                {
                    self.bridge.emit_lifecycle(vm_id, LifecycleState::Ready)?;
                }
                if public_execution {
                    Ok(self
                        .complete_public_execution(vm_id, process_id, exit_code)
                        .map(|payload| EventFrame::new(ownership, payload)))
                } else {
                    Ok(Some(EventFrame::new(
                        ownership,
                        EventPayload::ProcessExited(ProcessExitedEvent {
                            process_id: process_id.to_owned(),
                            exit_code,
                        }),
                    )))
                }
            }
            other => Err(VmError::InvalidState(format!(
                "ERR_AGENTOS_INTERNAL_EVENT_ON_PUBLIC_BROKER: process {process_id} produced internal event {other:?}"
            ))),
        }
    }

    pub async fn pump_process_events(
        &mut self,
        ownership: &OwnershipScope,
    ) -> Result<bool, VmError> {
        let mut emitted_any = self.poll_in_process_event_services_nowait();
        self.expire_public_execution_deadlines()?;
        emitted_any |= self.drain_runtime_process_event_channel_nowait()?;
        let vm_ids = self.vm_ids_for_scope(ownership)?;
        for vm_id in vm_ids {
            let vm_work_limit = self.config.runtime.fairness.vm_quantum_operations;
            let mut vm_work = 0usize;
            if let Some(vm) = self.vms.get(&vm_id) {
                vm.kernel.reap_due_zombies();
            }
            'vm_event_turn: while self.vms.contains_key(&vm_id) {
                let Some((connection_id, session_id, process_ids)) =
                    self.vms.get(&vm_id).map(|vm| {
                        (
                            vm.connection_id.clone(),
                            vm.session_id.clone(),
                            vm.active_processes.keys().cloned().collect::<Vec<_>>(),
                        )
                    })
                else {
                    break;
                };
                let mut emitted_this_pass = false;

                for process_id in process_ids {
                    if vm_work >= vm_work_limit {
                        self.process_event_notify.notify_one();
                        break 'vm_event_turn;
                    }
                    if self
                        .vms
                        .get(&vm_id)
                        .is_some_and(|vm| vm.detached_child_processes.contains(&process_id))
                    {
                        continue;
                    }
                    self.recheck_root_deferred_operations(&vm_id, &process_id, false)?;
                    enum ProcessPollResult {
                        Event(Box<Option<PolledExecutionEvent>>),
                        RecoverClosedChannel,
                    }
                    let poll_result = {
                        let Some(mut vm) = self.vms.get_mut(&vm_id) else {
                            continue;
                        };
                        let Some(process) = vm.active_processes.get_mut(&process_id) else {
                            continue;
                        };
                        if let Some(event) = process.lease_pending_execution_event() {
                            ProcessPollResult::Event(Box::new(Some(event)))
                        } else {
                            match process.try_poll_execution_event() {
                                Ok(event) => ProcessPollResult::Event(Box::new(event)),
                                Err(VmError::ExecutionEventChannelClosed { .. }) => {
                                    ProcessPollResult::RecoverClosedChannel
                                }
                                Err(other) => return Err(other),
                            }
                        }
                    };
                    let event = match poll_result {
                        ProcessPollResult::Event(event) => *event,
                        ProcessPollResult::RecoverClosedChannel => self
                            .recover_closed_root_runtime_process_event(&vm_id, &process_id)?
                            .map(PolledExecutionEvent::unreserved),
                    };

                    let Some(event) = event else {
                        continue;
                    };
                    if Self::terminal_execution_event(event.event()) {
                        record_execute_response_to_exit_milestone(
                            "execute_response_to_exit_event_polled",
                            &vm_id,
                            &process_id,
                        );
                    }

                    if Self::internal_execution_event(event.event()) {
                        // These events are sidecar work items, not client-facing
                        // process events. Handle them immediately so a sibling
                        // process can service sync RPCs while another request
                        // waits on VM-local networking.
                        self.handle_execution_event(&vm_id, &process_id, event.into_event())
                            .await?;
                    } else {
                        let PolledExecutionEvent { event, reservation } = event;
                        let envelope = ProcessEventEnvelope {
                            connection_id: connection_id.clone(),
                            session_id: session_id.clone(),
                            vm_id: vm_id.clone(),
                            child_path: Vec::new(),
                            process_id: process_id.clone(),
                            event,
                        };
                        if let Err(error) = self.check_pending_process_event_capacity(&envelope) {
                            if let Some(mut vm) = self.vms.get_mut(&vm_id) {
                                if let Some(process) = vm.active_processes.get_mut(&process_id) {
                                    process.requeue_pending_execution_event(
                                        PolledExecutionEvent {
                                            event: envelope.event,
                                            reservation,
                                        },
                                    )?;
                                }
                            }
                            return Err(error);
                        }
                        self.queue_pending_process_event(envelope)?;
                        drop(reservation);
                    }
                    emitted_any = true;
                    emitted_this_pass = true;
                    vm_work += 1;
                }

                if !emitted_this_pass {
                    break;
                }
            }

            if self.pump_child_process_events(&vm_id).await? {
                emitted_any = true;
                // Root waits, descriptor polls, and reads are probed before
                // descendant execution events in the main pass above. A
                // descendant exit in this turn can make all three ready by
                // changing process state, closing pipe writers, and releasing
                // record locks. Settle those durable kernel transitions in
                // the same turn instead of relying on a second coalesced
                // broker edge that another ready branch could absorb.
                let process_ids = self
                    .vms
                    .get(&vm_id)
                    .map(|vm| vm.active_processes.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                for process_id in process_ids {
                    if self
                        .vms
                        .get(&vm_id)
                        .is_some_and(|vm| vm.detached_child_processes.contains(&process_id))
                    {
                        continue;
                    }
                    self.recheck_root_deferred_operations(&vm_id, &process_id, true)?;
                }
            }
            if self.pump_detached_child_process_events(&vm_id).await? {
                emitted_any = true;
            }
        }

        if self.route_claimed_pending_process_events()? > 0 {
            emitted_any = true;
        }
        self.rearm_kernel_reaper_task()?;
        Ok(emitted_any)
    }

    pub(crate) fn recheck_root_deferred_operations(
        &mut self,
        vm_id: &str,
        process_id: &str,
        force_probe: bool,
    ) -> Result<(), VmError> {
        if force_probe {
            // Descendant progress is itself durable evidence that wait state,
            // pipe EOF/readiness, or record-lock ownership may have changed.
            // Do not wait for the notifier task to observe the same generation
            // transition before probing on the owner thread. Retire its old
            // registration now; each service below rearms it if still blocked.
            if let Some(mut vm) = self.vms.get_mut(vm_id) {
                if let Some(process) = vm.active_processes.get_mut(process_id) {
                    if let Some(task) = process
                        .deferred_guest_wait
                        .as_mut()
                        .and_then(|wait| wait.wake_task.take())
                    {
                        task.abort();
                    }
                    if let Some(task) = process
                        .deferred_kernel_poll
                        .as_mut()
                        .and_then(|poll| poll.wake_task.take())
                    {
                        task.abort();
                    }
                    if let Some(task) = process
                        .deferred_kernel_read
                        .as_mut()
                        .and_then(|read| read.wake_task.take())
                    {
                        task.abort();
                    }
                }
            }
        }

        self.recheck_root_deferred_guest_wait(vm_id, process_id)?;
        self.recheck_root_deferred_kernel_poll(vm_id, process_id)?;
        self.recheck_root_deferred_kernel_read(vm_id, process_id)
    }

    fn recheck_root_deferred_guest_wait(
        &mut self,
        vm_id: &str,
        process_id: &str,
    ) -> Result<(), VmError> {
        let notify = Arc::clone(&self.process_event_notify);
        let Some(mut vm) = self.vms.get_mut(vm_id) else {
            return Ok(());
        };
        let runtime = vm.runtime_context.clone();
        let wait_handle = vm.kernel.process_wait_handle();
        let generation = vm.generation;
        let vm = &mut *vm;
        let (kernel, active_processes) = (&mut vm.kernel, &mut vm.active_processes);
        let Some(process) = active_processes.get_mut(process_id) else {
            return Ok(());
        };
        process.apply_runtime_controls()?;
        if process.deferred_guest_wait.is_none() {
            return Ok(());
        }
        service_deferred_guest_wait(
            generation,
            &runtime,
            wait_handle,
            notify,
            kernel,
            process,
            None,
        )
    }

    fn recheck_root_deferred_kernel_poll(
        &mut self,
        vm_id: &str,
        process_id: &str,
    ) -> Result<(), VmError> {
        let notify = Arc::clone(&self.process_event_notify);
        let wake_lane = host_dispatch::deferred_posix_poll_wake_lane(self, vm_id, process_id)?;
        let socket_paths = self
            .vms
            .get(vm_id)
            .map(|vm| build_socket_path_context(&vm))
            .transpose()?;
        let Some(mut vm) = self.vms.get_mut(vm_id) else {
            return Ok(());
        };
        let runtime = vm.runtime_context.clone();
        let wait_handle = vm.kernel.poll_wait_handle();
        let generation = vm.generation;
        let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
        let capabilities = vm.capabilities.clone();
        let managed_descriptions = Arc::clone(&vm.managed_host_net_descriptions);
        let vm = &mut *vm;
        let (kernel, active_processes) = (&mut vm.kernel, &mut vm.active_processes);
        let Some(process) = active_processes.get_mut(process_id) else {
            return Ok(());
        };
        process.apply_runtime_controls()?;
        if process.deferred_kernel_poll.is_none() {
            return Ok(());
        }
        if process
            .deferred_kernel_poll
            .as_ref()
            .is_some_and(|poll| poll.combined)
        {
            return host_dispatch::service_deferred_posix_poll(
                generation,
                &runtime,
                wait_handle,
                notify,
                socket_paths
                    .as_ref()
                    .expect("registered VM has a socket path context"),
                kernel_readiness,
                capabilities,
                managed_descriptions,
                wake_lane,
                kernel,
                process,
                None,
            );
        }
        service_deferred_kernel_poll(
            generation,
            &runtime,
            wait_handle,
            notify,
            kernel,
            process,
            None,
        )
    }

    fn recheck_root_deferred_kernel_read(
        &mut self,
        vm_id: &str,
        process_id: &str,
    ) -> Result<(), VmError> {
        let notify = Arc::clone(&self.process_event_notify);
        let Some(mut vm) = self.vms.get_mut(vm_id) else {
            return Ok(());
        };
        let runtime = vm.runtime_context.clone();
        let wait_handle = vm.kernel.poll_wait_handle();
        let generation = vm.generation;
        let vm = &mut *vm;
        let (kernel, active_processes) = (&mut vm.kernel, &mut vm.active_processes);
        let Some(process) = active_processes.get_mut(process_id) else {
            return Ok(());
        };
        process.apply_runtime_controls()?;
        if process.deferred_kernel_read.is_none() {
            return Ok(());
        }
        service_deferred_kernel_read(
            generation,
            &runtime,
            wait_handle,
            notify,
            kernel,
            process,
            None,
        )
    }

    /// Arm exactly one sidecar task for the earliest zombie deadline across
    /// every VM. Kernel process tables remain runtime-neutral and are reaped on
    /// the next process-event turn after this coalesced wake.
    fn rearm_kernel_reaper_task(&mut self) -> Result<(), VmError> {
        if self
            .kernel_reaper_task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
        {
            self.kernel_reaper_task.take();
            self.kernel_reaper_deadline = None;
        }
        let next_deadline = self
            .vms
            .values()
            .filter_map(|vm| vm.kernel.next_zombie_reap_deadline())
            .min();
        let Some(next_deadline) = next_deadline else {
            if let Some(task) = self.kernel_reaper_task.take() {
                task.abort();
            }
            self.kernel_reaper_deadline = None;
            return Ok(());
        };
        if self.kernel_reaper_task.is_some()
            && self
                .kernel_reaper_deadline
                .is_some_and(|armed_deadline| armed_deadline <= next_deadline)
        {
            return Ok(());
        }
        if let Some(task) = self.kernel_reaper_task.take() {
            task.abort();
        }
        let runtime = self.runtime_context.clone().ok_or_else(|| {
            VmError::host(
                "ERR_AGENTOS_RUNTIME_UNAVAILABLE",
                String::from("kernel zombie reaper requires the process DriverHandle"),
            )
        })?;
        let notify = Arc::clone(&self.process_event_notify);
        let delay = next_deadline.saturating_duration_since(Instant::now());
        self.kernel_reaper_task = Some(
            runtime
                .spawn(agentos_driver_tokio::TaskClass::Timer, async move {
                    tokio::time::sleep(delay).await;
                    notify.notify_one();
                })
                .map_err(|error| VmError::Execution(error.to_string()))?,
        );
        self.kernel_reaper_deadline = Some(next_deadline);
        Ok(())
    }

    pub(super) fn internal_execution_event(event: &ActiveExecutionEvent) -> bool {
        matches!(
            event,
            ActiveExecutionEvent::Common(ExecutionEvent::HostCall { .. })
                | ActiveExecutionEvent::Common(ExecutionEvent::Warning(_))
                | ActiveExecutionEvent::HostRpcRequest(_)
                | ActiveExecutionEvent::HostCallCompletion(_)
                | ActiveExecutionEvent::DeferredPosixPollWake
                | ActiveExecutionEvent::ManagedStreamReadRecheck(_)
                | ActiveExecutionEvent::ManagedUdpPollRecheck(_)
                | ActiveExecutionEvent::SignalState { .. }
        )
    }

    pub(crate) fn terminal_execution_event(event: &ActiveExecutionEvent) -> bool {
        matches!(
            event,
            ActiveExecutionEvent::Exited(_)
                | ActiveExecutionEvent::Common(
                    ExecutionEvent::Exited(_) | ExecutionEvent::RuntimeFault(_)
                )
        )
    }

    pub(super) fn recover_closed_root_runtime_process_event(
        &mut self,
        vm_id: &str,
        process_id: &str,
    ) -> Result<Option<ActiveExecutionEvent>, VmError> {
        let Some(mut vm) = self.vms.get_mut(vm_id) else {
            return Ok(None);
        };
        let Some(process) = vm.active_processes.get_mut(process_id) else {
            return Ok(None);
        };
        let Some(runtime_child_pid) = process.execution.native_process_id() else {
            return Ok(None);
        };
        match runtime_child_exit_status(runtime_child_pid)? {
            RuntimeChildStatusObservation::Exited(status) => {
                process.exit_signal = status.signal;
                process.exit_core_dumped = status.core_dumped;
                Ok(Some(ActiveExecutionEvent::Exited(status.status)))
            }
            RuntimeChildStatusObservation::Running => Ok(None),
            RuntimeChildStatusObservation::NotWaitable => Err(VmError::host(
                "ECHILD",
                format!(
                    "guest runtime process {runtime_child_pid} exited without an observable wait status"
                ),
            )),
        }
    }

    pub(crate) fn active_process_by_path<'a>(
        process: &'a ActiveProcess,
        child_path: &[&str],
    ) -> Option<&'a ActiveProcess> {
        let mut current = process;
        for child_id in child_path {
            current = current.child_processes.get(*child_id)?;
        }
        Some(current)
    }

    pub(crate) fn active_process_by_path_mut<'a>(
        process: &'a mut ActiveProcess,
        child_path: &[&str],
    ) -> Option<&'a mut ActiveProcess> {
        let mut current = process;
        for child_id in child_path {
            current = current.child_processes.get_mut(*child_id)?;
        }
        Some(current)
    }

    pub(super) fn active_process_by_owned_path_mut<'a>(
        process: &'a mut ActiveProcess,
        child_path: &[String],
    ) -> Option<&'a mut ActiveProcess> {
        let mut current = process;
        for child_id in child_path {
            current = current.child_processes.get_mut(child_id)?;
        }
        Some(current)
    }

    pub(super) fn active_process_path_by_kernel_pid(
        process: &ActiveProcess,
        kernel_pid: u32,
    ) -> Option<Vec<String>> {
        if process.kernel_pid == kernel_pid {
            return Some(Vec::new());
        }

        for (child_id, child) in &process.child_processes {
            let Some(mut path) = Self::active_process_path_by_kernel_pid(child, kernel_pid) else {
                continue;
            };
            path.insert(0, child_id.clone());
            return Some(path);
        }

        None
    }

    pub(super) fn descendant_parent_process_mut<'a>(
        vm: &'a mut VmState,
        process_id: &str,
        child_path: &[&str],
    ) -> Option<&'a mut ActiveProcess> {
        let root = vm.active_processes.get_mut(process_id)?;
        Self::active_process_by_path_mut(root, child_path)
    }

    pub(super) fn child_process_path_label(process_id: &str, child_path: &[&str]) -> String {
        if child_path.is_empty() {
            process_id.to_owned()
        } else {
            format!("{process_id}/{}", child_path.join("/"))
        }
    }

    pub(super) fn adopt_detached_child_processes(
        current_process_id: &str,
        process: &mut ActiveProcess,
    ) -> Vec<(String, ActiveProcess)> {
        let mut adopted = Vec::new();
        let child_ids = process.child_processes.keys().cloned().collect::<Vec<_>>();
        for child_id in child_ids {
            let child_process_id = format!("{current_process_id}/{child_id}");
            let Some(mut child) = process.child_processes.remove(&child_id) else {
                continue;
            };
            if child.detached {
                adopted.push((child_process_id, child));
                continue;
            }

            adopted.extend(Self::adopt_detached_child_processes(
                &child_process_id,
                &mut child,
            ));
            process.child_processes.insert(child_id, child);
        }
        adopted
    }

    pub(super) fn terminating_process_tree_kernel_pids(process: &ActiveProcess) -> Vec<u32> {
        fn collect(process: &ActiveProcess, pids: &mut Vec<u32>) {
            pids.push(process.kernel_pid);
            for child in process.child_processes.values() {
                if !child.detached {
                    collect(child, pids);
                }
            }
        }

        let mut pids = Vec::new();
        collect(process, &mut pids);
        pids
    }

    pub(super) fn resolve_detached_child_process_path(
        vm: &VmState,
        detached_process_id: &str,
    ) -> Option<(String, Vec<String>)> {
        let root_process_id = vm
            .active_processes
            .keys()
            .filter(|candidate| {
                detached_process_id == candidate.as_str()
                    || detached_process_id
                        .strip_prefix(candidate.as_str())
                        .is_some_and(|remainder| remainder.starts_with('/'))
            })
            .max_by_key(|candidate| candidate.len())?
            .clone();

        let remainder = detached_process_id
            .strip_prefix(root_process_id.as_str())
            .unwrap_or_default();
        if remainder.is_empty() {
            return Some((root_process_id, Vec::new()));
        }

        Some((
            root_process_id,
            remainder
                .trim_start_matches('/')
                .split('/')
                .map(str::to_owned)
                .collect(),
        ))
    }

    pub(super) fn collect_attached_child_paths(
        process: &ActiveProcess,
        parent_path: &mut Vec<String>,
        paths: &mut Vec<Vec<String>>,
    ) {
        for (child_id, child) in &process.child_processes {
            // `detached` changes the child's process-group/session and lets it
            // survive its parent. Until the parent exits and adopts it into
            // `detached_child_processes`, it still lives in this tree and its
            // stdio, sync RPCs, and descendants must be pumped here.
            parent_path.push(child_id.clone());
            paths.push(parent_path.clone());
            Self::collect_attached_child_paths(child, parent_path, paths);
            parent_path.pop();
        }
    }

    /// Drain attached child runtimes from the same coalesced process wake used
    /// by top-level executions. Event data stays in runtime-owned bounded
    /// queues; this turn merely routes a bounded batch into the parent VM.
    pub(crate) async fn handle_execution_event(
        &mut self,
        vm_id: &str,
        process_id: &str,
        event: ActiveExecutionEvent,
    ) -> Result<Option<EventFrame>, VmError> {
        let event = match event {
            ActiveExecutionEvent::Common(ExecutionEvent::RuntimeFault(fault)) => {
                let fault = fault.into_error();
                let kernel_fault =
                    agentos_vm_kernel::process_runtime::ProcessRuntimeFault::try_new(
                        fault.code.clone(),
                        fault.message.clone(),
                        fault.details.clone(),
                    )
                    .map_err(|error| VmError::host(error.code(), error.message()))?;
                let Some(mut vm) = self.vms.get_mut(vm_id) else {
                    log_stale_process_event(
                        &self.bridge,
                        vm_id,
                        process_id,
                        "runtime fault dispatch",
                    );
                    return Ok(None);
                };
                let Some(process) = vm.active_processes.get_mut(process_id) else {
                    log_stale_process_event(
                        &self.bridge,
                        vm_id,
                        process_id,
                        "runtime fault dispatch",
                    );
                    return Ok(None);
                };
                process.kernel_handle.finish_runtime_fault(kernel_fault);
                tracing::error!(
                    vm_id,
                    process_id,
                    code = %fault.code,
                    message = %fault.message,
                    details = ?fault.details,
                    "executor reported a typed runtime fault"
                );
                ActiveExecutionEvent::Exited(1)
            }
            ActiveExecutionEvent::Common(ExecutionEvent::Exited(exit)) => {
                let exit_code = match exit {
                    crate::executor::backend::ExecutionExit::Exited(code) => code,
                    crate::executor::backend::ExecutionExit::Signaled { signal, .. } => {
                        128_i32.saturating_add(signal)
                    }
                };
                ActiveExecutionEvent::Exited(exit_code)
            }
            ActiveExecutionEvent::Common(ExecutionEvent::Output { stream, bytes }) => {
                match stream {
                    crate::executor::backend::OutputStream::Stdout => {
                        ActiveExecutionEvent::Stdout(bytes.into_vec())
                    }
                    crate::executor::backend::OutputStream::Stderr => {
                        ActiveExecutionEvent::Stderr(bytes.into_vec())
                    }
                }
            }
            event => event,
        };
        let Some((connection_id, session_id, active)) = self.vms.get(vm_id).map(|vm| {
            (
                vm.connection_id.clone(),
                vm.session_id.clone(),
                vm.active_processes.contains_key(process_id),
            )
        }) else {
            log_stale_process_event(&self.bridge, vm_id, process_id, "execution event dispatch");
            return Ok(None);
        };
        if !active {
            log_stale_process_event(&self.bridge, vm_id, process_id, "execution event dispatch");
            return Ok(None);
        }
        let ownership = OwnershipScope::vm(&connection_id, &session_id, vm_id);
        let public_execution = self.is_public_execution_process(vm_id, process_id);

        if self.capture_extension_process_output_event(vm_id, process_id, &event) {
            return Ok(None);
        }

        match event {
            ActiveExecutionEvent::Stdout(chunk) if public_execution => Ok(self
                .record_public_execution_output(
                    vm_id,
                    process_id,
                    ExecutionStreamChannel::Stdout,
                    chunk,
                )
                .map(|payload| EventFrame::new(ownership, payload))),
            ActiveExecutionEvent::Stderr(chunk) if public_execution => Ok(self
                .record_public_execution_output(
                    vm_id,
                    process_id,
                    ExecutionStreamChannel::Stderr,
                    chunk,
                )
                .map(|payload| EventFrame::new(ownership, payload))),
            ActiveExecutionEvent::Common(ExecutionEvent::HostCall { operation, reply }) => {
                let operation_debug = format!("{operation:?}");
                let Some((operation, reply)) =
                    dispatch_context_host_operation(self, vm_id, process_id, operation, reply)
                        .await
                        .map_err(|error| {
                            eprintln!(
                                "ERR_AGENTOS_HOST_OPERATION_CONTEXT_DISPATCH: vm={vm_id} process={process_id} operation={operation_debug} error={error}"
                            );
                            error
                        })?
                else {
                    return Ok(None);
                };
                let Some(mut vm) = self.vms.get_mut(vm_id) else {
                    log_stale_process_event(
                        &self.bridge,
                        vm_id,
                        process_id,
                        "common host operation",
                    );
                    return Ok(None);
                };
                let vm = &mut *vm;
                let generation = vm.generation;
                let vm = &mut *vm;
                let (kernel, active_processes) = (&mut vm.kernel, &mut vm.active_processes);
                let Some(process) = active_processes.get_mut(process_id) else {
                    log_stale_process_event(
                        &self.bridge,
                        vm_id,
                        process_id,
                        "common host operation",
                    );
                    return Ok(None);
                };
                let effects = dispatch_host_operation(generation, kernel, process, operation, reply)
                    .map_err(|error| {
                        eprintln!(
                            "ERR_AGENTOS_HOST_OPERATION_DISPATCH: vm={vm_id} process={process_id} operation={operation_debug} error={error}"
                        );
                        error
                    })?;
                if effects.may_make_fd_readable {
                    Self::wake_ready_deferred_fd_reads(vm)?;
                }
                if effects.may_make_fd_writable {
                    Self::wake_ready_deferred_fd_writes(vm)?;
                }
                Ok(None)
            }
            ActiveExecutionEvent::Common(ExecutionEvent::Output { stream, bytes }) => {
                let channel = match stream {
                    crate::executor::backend::OutputStream::Stdout => StreamChannel::Stdout,
                    crate::executor::backend::OutputStream::Stderr => StreamChannel::Stderr,
                };
                let replay_identity = self.record_ordinary_process_output(
                    vm_id,
                    process_id,
                    channel.clone(),
                    bytes.as_slice(),
                );
                Ok(Some(EventFrame::new(
                    ownership,
                    EventPayload::ProcessOutput(ProcessOutputEvent {
                        process_id: process_id.to_owned(),
                        channel,
                        chunk: bytes.into_vec(),
                        sequence: replay_identity.map(|identity| identity.0),
                        timestamp_ms: replay_identity.map(|identity| identity.1),
                    }),
                )))
            }
            ActiveExecutionEvent::Common(ExecutionEvent::Warning(error)) => {
                eprintln!("ERR_AGENTOS_EXECUTION_WARNING: {error}");
                Ok(None)
            }
            ActiveExecutionEvent::Common(ExecutionEvent::RuntimeFault(_)) => {
                unreachable!("runtime fault events are normalized before dispatch")
            }
            ActiveExecutionEvent::Common(ExecutionEvent::Exited(_)) => {
                unreachable!("common exit events are normalized before dispatch")
            }
            ActiveExecutionEvent::Common(_) => Err(VmError::host(
                "ENOSYS",
                "execution backend emitted an unsupported common event",
            )),
            ActiveExecutionEvent::Stdout(chunk) => {
                let replay_identity = self.record_ordinary_process_output(
                    vm_id,
                    process_id,
                    StreamChannel::Stdout,
                    &chunk,
                );
                Ok(Some(EventFrame::new(
                    ownership,
                    EventPayload::ProcessOutput(ProcessOutputEvent {
                        process_id: process_id.to_owned(),
                        channel: StreamChannel::Stdout,
                        chunk,
                        sequence: replay_identity.map(|identity| identity.0),
                        timestamp_ms: replay_identity.map(|identity| identity.1),
                    }),
                )))
            }
            ActiveExecutionEvent::Stderr(chunk) => {
                let replay_identity = self.record_ordinary_process_output(
                    vm_id,
                    process_id,
                    StreamChannel::Stderr,
                    &chunk,
                );
                Ok(Some(EventFrame::new(
                    ownership,
                    EventPayload::ProcessOutput(ProcessOutputEvent {
                        process_id: process_id.to_owned(),
                        channel: StreamChannel::Stderr,
                        chunk,
                        sequence: replay_identity.map(|identity| identity.0),
                        timestamp_ms: replay_identity.map(|identity| identity.1),
                    }),
                )))
            }
            ActiveExecutionEvent::HostRpcRequest(request) => {
                self.handle_javascript_sync_rpc_request(vm_id, process_id, request)
                    .await?;
                Ok(None)
            }
            ActiveExecutionEvent::HostCallCompletion(completion) => {
                self.handle_host_call_completion(vm_id, process_id, completion)?;
                Ok(None)
            }
            ActiveExecutionEvent::DeferredPosixPollWake => Ok(None),
            ActiveExecutionEvent::ManagedStreamReadRecheck(pending) => {
                dispatch_claimed_context_stream_read(self, vm_id, process_id, *pending)?;
                Ok(None)
            }
            ActiveExecutionEvent::ManagedUdpPollRecheck(pending) => {
                dispatch_claimed_context_udp_poll(self, vm_id, process_id, *pending)?;
                Ok(None)
            }
            ActiveExecutionEvent::SignalState {
                signal,
                registration,
            } => {
                let Some(vm) = self.vms.get_mut(vm_id) else {
                    return Ok(None);
                };
                let Some(process) = vm.active_processes.get(process_id) else {
                    return Ok(None);
                };
                apply_kernel_signal_registration(process, signal, &registration)?;
                Ok(None)
            }
            ActiveExecutionEvent::Exited(exit_code) => {
                self.record_ordinary_process_exit(vm_id, process_id, exit_code);
                record_execute_response_to_exit_milestone(
                    "execute_response_to_exit_event_handle",
                    vm_id,
                    process_id,
                );
                record_execute_response_to_exit(vm_id, process_id);
                let park_resident = public_execution
                    && self.should_park_public_execution_process(vm_id, process_id);
                let phase_start = Instant::now();
                let became_idle = if park_resident {
                    false
                } else {
                    self.finish_active_process_exit(vm_id, process_id, exit_code)?
                        .unwrap_or(false)
                };
                record_execute_phase("process_exit_cleanup", phase_start.elapsed());

                let phase_start = Instant::now();
                if became_idle || (park_resident && !self.has_running_nonresident_processes(vm_id))
                {
                    self.bridge.emit_lifecycle(vm_id, LifecycleState::Ready)?;
                }
                record_execute_phase("process_exit_lifecycle_emit", phase_start.elapsed());

                if public_execution {
                    Ok(self
                        .complete_public_execution(vm_id, process_id, exit_code)
                        .map(|payload| EventFrame::new(ownership, payload)))
                } else {
                    Ok(Some(EventFrame::new(
                        ownership,
                        EventPayload::ProcessExited(ProcessExitedEvent {
                            process_id: process_id.to_owned(),
                            exit_code,
                        }),
                    )))
                }
            }
        }
    }

    pub(super) fn handle_host_call_completion(
        &mut self,
        vm_id: &str,
        process_id: &str,
        completion: crate::state::HostCallCompletion,
    ) -> Result<(), VmError> {
        let Some(mut vm) = self.vms.get_mut(vm_id) else {
            completion
                .reply
                .fail(HostServiceError::new(
                    "ESTALE",
                    "deferred host-call VM no longer exists",
                ))
                .map_err(VmError::from)?;
            return Ok(());
        };
        let vm = &mut *vm;
        let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
        let unix_addresses = Arc::clone(&vm.unix_address_registry);
        let managed_descriptions = Arc::clone(&vm.managed_host_net_descriptions);
        let Some(process) = vm.active_processes.get_mut(process_id) else {
            completion
                .reply
                .fail(HostServiceError::new(
                    "ESTALE",
                    "deferred host-call process no longer exists",
                ))
                .map_err(VmError::from)?;
            return Ok(());
        };
        settle_host_call_completion_for_process(
            vm.generation,
            &mut vm.kernel,
            &kernel_readiness,
            &unix_addresses,
            &managed_descriptions,
            process,
            completion,
        )
    }

    pub(crate) fn finish_active_process_exit_owned(
        bridge: &SharedBridge<B>,
        vm_handle: &crate::state::VmHandle,
        vm_id: &str,
        process_id: &str,
        exit_code: i32,
    ) -> Result<Option<FinishedActiveProcessExit>, VmError> {
        let mut vm = vm_handle.try_borrow_mut("finish active process exit")?;
        if !vm.active_processes.contains_key(process_id) {
            log_stale_process_event(bridge, vm_id, process_id, "process exit cleanup");
            return Ok(None);
        }

        let phase_start = Instant::now();
        prune_exited_process_snapshots(&mut vm);
        record_execute_phase(
            "process_exit_cleanup_prune_snapshots",
            phase_start.elapsed(),
        );
        let phase_start = Instant::now();
        let process_table = vm.kernel.list_processes();
        record_execute_phase("process_exit_cleanup_list_processes", phase_start.elapsed());
        let phase_start = Instant::now();
        let terminating_kernel_pids = Self::terminating_process_tree_kernel_pids(
            vm.active_processes
                .get(process_id)
                .expect("validated exiting process remains registered"),
        );
        for kernel_pid in terminating_kernel_pids {
            retire_managed_process_routes(bridge, vm_id, &mut vm, kernel_pid)?;
        }
        record_execute_phase(
            "process_exit_cleanup_managed_network_routes",
            phase_start.elapsed(),
        );
        let phase_start = Instant::now();
        let Some(mut process) = vm.active_processes.remove(process_id) else {
            return Ok(None);
        };
        record_execute_phase("process_exit_cleanup_remove_active", phase_start.elapsed());
        let phase_start = Instant::now();
        if let Some(info) = process_table.get(&process.kernel_pid) {
            vm.exited_process_snapshots
                .push_back(ExitedProcessSnapshot {
                    captured_at: Instant::now(),
                    process: build_process_snapshot_entry(
                        process_id,
                        &process,
                        info,
                        Some(exit_code),
                    ),
                });
        }
        record_execute_phase("process_exit_cleanup_build_snapshot", phase_start.elapsed());
        let phase_start = Instant::now();
        let detached_children = Self::adopt_detached_child_processes(process_id, &mut process);
        let detached_process_ids = detached_children
            .iter()
            .map(|(detached_process_id, _)| detached_process_id.clone())
            .collect::<Vec<_>>();
        record_execute_phase("process_exit_cleanup_adopt_detached", phase_start.elapsed());
        let raw_mode_result = release_inherited_child_raw_mode(&mut vm.kernel, &process);
        let phase_start = Instant::now();
        let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
        let unix_address_registry = Arc::clone(&vm.unix_address_registry);
        terminate_child_process_tree(
            &mut vm.kernel,
            &mut process,
            &kernel_readiness,
            &unix_address_registry,
        );
        record_execute_phase(
            "process_exit_cleanup_terminate_child_tree",
            phase_start.elapsed(),
        );
        let phase_start = Instant::now();
        if let Some(signal) = process.exit_signal {
            process
                .kernel_handle
                .finish_signaled(signal, process.exit_core_dumped);
        } else {
            process.kernel_handle.finish(exit_code);
        }
        record_execute_phase("process_exit_cleanup_kernel_finish", phase_start.elapsed());
        let phase_start = Instant::now();
        if let Err(error) = vm.kernel.wait_and_reap(process.kernel_pid) {
            eprintln!(
                "ERR_AGENTOS_PROCESS_REAP: failed to reap exited kernel pid {}: {error}",
                process.kernel_pid
            );
        }
        retire_orphaned_managed_descriptions(&mut vm)?;
        record_execute_phase("process_exit_cleanup_wait_and_reap", phase_start.elapsed());
        let phase_start = Instant::now();
        record_execute_phase(
            "process_exit_cleanup_signal_state_remove",
            phase_start.elapsed(),
        );
        let phase_start = Instant::now();
        for (detached_process_id, detached_child) in detached_children {
            vm.detached_child_processes
                .insert(detached_process_id.clone());
            vm.active_processes
                .insert(detached_process_id, detached_child);
        }
        record_execute_phase(
            "process_exit_cleanup_reinsert_detached",
            phase_start.elapsed(),
        );
        let phase_start = Instant::now();
        let became_idle = vm.active_processes.is_empty();
        record_execute_phase("process_exit_cleanup_became_idle", phase_start.elapsed());
        let phase_start = Instant::now();

        record_execute_phase("process_exit_cleanup_prune_resource", phase_start.elapsed());

        // The process was removed from active_processes before the fallible
        // raw-mode cleanup. Surface the error only after all process-owned
        // resources have been finalized.
        raw_mode_result?;
        Ok(Some(FinishedActiveProcessExit {
            became_idle,
            process_id: process_id.to_owned(),
            detached_process_ids,
        }))
    }

    pub(crate) fn finish_active_process_exit(
        &mut self,
        vm_id: &str,
        process_id: &str,
        exit_code: i32,
    ) -> Result<Option<bool>, VmError> {
        let Some(vm_handle) = self.vms.handle(vm_id) else {
            log_stale_process_event(&self.bridge, vm_id, process_id, "process exit cleanup");
            return Ok(None);
        };
        let finished = Self::finish_active_process_exit_owned(
            &self.bridge,
            &vm_handle,
            vm_id,
            process_id,
            exit_code,
        )?;
        if let Some(finished) = finished {
            let phase_start = Instant::now();
            self.transfer_extension_process_resource(
                &finished.process_id,
                &finished.detached_process_ids,
            );
            record_execute_phase("process_exit_cleanup_prune_resource", phase_start.elapsed());
            return Ok(Some(finished.became_idle));
        }
        Ok(None)
    }
}

pub(crate) struct FinishedActiveProcessExit {
    pub(crate) detached_process_ids: Vec<String>,
    pub(crate) became_idle: bool,
    pub(crate) process_id: String,
}

#[cfg(test)]
mod process_event_channel_tests {
    use super::*;
    use crate::VmManagerConfig;
    use agentos_vm_host_interface::LocalVmHost as LocalBridge;
    use std::future::Future as _;
    use std::task::{Context, Poll, Waker};

    #[test]
    fn stale_internal_completion_settles_its_direct_reply() {
        #[derive(Default)]
        struct ReplyTarget(Mutex<Vec<String>>);
        impl crate::executor::backend::DirectHostReplyTarget for ReplyTarget {
            fn claim(&self, _call_id: u64) -> Result<bool, HostServiceError> {
                Ok(true)
            }
            fn respond(
                &self,
                _call_id: u64,
                _claimed: bool,
                result: Result<HostCallReply, HostServiceError>,
            ) -> Result<(), HostServiceError> {
                self.0
                    .lock()
                    .expect("record replies")
                    .push(result.expect_err("stale completion must fail").code);
                Ok(())
            }
        }
        let config = VmManagerConfig::default();
        let runtime =
            agentos_driver_tokio::TokioDriver::process(&config.runtime).expect("test runtime");
        let mut sidecar = VmManager::with_config_extensions_and_runtime(
            LocalBridge::default(),
            config,
            Vec::new(),
            runtime.handle(),
        )
        .expect("test VM manager");
        let target = Arc::new(ReplyTarget::default());
        let reply = DirectHostReplyHandle::new(
            crate::executor::backend::HostCallIdentity {
                generation: 1,
                pid: 1,
                call_id: 1,
            },
            target.clone(),
            1024,
        )
        .expect("direct reply");
        let envelope = ProcessEventEnvelope {
            connection_id: "connection".into(),
            session_id: "session".into(),
            vm_id: "disposed-vm".into(),
            process_id: "process".into(),
            child_path: Vec::new(),
            event: ActiveExecutionEvent::HostCallCompletion(crate::state::HostCallCompletion {
                reply,
                result: Ok(Value::Null),
            }),
        };
        assert!(sidecar
            .route_received_internal_process_event(envelope)
            .expect("route stale event")
            .is_none());
        assert_eq!(*target.0.lock().expect("read replies"), vec!["ESTALE"]);
    }

    #[test]
    fn receiver_admission_failure_does_not_drop_later_envelopes() {
        let config = VmManagerConfig::default();
        let runtime = agentos_driver_tokio::TokioDriver::process(&config.runtime)
            .expect("process-event channel test runtime");
        let mut sidecar = VmManager::with_config_extensions_and_runtime(
            LocalBridge::default(),
            config,
            Vec::new(),
            runtime.handle(),
        )
        .expect("process-event channel test sidecar");
        sidecar.config.runtime.protocol.max_process_events = 2;
        let envelope = |child_path, byte| ProcessEventEnvelope {
            connection_id: String::from("connection"),
            session_id: String::from("session"),
            vm_id: String::from("vm"),
            child_path,
            process_id: String::from("process"),
            event: ActiveExecutionEvent::Stdout(vec![byte]),
        };
        sidecar
            .process_event_sender
            .try_send(envelope(
                vec![String::from("a"), String::from("b"), String::from("c")],
                1,
            ))
            .expect("queue invalid first envelope");
        sidecar
            .process_event_sender
            .try_send(envelope(Vec::new(), 2))
            .expect("queue valid later envelope");

        let error = sidecar
            .drain_runtime_process_event_channel_nowait()
            .expect_err("invalid locator must fail admission");
        assert!(error
            .to_string()
            .contains("ERR_AGENTOS_PROCESS_EVENT_PATH_LIMIT"));
        assert_eq!(
            sidecar
                .process_event_receiver
                .as_ref()
                .expect("process event receiver")
                .len(),
            1,
            "the later envelope must remain in the bounded channel"
        );
    }

    #[test]
    fn deferred_current_envelope_retries_before_later_channel_envelopes() {
        let config = VmManagerConfig::default();
        let runtime = agentos_driver_tokio::TokioDriver::process(&config.runtime)
            .expect("process-event retry-order test runtime");
        let mut sidecar = VmManager::with_config_extensions_and_runtime(
            LocalBridge::default(),
            config,
            Vec::new(),
            runtime.handle(),
        )
        .expect("process-event retry-order test sidecar");
        let envelope = |byte| ProcessEventEnvelope {
            connection_id: String::from("connection"),
            session_id: String::from("session"),
            vm_id: String::from("vm"),
            child_path: Vec::new(),
            process_id: String::from("process"),
            event: ActiveExecutionEvent::Stdout(vec![byte]),
        };
        sidecar.deferred_process_event_envelope = Some(envelope(1));
        sidecar
            .process_event_sender
            .try_send(envelope(2))
            .expect("queue later envelope");

        assert!(sidecar
            .drain_runtime_process_event_channel_nowait()
            .expect("retry staged and later envelopes"));
        let bytes = sidecar
            .pending_process_events
            .drain(..)
            .map(|envelope| match envelope.event {
                ActiveExecutionEvent::Stdout(bytes) => bytes[0],
                other => panic!("expected stdout, received {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(bytes, vec![1, 2]);
        assert!(sidecar.deferred_process_event_envelope.is_none());
        assert!(sidecar
            .process_event_receiver
            .as_ref()
            .expect("process event receiver")
            .is_empty());
    }

    #[test]
    fn temporary_rejection_stays_open_and_rearms_only_after_capacity_release() {
        let config = VmManagerConfig::default();
        let runtime = agentos_driver_tokio::TokioDriver::process(&config.runtime)
            .expect("process-event no-spin test runtime");
        let mut sidecar = VmManager::with_config_extensions_and_runtime(
            LocalBridge::default(),
            config,
            Vec::new(),
            runtime.handle(),
        )
        .expect("process-event no-spin test sidecar");
        sidecar.config.runtime.protocol.max_process_events = 2;
        let envelope = |byte| ProcessEventEnvelope {
            connection_id: String::from("connection"),
            session_id: String::from("session"),
            vm_id: String::from("vm"),
            child_path: Vec::new(),
            process_id: String::from("process"),
            event: ActiveExecutionEvent::Stdout(vec![byte]),
        };
        sidecar.pending_process_events.push_back(envelope(8));
        sidecar.pending_process_events.push_back(envelope(9));
        sidecar.deferred_process_event_envelope = Some(envelope(1));
        sidecar
            .process_event_sender
            .try_send(envelope(2))
            .expect("queue later envelope");

        assert!(!sidecar
            .drain_runtime_process_event_channel_nowait()
            .expect("temporary saturation must not close the protocol or report progress"));
        assert!(sidecar.deferred_process_event_envelope.is_some());
        assert_eq!(sidecar.pending_process_events.len(), 2);
        assert_eq!(
            sidecar
                .process_event_receiver
                .as_ref()
                .expect("process event receiver")
                .len(),
            1
        );

        let mut notified = Box::pin(sidecar.process_event_notify.notified());
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(
            notified.as_mut().poll(&mut context),
            Poll::Pending
        ));

        sidecar.pending_process_events.pop_front();
        sidecar.observe_pending_process_event_depth();
        sidecar.rearm_deferred_process_event_after_capacity_release();
        assert!(matches!(
            notified.as_mut().poll(&mut context),
            Poll::Ready(())
        ));
        drop(notified);

        sidecar
            .drain_runtime_process_event_channel_nowait()
            .expect("retry staged current envelope");
        let queued = sidecar
            .pending_process_events
            .iter()
            .map(|envelope| match &envelope.event {
                ActiveExecutionEvent::Stdout(bytes) => bytes[0],
                other => panic!("expected stdout, received {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(queued, vec![9, 1]);
        assert_eq!(
            sidecar
                .process_event_receiver
                .as_ref()
                .expect("process event receiver")
                .len(),
            1,
            "later envelope remains behind the retried current envelope"
        );
    }
}
