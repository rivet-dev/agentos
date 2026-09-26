use super::*;

pub(super) trait DeferredResponseSettlement<T> {
    fn settle(self, value: T);
}

impl<T> DeferredResponseSettlement<T> for tokio::sync::oneshot::Sender<T> {
    fn settle(self, value: T) {
        if self.send(value).is_err() {
            eprintln!(
                "INFO_AGENTOS_STALE_DEFERRED_COMPLETION: deferred RPC waiter was dropped before settlement"
            );
        }
    }
}

pub(super) fn validate_guest_network_capability_alias(
    process: &ActiveProcess,
    request: &HostRpcRequest,
) -> Result<(), VmError> {
    if !(request.method.starts_with("net.")
        || request.method.starts_with("dgram.")
        || request.method.starts_with("tls."))
    {
        return Ok(());
    }

    if let Some(local_id) = request.args.first().and_then(Value::as_str) {
        for (key, kind) in [
            (
                NativeCapabilityKey::TcpSocket(local_id.to_owned()),
                CapabilityKind::TcpSocket,
            ),
            (
                NativeCapabilityKey::UnixSocket(local_id.to_owned()),
                CapabilityKind::UnixSocket,
            ),
            (
                NativeCapabilityKey::UdpSocket(local_id.to_owned()),
                CapabilityKind::UdpSocket,
            ),
            (
                NativeCapabilityKey::TcpListener(local_id.to_owned()),
                CapabilityKind::TcpListener,
            ),
            (
                NativeCapabilityKey::UnixListener(local_id.to_owned()),
                CapabilityKind::UnixListener,
            ),
            (
                NativeCapabilityKey::TlsSocket(local_id.to_owned()),
                CapabilityKind::TlsTransport,
            ),
        ] {
            if process.capability_leases.contains_key(&key) {
                process.validate_capability_alias(&key, kind)?;
            }
        }
    }

    let Some(id) = request.args.first().and_then(Value::as_u64) else {
        return Ok(());
    };
    let process_key = NativeCapabilityKey::HttpServer(id);
    if process.capability_leases.contains_key(&process_key) {
        process.validate_capability_alias(&process_key, CapabilityKind::TcpListener)?;
    }

    let state = process
        .http2
        .shared
        .lock()
        .map_err(|_| VmError::InvalidState(String::from("HTTP/2 state lock poisoned")))?;
    let generation = process.runtime_context.vm_generation().ok_or_else(|| {
        VmError::host(
            "ERR_AGENTOS_CAPABILITY_SESSION",
            String::from("process runtime is not VM-generation scoped"),
        )
    })?;
    for (key, kind) in [
        (
            NativeCapabilityKey::Http2Server(id),
            CapabilityKind::TcpListener,
        ),
        (
            NativeCapabilityKey::Http2Session(id),
            CapabilityKind::Http2Connection,
        ),
        (
            NativeCapabilityKey::Http2Stream(id),
            CapabilityKind::Http2Stream,
        ),
    ] {
        if let Some(lease) = state.capability_leases.get(&key) {
            lease.validate(generation, kind).map_err(VmError::from)?;
        }
    }
    Ok(())
}

pub(super) fn missing_vm_error(vm_id: &str) -> VmError {
    VmError::InvalidState(format!("VM {vm_id} is no longer active"))
}

pub(super) fn missing_process_error(vm_id: &str, process_id: &str) -> VmError {
    VmError::InvalidState(format!(
        "VM {vm_id} no longer has active process {process_id}"
    ))
}

/// Map a shared guest-kernel-call dispatcher error without reconstructing an
/// errno from its human-readable diagnostic.
pub(crate) type OwnedVmRouteFuture =
    Pin<Box<dyn Future<Output = Result<DispatchResult, VmError>> + 'static>>;

/// Everything an independently supervised VM-owned route needs after the
/// process coordinator has completed ownership validation. The request and VM
/// handle are owned so the returned future never retains `&mut VmManager`.
#[derive(Clone)]
pub(crate) struct OwnedVmRouteInput {
    pub(crate) request: RequestFrame,
    pub(crate) vm_id: String,
    pub(crate) vm: crate::state::VmHandle,
}

pub(crate) async fn resize_pty_owned<B>(
    bridge: SharedBridge<B>,
    input: OwnedVmRouteInput,
    payload: ResizePtyRequest,
) -> Result<DispatchResult, VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    // Signal registrations are execution events. Consume them before the
    // resize so a handler installed immediately before the host request is
    // visible when the kernel-generated SIGWINCH is delivered below.
    let foreground_pgid = input.vm.try_command("resize process PTY", |vm| {
        let process = vm
            .active_processes
            .get(&payload.process_id)
            .ok_or_else(|| {
                VmError::InvalidState(format!(
                    "VM {} has no active process {}",
                    input.vm_id, payload.process_id
                ))
            })?;
        let Some(writer_fd) = process.kernel_stdin_writer_fd else {
            return Err(VmError::InvalidState(format!(
                "process {} does not have a PTY",
                payload.process_id
            )));
        };
        let kernel_pid = process.kernel_pid;
        let foreground_pgid = vm
            .kernel
            .tcgetpgrp(EXECUTION_DRIVER_NAME, kernel_pid, writer_fd)
            .map_err(kernel_error)?;
        vm.kernel
            .pty_resize(
                EXECUTION_DRIVER_NAME,
                kernel_pid,
                writer_fd,
                payload.cols,
                payload.rows,
            )
            .map_err(kernel_error)?;
        Ok(foreground_pgid)
    })?;
    deliver_kernel_process_group_signal_to_tracked_runtimes_owned(
        &bridge,
        &input.vm,
        &input.vm_id,
        foreground_pgid,
        "SIGWINCH",
    )?;

    Ok(DispatchResult {
        response: crate::core::respond(
            &input.request,
            ResponsePayload::PtyResized(PtyResizedResponse {
                process_id: payload.process_id,
                cols: payload.cols,
                rows: payload.rows,
            }),
        ),
        events: Vec::new(),
    })
}

pub(crate) async fn write_stdin_owned(
    input: OwnedVmRouteInput,
    payload: WriteStdinRequest,
) -> Result<DispatchResult, VmError> {
    input.vm.try_command("write process stdin", |vm| {
        let VmState {
            kernel,
            active_processes,
            ..
        } = vm;
        let process = active_processes
            .get_mut(&payload.process_id)
            .ok_or_else(|| {
                VmError::InvalidState(format!(
                    "VM {} has no active process {}",
                    input.vm_id, payload.process_id
                ))
            })?;
        write_kernel_process_stdin(kernel, process, &payload.chunk)
    })?;

    Ok(DispatchResult {
        response: stdin_written_response(
            &input.request,
            payload.process_id,
            payload.chunk.len() as u64,
        ),
        events: Vec::new(),
    })
}

pub(crate) async fn close_stdin_owned(
    input: OwnedVmRouteInput,
    payload: CloseStdinRequest,
) -> Result<DispatchResult, VmError> {
    input.vm.try_command("close process stdin", |vm| {
        let VmState {
            kernel,
            active_processes,
            ..
        } = vm;
        let process = active_processes
            .get_mut(&payload.process_id)
            .ok_or_else(|| {
                VmError::InvalidState(format!(
                    "VM {} has no active process {}",
                    input.vm_id, payload.process_id
                ))
            })?;
        close_kernel_process_stdin(kernel, process)
    })?;

    Ok(DispatchResult {
        response: stdin_closed_response(&input.request, payload.process_id),
        events: Vec::new(),
    })
}

pub(crate) async fn find_listener_owned<B>(
    bridge: SharedBridge<B>,
    input: OwnedVmRouteInput,
    payload: FindListenerRequest,
) -> Result<DispatchResult, VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    require_vm_inspection_permission(
        &bridge,
        &input.vm_id,
        "network.inspect",
        "network",
        &socket_query_resource(SocketQueryKind::TcpListener, &payload),
    )?;
    let listener = input.vm.try_read("find TCP listener", |vm| {
        find_socket_state_entry(Some(vm), SocketQueryKind::TcpListener, &payload)
    })??;
    Ok(DispatchResult {
        response: listener_snapshot_response(&input.request, listener),
        events: Vec::new(),
    })
}

pub(crate) async fn find_bound_udp_owned<B>(
    bridge: SharedBridge<B>,
    input: OwnedVmRouteInput,
    payload: FindBoundUdpRequest,
) -> Result<DispatchResult, VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let lookup_request = FindListenerRequest {
        host: payload.host,
        port: payload.port,
        path: None,
    };
    require_vm_inspection_permission(
        &bridge,
        &input.vm_id,
        "network.inspect",
        "network",
        &socket_query_resource(SocketQueryKind::UdpBound, &lookup_request),
    )?;
    let socket = input.vm.try_read("find bound UDP socket", |vm| {
        find_socket_state_entry(Some(vm), SocketQueryKind::UdpBound, &lookup_request)
    })??;
    Ok(DispatchResult {
        response: bound_udp_snapshot_response(&input.request, socket),
        events: Vec::new(),
    })
}

pub(crate) async fn get_signal_state_owned(
    input: OwnedVmRouteInput,
    payload: GetSignalStateRequest,
) -> Result<DispatchResult, VmError> {
    let handlers = input.vm.try_read("read process signal state", |vm| {
        let mut handlers = BTreeMap::new();
        if let Some(process) = vm.active_processes.get(&payload.process_id) {
            for signal in 1..=64 {
                let action = process
                    .kernel_handle
                    .signal_action(signal, None)
                    .map_err(kernel_error)?;
                if action.disposition
                    != agentos_vm_kernel::process_table::SignalDisposition::Default
                {
                    handlers.insert(signal as u32, protocol_signal_registration(action));
                }
            }
        }
        Ok::<_, VmError>(handlers)
    })??;
    Ok(DispatchResult {
        response: signal_state_response(&input.request, payload.process_id, handlers),
        events: Vec::new(),
    })
}

/// Map a shared guest-kernel-call dispatcher error into a sidecar error,
/// preserving POSIX errno codes (`ECODE: message`) as kernel errors so guest
/// callers observe Linux-faithful failures, mirroring the filesystem path.
pub(crate) fn guest_kernel_core_error(error: crate::core::SidecarCoreError) -> VmError {
    match error.code() {
        Some(code) => VmError::Host(HostServiceError::new(code, error.message())),
        None => VmError::InvalidState(error.to_string()),
    }
}

pub(super) fn javascript_child_process_gone_error(
    process_id: &str,
    child_path: &[&str],
) -> VmError {
    let child_label = if child_path.is_empty() {
        process_id.to_owned()
    } else {
        format!("{process_id}/{}", child_path.join("/"))
    };
    VmError::Host(HostServiceError::new(
        "ECHILD",
        format!("child_process {child_label} is no longer available"),
    ))
}

pub(super) fn is_javascript_child_process_gone_error(error: &VmError) -> bool {
    guest_error_code(error) == Some("ECHILD")
}

pub(super) fn missing_javascript_child_cleanup_result(
    next_child_process_id: usize,
    child_process_id: &str,
    operation: &str,
) -> Result<(), VmError> {
    let previously_allocated = child_process_id
        .strip_prefix("child-")
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|sequence| {
            sequence != 0
                && sequence <= next_child_process_id
                && child_process_id == format!("child-{sequence}")
        });
    if previously_allocated {
        return Ok(());
    }
    Err(VmError::InvalidState(format!(
        "unknown child process {child_process_id} during {operation}"
    )))
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod child_kill_result_tests {
    use super::missing_javascript_child_cleanup_result;

    #[test]
    fn cleanup_kill_ignores_reaped_child_but_rejects_unknown_id() {
        missing_javascript_child_cleanup_result(1, "child-1", "kill")
            .expect("a previously allocated child is confirmed gone");
        missing_javascript_child_cleanup_result(1, "child-1", "stdin close")
            .expect("closing stdin after a child exits is idempotent");
        assert!(
            missing_javascript_child_cleanup_result(1, "child-2", "kill")
                .expect_err("a never-allocated child must remain an error")
                .to_string()
                .contains("unknown child process child-2")
        );
        assert!(
            missing_javascript_child_cleanup_result(1, "child-01", "stdin close")
                .expect_err("a non-canonical child id must remain an error")
                .to_string()
                .contains("unknown child process child-01")
        );
    }
}

impl<B> VmManager<B>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    pub(crate) fn prepare_owned_vm_route(
        &self,
        request: &RequestFrame,
    ) -> Result<OwnedVmRouteInput, VmError> {
        let (connection_id, session_id, vm_id) = self.vm_scope_for(&request.ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
        let vm = self
            .vms
            .handle(&vm_id)
            .ok_or_else(|| missing_vm_error(&vm_id))?;
        Ok(OwnedVmRouteInput {
            request: request.clone(),
            vm_id,
            vm,
        })
    }

    pub(crate) fn resize_pty(
        &mut self,
        request: &RequestFrame,
        payload: ResizePtyRequest,
    ) -> OwnedVmRouteFuture {
        let input = self.prepare_owned_vm_route(request);
        let bridge = self.bridge.clone();
        Box::pin(async move { resize_pty_owned(bridge, input?, payload).await })
    }

    pub(crate) fn write_stdin(
        &mut self,
        request: &RequestFrame,
        payload: WriteStdinRequest,
    ) -> OwnedVmRouteFuture {
        let input = self.prepare_owned_vm_route(request);
        Box::pin(async move { write_stdin_owned(input?, payload).await })
    }

    pub(crate) fn close_stdin(
        &mut self,
        request: &RequestFrame,
        payload: CloseStdinRequest,
    ) -> OwnedVmRouteFuture {
        let input = self.prepare_owned_vm_route(request);
        Box::pin(async move { close_stdin_owned(input?, payload).await })
    }

    pub(crate) fn find_listener(
        &mut self,
        request: &RequestFrame,
        payload: FindListenerRequest,
    ) -> OwnedVmRouteFuture {
        let input = self.prepare_owned_vm_route(request);
        let bridge = self.bridge.clone();
        Box::pin(async move { find_listener_owned(bridge, input?, payload).await })
    }

    pub(crate) fn get_process_snapshot(
        &mut self,
        request: &RequestFrame,
        _payload: GetProcessSnapshotRequest,
    ) -> OwnedVmRouteFuture {
        let input = self.prepare_owned_vm_route(request);
        let bridge = self.bridge.clone();
        Box::pin(async move {
            let input = input?;
            require_vm_inspection_permission(
                &bridge,
                &input.vm_id,
                "process.inspect",
                "process",
                "process://snapshot",
            )?;
            let processes = input.vm.try_command("get process snapshot", |vm| {
                prune_exited_process_snapshots(vm);
                Ok(snapshot_vm_processes(vm))
            })?;
            Ok(DispatchResult {
                response: process_snapshot_response(&input.request, processes),
                events: Vec::new(),
            })
        })
    }

    pub(crate) async fn guest_kernel_call(
        &mut self,
        request: &RequestFrame,
        payload: GuestKernelCallRequest,
    ) -> Result<DispatchResult, VmError> {
        let (connection_id, session_id, vm_id) = self.vm_scope_for(&request.ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;

        let mut vm = self.vms.get_mut(&vm_id).ok_or_else(|| {
            VmError::InvalidState(format!("VM {vm_id} no longer exists for guest kernel call"))
        })?;
        let kernel_pid = vm
            .active_processes
            .get(&payload.execution_id)
            .map(|process| process.kernel_pid)
            .ok_or_else(|| {
                VmError::InvalidState(format!(
                    "VM {vm_id} has no active process {} for guest kernel call",
                    payload.execution_id
                ))
            })?;

        let response = crate::core::handle_guest_kernel_call(
            &mut vm.kernel,
            kernel_pid,
            EXECUTION_DRIVER_NAME,
            &payload.operation,
            &payload.payload,
        )
        .map_err(guest_kernel_core_error)?;

        Ok(DispatchResult {
            response: self.respond(
                request,
                ResponsePayload::GuestKernelResult(GuestKernelResultResponse { payload: response }),
            ),
            events: Vec::new(),
        })
    }

    pub(crate) fn get_resource_snapshot(
        &mut self,
        request: &RequestFrame,
        _payload: GetResourceSnapshotRequest,
    ) -> OwnedVmRouteFuture {
        let input = self.prepare_owned_vm_route(request);
        let bridge = self.bridge.clone();
        Box::pin(async move {
            let input = input?;
            require_vm_inspection_permission(
                &bridge,
                &input.vm_id,
                "process.inspect",
                "process",
                "process://resources",
            )?;

            let vm = input.vm.borrow();
            let snapshot = vm.kernel.resource_snapshot();
            let wasm_reserved_memory_bytes =
                vm.resources.usage(ResourceClass::WasmMemoryBytes).used as u64;
            let wasmtime = vm
                .execution_engines
                .wasm("read resource snapshot")?
                .wasmtime_metrics()?;
            let queue_snapshots = queue_tracker::queue_snapshot()
                .into_iter()
                .map(|queue| QueueSnapshotEntry {
                    name: queue.name.as_str().to_owned(),
                    category: queue.category.as_str().to_owned(),
                    depth: queue.depth as u64,
                    high_water: queue.high_water as u64,
                    capacity: queue.capacity as u64,
                    fill_percent: queue.fill_percent as u64,
                })
                .collect();

            Ok(DispatchResult {
                response: crate::core::respond(
                    &input.request,
                    ResponsePayload::ResourceSnapshot(ResourceSnapshotResponse {
                        running_processes: snapshot.running_processes as u64,
                        stopped_processes: snapshot.stopped_processes as u64,
                        exited_processes: snapshot.exited_processes as u64,
                        fd_tables: snapshot.fd_tables as u64,
                        open_fds: snapshot.open_fds as u64,
                        pipes: snapshot.pipes as u64,
                        pipe_buffered_bytes: snapshot.pipe_buffered_bytes as u64,
                        ptys: snapshot.ptys as u64,
                        pty_buffered_input_bytes: snapshot.pty_buffered_input_bytes as u64,
                        pty_buffered_output_bytes: snapshot.pty_buffered_output_bytes as u64,
                        sockets: snapshot.sockets as u64,
                        socket_listeners: snapshot.socket_listeners as u64,
                        socket_connections: snapshot.socket_connections as u64,
                        socket_buffered_bytes: snapshot.socket_buffered_bytes as u64,
                        socket_datagram_queue_len: snapshot.socket_datagram_queue_len as u64,
                        wasm_reserved_memory_bytes,
                        wasmtime_engine_profiles: wasmtime.engine_profiles as u64,
                        wasmtime_module_entries: wasmtime.module_entries as u64,
                        wasmtime_module_cache_hits: wasmtime.module_cache_hits,
                        wasmtime_module_cache_misses: wasmtime.module_cache_misses,
                        wasmtime_module_cache_evictions: wasmtime.module_cache_evictions,
                        wasmtime_compiled_source_bytes: wasmtime.compiled_source_bytes,
                        wasmtime_charged_module_bytes: wasmtime.charged_module_bytes as u64,
                        wasmtime_compile_time_micros: u64::try_from(
                            wasmtime.compile_time.as_micros(),
                        )
                        .unwrap_or(u64::MAX),
                        wasmtime_process_retained_rss_bytes: wasmtime.process_retained_rss_bytes,
                        queue_snapshots,
                    }),
                ),
                events: Vec::new(),
            })
        })
    }

    pub(crate) fn find_bound_udp(
        &mut self,
        request: &RequestFrame,
        payload: FindBoundUdpRequest,
    ) -> OwnedVmRouteFuture {
        let input = self.prepare_owned_vm_route(request);
        let bridge = self.bridge.clone();
        Box::pin(async move { find_bound_udp_owned(bridge, input?, payload).await })
    }

    pub(crate) fn vm_fetch(
        &mut self,
        request: &RequestFrame,
        payload: VmFetchRequest,
    ) -> OwnedVmRouteFuture {
        let input = self.prepare_owned_vm_route(request);
        let max_frame_bytes = self.config.max_frame_bytes;
        Box::pin(async move {
            let input = input?;
            let response_json =
                dispatch_owned_vm_fetch(&input.vm_id, input.vm.clone(), payload).await?;
            let response = crate::core::respond(
                &input.request,
                ResponsePayload::VmFetchResult(VmFetchResponse { response_json }),
            );
            ensure_vm_fetch_response_frame_within_limit(&response, max_frame_bytes)?;
            Ok(DispatchResult {
                response,
                events: Vec::new(),
            })
        })
    }

    pub(crate) fn get_signal_state(
        &mut self,
        request: &RequestFrame,
        payload: GetSignalStateRequest,
    ) -> OwnedVmRouteFuture {
        let input = self.prepare_owned_vm_route(request);
        Box::pin(async move { get_signal_state_owned(input?, payload).await })
    }

    pub(crate) fn get_zombie_timer_count(
        &mut self,
        request: &RequestFrame,
        _payload: GetZombieTimerCountRequest,
    ) -> OwnedVmRouteFuture {
        let input = self.prepare_owned_vm_route(request);
        Box::pin(async move {
            let input = input?;
            let count = input.vm.try_read("get zombie timer count", |vm| {
                vm.kernel.zombie_timer_count() as u64
            })?;
            Ok(DispatchResult {
                response: zombie_timer_count_response(&input.request, count),
                events: Vec::new(),
            })
        })
    }
}
