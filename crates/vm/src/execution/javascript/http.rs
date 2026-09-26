use super::super::*;
use crate::executor::host::{BoundedString, HttpHeader};

const HTTP_LOOPBACK_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const VM_FETCH_STREAM_CHUNK_MAX_BYTES: usize = 64 * 1024;
const VM_FETCH_STREAM_COUNT_LIMIT: usize = 256;
type VmFetchResponseHead = (u16, String, Vec<(String, String)>, VmFetchBodyMode);

pub(in crate::execution) fn http_loopback_request_timeout() -> Duration {
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

pub(crate) struct LoopbackHttpDispatchRequest<'a> {
    pub(crate) process: &'a mut ActiveProcess,
    pub(crate) server_id: u64,
    pub(crate) request_json: &'a str,
}

pub(in crate::execution) fn parse_http_header_collection(
    headers: &BTreeMap<String, Value>,
    label: &str,
) -> Result<HttpHeaderCollection, VmError> {
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
                        VmError::InvalidState(format!(
                            "{label} header {raw_name} must contain only strings"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => {
                return Err(VmError::InvalidState(format!(
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
) -> Result<String, VmError> {
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
    .map_err(|error| VmError::host("ERR_AGENTOS_NODE_SYNC_RPC", format!("{error}")))
}

pub(in crate::execution) fn http_request_target(url: &Url) -> String {
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

pub(crate) fn find_kernel_http_listener_process(vm: &VmState, port: u16) -> Option<String> {
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

pub(crate) fn find_vm_fetch_target_process(vm: &VmState, port: u16) -> Option<String> {
    find_kernel_http_listener_process(vm, port).or_else(|| {
        vm.active_processes
            .iter()
            .find_map(|(process_id, process)| {
                process
                    .http_servers
                    .values()
                    .any(|server| server.guest_local_addr.port() == port)
                    .then(|| process_id.clone())
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
    body_bytes: Option<&[u8]>,
) -> Result<Vec<u8>, VmError> {
    let method = options.method.as_deref().unwrap_or("GET");
    let path = format!("/{}", path.trim_start_matches('/'));
    let metadata_limit =
        PayloadLimit::new("vm.fetch.requestMetadata", VM_FETCH_BUFFER_LIMIT_BYTES)?;
    let metadata_headers = headers
        .raw_pairs
        .iter()
        .map(|(name, value)| {
            Ok(HttpHeader {
                name: BoundedString::try_new(name.clone(), &metadata_limit)?,
                value: BoundedString::try_new(value.clone(), &metadata_limit)?,
            })
        })
        .collect::<Result<Vec<_>, HostServiceError>>()?;
    validate_http_request_metadata(method, &metadata_headers)?;

    let connection_scoped_headers = headers
        .normalized
        .get("connection")
        .into_iter()
        .flatten()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>();
    let mut lines = vec![format!("{method} {path} HTTP/1.1")];
    let mut has_host = false;
    for (name, values) in &headers.normalized {
        // This function creates a new HTTP/1.1 message from a decoded request
        // body. Forwarding the source connection's framing, fixed hop-by-hop
        // headers, or fields nominated by Connection can produce an invalid
        // CL/TE combination or leak connection-scoped metadata.
        if connection_scoped_headers.contains(name)
            || matches!(
                name.as_str(),
                "connection"
                    | "content-length"
                    | "keep-alive"
                    | "proxy-connection"
                    | "te"
                    | "proxy-authenticate"
                    | "proxy-authorization"
                    | "trailer"
                    | "transfer-encoding"
                    | "upgrade"
            )
        {
            continue;
        }
        if name == "host" {
            has_host = true;
        }
        lines.push(format!("{name}: {}", values.join(", ")));
    }
    if !has_host {
        lines.push(format!("Host: 127.0.0.1:{port}"));
    }
    lines.push(String::from("Connection: close"));
    let body = body_bytes.unwrap_or_else(|| options.body.as_deref().unwrap_or("").as_bytes());
    if !body.is_empty() {
        lines.push(format!("Content-Length: {}", body.len()));
    }
    lines.push(String::new());
    lines.push(String::new());

    let mut request = lines.join("\r\n").into_bytes();
    request.extend_from_slice(body);
    Ok(request)
}

fn find_http_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_stream_response_head(
    bytes: &[u8],
    request_method: &str,
    max_response_bytes: usize,
) -> Result<VmFetchResponseHead, VmError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        VmError::Execution(format!(
            "ERR_AGENTOS_VM_FETCH_INVALID_RESPONSE: response headers were not UTF-8: {error}"
        ))
    })?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut status_parts = status_line.splitn(3, ' ');
    let version = status_parts.next().unwrap_or_default();
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(VmError::Execution(format!(
            "ERR_AGENTOS_VM_FETCH_INVALID_RESPONSE: invalid status line {status_line:?}"
        )));
    }
    let status = status_parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| (100..=599).contains(value))
        .ok_or_else(|| {
            VmError::Execution(format!(
                "ERR_AGENTOS_VM_FETCH_INVALID_RESPONSE: invalid status line {status_line:?}"
            ))
        })?;
    let status_text = status_parts.next().unwrap_or_default().to_owned();
    let mut headers = Vec::new();
    let mut content_length = None;
    let mut chunked = false;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or_else(|| {
            VmError::Execution(format!(
                "ERR_AGENTOS_VM_FETCH_INVALID_RESPONSE: malformed header {line:?}"
            ))
        })?;
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_owned();
        if name == "content-length" {
            let parsed = value.parse::<usize>().map_err(|error| {
                VmError::Execution(format!(
                    "ERR_AGENTOS_VM_FETCH_INVALID_RESPONSE: invalid content-length {value:?}: {error}"
                ))
            })?;
            if content_length
                .replace(parsed)
                .is_some_and(|prior| prior != parsed)
            {
                return Err(VmError::Execution(String::from(
                    "ERR_AGENTOS_VM_FETCH_INVALID_RESPONSE: conflicting content-length headers",
                )));
            }
        }
        if name == "transfer-encoding"
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("chunked"))
        {
            chunked = true;
        }
        headers.push((name, value));
    }
    if chunked && content_length.is_some() {
        return Err(VmError::Execution(String::from(
            "ERR_AGENTOS_VM_FETCH_INVALID_RESPONSE: response supplied both chunked encoding and content-length",
        )));
    }
    if content_length.is_some_and(|length| length > max_response_bytes) {
        return Err(VmError::Execution(format!(
            "ERR_AGENTOS_VM_FETCH_LIMIT: response content-length exceeds max_fetch_response_bytes {max_response_bytes}; raise limits.http.maxFetchResponseBytes"
        )));
    }
    let body_mode =
        if request_method.eq_ignore_ascii_case("HEAD") || matches!(status, 100..=199 | 204 | 304) {
            VmFetchBodyMode::Empty
        } else if chunked {
            VmFetchBodyMode::Chunked {
                chunk_remaining: None,
            }
        } else if let Some(remaining) = content_length {
            if remaining == 0 {
                VmFetchBodyMode::Empty
            } else {
                VmFetchBodyMode::ContentLength { remaining }
            }
        } else {
            VmFetchBodyMode::UntilClose
        };
    Ok((status, status_text, headers, body_mode))
}

fn append_decoded_stream_bytes(
    state: &mut VmFetchStreamState,
    bytes: &[u8],
) -> Result<(), VmError> {
    let next = state
        .response_bytes
        .checked_add(bytes.len())
        .ok_or_else(|| {
            VmError::Execution(String::from(
                "ERR_AGENTOS_VM_FETCH_LIMIT: streamed response byte counter overflowed",
            ))
        })?;
    if next > state.max_response_bytes {
        return Err(VmError::Execution(format!(
            "ERR_AGENTOS_VM_FETCH_LIMIT: streamed response exceeds max_fetch_response_bytes {}; raise limits.http.maxFetchResponseBytes",
            state.max_response_bytes
        )));
    }
    state.response_bytes = next;
    state.decoded_buffer.extend(bytes.iter().copied());
    Ok(())
}

fn decode_stream_body(state: &mut VmFetchStreamState) -> Result<(), VmError> {
    loop {
        match state.body_mode {
            VmFetchBodyMode::Empty => return Ok(()),
            VmFetchBodyMode::ContentLength { remaining } => {
                if remaining == 0 {
                    state.body_mode = VmFetchBodyMode::Empty;
                    continue;
                }
                let take = remaining.min(state.raw_buffer.len());
                if take == 0 {
                    if state.peer_closed {
                        return Err(VmError::Execution(String::from(
                            "ERR_AGENTOS_VM_FETCH_TRUNCATED: peer closed before content-length bytes arrived",
                        )));
                    }
                    return Ok(());
                }
                let bytes: Vec<u8> = state.raw_buffer.drain(..take).collect();
                append_decoded_stream_bytes(state, &bytes)?;
                state.body_mode = if take == remaining {
                    VmFetchBodyMode::Empty
                } else {
                    VmFetchBodyMode::ContentLength {
                        remaining: remaining - take,
                    }
                };
            }
            VmFetchBodyMode::UntilClose => {
                if !state.raw_buffer.is_empty() {
                    let bytes = std::mem::take(&mut state.raw_buffer);
                    append_decoded_stream_bytes(state, &bytes)?;
                }
                if state.peer_closed {
                    state.body_mode = VmFetchBodyMode::Empty;
                }
                return Ok(());
            }
            VmFetchBodyMode::Chunked { chunk_remaining } => {
                let remaining = if let Some(remaining) = chunk_remaining {
                    remaining
                } else {
                    let Some(line_end) = state
                        .raw_buffer
                        .windows(2)
                        .position(|window| window == b"\r\n")
                    else {
                        if state.peer_closed {
                            return Err(VmError::Execution(String::from(
                                "ERR_AGENTOS_VM_FETCH_TRUNCATED: peer closed inside chunk header",
                            )));
                        }
                        return Ok(());
                    };
                    let line = std::str::from_utf8(&state.raw_buffer[..line_end]).map_err(|error| {
                        VmError::Execution(format!(
                            "ERR_AGENTOS_VM_FETCH_INVALID_RESPONSE: chunk header was not UTF-8: {error}"
                        ))
                    })?;
                    let size_text = line.split(';').next().unwrap_or_default().trim();
                    let size = usize::from_str_radix(size_text, 16).map_err(|error| {
                        VmError::Execution(format!(
                            "ERR_AGENTOS_VM_FETCH_INVALID_RESPONSE: invalid chunk size {size_text:?}: {error}"
                        ))
                    })?;
                    state.raw_buffer.drain(..line_end + 2);
                    if size == 0 {
                        state.body_mode = VmFetchBodyMode::Empty;
                        return Ok(());
                    }
                    size
                };
                if state.raw_buffer.len() < remaining + 2 {
                    state.body_mode = VmFetchBodyMode::Chunked {
                        chunk_remaining: Some(remaining),
                    };
                    if state.peer_closed {
                        return Err(VmError::Execution(String::from(
                            "ERR_AGENTOS_VM_FETCH_TRUNCATED: peer closed inside chunk body",
                        )));
                    }
                    return Ok(());
                }
                if &state.raw_buffer[remaining..remaining + 2] != b"\r\n" {
                    return Err(VmError::Execution(String::from(
                        "ERR_AGENTOS_VM_FETCH_INVALID_RESPONSE: chunk body was not followed by CRLF",
                    )));
                }
                let bytes: Vec<u8> = state.raw_buffer.drain(..remaining).collect();
                state.raw_buffer.drain(..2);
                append_decoded_stream_bytes(state, &bytes)?;
                state.body_mode = VmFetchBodyMode::Chunked {
                    chunk_remaining: None,
                };
            }
        }
    }
}

pub(in crate::execution) struct KernelHttpFetch {
    kernel_pid: u32,
    socket_id: SocketId,
    response_buffer: Vec<u8>,
    peer_closed: bool,
    url: String,
    deadline: Instant,
    max_fetch_response_bytes: usize,
    _capability: agentos_driver_tokio::capability::CapabilityLease,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::execution) fn begin_kernel_http_fetch(
    vm: &mut VmState,
    target_process_id: &str,
    port: u16,
    path: &str,
    options: &JavascriptHttpRequestOptions,
    headers: &HttpHeaderCollection,
    body_bytes: Option<&[u8]>,
    max_fetch_response_bytes: usize,
) -> Result<KernelHttpFetch, VmError> {
    // Validate and serialize before reserving capabilities or creating a
    // socket. Rejected request metadata must have no observable side effects.
    let request_bytes =
        serialize_kernel_http_fetch_request(port, path, options, headers, body_bytes)?;
    // Client source ports belong to the kernel socket table. The listen-port
    // allocator does not reserve active client sockets and can hand the same
    // source port to concurrent requests.
    let local_port = 0;
    let pending_capability = reserve_capability(&vm.capabilities, CapabilityKind::TcpSocket)?;

    let kernel_pid = vm
        .active_processes
        .get(target_process_id)
        .ok_or_else(|| {
            VmError::InvalidState(format!(
                "vm.fetch target process disappeared: {target_process_id}"
            ))
        })?
        .kernel_pid;
    let socket_id = vm
        .kernel
        .socket_create(EXECUTION_DRIVER_NAME, kernel_pid, SocketSpec::tcp())
        .map_err(kernel_error)?;
    let capability = pending_capability
        .commit(CapabilityBackend::Kernel { socket_id })
        .map_err(|error| VmError::Execution(error.to_string()))?;
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
    vm.kernel
        .socket_write(EXECUTION_DRIVER_NAME, kernel_pid, socket_id, &request_bytes)
        .map_err(kernel_error)?;

    Ok(KernelHttpFetch {
        kernel_pid,
        socket_id,
        response_buffer: Vec::new(),
        peer_closed: false,
        url: format!("http://127.0.0.1:{port}{path}"),
        deadline: Instant::now() + http_loopback_request_timeout(),
        max_fetch_response_bytes,
        _capability: capability,
    })
}

pub(in crate::execution) fn poll_kernel_http_fetch(
    vm: &mut VmState,
    fetch: &mut KernelHttpFetch,
) -> Result<Option<String>, VmError> {
    if let Some(response) =
        parse_kernel_http_fetch_response(&fetch.response_buffer, fetch.peer_closed, &fetch.url)
            .map_err(sidecar_core_execution_error)?
    {
        ensure_vm_fetch_response_within_limit(
            &response,
            "vm.fetch",
            fetch.max_fetch_response_bytes,
        )
        .map_err(sidecar_core_execution_error)?;
        return Ok(Some(response));
    }
    if Instant::now() >= fetch.deadline {
        let preview = String::from_utf8_lossy(&fetch.response_buffer);
        return Err(VmError::Execution(format!(
            "ERR_AGENTOS_VM_FETCH_TIMEOUT: vm.fetch timed out waiting for kernel TCP HTTP response ({} buffered bytes: {:?}); raise AGENTOS_HTTP_LOOPBACK_REQUEST_TIMEOUT_MS",
            fetch.response_buffer.len(),
            preview.chars().take(200).collect::<String>()
        )));
    }

    let poll = vm
        .kernel
        .poll_targets(
            EXECUTION_DRIVER_NAME,
            fetch.kernel_pid,
            vec![PollTargetEntry::socket(
                fetch.socket_id,
                POLLIN | POLLHUP | POLLERR,
            )],
            0,
        )
        .map_err(kernel_error)?;
    let revents = poll
        .targets
        .first()
        .map(|entry| entry.revents)
        .unwrap_or_else(PollEvents::empty);
    if revents.intersects(POLLERR) {
        return Err(VmError::Execution(String::from(
            "vm.fetch kernel TCP socket reported POLLERR",
        )));
    }
    if revents.intersects(POLLIN) {
        loop {
            match vm.kernel.socket_read(
                EXECUTION_DRIVER_NAME,
                fetch.kernel_pid,
                fetch.socket_id,
                64 * 1024,
            ) {
                Ok(Some(bytes)) if !bytes.is_empty() => {
                    fetch.response_buffer.extend(bytes);
                    ensure_vm_fetch_raw_response_buffer_within_limit(
                        fetch.response_buffer.len(),
                        "vm.fetch",
                    )
                    .map_err(sidecar_core_execution_error)?;
                }
                Ok(Some(_)) => break,
                Ok(None) => {
                    fetch.peer_closed = true;
                    break;
                }
                Err(error) if error.code() == "EAGAIN" => break,
                Err(error) => return Err(kernel_error(error)),
            }
        }
    }
    if revents.intersects(POLLHUP) {
        fetch.peer_closed = true;
    }
    // A readiness probe must settle data made available in that same probe.
    // Returning Pending after draining a complete response forces callers to
    // wait for a second edge that may instead be the one-shot server's exit.
    if let Some(response) =
        parse_kernel_http_fetch_response(&fetch.response_buffer, fetch.peer_closed, &fetch.url)
            .map_err(sidecar_core_execution_error)?
    {
        ensure_vm_fetch_response_within_limit(
            &response,
            "vm.fetch",
            fetch.max_fetch_response_bytes,
        )
        .map_err(sidecar_core_execution_error)?;
        return Ok(Some(response));
    }
    Ok(None)
}

pub(in crate::execution) fn close_kernel_http_fetch(
    vm: &mut VmState,
    fetch: &KernelHttpFetch,
) -> Result<(), VmError> {
    vm.kernel
        .socket_close(EXECUTION_DRIVER_NAME, fetch.kernel_pid, fetch.socket_id)
        .map_err(kernel_error)
}

pub(in crate::execution) struct PendingKernelHttpFetchStream {
    target_process_id: String,
    kernel_pid: u32,
    socket_id: SocketId,
    capability: agentos_driver_tokio::capability::CapabilityLease,
    response_buffer: Vec<u8>,
    peer_closed: bool,
    deadline: Instant,
    request_method: String,
    max_response_bytes: usize,
}

pub(in crate::execution) struct KernelHttpFetchStreamHead {
    status: u16,
    status_text: String,
    response_headers: Vec<(String, String)>,
    body_mode: VmFetchBodyMode,
}

pub(in crate::execution) enum KernelHttpFetchStreamRead {
    Pending,
    Chunk {
        response_json: String,
        closed_target_process_id: Option<String>,
    },
}

#[allow(clippy::too_many_arguments)]
pub(in crate::execution) fn begin_kernel_http_fetch_stream(
    vm: &mut VmState,
    target_process_id: &str,
    port: u16,
    path: &str,
    options: &JavascriptHttpRequestOptions,
    headers: &HttpHeaderCollection,
    body_bytes: Option<&[u8]>,
    max_response_bytes: usize,
) -> Result<PendingKernelHttpFetchStream, VmError> {
    if vm.vm_fetch_streams.len() >= VM_FETCH_STREAM_COUNT_LIMIT {
        return Err(VmError::Execution(format!(
            "ERR_AGENTOS_VM_FETCH_STREAM_LIMIT: VM has {} open fetch streams; close or cancel a stream before opening another (limit {})",
            vm.vm_fetch_streams.len(),
            VM_FETCH_STREAM_COUNT_LIMIT
        )));
    }
    let request_bytes =
        serialize_kernel_http_fetch_request(port, path, options, headers, body_bytes)?;
    let pending_capability = reserve_capability(&vm.capabilities, CapabilityKind::TcpSocket)?;
    let kernel_pid = vm
        .active_processes
        .get(target_process_id)
        .ok_or_else(|| {
            VmError::InvalidState(format!(
                "vm.fetch target process disappeared: {target_process_id}"
            ))
        })?
        .kernel_pid;
    let socket_id = vm
        .kernel
        .socket_create(EXECUTION_DRIVER_NAME, kernel_pid, SocketSpec::tcp())
        .map_err(kernel_error)?;
    let capability = pending_capability
        .commit(CapabilityBackend::Kernel { socket_id })
        .map_err(|error| VmError::Execution(error.to_string()))?;

    let setup_result = (|| {
        // Port zero delegates ephemeral source-port selection to the kernel
        // socket table. The listener allocator does not reserve client ports.
        vm.kernel
            .socket_bind_inet(
                EXECUTION_DRIVER_NAME,
                kernel_pid,
                socket_id,
                InetSocketAddress::new("127.0.0.1", 0),
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
        vm.kernel
            .socket_write(EXECUTION_DRIVER_NAME, kernel_pid, socket_id, &request_bytes)
            .map_err(kernel_error)
    })();
    if let Err(error) = setup_result {
        if let Err(close_error) =
            vm.kernel
                .socket_close(EXECUTION_DRIVER_NAME, kernel_pid, socket_id)
        {
            tracing::error!(
                socket_id,
                error = %close_error,
                "failed to close kernel socket after VM fetch stream setup error"
            );
        }
        return Err(error);
    }

    Ok(PendingKernelHttpFetchStream {
        target_process_id: target_process_id.to_owned(),
        kernel_pid,
        socket_id,
        capability,
        response_buffer: Vec::new(),
        peer_closed: false,
        deadline: Instant::now() + http_loopback_request_timeout(),
        request_method: options.method.as_deref().unwrap_or("GET").to_owned(),
        max_response_bytes,
    })
}

pub(in crate::execution) fn poll_kernel_http_fetch_stream_start(
    vm: &mut VmState,
    pending: &mut PendingKernelHttpFetchStream,
) -> Result<Option<KernelHttpFetchStreamHead>, VmError> {
    loop {
        if let Some(header_end) = find_http_header_end(&pending.response_buffer) {
            let (status, status_text, response_headers, body_mode) = parse_stream_response_head(
                &pending.response_buffer[..header_end],
                &pending.request_method,
                pending.max_response_bytes,
            )?;
            pending.response_buffer.drain(..header_end + 4);
            if (100..200).contains(&status) && status != 101 {
                continue;
            }
            return Ok(Some(KernelHttpFetchStreamHead {
                status,
                status_text,
                response_headers,
                body_mode,
            }));
        }
        break;
    }

    if Instant::now() >= pending.deadline {
        return Err(VmError::Execution(format!(
            "ERR_AGENTOS_VM_FETCH_TIMEOUT: timed out waiting for response headers after {} ms; raise AGENTOS_HTTP_LOOPBACK_REQUEST_TIMEOUT_MS",
            http_loopback_request_timeout().as_millis()
        )));
    }
    let poll = vm
        .kernel
        .poll_targets(
            EXECUTION_DRIVER_NAME,
            pending.kernel_pid,
            vec![PollTargetEntry::socket(
                pending.socket_id,
                POLLIN | POLLHUP | POLLERR,
            )],
            0,
        )
        .map_err(kernel_error)?;
    let revents = poll
        .targets
        .first()
        .map(|entry| entry.revents)
        .unwrap_or_else(PollEvents::empty);
    if revents.intersects(POLLERR) {
        return Err(VmError::Execution(String::from(
            "ERR_AGENTOS_VM_FETCH_SOCKET: kernel TCP socket reported POLLERR",
        )));
    }
    if revents.intersects(POLLIN) {
        loop {
            match vm.kernel.socket_read(
                EXECUTION_DRIVER_NAME,
                pending.kernel_pid,
                pending.socket_id,
                VM_FETCH_STREAM_CHUNK_MAX_BYTES,
            ) {
                Ok(Some(bytes)) if !bytes.is_empty() => {
                    pending.response_buffer.extend(bytes);
                    ensure_vm_fetch_raw_response_buffer_within_limit(
                        pending.response_buffer.len(),
                        "vm.fetchStream",
                    )
                    .map_err(sidecar_core_execution_error)?;
                }
                Ok(Some(_)) => break,
                Ok(None) => {
                    pending.peer_closed = true;
                    break;
                }
                Err(error) if error.code() == "EAGAIN" => break,
                Err(error) => return Err(kernel_error(error)),
            }
        }
    }
    if revents.intersects(POLLHUP) {
        pending.peer_closed = true;
    }
    if pending.peer_closed && find_http_header_end(&pending.response_buffer).is_none() {
        return Err(VmError::Execution(String::from(
            "ERR_AGENTOS_VM_FETCH_TRUNCATED: peer closed before response headers completed",
        )));
    }
    Ok(None)
}

pub(in crate::execution) fn complete_kernel_http_fetch_stream_start(
    vm: &mut VmState,
    pending: PendingKernelHttpFetchStream,
    head: KernelHttpFetchStreamHead,
) -> Result<String, VmError> {
    let PendingKernelHttpFetchStream {
        target_process_id,
        kernel_pid,
        socket_id,
        capability,
        response_buffer,
        peer_closed,
        max_response_bytes,
        ..
    } = pending;
    vm.next_vm_fetch_stream_id = vm.next_vm_fetch_stream_id.wrapping_add(1);
    let stream_id = format!("{}:{}", vm.generation, vm.next_vm_fetch_stream_id);
    let mut state = VmFetchStreamState {
        target_process_id,
        kernel_pid,
        socket_id,
        _capability: capability,
        raw_buffer: response_buffer,
        decoded_buffer: VecDeque::new(),
        body_mode: head.body_mode,
        peer_closed,
        response_bytes: 0,
        max_response_bytes,
        last_progress_at: Instant::now(),
    };
    let result = (|| {
        decode_stream_body(&mut state)?;
        serde_json::to_string(&json!({
            "streamId": stream_id,
            "status": head.status,
            "statusText": head.status_text,
            "headers": head.response_headers,
        }))
        .map_err(|error| {
            VmError::Execution(format!(
                "ERR_AGENTOS_VM_FETCH_SERIALIZE: failed to serialize response head: {error}"
            ))
        })
    })();
    match result {
        Ok(response_json) => {
            vm.vm_fetch_streams.insert(stream_id, state);
            Ok(response_json)
        }
        Err(error) => {
            if let Err(close_error) =
                vm.kernel
                    .socket_close(EXECUTION_DRIVER_NAME, kernel_pid, socket_id)
            {
                tracing::error!(
                    socket_id,
                    error = %close_error,
                    "failed to close kernel socket after VM fetch stream completion error"
                );
            }
            Err(error)
        }
    }
}

pub(in crate::execution) fn abort_kernel_http_fetch_stream_start(
    vm: &mut VmState,
    pending: PendingKernelHttpFetchStream,
) -> Result<(), VmError> {
    vm.kernel
        .socket_close(EXECUTION_DRIVER_NAME, pending.kernel_pid, pending.socket_id)
        .map_err(kernel_error)
}

fn close_kernel_http_fetch_stream_state(
    vm: &mut VmState,
    state: VmFetchStreamState,
) -> Result<String, VmError> {
    let target_process_id = state.target_process_id.clone();
    vm.kernel
        .socket_close(EXECUTION_DRIVER_NAME, state.kernel_pid, state.socket_id)
        .map_err(kernel_error)?;
    drop(state);
    Ok(target_process_id)
}

pub(in crate::execution) fn poll_kernel_http_fetch_stream_read(
    vm: &mut VmState,
    stream_id: &str,
    requested_max_bytes: usize,
) -> Result<KernelHttpFetchStreamRead, VmError> {
    let max_bytes = requested_max_bytes.clamp(1, VM_FETCH_STREAM_CHUNK_MAX_BYTES);
    enum Probe {
        Pending,
        Chunk { response_json: String, done: bool },
    }

    let probe_result = (|| {
        let (kernel, streams) = (&mut vm.kernel, &mut vm.vm_fetch_streams);
        let state = streams.get_mut(stream_id).ok_or_else(|| {
            VmError::InvalidState(format!(
                "ERR_AGENTOS_VM_FETCH_STREAM_NOT_FOUND: stream {stream_id:?} is closed or unknown"
            ))
        })?;
        decode_stream_body(state)?;
        if state.decoded_buffer.is_empty() && !matches!(state.body_mode, VmFetchBodyMode::Empty) {
            if state.last_progress_at.elapsed() >= http_loopback_request_timeout() {
                return Err(VmError::Execution(format!(
                    "ERR_AGENTOS_VM_FETCH_TIMEOUT: stream produced no data for {} ms; raise AGENTOS_HTTP_LOOPBACK_REQUEST_TIMEOUT_MS",
                    http_loopback_request_timeout().as_millis()
                )));
            }
            let poll = kernel
                .poll_targets(
                    EXECUTION_DRIVER_NAME,
                    state.kernel_pid,
                    vec![PollTargetEntry::socket(
                        state.socket_id,
                        POLLIN | POLLHUP | POLLERR,
                    )],
                    0,
                )
                .map_err(kernel_error)?;
            let revents = poll
                .targets
                .first()
                .map(|entry| entry.revents)
                .unwrap_or_else(PollEvents::empty);
            if revents.intersects(POLLERR) {
                return Err(VmError::Execution(String::from(
                    "ERR_AGENTOS_VM_FETCH_SOCKET: kernel TCP stream reported POLLERR",
                )));
            }
            let before = state.raw_buffer.len();
            let was_peer_closed = state.peer_closed;
            if revents.intersects(POLLIN) {
                loop {
                    match kernel.socket_read(
                        EXECUTION_DRIVER_NAME,
                        state.kernel_pid,
                        state.socket_id,
                        VM_FETCH_STREAM_CHUNK_MAX_BYTES,
                    ) {
                        Ok(Some(bytes)) if !bytes.is_empty() => {
                            state.raw_buffer.extend(bytes);
                            ensure_vm_fetch_raw_response_buffer_within_limit(
                                state.raw_buffer.len(),
                                "vm.fetchStream",
                            )
                            .map_err(sidecar_core_execution_error)?;
                        }
                        Ok(Some(_)) => break,
                        Ok(None) => {
                            state.peer_closed = true;
                            break;
                        }
                        Err(error) if error.code() == "EAGAIN" => break,
                        Err(error) => return Err(kernel_error(error)),
                    }
                }
            }
            if revents.intersects(POLLHUP) {
                state.peer_closed = true;
            }
            if state.raw_buffer.len() != before || state.peer_closed != was_peer_closed {
                state.last_progress_at = Instant::now();
            }
            decode_stream_body(state)?;
        }

        if state.decoded_buffer.is_empty() && !matches!(state.body_mode, VmFetchBodyMode::Empty) {
            return Ok(Probe::Pending);
        }
        let take = max_bytes.min(state.decoded_buffer.len());
        let body: Vec<u8> = state.decoded_buffer.drain(..take).collect();
        let done =
            state.decoded_buffer.is_empty() && matches!(state.body_mode, VmFetchBodyMode::Empty);
        let response_json = serde_json::to_string(&json!({
            "body": base64::engine::general_purpose::STANDARD.encode(body),
            "done": done,
        }))
        .map_err(|error| {
            VmError::Execution(format!(
                "ERR_AGENTOS_VM_FETCH_SERIALIZE: failed to serialize stream chunk: {error}"
            ))
        })?;
        Ok(Probe::Chunk {
            response_json,
            done,
        })
    })();

    match probe_result {
        Ok(Probe::Pending) => Ok(KernelHttpFetchStreamRead::Pending),
        Ok(Probe::Chunk {
            response_json,
            done: false,
        }) => Ok(KernelHttpFetchStreamRead::Chunk {
            response_json,
            closed_target_process_id: None,
        }),
        Ok(Probe::Chunk {
            response_json,
            done: true,
        }) => {
            let state = vm.vm_fetch_streams.remove(stream_id).ok_or_else(|| {
                VmError::InvalidState(format!(
                    "ERR_AGENTOS_VM_FETCH_STREAM_NOT_FOUND: stream {stream_id:?} disappeared while closing"
                ))
            })?;
            let target_process_id = close_kernel_http_fetch_stream_state(vm, state)?;
            Ok(KernelHttpFetchStreamRead::Chunk {
                response_json,
                closed_target_process_id: Some(target_process_id),
            })
        }
        Err(error) => {
            if let Some(state) = vm.vm_fetch_streams.remove(stream_id) {
                if let Err(close_error) = close_kernel_http_fetch_stream_state(vm, state) {
                    tracing::error!(
                        stream_id,
                        error = %close_error,
                        "failed to close errored VM fetch stream"
                    );
                }
            }
            Err(error)
        }
    }
}

pub(in crate::execution) fn cancel_kernel_http_fetch_stream_nonblocking(
    vm: &mut VmState,
    stream_id: &str,
) -> Result<(String, String), VmError> {
    let state = vm.vm_fetch_streams.remove(stream_id).ok_or_else(|| {
        VmError::InvalidState(format!(
            "ERR_AGENTOS_VM_FETCH_STREAM_NOT_FOUND: stream {stream_id:?} is closed or unknown"
        ))
    })?;
    let target_process_id = close_kernel_http_fetch_stream_state(vm, state)?;
    Ok((String::from("{\"cancelled\":true}"), target_process_id))
}

pub(in crate::execution) fn begin_loopback_http_request(
    process: &mut ActiveProcess,
    server_id: u64,
    request_json: &str,
    pending: impl FnOnce() -> PendingHttpRequest,
) -> Result<(u64, u64), VmError> {
    process.pending_http_requests.retain(
        |_, pending| !matches!(pending, PendingHttpRequest::Deferred(sender) if sender.is_closed()),
    );
    let request_id = {
        let server = process.http_servers.get_mut(&server_id).ok_or_else(|| {
            VmError::InvalidState(format!("HTTP target server disappeared: {server_id}"))
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

pub(in crate::execution) fn take_loopback_http_response(
    process: &mut ActiveProcess,
    request_key: (u64, u64),
) -> Option<String> {
    let response = match process.pending_http_requests.get(&request_key) {
        Some(PendingHttpRequest::Buffered(response)) => response.clone(),
        Some(PendingHttpRequest::Deferred(_)) | None => None,
    }?;
    process.pending_http_requests.remove(&request_key);
    Some(response)
}

pub(in crate::execution) fn complete_loopback_http_request(
    process: &mut ActiveProcess,
    request_key: (u64, u64),
    response_json: String,
) -> Result<(), VmError> {
    let pending = process
        .pending_http_requests
        .remove(&request_key)
        .ok_or_else(|| {
            VmError::InvalidState(format!(
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
                    VmError::InvalidState(String::from(
                        "HTTP loopback response waiter closed before net.http_respond",
                    ))
                })?;
        }
    }
    Ok(())
}

pub(crate) fn dispatch_loopback_http_request_deferred(
    request: LoopbackHttpDispatchRequest<'_>,
) -> Result<HostServiceResponse, VmError> {
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
    Ok(HostServiceResponse::Deferred {
        receiver,
        timeout: Some(http_loopback_request_timeout()),
        task_class: agentos_driver_tokio::TaskClass::Listener,
    })
}

pub(in crate::execution) fn sidecar_core_execution_error(error: SidecarCoreError) -> VmError {
    VmError::Execution(error.to_string())
}

pub(crate) fn ensure_vm_fetch_response_frame_within_limit(
    response: &ResponseFrame,
    max_frame_bytes: usize,
) -> Result<(), VmError> {
    let max_frame_bytes = max_frame_bytes.min(VM_FETCH_BUFFER_LIMIT_BYTES);
    let frame = crate::protocol::to_generated_protocol_frame(
        &crate::protocol::ProtocolFrame::Response(response.clone()),
    )
    .map_err(|error| VmError::FrameTooLarge(error.to_string()))?;
    let WireProtocolFrame::ResponseFrame(_) = &frame else {
        return Err(VmError::FrameTooLarge(String::from(
            "vm fetch response converted to non-response wire frame",
        )));
    };
    WireFrameCodec::new(max_frame_bytes)
        .encode(&frame)
        .map(|_| ())
        .map_err(|error| VmError::FrameTooLarge(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_options(method: &str) -> JavascriptHttpRequestOptions {
        JavascriptHttpRequestOptions {
            method: Some(method.to_owned()),
            headers: BTreeMap::new(),
            body: None,
            reject_unauthorized: None,
        }
    }

    #[test]
    fn vm_fetch_serializes_exactly_one_leading_path_slash() {
        let options = request_options("GET");
        let headers =
            parse_http_header_collection(&BTreeMap::new(), "test headers").expect("headers");
        let request =
            serialize_kernel_http_fetch_request(3000, "///nested?q=1", &options, &headers, None)
                .expect("serialize request");
        assert!(
            request.starts_with(b"GET /nested?q=1 HTTP/1.1\r\n"),
            "request line was {:?}",
            String::from_utf8_lossy(&request)
        );
    }

    #[test]
    fn vm_fetch_serializes_binary_body_without_utf8_or_json_round_trip() {
        let options = request_options("POST");
        let headers =
            parse_http_header_collection(&BTreeMap::new(), "test headers").expect("headers");
        let body = [0, 0xff, b'\r', b'\n', 0x80, b'Z'];
        let request =
            serialize_kernel_http_fetch_request(3000, "/", &options, &headers, Some(&body))
                .expect("serialize request");
        let header_end = find_http_header_end(&request).expect("request header terminator") + 4;
        assert_eq!(&request[header_end..], body);
        assert!(
            request[..header_end]
                .windows(b"Content-Length: 6\r\n".len())
                .any(|window| window == b"Content-Length: 6\r\n"),
            "request headers were {:?}",
            String::from_utf8_lossy(&request[..header_end])
        );
    }
}

struct OwnedKernelFetchSocket {
    vm: crate::state::VmHandle,
    kernel_readiness: KernelSocketReadinessRegistry,
    readiness_notify: Arc<tokio::sync::Notify>,
    kernel_pid: u32,
    socket_id: SocketId,
    capability: Option<agentos_driver_tokio::capability::CapabilityLease>,
    armed: bool,
}

impl OwnedKernelFetchSocket {
    fn identity(&self) -> Option<(u64, u64)> {
        self.capability
            .as_ref()
            .map(|capability| (capability.id(), capability.generation()))
    }

    fn unregister_readiness(&self) {
        if let Some(identity) = self.identity() {
            self.kernel_readiness.unregister(self.socket_id, identity);
        }
    }

    fn close(&mut self) -> Result<(), VmError> {
        if !self.armed {
            return Ok(());
        }
        self.unregister_readiness();
        self.vm.try_command("close owned VM fetch socket", |vm| {
            close_kernel_socket_idempotent(&mut vm.kernel, self.kernel_pid, self.socket_id)
        })?;
        self.capability.take();
        self.armed = false;
        Ok(())
    }

    fn into_stream_state(
        mut self,
        target_process_id: String,
        raw_buffer: Vec<u8>,
        body_mode: VmFetchBodyMode,
        peer_closed: bool,
        max_response_bytes: usize,
    ) -> VmFetchStreamState {
        self.unregister_readiness();
        self.armed = false;
        VmFetchStreamState {
            target_process_id,
            kernel_pid: self.kernel_pid,
            socket_id: self.socket_id,
            _capability: self
                .capability
                .take()
                .expect("armed VM fetch socket must retain its capability"),
            raw_buffer,
            decoded_buffer: VecDeque::new(),
            body_mode,
            peer_closed,
            response_bytes: 0,
            max_response_bytes,
            last_progress_at: Instant::now(),
        }
    }
}

impl Drop for OwnedKernelFetchSocket {
    fn drop(&mut self) {
        if self.armed {
            if let Err(error) = self.close() {
                eprintln!(
                    "ERR_AGENTOS_VM_FETCH_SOCKET_CLEANUP: failed to close cancelled VM fetch socket: {error}"
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn open_owned_kernel_fetch_socket(
    vm: &crate::state::VmHandle,
    target_process_id: &str,
    port: u16,
    path: &str,
    options: &JavascriptHttpRequestOptions,
    headers: &HttpHeaderCollection,
    body_bytes: Option<&[u8]>,
    enforce_stream_limit: bool,
) -> Result<OwnedKernelFetchSocket, VmError> {
    let handle = vm.clone();
    let readiness_notify = Arc::new(tokio::sync::Notify::new());
    let notify = Arc::clone(&readiness_notify);
    vm.try_command("open owned VM fetch socket", move |vm| {
        if enforce_stream_limit && vm.vm_fetch_streams.len() >= VM_FETCH_STREAM_COUNT_LIMIT {
            return Err(VmError::Execution(format!(
                "ERR_AGENTOS_VM_FETCH_STREAM_LIMIT: VM has {} open fetch streams; close or cancel a stream before opening another (limit {})",
                vm.vm_fetch_streams.len(),
                VM_FETCH_STREAM_COUNT_LIMIT
            )));
        }
        // Reject malformed methods and headers before connecting. Closing a
        // client socket cannot undo a peer socket already accepted by the VM.
        let request_bytes =
            serialize_kernel_http_fetch_request(port, path, options, headers, body_bytes)?;
        let pending_capability =
            reserve_capability(&vm.capabilities, CapabilityKind::TcpSocket)?;
        let kernel_pid = vm
            .active_processes
            .get(target_process_id)
            .ok_or_else(|| {
                VmError::InvalidState(format!(
                    "vm.fetch target process disappeared: {target_process_id}"
                ))
            })?
            .kernel_pid;
        let socket_id = vm
            .kernel
            .socket_create(EXECUTION_DRIVER_NAME, kernel_pid, SocketSpec::tcp())
            .map_err(kernel_error)?;
        let capability = match pending_capability.commit(CapabilityBackend::Kernel { socket_id }) {
            Ok(capability) => capability,
            Err(error) => {
                if let Err(cleanup) = close_kernel_socket_idempotent(&mut vm.kernel, kernel_pid, socket_id) {
                    eprintln!("ERR_AGENTOS_VM_FETCH_SOCKET_CLEANUP: capability rollback failed: {cleanup}");
                }
                return Err(VmError::Execution(error.to_string()));
            }
        };
        let setup = (|| {
            vm.kernel
                .socket_bind_inet(
                    EXECUTION_DRIVER_NAME,
                    kernel_pid,
                    socket_id,
                    InetSocketAddress::new("127.0.0.1", 0),
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
            vm.kernel
                .socket_write(EXECUTION_DRIVER_NAME, kernel_pid, socket_id, &request_bytes)
                .map_err(kernel_error)
        })();
        if let Err(error) = setup {
            let close_result =
                close_kernel_socket_idempotent(&mut vm.kernel, kernel_pid, socket_id);
            if let Err(close_error) = close_result {
                eprintln!(
                    "ERR_AGENTOS_VM_FETCH_SOCKET_CLEANUP: setup rollback failed: {close_error}"
                );
            }
            return Err(error);
        }

        let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
        let target = KernelSocketReadinessTarget {
            session: None,
            notify: Some(Arc::clone(&notify)),
            capability_id: capability.id(),
            capability_generation: capability.generation(),
            target_id: format!("vm-fetch:{target_process_id}:{socket_id}"),
            event: KernelSocketReadinessEvent::Data,
            live: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };
        if let Err(error) = kernel_readiness.register(socket_id, target) {
            let close_result =
                close_kernel_socket_idempotent(&mut vm.kernel, kernel_pid, socket_id);
            if let Err(close_error) = close_result {
                eprintln!(
                    "ERR_AGENTOS_VM_FETCH_SOCKET_CLEANUP: readiness rollback failed: {close_error}"
                );
            }
            return Err(error);
        }
        // Registration is level-triggered. Force an initial zero-time probe so
        // readiness that preceded registration cannot be lost.
        notify.notify_one();
        Ok(OwnedKernelFetchSocket {
            vm: handle,
            kernel_readiness,
            readiness_notify: notify,
            kernel_pid,
            socket_id,
            capability: Some(capability),
            armed: true,
        })
    })
}

fn poll_owned_kernel_fetch_socket(
    socket: &OwnedKernelFetchSocket,
    response_buffer: &mut Vec<u8>,
    peer_closed: &mut bool,
    label: &str,
) -> Result<bool, VmError> {
    socket.vm.try_command("poll owned VM fetch socket", |vm| {
        let poll = vm
            .kernel
            .poll_targets(
                EXECUTION_DRIVER_NAME,
                socket.kernel_pid,
                vec![PollTargetEntry::socket(
                    socket.socket_id,
                    POLLIN | POLLHUP | POLLERR,
                )],
                0,
            )
            .map_err(kernel_error)?;
        let revents = poll
            .targets
            .first()
            .map(|entry| entry.revents)
            .unwrap_or_else(PollEvents::empty);
        if revents.intersects(POLLERR) {
            return Err(VmError::Execution(String::from(
                "ERR_AGENTOS_VM_FETCH_SOCKET: kernel TCP socket reported POLLERR",
            )));
        }
        let before = response_buffer.len();
        if revents.intersects(POLLIN) {
            loop {
                match vm.kernel.socket_read(
                    EXECUTION_DRIVER_NAME,
                    socket.kernel_pid,
                    socket.socket_id,
                    VM_FETCH_STREAM_CHUNK_MAX_BYTES,
                ) {
                    Ok(Some(bytes)) if !bytes.is_empty() => {
                        response_buffer.extend(bytes);
                        ensure_vm_fetch_raw_response_buffer_within_limit(
                            response_buffer.len(),
                            label,
                        )
                        .map_err(sidecar_core_execution_error)?;
                    }
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        *peer_closed = true;
                        break;
                    }
                    Err(error) if error.code() == "EAGAIN" => break,
                    Err(error) => return Err(kernel_error(error)),
                }
            }
        }
        if revents.intersects(POLLHUP) {
            *peer_closed = true;
        }
        Ok(response_buffer.len() != before || *peer_closed)
    })
}

fn owned_fetch_process_notify(
    vm: &crate::state::VmHandle,
    process_id: &str,
) -> Result<Arc<tokio::sync::Notify>, VmError> {
    vm.try_read("clone VM fetch process notification", |vm| {
        vm.active_processes
            .get(process_id)
            .map(|process| Arc::clone(&process.process_event_notify))
            .ok_or_else(|| {
                VmError::InvalidState(format!("vm.fetch target process disappeared: {process_id}"))
            })
    })?
}

async fn wait_for_owned_fetch_progress(
    readiness_notify: &Arc<tokio::sync::Notify>,
    process_notify: &Arc<tokio::sync::Notify>,
    deadline: Instant,
) -> Result<(), VmError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(VmError::Execution(String::from(
            "ERR_AGENTOS_VM_FETCH_TIMEOUT: VM fetch progress deadline elapsed",
        )));
    }
    tokio::time::timeout(remaining, async {
        tokio::select! {
            _ = readiness_notify.notified() => {},
            _ = tokio::time::sleep(Duration::from_millis(1)) => {
                // The ordinary dispatcher exclusively owns guest execution
                // events. Wake it so runnable microtask or WASM work advances;
                // this fetch path only owns the kernel response socket.
                process_notify.notify_one();
                tokio::task::yield_now().await;
            },
        }
    })
    .await
    .map_err(|_| {
        VmError::Execution(format!(
            "ERR_AGENTOS_VM_FETCH_TIMEOUT: timed out after {} ms waiting for VM fetch progress",
            http_loopback_request_timeout().as_millis()
        ))
    })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_owned_kernel_http_fetch(
    vm: &crate::state::VmHandle,
    target_process_id: &str,
    port: u16,
    path: &str,
    options: &JavascriptHttpRequestOptions,
    headers: &HttpHeaderCollection,
    body_bytes: Option<&[u8]>,
    max_fetch_response_bytes: usize,
) -> Result<String, VmError> {
    let mut socket = open_owned_kernel_fetch_socket(
        vm,
        target_process_id,
        port,
        path,
        options,
        headers,
        body_bytes,
        false,
    )?;
    let mut response_buffer = Vec::new();
    let mut peer_closed = false;
    let url = format!("http://127.0.0.1:{port}{path}");
    let deadline = Instant::now() + http_loopback_request_timeout();
    let process_notify = owned_fetch_process_notify(vm, target_process_id)?;
    loop {
        if let Some(response) =
            parse_kernel_http_fetch_response(&response_buffer, peer_closed, &url)
                .map_err(sidecar_core_execution_error)?
        {
            ensure_vm_fetch_response_within_limit(&response, "vm.fetch", max_fetch_response_bytes)
                .map_err(sidecar_core_execution_error)?;
            socket.close()?;
            return Ok(response);
        }
        if Instant::now() >= deadline {
            let preview = String::from_utf8_lossy(&response_buffer);
            return Err(VmError::Execution(format!(
                "ERR_AGENTOS_VM_FETCH_TIMEOUT: vm.fetch timed out waiting for kernel TCP HTTP response ({} buffered bytes: {:?}); raise AGENTOS_HTTP_LOOPBACK_REQUEST_TIMEOUT_MS",
                response_buffer.len(),
                preview.chars().take(200).collect::<String>()
            )));
        }
        let progressed = poll_owned_kernel_fetch_socket(
            &socket,
            &mut response_buffer,
            &mut peer_closed,
            "vm.fetch",
        )?;
        if !progressed {
            wait_for_owned_fetch_progress(&socket.readiness_notify, &process_notify, deadline)
                .await?;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn start_owned_kernel_http_fetch_stream(
    vm: &crate::state::VmHandle,
    target_process_id: &str,
    port: u16,
    path: &str,
    options: &JavascriptHttpRequestOptions,
    headers: &HttpHeaderCollection,
    body_bytes: Option<&[u8]>,
    max_response_bytes: usize,
) -> Result<String, VmError> {
    let socket = open_owned_kernel_fetch_socket(
        vm,
        target_process_id,
        port,
        path,
        options,
        headers,
        body_bytes,
        true,
    )?;
    let mut response_buffer = Vec::new();
    let mut peer_closed = false;
    let deadline = Instant::now() + http_loopback_request_timeout();
    let request_method = options.method.as_deref().unwrap_or("GET");
    let process_notify = owned_fetch_process_notify(vm, target_process_id)?;
    let (status, status_text, response_headers, body_mode) = loop {
        if let Some(header_end) = find_http_header_end(&response_buffer) {
            let parsed = parse_stream_response_head(
                &response_buffer[..header_end],
                request_method,
                max_response_bytes,
            )?;
            if (100..200).contains(&parsed.0) && parsed.0 != 101 {
                response_buffer.drain(..header_end + 4);
                continue;
            }
            response_buffer.drain(..header_end + 4);
            break parsed;
        }
        if peer_closed {
            return Err(VmError::Execution(String::from(
                "ERR_AGENTOS_VM_FETCH_TRUNCATED: peer closed before response headers completed",
            )));
        }
        if Instant::now() >= deadline {
            return Err(VmError::Execution(format!(
                "ERR_AGENTOS_VM_FETCH_TIMEOUT: timed out waiting for response headers after {} ms; raise AGENTOS_HTTP_LOOPBACK_REQUEST_TIMEOUT_MS",
                http_loopback_request_timeout().as_millis()
            )));
        }
        let progressed = poll_owned_kernel_fetch_socket(
            &socket,
            &mut response_buffer,
            &mut peer_closed,
            "vm.fetchStream",
        )?;
        if !progressed {
            wait_for_owned_fetch_progress(&socket.readiness_notify, &process_notify, deadline)
                .await?;
        }
    };

    let mut state = socket.into_stream_state(
        target_process_id.to_owned(),
        response_buffer,
        body_mode,
        peer_closed,
        max_response_bytes,
    );
    decode_stream_body(&mut state)?;
    let stream_id = vm.try_command("register owned VM fetch stream", |vm| {
        vm.next_vm_fetch_stream_id = vm.next_vm_fetch_stream_id.wrapping_add(1);
        let stream_id = format!("{}:{}", vm.generation, vm.next_vm_fetch_stream_id);
        vm.vm_fetch_streams.insert(stream_id.clone(), state);
        Ok(stream_id)
    })?;
    serde_json::to_string(&json!({
        "streamId": stream_id,
        "status": status,
        "statusText": status_text,
        "headers": response_headers,
    }))
    .map_err(|error| {
        VmError::Execution(format!(
            "ERR_AGENTOS_VM_FETCH_SERIALIZE: failed to serialize response head: {error}"
        ))
    })
}

struct OwnedFetchStreamLease {
    vm: crate::state::VmHandle,
    stream_id: String,
    kernel_readiness: KernelSocketReadinessRegistry,
    readiness_notify: Arc<tokio::sync::Notify>,
    state: Option<VmFetchStreamState>,
}

impl OwnedFetchStreamLease {
    fn state(&self) -> &VmFetchStreamState {
        self.state
            .as_ref()
            .expect("owned VM fetch stream state must be present")
    }

    fn state_mut(&mut self) -> &mut VmFetchStreamState {
        self.state
            .as_mut()
            .expect("owned VM fetch stream state must be present")
    }

    fn identity(&self) -> (u64, u64) {
        let capability = &self.state()._capability;
        (capability.id(), capability.generation())
    }

    fn unregister_readiness(&self) {
        self.kernel_readiness
            .unregister(self.state().socket_id, self.identity());
    }

    fn reinsert(mut self) -> Result<(), VmError> {
        self.unregister_readiness();
        let state = self
            .state
            .take()
            .expect("owned VM fetch stream state must be present");
        self.vm.try_command("restore owned VM fetch stream", |vm| {
            vm.vm_fetch_streams.insert(self.stream_id.clone(), state);
            Ok(())
        })
    }

    fn close(mut self) -> Result<(), VmError> {
        self.unregister_readiness();
        let state = self
            .state
            .take()
            .expect("owned VM fetch stream state must be present");
        self.vm.try_command("close owned VM fetch stream", |vm| {
            close_kernel_socket_idempotent(&mut vm.kernel, state.kernel_pid, state.socket_id)
        })?;
        drop(state);
        Ok(())
    }
}

impl Drop for OwnedFetchStreamLease {
    fn drop(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };
        self.kernel_readiness.unregister(
            state.socket_id,
            (state._capability.id(), state._capability.generation()),
        );
        let result = self.vm.try_command("cancel owned VM fetch stream", |vm| {
            close_kernel_socket_idempotent(&mut vm.kernel, state.kernel_pid, state.socket_id)
        });
        if let Err(error) = result {
            eprintln!(
                "ERR_AGENTOS_VM_FETCH_STREAM_CLEANUP: failed to close cancelled stream {}: {error}",
                self.stream_id
            );
        }
        drop(state);
    }
}

fn lease_owned_fetch_stream(
    vm: &crate::state::VmHandle,
    stream_id: &str,
) -> Result<OwnedFetchStreamLease, VmError> {
    let readiness_notify = Arc::new(tokio::sync::Notify::new());
    let notify = Arc::clone(&readiness_notify);
    let handle = vm.clone();
    vm.try_command("lease owned VM fetch stream", |vm| {
        let state = vm.vm_fetch_streams.remove(stream_id).ok_or_else(|| {
            VmError::InvalidState(format!(
                "ERR_AGENTOS_VM_FETCH_STREAM_NOT_FOUND: stream {stream_id:?} is closed or unknown"
            ))
        })?;
        let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
        let target = KernelSocketReadinessTarget {
            session: None,
            notify: Some(Arc::clone(&notify)),
            capability_id: state._capability.id(),
            capability_generation: state._capability.generation(),
            target_id: format!("vm-fetch-stream:{stream_id}"),
            event: KernelSocketReadinessEvent::Data,
            live: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };
        if let Err(error) = kernel_readiness.register(state.socket_id, target) {
            vm.vm_fetch_streams.insert(stream_id.to_owned(), state);
            return Err(error);
        }
        notify.notify_one();
        Ok(OwnedFetchStreamLease {
            vm: handle,
            stream_id: stream_id.to_owned(),
            kernel_readiness,
            readiness_notify: notify,
            state: Some(state),
        })
    })
}

async fn read_owned_kernel_http_fetch_stream(
    vm: &crate::state::VmHandle,
    stream_id: &str,
    requested_max_bytes: usize,
) -> Result<String, VmError> {
    let max_bytes = requested_max_bytes.clamp(1, VM_FETCH_STREAM_CHUNK_MAX_BYTES);
    let mut lease = lease_owned_fetch_stream(vm, stream_id)?;
    let target_process_id = lease.state().target_process_id.clone();
    let process_notify = owned_fetch_process_notify(vm, &target_process_id)?;
    loop {
        decode_stream_body(lease.state_mut())?;
        if !lease.state().decoded_buffer.is_empty()
            || matches!(lease.state().body_mode, VmFetchBodyMode::Empty)
        {
            break;
        }
        let deadline = lease.state().last_progress_at + http_loopback_request_timeout();
        if Instant::now() >= deadline {
            return Err(VmError::Execution(format!(
                "ERR_AGENTOS_VM_FETCH_TIMEOUT: stream produced no data for {} ms; raise AGENTOS_HTTP_LOOPBACK_REQUEST_TIMEOUT_MS",
                http_loopback_request_timeout().as_millis()
            )));
        }
        let (kernel_pid, socket_id) = (lease.state().kernel_pid, lease.state().socket_id);
        let mut raw_buffer = std::mem::take(&mut lease.state_mut().raw_buffer);
        let mut peer_closed = lease.state().peer_closed;
        let progressed = lease.vm.try_command("poll owned VM fetch stream", |vm| {
            let poll = vm
                .kernel
                .poll_targets(
                    EXECUTION_DRIVER_NAME,
                    kernel_pid,
                    vec![PollTargetEntry::socket(
                        socket_id,
                        POLLIN | POLLHUP | POLLERR,
                    )],
                    0,
                )
                .map_err(kernel_error)?;
            let revents = poll
                .targets
                .first()
                .map(|entry| entry.revents)
                .unwrap_or_else(PollEvents::empty);
            if revents.intersects(POLLERR) {
                return Err(VmError::Execution(String::from(
                    "ERR_AGENTOS_VM_FETCH_SOCKET: kernel TCP stream reported POLLERR",
                )));
            }
            let before = raw_buffer.len();
            if revents.intersects(POLLIN) {
                loop {
                    match vm.kernel.socket_read(
                        EXECUTION_DRIVER_NAME,
                        kernel_pid,
                        socket_id,
                        VM_FETCH_STREAM_CHUNK_MAX_BYTES,
                    ) {
                        Ok(Some(bytes)) if !bytes.is_empty() => {
                            raw_buffer.extend(bytes);
                            ensure_vm_fetch_raw_response_buffer_within_limit(
                                raw_buffer.len(),
                                "vm.fetchStream",
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
            Ok(raw_buffer.len() != before || peer_closed)
        })?;
        lease.state_mut().raw_buffer = raw_buffer;
        lease.state_mut().peer_closed = peer_closed;
        if progressed {
            lease.state_mut().last_progress_at = Instant::now();
        } else {
            wait_for_owned_fetch_progress(&lease.readiness_notify, &process_notify, deadline)
                .await?;
        }
    }

    let state = lease.state_mut();
    let take = max_bytes.min(state.decoded_buffer.len());
    let body: Vec<u8> = state.decoded_buffer.drain(..take).collect();
    let done = state.decoded_buffer.is_empty() && matches!(state.body_mode, VmFetchBodyMode::Empty);
    let response = serde_json::to_string(&json!({
        "body": base64::engine::general_purpose::STANDARD.encode(body),
        "done": done,
    }))
    .map_err(|error| {
        VmError::Execution(format!(
            "ERR_AGENTOS_VM_FETCH_SERIALIZE: failed to serialize stream chunk: {error}"
        ))
    })?;
    if done {
        lease.close()?;
    } else {
        lease.reinsert()?;
    }
    Ok(response)
}

fn cancel_owned_kernel_http_fetch_stream(
    vm: &crate::state::VmHandle,
    stream_id: &str,
) -> Result<String, VmError> {
    let lease = lease_owned_fetch_stream(vm, stream_id)?;
    lease.close()?;
    Ok(String::from("{\"cancelled\":true}"))
}

struct OwnedLoopbackHttpRequest {
    vm: crate::state::VmHandle,
    process_id: String,
    request_key: (u64, u64),
    armed: bool,
}

impl OwnedLoopbackHttpRequest {
    fn complete(&mut self) {
        self.armed = false;
    }
}

impl Drop for OwnedLoopbackHttpRequest {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let result = self
            .vm
            .try_command("cancel owned loopback HTTP request", |vm| {
                if let Some(process) = vm.active_processes.get_mut(&self.process_id) {
                    process.pending_http_requests.remove(&self.request_key);
                }
                Ok(())
            });
        if let Err(error) = result {
            eprintln!(
                "ERR_AGENTOS_VM_FETCH_LOOPBACK_CLEANUP: failed to cancel pending request: {error}"
            );
        }
    }
}

async fn dispatch_owned_loopback_http_request(
    vm: &crate::state::VmHandle,
    process_id: &str,
    server_id: u64,
    request_json: &str,
) -> Result<String, VmError> {
    let (request_key, receiver) = vm.try_command("start owned loopback HTTP request", |vm| {
        let process = vm.active_processes.get_mut(process_id).ok_or_else(|| {
            VmError::InvalidState(format!("vm.fetch target process disappeared: {process_id}"))
        })?;
        let (respond_to, receiver) = tokio::sync::oneshot::channel();
        let request_key = begin_loopback_http_request(process, server_id, request_json, || {
            PendingHttpRequest::Deferred(respond_to)
        })?;
        Ok((request_key, receiver))
    })?;
    let mut guard = OwnedLoopbackHttpRequest {
        vm: vm.clone(),
        process_id: process_id.to_owned(),
        request_key,
        armed: true,
    };
    let response = tokio::time::timeout(http_loopback_request_timeout(), receiver)
        .await
        .map_err(|_| {
            VmError::Execution(String::from(
                "HTTP loopback request timed out waiting for net.http_respond",
            ))
        })?
        .map_err(|_| {
            VmError::InvalidState(String::from(
                "HTTP loopback response waiter closed before net.http_respond",
            ))
        })?
        .map_err(|error| VmError::Execution(format!("{}: {}", error.code, error.message)))?;
    guard.complete();
    response.as_str().map(str::to_owned).ok_or_else(|| {
        VmError::InvalidState(String::from(
            "HTTP loopback response completed with a non-string value",
        ))
    })
}

/// Run `vm.fetch` without holding either the process coordinator or a VM state
/// borrow over readiness and adapter-response waits.
pub(in crate::execution) async fn dispatch_owned_vm_fetch(
    vm_id: &str,
    vm: crate::state::VmHandle,
    payload: VmFetchRequest,
) -> Result<String, VmError> {
    let stream_operation = payload.stream_operation.as_deref();
    if matches!(stream_operation, Some("read" | "cancel")) {
        let stream_id = payload.stream_id.as_deref().ok_or_else(|| {
            VmError::InvalidState(String::from(
                "vm.fetch stream read/cancel requires stream_id",
            ))
        })?;
        return if stream_operation == Some("read") {
            read_owned_kernel_http_fetch_stream(
                &vm,
                stream_id,
                payload.max_bytes.unwrap_or(64 * 1024) as usize,
            )
            .await
        } else {
            cancel_owned_kernel_http_fetch_stream(&vm, stream_id)
        };
    }
    if let Some(operation) = stream_operation {
        if operation != "start" {
            return Err(VmError::InvalidState(format!(
                "unknown vm.fetch stream operation {operation:?}; expected start, read, or cancel"
            )));
        }
    }

    let target_path = format!("/{}", payload.path.trim_start_matches('/'));
    let request_url = Url::parse(&format!("http://127.0.0.1:{}{target_path}", payload.port))
        .map_err(|error| {
            VmError::InvalidState(format!("invalid vm.fetch target {target_path:?}: {error}"))
        })?;
    let header_values: BTreeMap<String, Value> = serde_json::from_str(&payload.headers_json)
        .map_err(|error| {
            VmError::InvalidState(format!("vm.fetch headers_json must be valid JSON: {error}"))
        })?;
    if payload.body.is_some() && payload.body_base64.is_some() {
        return Err(VmError::InvalidState(String::from(
            "vm.fetch accepts either body or body_base64, not both",
        )));
    }
    let body_bytes = payload
        .body_base64
        .as_deref()
        .map(|body| {
            base64::engine::general_purpose::STANDARD
                .decode(body)
                .map_err(|error| {
                    VmError::InvalidState(format!(
                        "vm.fetch body_base64 must be valid base64: {error}"
                    ))
                })
        })
        .transpose()?;
    let options = JavascriptHttpRequestOptions {
        method: Some(payload.method),
        headers: header_values,
        body: payload.body,
        reject_unauthorized: None,
    };
    let headers = parse_http_header_collection(&options.headers, "vm.fetch headers")?;
    let (kernel_target, max_fetch_response_bytes) =
        vm.try_read("resolve VM fetch target", |vm| {
            (
                find_kernel_http_listener_process(vm, payload.port),
                vm.limits.http.max_fetch_response_bytes,
            )
        })?;
    if let Some(target_process_id) = kernel_target {
        return if stream_operation == Some("start") {
            start_owned_kernel_http_fetch_stream(
                &vm,
                &target_process_id,
                payload.port,
                &target_path,
                &options,
                &headers,
                body_bytes.as_deref(),
                max_fetch_response_bytes,
            )
            .await
        } else {
            dispatch_owned_kernel_http_fetch(
                &vm,
                &target_process_id,
                payload.port,
                &target_path,
                &options,
                &headers,
                body_bytes.as_deref(),
                max_fetch_response_bytes,
            )
            .await
        };
    }

    let target = vm.try_read("resolve loopback VM fetch target", |vm| {
        vm.active_processes
            .iter()
            .find_map(|(process_id, process)| {
                process
                    .http_servers
                    .iter()
                    .find(|(_, server)| server.guest_local_addr.port() == payload.port)
                    .map(|(server_id, _)| (process_id.clone(), *server_id))
            })
    })?;
    let Some((target_process_id, server_id)) = target else {
        return Err(VmError::Execution(format!(
            "vm.fetch could not find a guest HTTP listener on port {} in VM {vm_id}",
            payload.port
        )));
    };
    if stream_operation == Some("start") {
        return Err(VmError::InvalidState(String::from(
            "vm.fetch streaming requires a kernel-backed HTTP listener",
        )));
    }
    if body_bytes.is_some() {
        return Err(VmError::InvalidState(String::from(
            "binary vm.fetch bodies require a kernel-backed HTTP listener",
        )));
    }
    let request_json = serialize_http_loopback_request(&request_url, &options, &headers)?;
    dispatch_owned_loopback_http_request(&vm, &target_process_id, server_id, &request_json).await
}
