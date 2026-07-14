use super::super::*;

const HTTP_LOOPBACK_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

fn http_loopback_request_timeout() -> Duration {
    std::env::var(HTTP_LOOPBACK_REQUEST_TIMEOUT_MS_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(HTTP_LOOPBACK_REQUEST_TIMEOUT)
}

/// Block until `fd` is readable or `deadline` passes. Returns whether it became readable.
///
/// BLOCKING: parks the calling OS thread in `poll(2)`. The unix/tcp accept and
/// udp recv callers run on the sidecar's single-thread tokio runtime, so a
/// non-zero wait stalls the whole event loop for up to `deadline` — the same
/// stall as the fixed sleeps this replaced, and only acceptable because the
/// guest net path always polls with wait == 0. Keep deadlines bounded and do
/// not add wait > 0 callers on paths that service concurrent VM traffic.

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::execution) struct JavascriptHttpListenRequest {
    pub(in crate::execution) server_id: u64,
    #[serde(default)]
    pub(in crate::execution) port: Option<u16>,
    #[serde(default)]
    pub(in crate::execution) hostname: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(in crate::execution) struct JavascriptHttpRequestOptions {
    pub(in crate::execution) method: Option<String>,
    pub(in crate::execution) headers: BTreeMap<String, Value>,
    pub(in crate::execution) body: Option<String>,
    pub(in crate::execution) reject_unauthorized: Option<bool>,
}

#[derive(Debug, Clone)]
pub(in crate::execution) struct HttpHeaderCollection {
    normalized: BTreeMap<String, Vec<String>>,
    raw_pairs: Vec<(String, String)>,
}

struct LoopbackHttpResponseWaitRequest<'a, B> {
    bridge: &'a SharedBridge<B>,
    vm_id: &'a str,
    dns: &'a VmDnsConfig,
    socket_paths: &'a JavascriptSocketPathContext,
    kernel: &'a mut SidecarKernel,
    kernel_readiness: KernelSocketReadinessRegistry,
    process: &'a mut ActiveProcess,
    request_key: (u64, u64),
    capabilities: CapabilityRegistry,
}

pub(crate) struct LoopbackHttpDispatchRequest<'a, B> {
    pub(crate) bridge: &'a SharedBridge<B>,
    pub(crate) vm_id: &'a str,
    pub(crate) dns: &'a VmDnsConfig,
    pub(crate) socket_paths: &'a JavascriptSocketPathContext,
    pub(crate) kernel: &'a mut SidecarKernel,
    pub(crate) kernel_readiness: KernelSocketReadinessRegistry,
    pub(crate) process: &'a mut ActiveProcess,
    pub(crate) server_id: u64,
    pub(crate) request_json: &'a str,
    pub(crate) capabilities: CapabilityRegistry,
}

pub(in crate::execution) fn parse_http_header_collection(
    headers: &BTreeMap<String, Value>,
    label: &str,
) -> Result<HttpHeaderCollection, SidecarError> {
    let mut normalized = BTreeMap::<String, Vec<String>>::new();
    let mut raw_pairs = Vec::new();

    for (raw_name, value) in headers {
        let normalized_name = raw_name.to_ascii_lowercase();
        let values = match value {
            Value::String(text) => vec![text.clone()],
            Value::Array(values) => values
                .iter()
                .map(|entry| {
                    entry.as_str().map(str::to_owned).ok_or_else(|| {
                        SidecarError::InvalidState(format!(
                            "{label} header {raw_name} must contain only strings"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => {
                return Err(SidecarError::InvalidState(format!(
                    "{label} header {raw_name} must be a string or string array, received {other}"
                )));
            }
        };
        raw_pairs.extend(
            values
                .iter()
                .cloned()
                .map(|entry| (raw_name.clone(), entry)),
        );
        normalized
            .entry(normalized_name)
            .or_default()
            .extend(values);
    }

    Ok(HttpHeaderCollection {
        normalized,
        raw_pairs,
    })
}

fn http_headers_json(headers: &HttpHeaderCollection) -> Value {
    let map = headers
        .normalized
        .iter()
        .map(|(name, values)| {
            let value = if values.len() == 1 {
                Value::String(values[0].clone())
            } else {
                Value::Array(values.iter().cloned().map(Value::String).collect())
            };
            (name.clone(), value)
        })
        .collect::<Map<String, Value>>();
    Value::Object(map)
}

fn http_raw_headers_json(headers: &HttpHeaderCollection) -> Value {
    Value::Array(
        headers
            .raw_pairs
            .iter()
            .flat_map(|(name, value)| [Value::String(name.clone()), Value::String(value.clone())])
            .collect(),
    )
}

pub(in crate::execution) fn is_loopback_request_host(host: &str) -> bool {
    let bare = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    matches!(bare, "localhost" | "127.0.0.1" | "::1")
}

pub(in crate::execution) fn serialize_http_loopback_request(
    url: &Url,
    options: &JavascriptHttpRequestOptions,
    headers: &HttpHeaderCollection,
) -> Result<String, SidecarError> {
    let body_base64 = options
        .body
        .as_ref()
        .map(|body| base64::engine::general_purpose::STANDARD.encode(body.as_bytes()));
    serde_json::to_string(&json!({
        "method": options.method.clone().unwrap_or_else(|| String::from("GET")),
        "url": http_request_target(url),
        "headers": http_headers_json(headers),
        "rawHeaders": http_raw_headers_json(headers),
        "bodyBase64": body_base64,
    }))
    .map_err(|error| SidecarError::Execution(format!("ERR_AGENTOS_NODE_SYNC_RPC: {error}")))
}

fn http_request_target(url: &Url) -> String {
    let path = if url.path().is_empty() {
        "/"
    } else {
        url.path()
    };
    format!(
        "{path}{}",
        url.query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default()
    )
}

pub(in crate::execution) fn find_kernel_http_listener_process(
    vm: &VmState,
    port: u16,
) -> Option<String> {
    vm.active_processes
        .iter()
        .find_map(|(process_id, process)| {
            process.tcp_listeners.values().find_map(|listener| {
                let socket_id = listener.kernel_socket_id?;
                let record = vm.kernel.socket_get(socket_id)?;
                let local_addr = record
                    .local_address()
                    .and_then(|address| resolve_tcp_bind_addr(address.host(), address.port()).ok())
                    .unwrap_or_else(|| listener.guest_local_addr());
                if local_addr.port() == port && is_vm_local_http_listener_addr(local_addr.ip()) {
                    Some(process_id.to_owned())
                } else {
                    None
                }
            })
        })
}

fn is_vm_local_http_listener_addr(ip: IpAddr) -> bool {
    ip.is_loopback() || ip.is_unspecified()
}

fn serialize_kernel_http_fetch_request(
    port: u16,
    path: &str,
    options: &JavascriptHttpRequestOptions,
    headers: &HttpHeaderCollection,
) -> Vec<u8> {
    let method = options.method.as_deref().unwrap_or("GET");
    let mut lines = vec![format!("{method} {path} HTTP/1.1")];
    let mut has_host = false;
    let mut has_connection = false;
    let mut has_content_length = false;
    for (name, values) in &headers.normalized {
        match name.as_str() {
            "host" => has_host = true,
            "connection" => has_connection = true,
            "content-length" => has_content_length = true,
            _ => {}
        }
        lines.push(format!("{name}: {}", values.join(", ")));
    }
    if !has_host {
        lines.push(format!("Host: 127.0.0.1:{port}"));
    }
    if !has_connection {
        lines.push(String::from("Connection: close"));
    }
    let body = options.body.as_deref().unwrap_or("").as_bytes();
    if !has_content_length && !body.is_empty() {
        lines.push(format!("Content-Length: {}", body.len()));
    }
    lines.push(String::new());
    lines.push(String::new());

    let mut request = lines.join("\r\n").into_bytes();
    request.extend_from_slice(body);
    request
}

pub(in crate::execution) fn kernel_http_fetch_target_exit_code(
    error: &SidecarError,
) -> Option<i32> {
    let SidecarError::Execution(message) = error else {
        return None;
    };
    message
        .strip_prefix("vm.fetch target exited before responding (exit code ")?
        .strip_suffix(')')?
        .parse()
        .ok()
}

#[allow(clippy::too_many_arguments)]
async fn service_host_fetch_target_event<B>(
    bridge: &SharedBridge<B>,
    vm_id: &str,
    dns: &VmDnsConfig,
    socket_paths: &JavascriptSocketPathContext,
    kernel: &mut SidecarKernel,
    kernel_readiness: &KernelSocketReadinessRegistry,
    process: &mut ActiveProcess,
    _wait: Duration,
    capabilities: &CapabilityRegistry,
) -> Result<bool, SidecarError>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let Some(event) = process
        .execution
        .try_poll_event()
        .map_err(|error| SidecarError::Execution(error.to_string()))?
    else {
        return Ok(false);
    };

    match event {
        ActiveExecutionEvent::JavascriptSyncRpcRequest(request) => {
            let response = service_javascript_sync_rpc(JavascriptSyncRpcServiceRequest {
                bridge,
                vm_id,
                dns,
                socket_paths,
                kernel,
                kernel_readiness: Arc::clone(kernel_readiness),
                process,
                sync_request: &request,
                capabilities: capabilities.clone(),
            })
            .await;
            match response {
                Ok(result) => process
                    .execution
                    .respond_javascript_sync_rpc_response(request.id, result)
                    .or_else(ignore_stale_javascript_sync_rpc_response)?,
                Err(error) => process
                    .execution
                    .respond_javascript_sync_rpc_error(
                        request.id,
                        javascript_sync_rpc_error_code(&error),
                        javascript_sync_rpc_error_message(&error),
                    )
                    .or_else(ignore_stale_javascript_sync_rpc_response)?,
            }
        }
        ActiveExecutionEvent::Exited(code) => {
            return Err(SidecarError::Execution(format!(
                "vm.fetch target exited before responding (exit code {code})"
            )));
        }
        other => {
            process.queue_pending_execution_event(other)?;
        }
    }
    Ok(true)
}

async fn drain_host_fetch_target_events<B>(
    bridge: &SharedBridge<B>,
    vm_id: &str,
    vm: &mut VmState,
    target_process_id: &str,
    socket_paths: &JavascriptSocketPathContext,
) -> Result<(), SidecarError>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    for _ in 0..32 {
        let dns = vm.dns.clone();
        let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
        let capabilities = vm.capabilities.clone();
        let Some(process) = vm.active_processes.get_mut(target_process_id) else {
            break;
        };
        let serviced = service_host_fetch_target_event(
            bridge,
            vm_id,
            &dns,
            socket_paths,
            &mut vm.kernel,
            &kernel_readiness,
            process,
            Duration::from_millis(1),
            &capabilities,
        )
        .await?;
        if !serviced {
            break;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(in crate::execution) async fn dispatch_kernel_http_fetch<B>(
    bridge: &SharedBridge<B>,
    vm_id: &str,
    vm: &mut VmState,
    target_process_id: &str,
    port: u16,
    path: &str,
    options: &JavascriptHttpRequestOptions,
    headers: &HttpHeaderCollection,
    max_fetch_response_bytes: usize,
) -> Result<String, SidecarError>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let socket_paths = build_javascript_socket_path_context(vm)?;
    let family = JavascriptSocketFamily::Ipv4;
    let local_port = allocate_guest_listen_port(
        0,
        family,
        &socket_paths.used_tcp_guest_ports,
        socket_paths.listen_policy,
    )?;
    let pending_capability = reserve_capability(&vm.capabilities, CapabilityKind::TcpSocket)?;

    let kernel_pid = vm
        .active_processes
        .get(target_process_id)
        .ok_or_else(|| {
            SidecarError::InvalidState(format!(
                "vm.fetch target process disappeared: {target_process_id}"
            ))
        })?
        .kernel_pid;
    let socket_id = vm
        .kernel
        .socket_create(EXECUTION_DRIVER_NAME, kernel_pid, SocketSpec::tcp())
        .map_err(kernel_error)?;
    let _fetch_capability = pending_capability
        .commit(CapabilityBackend::Kernel { socket_id })
        .map_err(|error| SidecarError::Execution(error.to_string()))?;

    let result = dispatch_kernel_http_fetch_with_socket(
        bridge,
        vm_id,
        vm,
        target_process_id,
        kernel_pid,
        socket_id,
        local_port,
        port,
        path,
        options,
        headers,
        &socket_paths,
        max_fetch_response_bytes,
    )
    .await;
    let close_result = vm
        .kernel
        .socket_close(EXECUTION_DRIVER_NAME, kernel_pid, socket_id)
        .map_err(kernel_error);
    let cleanup_result = if result.is_err() {
        drain_host_fetch_target_events(bridge, vm_id, vm, target_process_id, &socket_paths).await
    } else {
        Ok(())
    };
    match (result, close_result) {
        (Ok(response), Ok(())) => cleanup_result.map(|()| response),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_kernel_http_fetch_with_socket<B>(
    bridge: &SharedBridge<B>,
    vm_id: &str,
    vm: &mut VmState,
    target_process_id: &str,
    kernel_pid: u32,
    socket_id: SocketId,
    local_port: u16,
    port: u16,
    path: &str,
    options: &JavascriptHttpRequestOptions,
    headers: &HttpHeaderCollection,
    socket_paths: &JavascriptSocketPathContext,
    max_fetch_response_bytes: usize,
) -> Result<String, SidecarError>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    vm.kernel
        .socket_bind_inet(
            EXECUTION_DRIVER_NAME,
            kernel_pid,
            socket_id,
            InetSocketAddress::new("127.0.0.1", local_port),
        )
        .map_err(kernel_error)?;
    vm.kernel
        .socket_connect_inet_loopback(
            EXECUTION_DRIVER_NAME,
            kernel_pid,
            socket_id,
            InetSocketAddress::new("127.0.0.1", port),
        )
        .map_err(kernel_error)?;

    let request_bytes = serialize_kernel_http_fetch_request(port, path, options, headers);
    vm.kernel
        .socket_write(EXECUTION_DRIVER_NAME, kernel_pid, socket_id, &request_bytes)
        .map_err(kernel_error)?;

    let mut response_buffer = Vec::new();
    let mut peer_closed = false;
    let url = format!("http://127.0.0.1:{port}{path}");
    let deadline = Instant::now() + http_loopback_request_timeout();
    loop {
        if let Some(response) =
            parse_kernel_http_fetch_response(&response_buffer, peer_closed, &url)
                .map_err(sidecar_core_execution_error)?
        {
            ensure_vm_fetch_response_within_limit(&response, "vm.fetch", max_fetch_response_bytes)
                .map_err(sidecar_core_execution_error)?;
            return Ok(response);
        }
        if Instant::now() >= deadline {
            let preview = String::from_utf8_lossy(&response_buffer);
            return Err(SidecarError::Execution(format!(
                "vm.fetch timed out waiting for kernel TCP HTTP response ({} buffered bytes: {:?})",
                response_buffer.len(),
                preview.chars().take(200).collect::<String>()
            )));
        }

        {
            let dns = vm.dns.clone();
            let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
            let capabilities = vm.capabilities.clone();
            let process = vm
                .active_processes
                .get_mut(target_process_id)
                .ok_or_else(|| {
                    SidecarError::InvalidState(format!(
                        "vm.fetch target process disappeared: {target_process_id}"
                    ))
                })?;
            service_host_fetch_target_event(
                bridge,
                vm_id,
                &dns,
                socket_paths,
                &mut vm.kernel,
                &kernel_readiness,
                process,
                Duration::from_millis(5),
                &capabilities,
            )
            .await?;
        }

        let poll = vm
            .kernel
            .poll_targets(
                EXECUTION_DRIVER_NAME,
                kernel_pid,
                vec![PollTargetEntry::socket(
                    socket_id,
                    POLLIN | POLLHUP | POLLERR,
                )],
                5,
            )
            .map_err(kernel_error)?;
        let revents = poll
            .targets
            .first()
            .map(|entry| entry.revents)
            .unwrap_or_else(PollEvents::empty);
        if revents.intersects(POLLERR) {
            return Err(SidecarError::Execution(String::from(
                "vm.fetch kernel TCP socket reported POLLERR",
            )));
        }
        if revents.intersects(POLLIN) {
            loop {
                match vm
                    .kernel
                    .socket_read(EXECUTION_DRIVER_NAME, kernel_pid, socket_id, 64 * 1024)
                {
                    Ok(Some(bytes)) if !bytes.is_empty() => {
                        response_buffer.extend(bytes);
                        ensure_vm_fetch_raw_response_buffer_within_limit(
                            response_buffer.len(),
                            "vm.fetch",
                        )
                        .map_err(sidecar_core_execution_error)?;
                    }
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        peer_closed = true;
                        break;
                    }
                    Err(error) if error.code() == "EAGAIN" => break,
                    Err(error) => return Err(kernel_error(error)),
                }
            }
        }
        if revents.intersects(POLLHUP) {
            peer_closed = true;
        }
    }
}

fn outbound_http_response_json(url: &Url, response: ureq::Response) -> Result<Value, SidecarError> {
    let status = response.status();
    let status_text = response.status_text().to_owned();
    let mut header_pairs = Vec::new();
    let mut raw_headers = Vec::new();
    for raw_name in response.headers_names() {
        for value in response.all(&raw_name) {
            header_pairs.push(json!([raw_name.to_ascii_lowercase(), value]));
            raw_headers.push(Value::String(raw_name.clone()));
            raw_headers.push(Value::String(value.to_owned()));
        }
    }
    let mut reader = response.into_reader();
    let mut body = Vec::new();
    reader.read_to_end(&mut body).map_err(|error| {
        SidecarError::Execution(format!("failed to read HTTP response: {error}"))
    })?;
    serde_json::to_string(&json!({
        "status": status,
        "statusText": status_text,
        "headers": header_pairs,
        "rawHeaders": raw_headers,
        "body": base64::engine::general_purpose::STANDARD.encode(body),
        "bodyEncoding": "base64",
        "url": url.as_str(),
    }))
    .map(Value::String)
    .map_err(|error| SidecarError::Execution(format!("ERR_AGENTOS_NODE_SYNC_RPC: {error}")))
}

/// Split a ureq resolver `netloc` (`host:port`, with optional `[..]` IPv6
/// brackets) into its host and port components. Returns `None` if the port is
/// missing or unparseable.
fn split_netloc(netloc: &str) -> Option<(&str, u16)> {
    let (host, port) = netloc.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    let host = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    Some((host, port))
}

pub(in crate::execution) fn issue_outbound_http_request(
    url: &Url,
    options: &JavascriptHttpRequestOptions,
    headers: &HttpHeaderCollection,
    pinned_addresses: &[IpAddr],
    default_ca_bundle: &[u8],
) -> Result<Value, SidecarError> {
    let method = options.method.as_deref().unwrap_or("GET");
    if pinned_addresses.is_empty() {
        return Err(SidecarError::Execution(String::from(
            "EACCES: no egress-vetted address available for outbound HTTP request",
        )));
    }
    // Pin the underlying resolver to the egress-vetted addresses. ureq performs
    // its own DNS resolution for the TCP/TLS connect; without this override an
    // https:// request would re-resolve the hostname through the host resolver
    // (a rebinding DNS server could then return a private/metadata IP that the
    // earlier range check would have rejected). The pinned resolver returns only
    // the vetted addresses and refuses any host it was not vetted for, while the
    // request URL keeps the original hostname so TLS SNI and the Host header stay
    // correct.
    let pinned_host = url.host_str().map(str::to_owned);
    let pinned: Vec<IpAddr> = pinned_addresses.to_vec();
    let resolver = move |netloc: &str| -> std::io::Result<Vec<SocketAddr>> {
        let (host, port) = split_netloc(netloc).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid network location: {netloc}"),
            )
        })?;
        let expected_host = pinned_host.as_deref();
        if expected_host != Some(host) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "EACCES: outbound HTTP resolver pinned to {expected_host:?}, refusing {host}"
                ),
            ));
        }
        if pinned.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "EACCES: no egress-vetted address available for outbound HTTP request",
            ));
        }
        Ok(pinned.iter().map(|ip| SocketAddr::new(*ip, port)).collect())
    };
    let mut agent_builder = ureq::AgentBuilder::new()
        .resolver(resolver)
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(15))
        .timeout_write(Duration::from_secs(15));
    if url.scheme() == "https" {
        let tls_options = JavascriptTlsBridgeOptions {
            is_server: false,
            servername: url.host_str().map(str::to_owned),
            alpn_protocols: Some(vec![String::from("http/1.1")]),
            reject_unauthorized: options.reject_unauthorized,
            ..JavascriptTlsBridgeOptions::default()
        };
        agent_builder = agent_builder.tls_config(Arc::new(build_client_tls_config(
            &tls_options,
            default_ca_bundle,
        )?));
    }
    let agent = agent_builder.build();
    let mut request = agent.request_url(method, url);
    for (name, values) in &headers.normalized {
        if name == "host" {
            continue;
        }
        let header_value = values.join(", ");
        request = request.set(name, &header_value);
    }
    let response = match options.body.as_deref() {
        Some(body) => request.send_string(body),
        None => request.call(),
    };

    match response {
        Ok(response) => outbound_http_response_json(url, response),
        Err(ureq::Error::Status(_, response)) => outbound_http_response_json(url, response),
        Err(ureq::Error::Transport(error)) => Err(SidecarError::Execution(format!(
            "ERR_HTTP_REQUEST_FAILED: {error}"
        ))),
    }
}

async fn wait_for_loopback_http_response<B>(
    request: LoopbackHttpResponseWaitRequest<'_, B>,
) -> Result<String, SidecarError>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let LoopbackHttpResponseWaitRequest {
        bridge,
        vm_id,
        dns,
        socket_paths,
        kernel,
        kernel_readiness,
        process,
        request_key,
        capabilities,
    } = request;
    let deadline = Instant::now() + http_loopback_request_timeout();
    loop {
        let response = match process.pending_http_requests.get(&request_key) {
            Some(PendingHttpRequest::Buffered(response)) => response.clone(),
            Some(PendingHttpRequest::Deferred(_)) | None => None,
        };
        if let Some(response) = response {
            process.pending_http_requests.remove(&request_key);
            return Ok(response);
        }

        if Instant::now() >= deadline {
            process.pending_http_requests.remove(&request_key);
            return Err(SidecarError::Execution(String::from(
                "HTTP loopback request timed out waiting for net.http_respond",
            )));
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        let Some(event) = process
            .execution
            .poll_event(remaining)
            .await
            .map_err(|error| SidecarError::Execution(error.to_string()))?
        else {
            continue;
        };

        match event {
            ActiveExecutionEvent::JavascriptSyncRpcRequest(request) => {
                let response = service_javascript_sync_rpc(JavascriptSyncRpcServiceRequest {
                    bridge,
                    vm_id,
                    dns,
                    socket_paths,
                    kernel,
                    kernel_readiness: Arc::clone(&kernel_readiness),
                    process,
                    sync_request: &request,
                    capabilities: capabilities.clone(),
                })
                .await;
                match response {
                    Ok(result) => process
                        .execution
                        .respond_javascript_sync_rpc_response(request.id, result)
                        .or_else(ignore_stale_javascript_sync_rpc_response)?,
                    Err(error) => process
                        .execution
                        .respond_javascript_sync_rpc_error(
                            request.id,
                            javascript_sync_rpc_error_code(&error),
                            javascript_sync_rpc_error_message(&error),
                        )
                        .or_else(ignore_stale_javascript_sync_rpc_response)?,
                }
            }
            ActiveExecutionEvent::Exited(code) => {
                process.pending_http_requests.remove(&request_key);
                return Err(SidecarError::Execution(format!(
                    "HTTP loopback server exited before responding (exit code {code})"
                )));
            }
            ActiveExecutionEvent::Stdout(_)
            | ActiveExecutionEvent::Stderr(_)
            | ActiveExecutionEvent::JavascriptSyncRpcCompletion(_)
            | ActiveExecutionEvent::PythonVfsRpcRequest(_)
            | ActiveExecutionEvent::PythonSocketConnectCompletion(_)
            | ActiveExecutionEvent::SignalState { .. } => {}
        }
    }
}

fn begin_loopback_http_request(
    process: &mut ActiveProcess,
    server_id: u64,
    request_json: &str,
    pending: impl FnOnce() -> PendingHttpRequest,
) -> Result<(u64, u64), SidecarError> {
    process.pending_http_requests.retain(
        |_, pending| !matches!(pending, PendingHttpRequest::Deferred(sender) if sender.is_closed()),
    );
    let request_id = {
        let server = process.http_servers.get_mut(&server_id).ok_or_else(|| {
            SidecarError::InvalidState(format!("HTTP target server disappeared: {server_id}"))
        })?;
        server.next_request_id += 1;
        server.next_request_id
    };
    process
        .pending_http_requests
        .insert((server_id, request_id), pending());
    process.execution.send_javascript_stream_event(
        "http_request",
        json!({
            "serverId": server_id,
            "requestId": request_id,
            "request": request_json,
        }),
    )?;
    Ok((server_id, request_id))
}

pub(in crate::execution) fn complete_loopback_http_request(
    process: &mut ActiveProcess,
    request_key: (u64, u64),
    response_json: String,
) -> Result<(), SidecarError> {
    let pending = process
        .pending_http_requests
        .remove(&request_key)
        .ok_or_else(|| {
            SidecarError::InvalidState(format!(
                "unknown pending HTTP request {} for server {}",
                request_key.1, request_key.0
            ))
        })?;
    match pending {
        PendingHttpRequest::Buffered(_) => {
            process.pending_http_requests.insert(
                request_key,
                PendingHttpRequest::Buffered(Some(response_json)),
            );
        }
        PendingHttpRequest::Deferred(respond_to) => {
            respond_to
                .send(Ok(Value::String(response_json)))
                .map_err(|_| {
                    SidecarError::InvalidState(String::from(
                        "HTTP loopback response waiter closed before net.http_respond",
                    ))
                })?;
        }
    }
    Ok(())
}

pub(crate) async fn dispatch_loopback_http_request<B>(
    request: LoopbackHttpDispatchRequest<'_, B>,
) -> Result<String, SidecarError>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let LoopbackHttpDispatchRequest {
        bridge,
        vm_id,
        dns,
        socket_paths,
        kernel,
        kernel_readiness,
        process,
        server_id,
        request_json,
        capabilities,
    } = request;
    let request_key = begin_loopback_http_request(process, server_id, request_json, || {
        PendingHttpRequest::Buffered(None)
    })?;
    wait_for_loopback_http_response(LoopbackHttpResponseWaitRequest {
        bridge,
        vm_id,
        dns,
        socket_paths,
        kernel,
        kernel_readiness,
        process,
        request_key,
        capabilities,
    })
    .await
}

pub(crate) fn dispatch_loopback_http_request_deferred<B>(
    request: LoopbackHttpDispatchRequest<'_, B>,
) -> Result<JavascriptSyncRpcServiceResponse, SidecarError>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let LoopbackHttpDispatchRequest {
        process,
        server_id,
        request_json,
        ..
    } = request;
    let (respond_to, receiver) = tokio::sync::oneshot::channel();
    begin_loopback_http_request(process, server_id, request_json, || {
        PendingHttpRequest::Deferred(respond_to)
    })?;
    Ok(JavascriptSyncRpcServiceResponse::Deferred {
        receiver,
        timeout: Some(http_loopback_request_timeout()),
        task_class: agentos_runtime::TaskClass::Listener,
    })
}

pub(in crate::execution) fn sidecar_core_execution_error(error: SidecarCoreError) -> SidecarError {
    SidecarError::Execution(error.to_string())
}

pub(crate) fn ensure_vm_fetch_response_frame_within_limit(
    response: &ResponseFrame,
    max_frame_bytes: usize,
) -> Result<(), SidecarError> {
    let max_frame_bytes = max_frame_bytes.min(VM_FETCH_BUFFER_LIMIT_BYTES);
    let frame = crate::protocol::to_generated_protocol_frame(
        &crate::protocol::ProtocolFrame::Response(response.clone()),
    )
    .map_err(|error| SidecarError::FrameTooLarge(error.to_string()))?;
    let WireProtocolFrame::ResponseFrame(_) = &frame else {
        return Err(SidecarError::FrameTooLarge(String::from(
            "vm fetch response converted to non-response wire frame",
        )));
    };
    WireFrameCodec::new(max_frame_bytes)
        .encode(&frame)
        .map(|_| ())
        .map_err(|error| SidecarError::FrameTooLarge(error.to_string()))
}

/// Adversarial coverage for the DNS-rebinding gap (VECTORS.md D.3) on the
/// Python/Pyodide `httpRequestSync` outbound HTTP path. The egress range guard
/// (`filter_dns_safe_ip_addrs`) runs at resolution time, but `ureq` performs its
/// own DNS resolution for the TCP/TLS connect, so a rebinding DNS server could
/// previously make the second lookup land on a private/link-local/metadata IP
/// the first check rejected. The fix pins `ureq`'s resolver to the vetted
/// address set; these tests prove the connect is pinned and refuses any other
/// host or an empty (fully-rejected) address set.
#[cfg(test)]
mod dns_rebinding_pin_tests {
    use super::{issue_outbound_http_request, split_netloc, JavascriptHttpRequestOptions};
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, TcpListener};
    use std::thread;
    use url::Url;

    fn empty_headers() -> super::HttpHeaderCollection {
        super::parse_http_header_collection(&BTreeMap::new(), "test headers")
            .expect("empty header collection")
    }

    fn options() -> JavascriptHttpRequestOptions {
        JavascriptHttpRequestOptions {
            method: Some(String::from("GET")),
            headers: BTreeMap::new(),
            body: None,
            reject_unauthorized: None,
        }
    }

    #[test]
    fn split_netloc_handles_hostnames_and_bracketed_ipv6() {
        assert_eq!(
            split_netloc("attacker.example:80"),
            Some(("attacker.example", 80))
        );
        assert_eq!(split_netloc("[::1]:443"), Some(("::1", 443)));
        assert_eq!(split_netloc("10.0.0.1:8080"), Some(("10.0.0.1", 8080)));
        assert_eq!(split_netloc("no-port"), None);
        assert_eq!(split_netloc("host:notaport"), None);
    }

    /// A loopback HTTP server stands in for the egress-vetted target. The
    /// request URL uses a *different* hostname (`attacker.example`) whose real
    /// DNS would resolve elsewhere; pinning forces the connect onto the vetted
    /// IP only. If the resolver were unpinned, the request would fail to reach
    /// this server (and on a real host could land on a private/metadata IP).
    #[test]
    fn outbound_http_connect_is_pinned_to_vetted_ip() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind loopback server");
        let port = listener.local_addr().expect("local addr").port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi")
                .expect("write response");
            let _ = stream.flush();
        });

        let url = Url::parse(&format!("http://attacker.example:{port}/")).expect("url");
        let pinned = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
        let result = issue_outbound_http_request(&url, &options(), &empty_headers(), &pinned, &[])
            .expect("pinned request should reach the vetted loopback target");
        let payload = result.as_str().expect("string payload");
        assert!(
            payload.contains("\"status\":200"),
            "expected 200 from pinned target, got: {payload}"
        );
        server.join().expect("server thread");
    }

    /// With no vetted address (every resolved IP was rejected by the range
    /// guard, or the literal IP was a blocked range), the pinned resolver must
    /// refuse rather than fall back to the host resolver.
    #[test]
    fn outbound_http_refuses_when_no_vetted_address() {
        let url = Url::parse("https://attacker.example/").expect("url");
        let error = issue_outbound_http_request(&url, &options(), &empty_headers(), &[], &[])
            .expect_err("empty pinned set must be refused");
        let message = error.to_string();
        assert!(
            message.contains("EACCES") || message.contains("ERR_HTTP_REQUEST_FAILED"),
            "expected an egress refusal, got: {message}"
        );
    }
}
