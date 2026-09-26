//! Admitted compatibility RPC work detached from the VM coordinator.
use super::*;
use crate::executor::backend::{ExecutionEvent, PayloadLimit};
use crate::executor::host::{
    BoundedProcessLaunchRequest, BoundedUsize, HostOperation, ProcessOperation,
};
use crate::state::VmHandle;
use std::task::{Context, Poll, Waker};

impl<B> VmManager<B>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    pub(crate) fn prepare_owned_host_rpc(
        &mut self,
        vm_id: &str,
        root_id: &str,
        path: &[String],
        vm: VmHandle,
        call: ExecutionHostCall,
    ) -> Pin<Box<dyn Future<Output = Result<(), VmError>> + 'static>> {
        let prepared = (|| {
            let (max_request_bytes, max_reply_bytes) =
                vm.try_read("prepare admitted compatibility RPC", |vm| {
                    let root = vm.active_processes.get(root_id).ok_or_else(|| {
                        VmError::host("ESTALE", "compatibility RPC caller exited")
                    })?;
                    let refs = path.iter().map(String::as_str).collect::<Vec<_>>();
                    let process = Self::active_process_by_path(root, &refs).ok_or_else(|| {
                        VmError::host("ESTALE", "compatibility RPC caller exited")
                    })?;
                    let identity = call.reply.identity();
                    if identity.generation != vm.generation || identity.pid != process.kernel_pid {
                        return Err(VmError::host(
                            "ESTALE",
                            "compatibility RPC identity does not match the active kernel process",
                        ));
                    }
                    Ok((
                        vm.limits.reactor.max_bridge_request_bytes,
                        vm.limits.reactor.max_bridge_response_bytes,
                    ))
                })??;
            // Node's fs.writeSync uses the filesystem compatibility name,
            // including inherited pipe descriptors. Route child pipe writes
            // through typed dispatch so the coordinator parks them with its
            // bounded write deadline instead of blocking in Kernel::fd_write.
            let mut decoded_call = call.clone();
            let mut child_pipe_write = false;
            if !path.is_empty()
                && matches!(call.request.method.as_str(), "fs.write" | "fs.writeSync")
                && javascript_sync_rpc_arg_u64_optional(&call.request.args, 2, "write position")?
                    .is_none()
            {
                let fd = javascript_sync_rpc_arg_u32(&call.request.args, 0, "write fd")?;
                let is_pipe = vm.try_read("classify child filesystem write", |vm| {
                    let refs = path.iter().map(String::as_str).collect::<Vec<_>>();
                    let child = vm
                        .active_processes
                        .get(root_id)
                        .and_then(|root| Self::active_process_by_path(root, &refs))
                        .ok_or_else(|| VmError::host("ESTALE", "child write target exited"))?;
                    vm.kernel
                        .fd_stat(EXECUTION_DRIVER_NAME, child.kernel_pid, fd)
                        .map(|stat| stat.filetype == agentos_vm_kernel::fd_table::FILETYPE_PIPE)
                        .map_err(kernel_error)
                })??;
                if is_pipe {
                    decoded_call.request.method = String::from("process.fd_write");
                    child_pipe_write = true;
                }
            }
            let mut event = if call.request.method == "child_process.spawn_sync" {
                let (request, maximum, limit) =
                    vm.try_read("prepare captured child launch", |vm| {
                        let (request, requested) =
                            crate::service::parse_javascript_child_process_spawn_request(
                                vm,
                                &call.request.args,
                            )?;
                        let root = vm
                            .active_processes
                            .get(root_id)
                            .ok_or_else(|| missing_process_error(vm_id, root_id))?;
                        let refs = path.iter().map(String::as_str).collect::<Vec<_>>();
                        let process =
                            Self::active_process_by_path(root, &refs).ok_or_else(|| {
                                VmError::host("ESTALE", "captured child caller exited")
                            })?;
                        Ok::<_, VmError>((
                            request,
                            requested.unwrap_or(1024 * 1024),
                            (process.adapter_policy.captured_output_limit)(&process.limits),
                        ))
                    })??;
                let request_limit =
                    PayloadLimit::new("limits.reactor.maxBridgeRequestBytes", max_request_bytes)
                        .map_err(VmError::from)?;
                let output_limit = PayloadLimit::new("limits.execution.maxOutputBytes", limit)
                    .map_err(VmError::from)?;
                ActiveExecutionEvent::Common(ExecutionEvent::HostCall {
                    operation: HostOperation::Process(ProcessOperation::RunCaptured {
                        request: BoundedProcessLaunchRequest::try_new(request, &request_limit)
                            .map_err(VmError::from)?,
                        max_buffer: BoundedUsize::try_new(maximum, &output_limit)
                            .map_err(VmError::from)?,
                    }),
                    reply: call.reply.clone(),
                })
            } else {
                decode_compatibility_host_call(
                    decoded_call,
                    self.bridge.filesystem_unrestricted(vm_id),
                    max_reply_bytes,
                )?
            };
            if child_pipe_write {
                if let ActiveExecutionEvent::Common(ExecutionEvent::HostCall {
                    operation:
                        HostOperation::Filesystem(crate::executor::host::FilesystemOperation::Write {
                            nonblocking,
                            ..
                        }),
                    ..
                }) = &mut event
                {
                    *nonblocking = false;
                }
            }
            if let ActiveExecutionEvent::Common(ExecutionEvent::HostCall { operation, reply }) =
                event
            {
                let future = if path.is_empty() {
                    self.prepare_owned_root_host_call(vm_id, root_id, vm.clone(), operation, reply)
                } else {
                    self.prepare_owned_descendant_host_call(
                        vm_id,
                        root_id,
                        path,
                        vm.clone(),
                        operation,
                        reply,
                    )
                };
                return Ok(Some(future));
            }
            if call.request.method == "child_process.kill" {
                let child_id =
                    javascript_sync_rpc_arg_str(&call.request.args, 0, "child process id")?;
                let signal = javascript_sync_rpc_arg_str(&call.request.args, 1, "child signal")?;
                let refs = path.iter().map(String::as_str).collect::<Vec<_>>();
                self.kill_descendant_javascript_child_process(
                    vm_id, root_id, &refs, child_id, signal,
                )?;
                settle_execution_host_call(&call.reply, Ok(Value::Null.into()))?;
                return Ok(Some(Box::pin(async { Ok(()) })
                    as Pin<Box<dyn Future<Output = Result<(), VmError>> + 'static>>));
            }
            Ok(None)
        })();
        match prepared {
            Ok(Some(future)) => return future,
            Err(error) => {
                return Box::pin(
                    async move { settle_execution_host_call(&call.reply, Err(error)) },
                );
            }
            Ok(None) => {}
        }
        let bridge = self.bridge.clone();
        let vm_id = vm_id.to_owned();
        let root_id = root_id.to_owned();
        let path = path.to_vec();
        let notify = Arc::clone(&self.process_event_notify);
        Box::pin(async move {
            if call.reply.is_terminal() {
                return Ok(());
            }
            let response = vm.try_command("service admitted compatibility RPC", |vm| {
                let socket_paths = build_socket_path_context(vm)?;
                if call.request.method == "net.http_request" {
                    let payload: crate::service::JavascriptHttpLoopbackRequest = serde_json::from_value(call.request.args.first().cloned().ok_or_else(|| VmError::host("EINVAL", "net.http_request requires payload"))?).map_err(|error| VmError::host("EINVAL", format!("invalid loopback HTTP request: {error}")))?;
                    if !crate::service::is_javascript_loopback_host(&payload.host) {
                        return Err(VmError::host("EACCES", "HTTP loopback request requires a loopback host"));
                    }
                    bridge.require_network_access(&vm_id, NetworkOperation::Http, format_tcp_resource(&payload.host, payload.port))?;
                    if ![SocketFamily::Ipv4, SocketFamily::Ipv6].iter().any(|family| socket_paths.http_loopback_target(*family, payload.port).is_some_and(|target| target.process_id == payload.process_id && target.server_id == payload.server_id)) {
                        return Err(VmError::host("ESTALE", "HTTP loopback target no longer exists"));
                    }
                    let process = vm.active_processes.get_mut(&payload.process_id).ok_or_else(|| VmError::host("ESTALE", "HTTP loopback process exited"))?;
                    return Ok(dispatch_loopback_http_request_deferred(LoopbackHttpDispatchRequest { process, server_id: payload.server_id, request_json: &payload.request }));
                }
                let dns = vm.dns.clone();
                let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
                let capabilities = vm.capabilities.clone();
                let managed_descriptions = Arc::clone(&vm.managed_host_net_descriptions);
                let VmState { kernel, active_processes, .. } = vm;
                let root = active_processes.get_mut(&root_id).ok_or_else(|| missing_process_error(&vm_id, &root_id))?;
                let refs = path.iter().map(String::as_str).collect::<Vec<_>>();
                let process = Self::active_process_by_path_mut(root, &refs).ok_or_else(|| VmError::host("ESTALE", "compatibility RPC caller exited"))?;
                let mut future = Box::pin(service_javascript_sync_rpc(JavascriptSyncRpcServiceRequest {
                    bridge: &bridge, vm_id: &vm_id, dns: &dns, socket_paths: &socket_paths,
                    kernel, kernel_readiness, process, sync_request: &call.request,
                    capabilities, managed_descriptions: Some(managed_descriptions),
                }));
                let mut context = Context::from_waker(Waker::noop());
                let result = future.as_mut().poll(&mut context);
                drop(future);
                match result {
                    Poll::Ready(response) => Ok(response),
                    Poll::Pending => Err(VmError::host("ERR_AGENTOS_RPC_OWNERSHIP", format!("{} suspended in the synchronous VM dispatcher; it requires an owned service", call.request.method))),
                }
            }).and_then(|response| response);
            let response = match response {
                Ok(HostServiceResponse::Deferred {
                    receiver, timeout, ..
                }) => {
                    let receive = async {
                        receiver
                            .await
                            .map_err(|_| {
                                VmError::host(
                                    "ERR_AGENTOS_DEFERRED_RPC_RESPONSE_CHANNEL_CLOSED",
                                    "compatibility RPC response channel closed",
                                )
                            })?
                            .map(HostServiceResponse::Json)
                            .map_err(VmError::from)
                    };
                    match timeout {
                        Some(timeout) => {
                            match operation_deadline_timeout(&call.request.method, timeout, receive)
                                .await
                            {
                                Ok(response) => response,
                                Err(_) => Err(VmError::host(
                                    "ERR_AGENTOS_DEFERRED_RPC_TIMEOUT",
                                    format!(
                                        "{} exceeded its configured operation deadline",
                                        call.request.method
                                    ),
                                )),
                            }
                        }
                        None => receive.await,
                    }
                }
                response => response,
            };
            if response.is_ok() {
                vm.try_command("wake compatibility RPC waiters", |vm| {
                    if javascript_sync_rpc_may_make_fd_readable(&call.request) {
                        Self::wake_ready_deferred_fd_reads(vm)?;
                    }
                    if javascript_sync_rpc_may_make_fd_writable(&call.request) {
                        Self::wake_ready_deferred_fd_writes(vm)?;
                    }
                    Ok(())
                })?;
                notify.notify_one();
            }
            settle_execution_host_call(&call.reply, response)
        })
    }
}
