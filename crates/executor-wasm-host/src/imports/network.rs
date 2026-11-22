//! agentOS host-network ABI codecs.
//!
//! The sidecar kernel owns socket descriptions and guest fd allocation. These
//! codecs translate only the owned libc wire format; they do not project or
//! synchronize a second executor-local socket table.

use super::preview1::{
    call, commit, errno, json_reply, reply_bytes, simple_call, value_u64, ERRNO_2BIG, ERRNO_FAULT,
    ERRNO_INVAL, ERRNO_IO, SUCCESS,
};
use super::{i32_arg, set_i32_result};
use crate::abi::{AbiBinding, ImportId};
use crate::backend::HostCallReply;
use crate::memory;
use crate::{Val, WasmGuest};
use base64::Engine as _;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Instant;

const ERRNO_AGAIN: i32 = 6;
const ERRNO_BADF: i32 = 8;
const ERRNO_NOBUFS: i32 = 42;
const ERRNO_NOTSOCK: i32 = 57;
const ERRNO_NOTSUP: i32 = 58;
const ERRNO_MSGSIZE: i32 = 35;
const ERRNO_NOPROTOOPT: i32 = 50;
const MSG_PEEK: u32 = 0x2;
const MSG_CTRUNC: u32 = 0x8;
const MSG_WAITALL: u32 = 0x100;
const MSG_CMSG_CLOEXEC: u32 = 0x4000_0000;
const MAX_SOCKET_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
/// Linux socket domains as the kernel reports them.
const LINUX_AF_UNIX: i64 = 1;
const LINUX_AF_INET: i64 = 2;
const LINUX_AF_INET6: i64 = 10;
const LINUX_SOCK_DGRAM: i64 = 2;
/// Address families in the owned wasm32 libc ABI.
const WASI_AF_INET: u16 = 1;
const WASI_AF_INET6: u16 = 2;
const ERRNO_TIMEDOUT: i32 = 73;
const MAX_POLL_FDS: usize = 1024;
const MAX_DNS_RECORDS: usize = 4096;
const MAX_DNS_PAYLOAD: usize = 64 * 1024;
const SOCK_TYPE_MASK: u32 = 0xf;
const SOCK_DGRAM: u32 = 5;
const SOCK_STREAM: u32 = 6;
const SOCK_CLOEXEC: u32 = 0x2000;
const SOCK_NONBLOCK: u32 = 0x4000;
const KERNEL_O_NONBLOCK: u32 = 0x800;
const MSG_DONTWAIT: u32 = 0x40;
const MSG_TRUNC: u32 = 0x20;
const POLLIN: u32 = 0x001;
const POLLOUT: u32 = 0x004;
const POLLRDNORM: u32 = 0x040;
const POLLWRNORM: u32 = 0x100;

pub async fn dispatch<C: WasmGuest>(
    caller: &mut C,
    abi: AbiBinding,
    params: &[Val],
    results: &mut [Val],
) -> crate::Result<bool> {
    use ImportId::*;
    let status = match abi.id {
        HostNetNetSocket => socket(caller, params).await,
        HostNetNetSetNonblock => set_nonblock(caller, params).await,
        HostNetNetConnect => address_call(caller, params, "process.hostnet_connect").await,
        HostNetNetBind => address_call(caller, params, "process.hostnet_bind").await,
        HostNetNetGetaddrinfo => getaddrinfo(caller, params).await,
        HostNetNetDnsQueryRrV1 => dns_query(caller, params).await,
        HostNetNetListen => listen(caller, params).await,
        HostNetNetAccept => accept(caller, params).await,
        HostNetNetValidateSocket => validate(caller, params, false).await,
        HostNetNetValidateAccept => validate(caller, params, true).await,
        HostNetNetGetsockname => address_output(caller, params, false).await,
        HostNetNetGetpeername => address_output(caller, params, true).await,
        HostNetNetSend => send(caller, params, false).await,
        HostNetNetSendto => send(caller, params, true).await,
        HostNetNetRecv => receive(caller, params, false).await,
        HostNetNetRecvfrom => receive(caller, params, true).await,
        HostNetNetRecvmsg => receive_message(caller, params).await,
        HostNetNetSetsockopt => set_option(caller, params).await,
        HostNetNetGetsockopt => get_option(caller, params).await,
        HostNetNetClose => close(caller, params).await,
        HostNetNetTlsConnect => tls_connect(caller, params).await,
        HostNetNetPoll => poll(caller, params).await,
        _ => return Ok(false),
    };
    set_i32_result(results, status)?;
    Ok(true)
}

async fn socket<C: WasmGuest>(caller: &mut C, params: &[Val]) -> i32 {
    let (Ok(domain), Ok(socket_type), Ok(_protocol), Ok(output)) = (
        i32_arg(params, 0),
        i32_arg(params, 1),
        i32_arg(params, 2),
        i32_arg(params, 3),
    ) else {
        return ERRNO_INVAL;
    };
    if memory::validate_range(caller, output, 4).is_err() {
        return ERRNO_FAULT;
    }
    let kind = socket_type & SOCK_TYPE_MASK;
    if !matches!(kind, SOCK_DGRAM | SOCK_STREAM) {
        return ERRNO_NOTSUP;
    }
    if domain == 3 && kind != SOCK_STREAM {
        return ERRNO_NOTSUP;
    }
    match call(
        caller,
        "process.hostnet_fd_open",
        vec![
            json!(domain),
            json!(kind),
            json!(socket_type & SOCK_NONBLOCK != 0),
            json!(socket_type & SOCK_CLOEXEC != 0),
        ],
        HashMap::new(),
    )
    .await
    {
        Ok(reply) => {
            let Ok(value) = json_reply(reply) else {
                return ERRNO_IO;
            };
            let Some(fd) = value
                .get("fd")
                .and_then(value_u64)
                .and_then(|value| u32::try_from(value).ok())
            else {
                return ERRNO_IO;
            };
            if commit(caller, output, &fd.to_le_bytes()) == SUCCESS {
                SUCCESS
            } else {
                log_fd_rollback(caller, fd, "socket output commit").await;
                ERRNO_FAULT
            }
        }
        Err(error) => errno(&error),
    }
}

async fn set_nonblock<C: WasmGuest>(caller: &mut C, params: &[Val]) -> i32 {
    let (Ok(fd), Ok(enable)) = (i32_arg(params, 0), i32_arg(params, 1)) else {
        return ERRNO_INVAL;
    };
    let flags = match call(caller, "process.fd_stat", vec![json!(fd)], HashMap::new()).await {
        Ok(reply) => match json_reply(reply)
            .ok()
            .and_then(|value| value.get("flags").and_then(value_u64))
            .and_then(|value| u32::try_from(value).ok())
        {
            Some(value) => value,
            None => return ERRNO_IO,
        },
        Err(error) => return errno(&error),
    };
    let flags = if enable != 0 {
        flags | KERNEL_O_NONBLOCK
    } else {
        flags & !KERNEL_O_NONBLOCK
    };
    simple_call(
        caller,
        "process.fd_set_flags",
        vec![json!(fd), json!(flags)],
    )
    .await
}

async fn address_call<C: WasmGuest>(caller: &mut C, params: &[Val], method: &str) -> i32 {
    let (Ok(fd), Ok(pointer), Ok(length)) =
        (i32_arg(params, 0), i32_arg(params, 1), i32_arg(params, 2))
    else {
        return ERRNO_INVAL;
    };
    let Ok(mut text) = memory::read_string(caller, pointer, length as usize) else {
        return ERRNO_FAULT;
    };
    if let Some(index) = text.find('\0') {
        text.truncate(index);
    }
    let Ok(address) = decode_address(&text) else {
        return ERRNO_INVAL;
    };
    let args = if method.ends_with("connect") {
        vec![json!(fd), address, Value::Null]
    } else {
        vec![json!(fd), address]
    };
    simple_call(caller, method, args).await
}

fn decode_address(text: &str) -> Result<Value, ()> {
    if text == "unspecified" {
        return Ok(json!({"type": "unspecified"}));
    }
    if text == "unix-autobind" {
        return Ok(json!({"type": "unix-autobind"}));
    }
    if let Some(hex) = text.strip_prefix("unix-abstract:") {
        if hex.len().is_multiple_of(2) && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(json!({"type": "unix-abstract", "hex": hex.to_ascii_lowercase()}));
        }
        return Err(());
    }
    if let Some(hex) = text.strip_prefix("unix-path-hex:") {
        let bytes = decode_hex(hex)?;
        let path = String::from_utf8(bytes).map_err(|_| ())?;
        return Ok(json!({"type": "unix-path", "path": path}));
    }
    if let Some(path) = text.strip_prefix("unix:") {
        return Ok(json!({"type": "unix-path", "path": path}));
    }
    let (host, port) = if let Some(rest) = text.strip_prefix('[') {
        let (host, port) = rest.split_once("]:").ok_or(())?;
        (host, port)
    } else {
        text.rsplit_once(':').ok_or(())?
    };
    if host.is_empty() {
        return Err(());
    }
    let port = port.parse::<u16>().map_err(|_| ())?;
    Ok(json!({"type": "inet", "host": host, "port": port}))
}

fn decode_hex(text: &str) -> Result<Vec<u8>, ()> {
    if !text.len().is_multiple_of(2) {
        return Err(());
    }
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let pair = std::str::from_utf8(pair).map_err(|_| ())?;
            u8::from_str_radix(pair, 16).map_err(|_| ())
        })
        .collect()
}

async fn getaddrinfo<C: WasmGuest>(caller: &mut C, params: &[Val]) -> i32 {
    let (Ok(host_pointer), Ok(host_length), Ok(family), Ok(output), Ok(length_output)) = (
        i32_arg(params, 0),
        i32_arg(params, 1),
        i32_arg(params, 4),
        i32_arg(params, 5),
        i32_arg(params, 6),
    ) else {
        return ERRNO_FAULT;
    };
    let Ok(hostname) = memory::read_string(caller, host_pointer, host_length as usize) else {
        return ERRNO_FAULT;
    };
    let Ok(capacity) = memory::read_u32(caller, length_output) else {
        return ERRNO_FAULT;
    };
    if memory::validate_range(caller, output, capacity as usize).is_err()
        || memory::validate_range(caller, length_output, 4).is_err()
    {
        return ERRNO_FAULT;
    }
    if !matches!(family, 0 | 4 | 6) {
        return ERRNO_INVAL;
    }
    let mut options = serde_json::Map::from_iter([
        ("hostname".to_owned(), json!(hostname)),
        ("all".to_owned(), json!(true)),
    ]);
    if family != 0 {
        options.insert("family".to_owned(), json!(family));
    }
    match call(
        caller,
        "dns.lookup",
        vec![Value::Object(options)],
        HashMap::new(),
    )
    .await
    {
        Ok(reply) => {
            let Ok(records) = json_reply(reply) else {
                return ERRNO_IO;
            };
            let Some(records) = records.as_array() else {
                return ERRNO_IO;
            };
            let mut normalized = Vec::with_capacity(records.len());
            for record in records {
                let Some(family) = record.get("family").and_then(value_u64) else {
                    return ERRNO_IO;
                };
                let Some(address) = record.get("address").and_then(Value::as_str) else {
                    return ERRNO_IO;
                };
                if !matches!(family, 4 | 6) {
                    return ERRNO_IO;
                }
                normalized.push(json!({"addr": address, "family": family}));
            }
            let Ok(bytes) = serde_json::to_vec(&normalized) else {
                return ERRNO_IO;
            };
            publish_bytes(caller, output, capacity, length_output, &bytes, false)
        }
        Err(error) => errno(&error),
    }
}

async fn dns_query<C: WasmGuest>(caller: &mut C, params: &[Val]) -> i32 {
    let (
        Ok(name_pointer),
        Ok(name_length),
        Ok(record_type),
        Ok(output),
        Ok(capacity),
        Ok(length_output),
        Ok(ttl_output),
        Ok(flags_output),
    ) = (
        i32_arg(params, 0),
        i32_arg(params, 1),
        i32_arg(params, 2),
        i32_arg(params, 3),
        i32_arg(params, 4),
        i32_arg(params, 5),
        i32_arg(params, 6),
        i32_arg(params, 7),
    )
    else {
        return ERRNO_FAULT;
    };
    let Ok(name) = memory::read_string(caller, name_pointer, name_length as usize) else {
        return ERRNO_FAULT;
    };
    if memory::validate_range(caller, output, capacity as usize).is_err()
        || [length_output, ttl_output, flags_output]
            .into_iter()
            .any(|pointer| memory::validate_range(caller, pointer, 4).is_err())
    {
        return ERRNO_FAULT;
    }
    let requested = match record_type {
        12 => "PTR",
        44 => "SSHFP",
        _ => return ERRNO_NOTSUP,
    };
    match call(
        caller,
        "dns.resolveRawRr",
        vec![json!({"hostname": name, "rrtype": requested})],
        HashMap::new(),
    )
    .await
    {
        Ok(reply) => {
            let Ok(value) = json_reply(reply) else {
                return ERRNO_IO;
            };
            let Some(status) = value.get("status").and_then(Value::as_str) else {
                return ERRNO_IO;
            };
            if !matches!(status, "ok" | "nxdomain" | "nodata") {
                return ERRNO_IO;
            }
            let Some(records) = value.get("records").and_then(Value::as_array) else {
                return ERRNO_IO;
            };
            if records.len() > MAX_DNS_RECORDS {
                return ERRNO_NOBUFS;
            }
            let mut payload = Vec::from((records.len() as u32).to_le_bytes());
            let mut ttl: Option<u32> = None;
            for record in records {
                let Some(encoded) = record.get("data").and_then(Value::as_str) else {
                    return ERRNO_IO;
                };
                let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
                    return ERRNO_IO;
                };
                if (requested == "PTR" && bytes.is_empty())
                    || (requested == "SSHFP" && bytes.len() < 2)
                {
                    return ERRNO_IO;
                }
                let Some(record_ttl) = record
                    .get("ttl")
                    .and_then(value_u64)
                    .and_then(|value| u32::try_from(value).ok())
                else {
                    return ERRNO_IO;
                };
                ttl = Some(ttl.map_or(record_ttl, |prior| prior.min(record_ttl)));
                let Ok(length) = u32::try_from(bytes.len()) else {
                    return ERRNO_NOBUFS;
                };
                payload.extend_from_slice(&length.to_le_bytes());
                payload.extend_from_slice(&bytes);
                if payload.len() > MAX_DNS_PAYLOAD {
                    return ERRNO_NOBUFS;
                }
            }
            if commit(caller, length_output, &(payload.len() as u32).to_le_bytes()) != SUCCESS {
                return ERRNO_FAULT;
            }
            if payload.len() > capacity as usize {
                return ERRNO_NOBUFS;
            }
            if commit(caller, output, &payload) != SUCCESS
                || commit(caller, ttl_output, &ttl.unwrap_or(0).to_le_bytes()) != SUCCESS
                || commit(
                    caller,
                    flags_output,
                    &(if status == "nxdomain" {
                        2u32
                    } else if status == "nodata" {
                        4
                    } else {
                        0
                    })
                    .to_le_bytes(),
                ) != SUCCESS
            {
                ERRNO_FAULT
            } else {
                SUCCESS
            }
        }
        Err(error) => errno(&error),
    }
}

async fn listen<C: WasmGuest>(caller: &mut C, params: &[Val]) -> i32 {
    let (Ok(fd), Ok(backlog)) = (i32_arg(params, 0), i32_arg(params, 1)) else {
        return ERRNO_INVAL;
    };
    simple_call(
        caller,
        "process.hostnet_listen",
        vec![json!(fd), json!(backlog)],
    )
    .await
}

async fn validate<C: WasmGuest>(caller: &mut C, params: &[Val], listening: bool) -> i32 {
    let Ok(fd) = i32_arg(params, 0) else {
        return ERRNO_BADF;
    };
    simple_call(
        caller,
        "process.hostnet_validate",
        vec![json!(fd), json!(listening)],
    )
    .await
}

async fn accept<C: WasmGuest>(caller: &mut C, params: &[Val]) -> i32 {
    let (Ok(fd), Ok(fd_output), Ok(address_output), Ok(length_output)) = (
        i32_arg(params, 0),
        i32_arg(params, 1),
        i32_arg(params, 2),
        i32_arg(params, 3),
    ) else {
        return ERRNO_FAULT;
    };
    let Ok(capacity) = memory::read_u32(caller, length_output) else {
        return ERRNO_FAULT;
    };
    if memory::validate_range(caller, fd_output, 4).is_err()
        || memory::validate_range(caller, address_output, capacity as usize).is_err()
        || memory::validate_range(caller, length_output, 4).is_err()
    {
        return ERRNO_FAULT;
    }
    let nonblocking = match fd_is_nonblocking(caller, fd).await {
        Ok(nonblocking) => nonblocking,
        Err(error) => return error,
    };
    let started = Instant::now();
    let mut warned_near_limit = false;
    loop {
        match call(
            caller,
            "process.hostnet_accept",
            vec![json!(fd), json!(false), json!(false), Value::Null],
            HashMap::new(),
        )
        .await
        {
            Ok(reply) => {
                let Ok(value) = json_reply(reply) else {
                    return ERRNO_IO;
                };
                if value.is_null()
                    || value.get("kind").and_then(Value::as_str) == Some("wouldBlock")
                {
                    if nonblocking {
                        return ERRNO_AGAIN;
                    }
                    let wait = wait_for_socket_readable(
                        caller,
                        fd,
                        "blocking socket accept",
                        started,
                        &mut warned_near_limit,
                    )
                    .await;
                    if wait != SUCCESS {
                        return wait;
                    }
                    continue;
                }
                let Some(accepted_fd) = value
                    .get("fd")
                    .and_then(value_u64)
                    .and_then(|value| u32::try_from(value).ok())
                else {
                    return ERRNO_IO;
                };
                let address = encode_address(value.get("info").unwrap_or(&value), true);
                let copied = if commit(caller, fd_output, &accepted_fd.to_le_bytes()) == SUCCESS {
                    publish_bytes(
                        caller,
                        address_output,
                        capacity,
                        length_output,
                        address.as_bytes(),
                        false,
                    )
                } else {
                    ERRNO_FAULT
                };
                if copied == SUCCESS {
                    return SUCCESS;
                } else {
                    log_fd_rollback(caller, accepted_fd, "accept output commit").await;
                    return copied;
                }
            }
            Err(error) => return errno(&error),
        }
    }
}

pub(super) async fn fd_is_nonblocking<C: WasmGuest>(caller: &mut C, fd: u32) -> Result<bool, i32> {
    match call(caller, "process.fd_stat", vec![json!(fd)], HashMap::new()).await {
        Ok(reply) => json_reply(reply)
            .ok()
            .and_then(|value| value.get("flags").and_then(value_u64))
            .map(|flags| flags & u64::from(KERNEL_O_NONBLOCK) != 0)
            .ok_or(ERRNO_IO),
        Err(error) => Err(errno(&error)),
    }
}

pub(super) async fn wait_for_socket_readable<C: WasmGuest>(
    caller: &mut C,
    fd: u32,
    operation: &str,
    started: Instant,
    warned_near_limit: &mut bool,
) -> i32 {
    wait_for_fd_ready(
        caller,
        fd,
        POLLIN | POLLRDNORM,
        operation,
        started,
        warned_near_limit,
    )
    .await
}

pub(super) async fn wait_for_fd_writable<C: WasmGuest>(
    caller: &mut C,
    fd: u32,
    operation: &str,
    started: Instant,
    warned_near_limit: &mut bool,
) -> i32 {
    wait_for_fd_ready(
        caller,
        fd,
        POLLOUT | POLLWRNORM,
        operation,
        started,
        warned_near_limit,
    )
    .await
}

async fn wait_for_fd_ready<C: WasmGuest>(
    caller: &mut C,
    fd: u32,
    events: u32,
    operation: &str,
    started: Instant,
    warned_near_limit: &mut bool,
) -> i32 {
    let limit_ms = caller.data().max_blocking_read_ms;
    loop {
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let warning_ms = limit_ms.saturating_mul(4) / 5;
        if !*warned_near_limit && elapsed_ms >= warning_ms {
            let warning = format!(
                "[agentos] {operation} is nearing limits.resources.maxBlockingReadMs ({limit_ms} ms)\n"
            );
            if caller
                .data()
                .host
                .publish_stderr(warning.into_bytes())
                .await
                .is_err()
            {
                return ERRNO_IO;
            }
            *warned_near_limit = true;
        }
        if elapsed_ms >= limit_ms {
            let warning = format!(
                "[agentos] {operation} exceeded limits.resources.maxBlockingReadMs ({limit_ms} ms); raise limits.resources.maxBlockingReadMs if needed\n"
            );
            if caller
                .data()
                .host
                .publish_stderr(warning.into_bytes())
                .await
                .is_err()
            {
                return ERRNO_IO;
            }
            return ERRNO_TIMEDOUT;
        }
        let checkpoint_ms = if *warned_near_limit {
            limit_ms
        } else {
            warning_ms.max(1)
        };
        let wait_ms = checkpoint_ms.saturating_sub(elapsed_ms).max(1);
        let reply = call(
            caller,
            "process.posix_poll",
            vec![
                json!([{"fd": fd, "events": events}]),
                json!(wait_ms),
                Value::Null,
            ],
            HashMap::new(),
        )
        .await;
        match reply {
            Ok(reply) => {
                let ready = json_reply(reply)
                    .ok()
                    .and_then(|value| value.get("readyCount").and_then(value_u64))
                    .unwrap_or_default()
                    > 0;
                if ready {
                    return SUCCESS;
                }
            }
            Err(error) => return errno(&error),
        }
    }
}

async fn log_fd_rollback<C: WasmGuest>(caller: &mut C, fd: u32, context: &'static str) {
    let error = simple_call(caller, "process.fd_close", vec![json!(fd)]).await;
    if error != SUCCESS {
        eprintln!("ERR_AGENTOS_WASM_FD_ROLLBACK: context={context} fd={fd} errno={error}");
    }
}

async fn address_output<C: WasmGuest>(caller: &mut C, params: &[Val], peer: bool) -> i32 {
    let (Ok(fd), Ok(output), Ok(length_output)) =
        (i32_arg(params, 0), i32_arg(params, 1), i32_arg(params, 2))
    else {
        return ERRNO_FAULT;
    };
    let Ok(capacity) = memory::read_u32(caller, length_output) else {
        return ERRNO_FAULT;
    };
    if memory::validate_range(caller, output, capacity as usize).is_err()
        || memory::validate_range(caller, length_output, 4).is_err()
    {
        return ERRNO_FAULT;
    }
    let method = if peer {
        "process.hostnet_peer_address"
    } else {
        "process.hostnet_local_address"
    };
    match call(caller, method, vec![json!(fd)], HashMap::new()).await {
        Ok(reply) => {
            let Ok(value) = json_reply(reply) else {
                return ERRNO_IO;
            };
            let text = encode_address(&value, false);
            publish_bytes(
                caller,
                output,
                capacity,
                length_output,
                text.as_bytes(),
                false,
            )
        }
        Err(error) => errno(&error),
    }
}

fn encode_address(value: &Value, peer_fields: bool) -> String {
    let prefix = if peer_fields { "remote" } else { "" };
    let field = |plain: &str, prefixed: &str| {
        value
            .get(if peer_fields { prefixed } else { plain })
            .or_else(|| value.get(plain))
    };
    if let (Some(address), Some(port)) = (
        field("address", "remoteAddress").and_then(Value::as_str),
        field("port", "remotePort").and_then(value_u64),
    ) {
        let _ = prefix;
        // The owned libc splits at the last colon, so IPv6 stays unbracketed.
        return format!("{address}:{port}");
    }
    if let Some(hex) = field("abstractPathHex", "remoteAbstractPathHex").and_then(Value::as_str) {
        return format!("unix-abstract:{hex}");
    }
    if let Some(path) = field("path", "remotePath").and_then(Value::as_str) {
        return format!("unix:{path}");
    }
    "unix-unnamed".to_owned()
}

async fn send<C: WasmGuest>(caller: &mut C, params: &[Val], to: bool) -> i32 {
    let (Ok(fd), Ok(pointer), Ok(length), Ok(flags)) = (
        i32_arg(params, 0),
        i32_arg(params, 1),
        i32_arg(params, 2),
        i32_arg(params, 3),
    ) else {
        return ERRNO_FAULT;
    };
    let output_index = if to { 6 } else { 4 };
    let Ok(output) = i32_arg(params, output_index) else {
        return ERRNO_FAULT;
    };
    let Ok(bytes) = memory::read_bytes(caller, pointer, length as usize) else {
        return ERRNO_FAULT;
    };
    if memory::validate_range(caller, output, 4).is_err() {
        return ERRNO_FAULT;
    }
    let address = if to {
        let (Ok(pointer), Ok(length)) = (i32_arg(params, 4), i32_arg(params, 5)) else {
            return ERRNO_FAULT;
        };
        let Ok(text) = memory::read_string(caller, pointer, length as usize) else {
            return ERRNO_FAULT;
        };
        match decode_address(&text) {
            Ok(value) => value,
            Err(_) => return ERRNO_INVAL,
        }
    } else {
        Value::Null
    };
    let mut raw = HashMap::new();
    raw.insert(1, bytes.clone());
    let host_reply = call(
        caller,
        "process.hostnet_send",
        vec![json!(fd), Value::Null, json!(flags), address, Value::Null],
        raw,
    )
    .await;
    let reply = match host_reply {
        reply @ Ok(_) => reply,
        // The kernel owns AF_UNIX socketpair data and message boundaries.
        // V8 routes non-host-network descriptors through this same operation;
        // keep the native linker as a codec rather than a second socket stack.
        Err(error) if !to && error.code == "ENOTSOCK" => {
            let mut raw = HashMap::new();
            raw.insert(1, bytes);
            call(
                caller,
                "process.fd_sendmsg_rights",
                vec![
                    json!(fd),
                    Value::Null,
                    Value::Array(Vec::new()),
                    json!(flags),
                ],
                raw,
            )
            .await
        }
        reply => reply,
    };
    match reply {
        Ok(reply) => {
            let written = match reply {
                HostCallReply::Json(value) => {
                    value_u64(&value).or_else(|| value.get("bytes").and_then(value_u64))
                }
                _ => None,
            };
            let Some(written) = written.and_then(|value| u32::try_from(value).ok()) else {
                return ERRNO_IO;
            };
            commit(caller, output, &written.to_le_bytes())
        }
        Err(error) => errno(&error),
    }
}

async fn receive<C: WasmGuest>(caller: &mut C, params: &[Val], from: bool) -> i32 {
    let (Ok(fd), Ok(output), Ok(capacity), Ok(flags)) = (
        i32_arg(params, 0),
        i32_arg(params, 1),
        i32_arg(params, 2),
        i32_arg(params, 3),
    ) else {
        return ERRNO_FAULT;
    };
    let Ok(length_output) = i32_arg(params, 4) else {
        return ERRNO_FAULT;
    };
    let address_outputs = if from {
        let (Ok(address), Ok(length)) = (i32_arg(params, 5), i32_arg(params, 6)) else {
            return ERRNO_FAULT;
        };
        let Ok(address_capacity) = memory::read_u32(caller, length) else {
            return ERRNO_FAULT;
        };
        Some((address, address_capacity, length))
    } else {
        None
    };
    if memory::validate_range(caller, output, capacity as usize).is_err()
        || memory::validate_range(caller, length_output, 4).is_err()
        || address_outputs.is_some_and(|(pointer, capacity, length)| {
            memory::validate_range(caller, pointer, capacity as usize).is_err()
                || memory::validate_range(caller, length, 4).is_err()
        })
    {
        return ERRNO_FAULT;
    }
    let nonblocking = match fd_is_nonblocking(caller, fd).await {
        Ok(nonblocking) => nonblocking || flags & MSG_DONTWAIT != 0,
        Err(error) => return error,
    };
    let started = Instant::now();
    let mut warned_near_limit = false;
    loop {
        let host_reply = call(
            caller,
            "process.hostnet_recv",
            vec![json!(fd), json!(capacity), json!(flags), Value::Null],
            HashMap::new(),
        )
        .await;
        let reply = match host_reply {
            reply @ Ok(_) => reply,
            Err(error) if !from && error.code == "ENOTSOCK" => {
                call(
                    caller,
                    "process.fd_recvmsg_rights",
                    vec![
                        json!(fd),
                        json!(capacity),
                        json!(0),
                        json!(false),
                        json!(flags & 0x2 != 0),
                        json!(flags & MSG_DONTWAIT != 0),
                        json!(flags & 0x100 != 0),
                    ],
                    HashMap::new(),
                )
                .await
            }
            reply => reply,
        };
        match reply {
            Ok(reply) => {
                if matches!(&reply, HostCallReply::Json(Value::Null)) {
                    return commit(caller, length_output, &0u32.to_le_bytes());
                }
                let full_length = match &reply {
                    HostCallReply::Json(value) => value
                        .get("fullLength")
                        .and_then(value_u64)
                        .and_then(|value| usize::try_from(value).ok()),
                    _ => None,
                };
                let (bytes, address) = match reply {
                    HostCallReply::Json(value)
                        if value.get("kind").and_then(Value::as_str) == Some("wouldBlock") =>
                    {
                        if nonblocking {
                            return ERRNO_AGAIN;
                        }
                        let wait = wait_for_socket_readable(
                            caller,
                            fd,
                            "blocking socket receive",
                            started,
                            &mut warned_near_limit,
                        )
                        .await;
                        if wait != SUCCESS {
                            return wait;
                        }
                        continue;
                    }
                    HostCallReply::Json(value)
                        if value.get("type").and_then(Value::as_str) == Some("message") =>
                    {
                        let bytes = value
                            .get("data")
                            .cloned()
                            .map(HostCallReply::Json)
                            .and_then(|value| reply_bytes(value).ok())
                            .ok_or(ERRNO_IO);
                        let address = encode_address(&value, true);
                        match bytes {
                            Ok(bytes) => (bytes, Some(address)),
                            Err(error) => return error,
                        }
                    }
                    HostCallReply::Json(value) if value.get("data").is_some() => {
                        let bytes = value
                            .get("data")
                            .cloned()
                            .map(HostCallReply::Json)
                            .and_then(|value| reply_bytes(value).ok())
                            .ok_or(ERRNO_IO);
                        match bytes {
                            Ok(bytes) => (bytes, None),
                            Err(error) => return error,
                        }
                    }
                    reply => match reply_bytes(reply) {
                        Ok(bytes) => (bytes, None),
                        Err(error) => return error,
                    },
                };
                let written = bytes.len().min(capacity as usize);
                if commit(caller, output, &bytes[..written]) != SUCCESS {
                    return ERRNO_FAULT;
                }
                let full_length = full_length.unwrap_or(bytes.len());
                let reported = if flags & MSG_TRUNC != 0 {
                    full_length
                } else {
                    written
                };
                let Ok(reported) = u32::try_from(reported) else {
                    return ERRNO_2BIG;
                };
                if commit(caller, length_output, &reported.to_le_bytes()) != SUCCESS {
                    return ERRNO_FAULT;
                }
                if let Some((address_output, address_capacity, address_length_output)) =
                    address_outputs
                {
                    let Some(address) = address else {
                        return ERRNO_IO;
                    };
                    return publish_bytes(
                        caller,
                        address_output,
                        address_capacity,
                        address_length_output,
                        address.as_bytes(),
                        false,
                    );
                }
                return SUCCESS;
            }
            Err(error) => return errno(&error),
        }
    }
}

fn timeval_ms(bytes: &[u8]) -> Result<Option<u64>, ()> {
    if bytes.len() != 16 {
        return Err(());
    }
    let seconds = i64::from_le_bytes(bytes[0..8].try_into().map_err(|_| ())?);
    let micros = i64::from_le_bytes(bytes[8..16].try_into().map_err(|_| ())?);
    if seconds < 0 || !(0..1_000_000).contains(&micros) {
        return Err(());
    }
    if seconds == 0 && micros == 0 {
        return Ok(None);
    }
    Ok(Some(
        (seconds as u64)
            .saturating_mul(1000)
            .saturating_add((micros as u64).div_ceil(1000)),
    ))
}

fn option_kind(level: u32, name: u32, length: u32) -> Option<&'static str> {
    let socket_level = matches!(level, 1 | 0x7fff_ffff);
    // Receive-ancillary selectors and IP traffic classes are owned by the
    // kernel socket description. Other Linux receive selectors have no
    // metadata path yet and are rejected rather than acknowledged as no-ops.
    match (level, name) {
        (0, 8) => return Some("receive-pktinfo-v4"),
        (0, 13) => return Some("receive-tos-v4"),
        (41, 49) => return Some("receive-pktinfo-v6"),
        (41, 66) => return Some("receive-tclass-v6"),
        (0, 6 | 7 | 11 | 12 | 20 | 26) | (41, 25 | 51 | 53 | 56 | 58 | 60 | 74) => {
            return Some("unsupported-receive")
        }
        (0, 1) => return Some("ip-tos"),
        (41, 67) => return Some("ipv6-traffic-class"),
        _ => {}
    }
    if socket_level && name == 2 && length == 4 {
        Some("reuse-address")
    } else if socket_level && name == 13 && length == 8 {
        Some("linger")
    } else if socket_level && matches!(name, 20 | 66) && length == 16 {
        Some("receive-timeout")
    } else if (socket_level && name == 9) || (level == 6 && name == 1) {
        Some("ignore")
    } else {
        None
    }
}

async fn set_option<C: WasmGuest>(caller: &mut C, params: &[Val]) -> i32 {
    let (Ok(fd), Ok(level), Ok(name), Ok(pointer), Ok(length)) = (
        i32_arg(params, 0),
        i32_arg(params, 1),
        i32_arg(params, 2),
        i32_arg(params, 3),
        i32_arg(params, 4),
    ) else {
        return ERRNO_INVAL;
    };
    let Some(kind) = option_kind(level, name, length) else {
        return ERRNO_INVAL;
    };
    if kind == "unsupported-receive" {
        return ERRNO_NOPROTOOPT;
    }
    if is_owned_integer_option(kind) {
        return set_integer_option(caller, fd, kind, pointer, length).await;
    }
    if kind == "ignore" {
        return SUCCESS;
    }
    let Ok(bytes) = memory::read_bytes(caller, pointer, length as usize) else {
        return ERRNO_FAULT;
    };
    if kind == "reuse-address" {
        let Ok(value) = <[u8; 4]>::try_from(bytes.as_slice()) else {
            return ERRNO_INVAL;
        };
        return simple_call(
            caller,
            "process.hostnet_set_option",
            vec![
                json!(fd),
                json!(kind),
                json!(u32::from_le_bytes(value) != 0),
            ],
        )
        .await;
    }
    if kind == "linger" {
        let (Ok(enabled), Ok(seconds)) = (
            <[u8; 4]>::try_from(&bytes[0..4]),
            <[u8; 4]>::try_from(&bytes[4..8]),
        ) else {
            return ERRNO_INVAL;
        };
        return simple_call(
            caller,
            "process.hostnet_set_option",
            vec![
                json!(fd),
                json!(kind),
                json!({
                    "enabled": u32::from_le_bytes(enabled) != 0,
                    "seconds": u32::from_le_bytes(seconds),
                }),
            ],
        )
        .await;
    }
    let Ok(duration) = timeval_ms(&bytes) else {
        return ERRNO_INVAL;
    };
    simple_call(
        caller,
        "process.hostnet_set_option",
        vec![json!(fd), json!(kind), json!({"durationMs": duration})],
    )
    .await
}

async fn get_option<C: WasmGuest>(caller: &mut C, params: &[Val]) -> i32 {
    let (Ok(fd), Ok(level), Ok(name), Ok(output), Ok(length_output)) = (
        i32_arg(params, 0),
        i32_arg(params, 1),
        i32_arg(params, 2),
        i32_arg(params, 3),
        i32_arg(params, 4),
    ) else {
        return ERRNO_FAULT;
    };
    let Ok(capacity) = memory::read_u32(caller, length_output) else {
        return ERRNO_FAULT;
    };
    match option_kind(level, name, capacity) {
        Some("unsupported-receive") => return ERRNO_NOPROTOOPT,
        Some(kind) if is_owned_integer_option(kind) => {
            return get_integer_option(caller, fd, kind, output, length_output, capacity).await;
        }
        _ => {}
    }
    if memory::validate_range(caller, output, capacity as usize).is_err()
        || memory::validate_range(caller, length_output, 4).is_err()
    {
        return ERRNO_FAULT;
    }
    if !matches!(level, 1 | 0x7fff_ffff) || name != 4 || capacity < 4 {
        return ERRNO_INVAL;
    }
    match call(
        caller,
        "process.hostnet_get_option",
        vec![json!(fd), json!("error")],
        HashMap::new(),
    )
    .await
    {
        Ok(reply) => {
            let Ok(value) = json_reply(reply) else {
                return ERRNO_IO;
            };
            let Some(value) = value.as_i64().and_then(|value| i32::try_from(value).ok()) else {
                return ERRNO_IO;
            };
            if commit(caller, output, &value.to_le_bytes()) != SUCCESS {
                ERRNO_FAULT
            } else {
                commit(caller, length_output, &4u32.to_le_bytes())
            }
        }
        Err(error) => errno(&error),
    }
}

fn is_owned_integer_option(kind: &str) -> bool {
    kind.starts_with("receive-") || kind == "ip-tos" || kind == "ipv6-traffic-class"
}

/// Read one integer fact from the kernel socket description.
async fn socket_fact<C: WasmGuest>(caller: &mut C, fd: u32, kind: &str) -> Result<i64, i32> {
    match call(
        caller,
        "process.hostnet_get_option",
        vec![json!(fd), json!(kind)],
        HashMap::new(),
    )
    .await
    {
        Ok(reply) => json_reply(reply)
            .ok()
            .and_then(|value| value.as_i64().or_else(|| value.as_bool().map(i64::from)))
            .ok_or(ERRNO_IO),
        Err(error) => Err(errno(&error)),
    }
}

/// Linux `setsockopt` for the receive-ancillary selectors and IP traffic
/// classes. Values are plain ints; a short buffer supplies one byte.
async fn set_integer_option<C: WasmGuest>(
    caller: &mut C,
    fd: u32,
    kind: &'static str,
    pointer: u32,
    length: u32,
) -> i32 {
    if length > 0x7fff_ffff {
        return ERRNO_INVAL;
    }
    let receive = kind.starts_with("receive-");
    let ipv6 = kind.ends_with("-v6") || kind == "ipv6-traffic-class";
    let domain = match socket_fact(caller, fd, "domain").await {
        Ok(domain) => domain,
        Err(error) => return error,
    };
    if domain == LINUX_AF_UNIX {
        return if receive {
            ERRNO_NOPROTOOPT
        } else {
            ERRNO_NOTSUP
        };
    }
    if ipv6 && domain != LINUX_AF_INET6 {
        return ERRNO_NOPROTOOPT;
    }
    if ipv6 && length < 4 {
        return ERRNO_INVAL;
    }
    let count = match length {
        0 => 0,
        1..=3 => 1,
        _ => 4,
    };
    let mut value = 0i32;
    // Linux IPv6 receive selectors read NULL as zero once the length checks
    // pass; IPv4 selectors copy their value and fault on NULL.
    if count != 0 && !(receive && ipv6 && pointer == 0) {
        if receive && pointer == 0 {
            return ERRNO_FAULT;
        }
        let Ok(bytes) = memory::read_bytes(caller, pointer, count) else {
            return ERRNO_FAULT;
        };
        value = match <[u8; 4]>::try_from(bytes.as_slice()) {
            Ok(word) => i32::from_le_bytes(word),
            Err(_) => i32::from(bytes[0]),
        };
    }
    if !receive && ipv6 && !(-1..=255).contains(&value) {
        return ERRNO_INVAL;
    }
    let argument = if receive {
        json!(value != 0)
    } else {
        json!(value)
    };
    simple_call(
        caller,
        "process.hostnet_set_option",
        vec![json!(fd), json!(kind), argument],
    )
    .await
}

/// Linux `getsockopt` for the same options: up to four bytes, one byte for a
/// short IPv4 buffer, and the copied length written back.
async fn get_integer_option<C: WasmGuest>(
    caller: &mut C,
    fd: u32,
    kind: &'static str,
    output: u32,
    length_output: u32,
    capacity: u32,
) -> i32 {
    if capacity > 0x7fff_ffff {
        return ERRNO_INVAL;
    }
    let observed = match socket_fact(caller, fd, kind).await {
        Ok(value) => value as i32,
        Err(error) => return error,
    };
    let ipv4 = kind.ends_with("-v4") || kind == "ip-tos";
    let count = if ipv4 && (1..4).contains(&capacity) {
        1
    } else {
        capacity.min(4) as usize
    };
    if count == 0 {
        return commit(caller, length_output, &0u32.to_le_bytes());
    }
    if kind.starts_with("receive-") && output == 0 {
        return ERRNO_FAULT;
    }
    if commit(caller, output, &observed.to_le_bytes()[..count]) != SUCCESS {
        return ERRNO_FAULT;
    }
    commit(caller, length_output, &(count as u32).to_le_bytes())
}

/// A validated wasm32 `msghdr`, captured before any wait so a guest cannot
/// redirect the writes by mutating it while the receive blocks.
struct MessageHeader {
    base: u32,
    name: u32,
    name_capacity: u32,
    iovecs: Vec<(u32, u32)>,
    total: usize,
    control: u32,
    control_capacity: u32,
}

async fn snapshot_message<C: WasmGuest>(caller: &mut C, base: u32) -> Result<MessageHeader, i32> {
    if base == 0 || memory::validate_range(caller, base, 28).is_err() {
        return Err(ERRNO_FAULT);
    }
    let field = |caller: &mut C, offset: u32| {
        memory::read_u32(caller, base + offset).map_err(|_| ERRNO_FAULT)
    };
    let name = field(caller, 0)?;
    let name_capacity = field(caller, 4)?;
    let iov = field(caller, 8)?;
    let count = field(caller, 12)?;
    let control = field(caller, 16)?;
    let control_capacity = if control == 0 { 0 } else { field(caller, 20)? };
    // Linux rejects a negative name length before importing the iovecs.
    if name != 0 && name_capacity > 0x7fff_ffff {
        return Err(ERRNO_INVAL);
    }
    let limit = super::check_fixed_request_limit(
        caller,
        "wasm.abi.maxIovecs",
        count as usize,
        super::preview1::max_iovecs(),
        ERRNO_MSGSIZE,
    )
    .await;
    if limit != SUCCESS {
        return Err(limit);
    }
    if count > 0 && iov == 0 {
        return Err(ERRNO_FAULT);
    }
    if memory::validate_range(caller, iov, count as usize * 8).is_err() {
        return Err(ERRNO_FAULT);
    }
    let mut iovecs = Vec::with_capacity(count as usize);
    let mut total = 0usize;
    for index in 0..count {
        let slot = iov + index * 8;
        let pointer = memory::read_u32(caller, slot).map_err(|_| ERRNO_FAULT)?;
        let length = memory::read_u32(caller, slot + 4).map_err(|_| ERRNO_FAULT)?;
        if memory::validate_range(caller, pointer, length as usize).is_err() {
            return Err(ERRNO_FAULT);
        }
        total = total.checked_add(length as usize).ok_or(ERRNO_INVAL)?;
        if total > u32::MAX as usize {
            return Err(ERRNO_INVAL);
        }
        iovecs.push((pointer, length));
    }
    if iovecs
        .iter()
        .any(|(pointer, length)| *length > 0 && *pointer == 0)
    {
        return Err(ERRNO_FAULT);
    }
    let limit = super::check_fixed_request_limit(
        caller,
        "wasm.abi.maxSocketMessageBytes",
        total,
        MAX_SOCKET_MESSAGE_BYTES,
        ERRNO_MSGSIZE,
    )
    .await;
    if limit != SUCCESS {
        return Err(limit);
    }
    if (name != 0 && memory::validate_range(caller, name, name_capacity as usize).is_err())
        || memory::validate_range(caller, control, control_capacity as usize).is_err()
    {
        return Err(ERRNO_FAULT);
    }
    Ok(MessageHeader {
        base,
        name,
        name_capacity,
        iovecs,
        total,
        control,
        control_capacity,
    })
}

/// The datagram source as the owned libc's 16-byte-aligned `sockaddr_in` or
/// `sockaddr_in6`, with IPv4 sources mapped on IPv6 sockets.
fn datagram_source(event: &Value, domain: i64) -> Result<Vec<u8>, i32> {
    let source = byte_array(event.get("remoteBytes")).ok_or(ERRNO_IO)?;
    let port = event
        .get("remotePort")
        .and_then(value_u64)
        .and_then(|port| u16::try_from(port).ok())
        .ok_or(ERRNO_IO)?;
    let ipv6 = domain == LINUX_AF_INET6;
    let mut bytes = vec![0u8; if ipv6 { 32 } else { 16 }];
    let family = if ipv6 { WASI_AF_INET6 } else { WASI_AF_INET };
    bytes[0..2].copy_from_slice(&family.to_le_bytes());
    bytes[2..4].copy_from_slice(&port.to_be_bytes());
    match (ipv6, source.len()) {
        (false, 4) => bytes[4..8].copy_from_slice(&source),
        (true, 16) => bytes[8..24].copy_from_slice(&source),
        (true, 4) => {
            bytes[18] = 255;
            bytes[19] = 255;
            bytes[20..24].copy_from_slice(&source);
        }
        _ => return Err(ERRNO_IO),
    }
    Ok(bytes)
}

fn byte_array(value: Option<&Value>) -> Option<Vec<u8>> {
    value?
        .as_array()?
        .iter()
        .map(|byte| byte.as_u64().and_then(|byte| u8::try_from(byte).ok()))
        .collect()
}

/// Linux control messages for the receive-ancillary selectors, in the owned
/// wasm32 layout: 12-byte `cmsghdr`, four-byte alignment.
fn datagram_controls(event: &Value, domain: i64) -> Result<Vec<(i32, i32, Vec<u8>)>, i32> {
    let mut controls = Vec::new();
    let Some(metadata) = event.get("ancillary").filter(|value| !value.is_null()) else {
        return Ok(controls);
    };
    let destination = byte_array(metadata.get("destination")).ok_or(ERRNO_IO)?;
    let ipv4 = metadata.get("family").and_then(value_u64) == Some(4);
    let options = metadata.get("options").and_then(value_u64).unwrap_or(0);
    let index = metadata
        .get("interfaceIndex")
        .and_then(value_u64)
        .and_then(|index| u32::try_from(index).ok())
        .ok_or(ERRNO_IO)?;
    let traffic_class = metadata
        .get("trafficClass")
        .and_then(Value::as_i64)
        .unwrap_or(0) as i32;
    let local = metadata
        .get("ipv4LocalAddress")
        .filter(|value| !value.is_null());
    if (ipv4 && destination.len() != 4)
        || (!ipv4 && destination.len() != 16)
        || !(1..=2).contains(&index)
    {
        eprintln!("ERR_AGENTOS_UDP_METADATA: invalid projected packet facts");
        return Err(ERRNO_IO);
    }
    if domain == LINUX_AF_INET6 && options & 4 != 0 {
        let mut bytes = vec![0u8; 20];
        if ipv4 {
            bytes[10] = 255;
            bytes[11] = 255;
            bytes[12..16].copy_from_slice(&destination);
        } else {
            bytes[..16].copy_from_slice(&destination);
        }
        let reported = if ipv4 && local.is_none() { 0 } else { index };
        bytes[16..20].copy_from_slice(&reported.to_le_bytes());
        controls.push((41, 50, bytes));
    }
    if ipv4 && options & 1 != 0 {
        let local_bytes = match local {
            Some(value) => byte_array(Some(value))
                .filter(|bytes| bytes.len() == 4)
                .ok_or(ERRNO_IO)?,
            None => vec![0; 4],
        };
        let mut bytes = vec![0u8; 12];
        let reported = if local.is_none() { 0 } else { index };
        bytes[0..4].copy_from_slice(&reported.to_le_bytes());
        bytes[4..8].copy_from_slice(&local_bytes);
        bytes[8..12].copy_from_slice(&destination);
        controls.push((0, 8, bytes));
    }
    if ipv4 && options & 2 != 0 {
        controls.push((0, 1, vec![(traffic_class & 0xff) as u8]));
    }
    if !ipv4 && options & 8 != 0 {
        controls.push((41, 67, traffic_class.to_le_bytes().to_vec()));
    }
    Ok(controls)
}

/// `recvmsg` on a UDP socket: payload into the iovecs, the source address,
/// and receive-ancillary control messages. Other sockets return `ENOTSOCK` so
/// the owned libc takes its descriptor-rights path.
async fn receive_message<C: WasmGuest>(caller: &mut C, params: &[Val]) -> i32 {
    let (Ok(fd), Ok(message), Ok(flags), Ok(received_output)) = (
        i32_arg(params, 0),
        i32_arg(params, 1),
        i32_arg(params, 2),
        i32_arg(params, 3),
    ) else {
        return ERRNO_FAULT;
    };
    let socket_type = match socket_fact(caller, fd, "type").await {
        Ok(value) => value,
        Err(error) => return error,
    };
    let domain = match socket_fact(caller, fd, "domain").await {
        Ok(value) => value,
        Err(error) => return error,
    };
    if socket_type != LINUX_SOCK_DGRAM || !matches!(domain, LINUX_AF_INET | LINUX_AF_INET6) {
        return ERRNO_NOTSOCK;
    }
    if memory::validate_range(caller, received_output, 4).is_err() {
        return ERRNO_FAULT;
    }
    let header = match snapshot_message(caller, message).await {
        Ok(header) => header,
        Err(error) => return error,
    };
    let ignored = MSG_WAITALL | MSG_CMSG_CLOEXEC;
    if flags & !(MSG_PEEK | MSG_DONTWAIT | MSG_TRUNC | ignored) != 0 {
        return ERRNO_INVAL;
    }
    let flags = flags & !ignored;
    let nonblocking = match fd_is_nonblocking(caller, fd).await {
        Ok(nonblocking) => nonblocking || flags & MSG_DONTWAIT != 0,
        Err(error) => return error,
    };
    let started = Instant::now();
    let mut warned_near_limit = false;
    let event = loop {
        let reply = call(
            caller,
            "process.hostnet_recv",
            vec![json!(fd), json!(header.total), json!(flags), Value::Null],
            HashMap::new(),
        )
        .await;
        let value = match reply.map(json_reply) {
            Ok(Ok(value)) => value,
            Ok(Err(_)) => return ERRNO_IO,
            Err(error) => return errno(&error),
        };
        if value.get("kind").and_then(Value::as_str) == Some("wouldBlock") || value.is_null() {
            if nonblocking {
                return ERRNO_AGAIN;
            }
            let wait = wait_for_socket_readable(
                caller,
                fd,
                "blocking socket receive",
                started,
                &mut warned_near_limit,
            )
            .await;
            if wait != SUCCESS {
                return wait;
            }
            continue;
        }
        if value.get("type").and_then(Value::as_str) != Some("message") {
            return ERRNO_IO;
        }
        break value;
    };
    let Some(data) = event
        .get("data")
        .cloned()
        .map(HostCallReply::Json)
        .and_then(|value| reply_bytes(value).ok())
    else {
        return ERRNO_IO;
    };
    let full_length = event
        .get("fullLength")
        .and_then(value_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(data.len());
    let source = match datagram_source(&event, domain) {
        Ok(source) => source,
        Err(error) => return error,
    };
    let controls = match datagram_controls(&event, domain) {
        Ok(controls) => controls,
        Err(error) => return error,
    };
    let mut copied = 0usize;
    for (pointer, length) in &header.iovecs {
        if copied >= data.len() {
            break;
        }
        let chunk = (*length as usize).min(data.len() - copied);
        if commit(caller, *pointer, &data[copied..copied + chunk]) != SUCCESS {
            return ERRNO_FAULT;
        }
        copied += chunk;
    }
    if header.name != 0 {
        let written = source.len().min(header.name_capacity as usize);
        if commit(caller, header.name, &source[..written]) != SUCCESS {
            return ERRNO_FAULT;
        }
    }
    let name_length = if header.name == 0 {
        0
    } else {
        source.len() as u32
    };
    if commit(caller, header.base + 4, &name_length.to_le_bytes()) != SUCCESS {
        return ERRNO_FAULT;
    }
    let mut used = 0usize;
    let mut truncated = false;
    let capacity = header.control_capacity as usize;
    for (level, kind, bytes) in controls {
        let available = capacity - used;
        if available < 12 {
            truncated = true;
            break;
        }
        let copied_bytes = bytes.len().min(available - 12);
        let mut record = Vec::with_capacity(12 + copied_bytes);
        record.extend_from_slice(&((12 + copied_bytes) as u32).to_le_bytes());
        record.extend_from_slice(&level.to_le_bytes());
        record.extend_from_slice(&kind.to_le_bytes());
        record.extend_from_slice(&bytes[..copied_bytes]);
        if commit(caller, header.control + used as u32, &record) != SUCCESS {
            return ERRNO_FAULT;
        }
        used += available.min((12 + bytes.len() + 3) & !3);
        if copied_bytes < bytes.len() {
            truncated = true;
            break;
        }
    }
    let message_flags = if truncated { MSG_CTRUNC } else { 0 }
        | if full_length > header.total {
            MSG_TRUNC
        } else {
            0
        };
    if commit(caller, header.base + 20, &(used as u32).to_le_bytes()) != SUCCESS
        || commit(caller, header.base + 24, &message_flags.to_le_bytes()) != SUCCESS
    {
        return ERRNO_FAULT;
    }
    let reported = if flags & MSG_TRUNC != 0 {
        full_length
    } else {
        copied
    };
    let Ok(reported) = u32::try_from(reported) else {
        return ERRNO_2BIG;
    };
    commit(caller, received_output, &reported.to_le_bytes())
}

async fn close<C: WasmGuest>(caller: &mut C, params: &[Val]) -> i32 {
    let Ok(fd) = i32_arg(params, 0) else {
        return ERRNO_BADF;
    };
    simple_call(caller, "process.fd_close", vec![json!(fd)]).await
}

async fn tls_connect<C: WasmGuest>(caller: &mut C, params: &[Val]) -> i32 {
    let (Ok(fd), Ok(pointer), Ok(length)) =
        (i32_arg(params, 0), i32_arg(params, 1), i32_arg(params, 2))
    else {
        return ERRNO_FAULT;
    };
    let Ok(hostname) = memory::read_string(caller, pointer, length as usize) else {
        return ERRNO_FAULT;
    };
    let reject_unauthorized = !caller.data().env.iter().any(|entry| {
        entry.strip_suffix(&[0]).unwrap_or(entry.as_slice()) == b"NODE_TLS_REJECT_UNAUTHORIZED=0"
    });
    simple_call(
        caller,
        "process.hostnet_tls_connect",
        vec![
            json!(fd),
            json!(hostname),
            json!([]),
            Value::Null,
            json!(reject_unauthorized),
        ],
    )
    .await
}

async fn poll<C: WasmGuest>(caller: &mut C, params: &[Val]) -> i32 {
    let (Ok(pointer), Ok(count), Ok(timeout), Ok(ready_output)) = (
        i32_arg(params, 0),
        i32_arg(params, 1),
        i32_arg(params, 2),
        i32_arg(params, 3),
    ) else {
        return ERRNO_FAULT;
    };
    let count = count as usize;
    let limit = super::check_fixed_request_limit(
        caller,
        "wasm.abi.maxPollFds",
        count,
        MAX_POLL_FDS,
        ERRNO_INVAL,
    )
    .await;
    if limit != SUCCESS {
        return limit;
    }
    if memory::validate_range(caller, pointer, count.saturating_mul(8)).is_err()
        || memory::validate_range(caller, ready_output, 4).is_err()
    {
        return ERRNO_FAULT;
    }
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let base = pointer + (index * 8) as u32;
        let Ok(fd) = memory::read_u32(caller, base) else {
            return ERRNO_FAULT;
        };
        let Ok(events) = memory::read_bytes(caller, base + 4, 2) else {
            return ERRNO_FAULT;
        };
        entries.push(json!({"fd": fd, "events": u16::from_le_bytes([events[0], events[1]])}));
    }
    let requested_timeout = timeout as i32;
    let blocking_limit_ms = caller.data().max_blocking_read_ms;
    let safeguard_applies = requested_timeout < 0
        || u64::try_from(requested_timeout).is_ok_and(|timeout| timeout > blocking_limit_ms);
    let first_wait_ms = if safeguard_applies {
        blocking_limit_ms.saturating_mul(4) / 5
    } else {
        u64::try_from(requested_timeout).unwrap_or_default()
    };
    let mut reply = call(
        caller,
        "process.posix_poll",
        vec![
            Value::Array(entries.clone()),
            json!(first_wait_ms),
            Value::Null,
        ],
        HashMap::new(),
    )
    .await;
    if safeguard_applies
        && reply
            .as_ref()
            .ok()
            .and_then(|reply| json_reply(reply.clone()).ok())
            .and_then(|value| value.get("readyCount").and_then(value_u64))
            .unwrap_or_default()
            == 0
    {
        let warning = format!(
            "[agentos] blocking poll is nearing limits.resources.maxBlockingReadMs ({blocking_limit_ms} ms)\n"
        );
        if caller
            .data()
            .host
            .publish_stderr(warning.into_bytes())
            .await
            .is_err()
        {
            return ERRNO_IO;
        }
        reply = call(
            caller,
            "process.posix_poll",
            vec![
                Value::Array(entries),
                json!(blocking_limit_ms.saturating_sub(first_wait_ms)),
                Value::Null,
            ],
            HashMap::new(),
        )
        .await;
        if reply
            .as_ref()
            .ok()
            .and_then(|reply| json_reply(reply.clone()).ok())
            .and_then(|value| value.get("readyCount").and_then(value_u64))
            .unwrap_or_default()
            == 0
        {
            let warning = format!(
                "[agentos] blocking poll exceeded limits.resources.maxBlockingReadMs ({blocking_limit_ms} ms); raise limits.resources.maxBlockingReadMs if needed\n"
            );
            if caller
                .data()
                .host
                .publish_stderr(warning.into_bytes())
                .await
                .is_err()
            {
                return ERRNO_IO;
            }
            return if commit(caller, ready_output, &0u32.to_le_bytes()) == SUCCESS {
                ERRNO_TIMEDOUT
            } else {
                ERRNO_FAULT
            };
        }
    }
    match reply {
        Ok(reply) => {
            let Ok(value) = json_reply(reply) else {
                return ERRNO_IO;
            };
            let Some(fds) = value.get("fds").and_then(Value::as_array) else {
                return ERRNO_IO;
            };
            if fds.len() != count {
                return ERRNO_IO;
            }
            for (index, entry) in fds.iter().enumerate() {
                let Some(fd) = entry.get("fd").and_then(value_u64) else {
                    return ERRNO_IO;
                };
                if memory::read_u32(caller, pointer + (index * 8) as u32).ok()
                    != u32::try_from(fd).ok()
                {
                    return ERRNO_IO;
                }
                let Some(revents) = entry
                    .get("revents")
                    .and_then(value_u64)
                    .and_then(|value| u16::try_from(value).ok())
                else {
                    return ERRNO_IO;
                };
                if commit(
                    caller,
                    pointer + (index * 8 + 6) as u32,
                    &revents.to_le_bytes(),
                ) != SUCCESS
                {
                    return ERRNO_FAULT;
                }
            }
            let ready = value
                .get("readyCount")
                .and_then(value_u64)
                .unwrap_or_else(|| {
                    fds.iter()
                        .filter(|entry| entry.get("revents").and_then(value_u64).unwrap_or(0) != 0)
                        .count() as u64
                });
            let Ok(ready) = u32::try_from(ready) else {
                return ERRNO_2BIG;
            };
            commit(caller, ready_output, &ready.to_le_bytes())
        }
        Err(error) => errno(&error),
    }
}

fn publish_bytes<C: WasmGuest>(
    caller: &mut C,
    output: u32,
    capacity: u32,
    length_output: u32,
    bytes: &[u8],
    require_capacity: bool,
) -> i32 {
    let written = bytes.len().min(capacity as usize);
    if require_capacity && written != bytes.len() {
        return ERRNO_NOBUFS;
    }
    if memory::validate_range(caller, output, written).is_err()
        || memory::validate_range(caller, length_output, 4).is_err()
        || memory::write_bytes(caller, output, &bytes[..written]).is_err()
        || memory::write_u32(caller, length_output, written as u32).is_err()
    {
        ERRNO_FAULT
    } else {
        SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_ancillary_selectors_are_owned_options() {
        assert_eq!(option_kind(0, 8, 4), Some("receive-pktinfo-v4"));
        assert_eq!(option_kind(41, 66, 4), Some("receive-tclass-v6"));
        assert_eq!(option_kind(0, 7, 4), Some("unsupported-receive"));
        assert_eq!(option_kind(0, 1, 1), Some("ip-tos"));
        assert_eq!(option_kind(41, 67, 4), Some("ipv6-traffic-class"));
        assert_eq!(option_kind(6, 1, 4), Some("ignore"));
    }

    #[test]
    fn datagram_source_matches_the_owned_libc_layout() {
        let event = json!({ "remoteBytes": [127, 0, 0, 1], "remotePort": 5353 });
        let v4 = datagram_source(&event, LINUX_AF_INET).unwrap();
        assert_eq!(v4.len(), 16);
        assert_eq!(&v4[0..4], &[1, 0, 0x14, 0xe9]);
        assert_eq!(&v4[4..8], &[127, 0, 0, 1]);
        let mapped = datagram_source(&event, LINUX_AF_INET6).unwrap();
        assert_eq!(mapped.len(), 32);
        assert_eq!(&mapped[0..2], &[2, 0]);
        assert_eq!(&mapped[18..24], &[255, 255, 127, 0, 0, 1]);
    }

    #[test]
    fn addresses_are_unbracketed_for_the_owned_libc_parser() {
        let value = json!({ "address": "::", "port": 0 });
        assert_eq!(encode_address(&value, false), ":::0");
    }
}
