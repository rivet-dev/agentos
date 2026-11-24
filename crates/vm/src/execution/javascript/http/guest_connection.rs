//! Trusted client publication of a VM-local TCP listener. No host socket is opened.
use super::*;

fn connection_error(message: impl Into<String>) -> VmError {
    VmError::host("ERR_AGENTOS_GUEST_CONNECTION", message.into())
}

struct Operation {
    vm: crate::state::VmHandle,
    id: String,
    write: bool,
    completed: bool,
}

impl Drop for Operation {
    fn drop(&mut self) {
        let result = self
            .vm
            .try_command("release guest connection operation", |vm| {
                if self.completed {
                    if let Some(state) = vm.vm_fetch_streams.get_mut(&self.id) {
                        if self.write {
                            state.write_pending = false;
                        } else {
                            state.read_pending = false;
                        }
                    }
                } else if let Some(state) = vm.vm_fetch_streams.remove(&self.id) {
                    close_kernel_socket_idempotent(
                        &mut vm.kernel,
                        state.kernel_pid,
                        state.socket_id,
                    )?;
                }
                Ok(())
            });
        if let Err(error) = result {
            eprintln!("ERR_AGENTOS_GUEST_CONNECTION_CLEANUP: {}: {error}", self.id);
        }
    }
}

pub(super) async fn dispatch(
    vm: &crate::state::VmHandle,
    payload: VmFetchRequest,
) -> Result<String, VmError> {
    let operation = payload.stream_operation.as_deref().unwrap_or_default();
    if operation == "socket-open" {
        let process = vm
            .try_read("resolve guest TCP listener", |vm| {
                find_kernel_http_listener_process(vm, payload.port)
            })?
            .ok_or_else(|| {
                VmError::host(
                    "ECONNREFUSED",
                    format!("no VM-local listener on port {}", payload.port),
                )
            })?;
        let socket = open_owned_kernel_http_socket(
            vm,
            &process,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), payload.port),
            &[],
            false, // Raw TCP connections use the kernel socket quota, not the fetch quota.
            true,
        )?;
        let state = socket.into_stream_state(
            process,
            Vec::new(),
            VmFetchBodyMode::UntilClose,
            false,
            0,
            true,
        );
        let id = vm.try_command("register guest connection", |vm| {
            vm.next_vm_fetch_stream_id = vm.next_vm_fetch_stream_id.wrapping_add(1);
            let id = format!("{}:{}", vm.generation, vm.next_vm_fetch_stream_id);
            vm.vm_fetch_streams.insert(id.clone(), state);
            Ok(id)
        })?;
        return serde_json::to_string(&json!({"streamId":id}))
            .map_err(|e| connection_error(e.to_string()));
    }
    let id = payload
        .stream_id
        .as_deref()
        .ok_or_else(|| connection_error("guest connection operation requires streamId"))?;
    if operation == "socket-close" {
        return vm.try_command("close guest connection", |vm| {
            if let Some(state) = vm.vm_fetch_streams.get(id) {
                if !state.raw_connection {
                    return Err(connection_error("stream is not a guest connection"));
                }
            }
            if let Some(state) = vm.vm_fetch_streams.remove(id) {
                close_kernel_socket_idempotent(&mut vm.kernel, state.kernel_pid, state.socket_id)?;
            }
            Ok("{}".to_owned())
        });
    }
    if !matches!(operation, "socket-read" | "socket-write" | "socket-end") {
        return Err(connection_error(format!(
            "unknown guest connection operation {operation}"
        )));
    }
    let write = operation != "socket-read";
    let bytes = if operation == "socket-write" {
        let encoded = payload.body_base64.as_deref().unwrap_or_default();
        if encoded.len() > VM_FETCH_STREAM_CHUNK_MAX_BYTES.div_ceil(3) * 4 {
            return Err(connection_error(
                "write exceeds 65536 bytes; split writes into 65536-byte chunks",
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| connection_error(e.to_string()))?;
        if bytes.len() > VM_FETCH_STREAM_CHUNK_MAX_BYTES {
            return Err(connection_error(
                "write exceeds 65536 bytes; split writes into 65536-byte chunks",
            ));
        }
        bytes
    } else {
        Vec::new()
    };
    let (socket_id, wait, read_notify) =
        vm.try_command("admit guest connection operation", |vm| {
            let state = vm
                .vm_fetch_streams
                .get_mut(id)
                .ok_or_else(|| connection_error("connection is closed"))?;
            if !state.raw_connection {
                return Err(connection_error("stream is not a guest connection"));
            }
            let pending = if write {
                &mut state.write_pending
            } else {
                &mut state.read_pending
            };
            if *pending {
                return Err(connection_error(
                    "one operation per direction is allowed; await the prior operation",
                ));
            }
            *pending = true;
            let read_notify = if write {
                None
            } else {
                Some(Arc::clone(
                    &state
                        .read_readiness
                        .as_ref()
                        .expect("raw connection retains its read readiness registration")
                        .notify,
                ))
            };
            Ok((state.socket_id, vm.kernel.poll_wait_handle(), read_notify))
        })?;
    let mut guard = Operation {
        vm: vm.clone(),
        id: id.to_owned(),
        write,
        completed: false,
    };
    loop {
        // Register before checking the kernel so close/data cannot be lost
        // between the nonblocking read and parking this one directional waiter.
        let notified = read_notify.as_ref().map(|notify| notify.notified());
        tokio::pin!(notified);
        if let Some(notification) = notified.as_mut().as_pin_mut() {
            notification.enable();
        }
        let observed = wait.snapshot();
        let result = vm.try_command("perform guest connection operation", |vm| {
            if !vm.vm_fetch_streams.contains_key(id) { return Err(connection_error("connection is closed")); }
            match operation {
                "socket-read" => match vm.kernel.vm_socket_read(socket_id,
                    (payload.max_bytes.unwrap_or(65536) as usize).clamp(1, VM_FETCH_STREAM_CHUNK_MAX_BYTES)) {
                    Ok(Some(bytes)) if bytes.is_empty() => Ok(None),
                    Ok(bytes) => Ok(Some(json!({"body":base64::engine::general_purpose::STANDARD.encode(bytes.as_deref().unwrap_or_default()),"done":bytes.is_none()}))),
                    Err(error) if error.code() == "EAGAIN" => Ok(None),
                    Err(error) => Err(kernel_error(error)),
                },
                "socket-write" => match vm.kernel.vm_socket_write(socket_id, &bytes) {
                    Ok(0) if !bytes.is_empty() => Ok(None),
                    Ok(written) => Ok(Some(json!({"written":written}))),
                    Err(error) if error.code() == "EAGAIN" => Ok(None),
                    Err(error) => Err(kernel_error(error)),
                },
                _ => {
                    vm.kernel.vm_socket_shutdown(socket_id, KernelSocketShutdown::Write).map_err(kernel_error)?;
                    Ok(Some(json!({})))
                }
            }
        })?;
        if let Some(result) = result {
            guard.completed = true;
            return serde_json::to_string(&result).map_err(|e| connection_error(e.to_string()));
        }
        // This is a TCP connection, including upgraded WebSockets. HTTP's
        // request deadline must not expire an otherwise healthy idle socket.
        // One pending operation per direction and the kernel buffer/socket
        // quotas bound retained work. close, process exit and VM disposal wake
        // or cancel this wait and release the operation guard.
        if let Some(notification) = notified.as_mut().as_pin_mut() {
            notification.await;
        } else {
            // Kernel write capacity still uses the poll sequence; raw reads
            // never wake for unrelated connections.
            wait.wait_for_change_async(observed).await;
        }
    }
}
