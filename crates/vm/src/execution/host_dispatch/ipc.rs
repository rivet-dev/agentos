use super::*;
use crate::executor::host::{ProcessIpcInterest, ProcessIpcOperation, ProcessIpcTarget};

fn token(value: Option<&Value>, label: &str) -> Result<u64, VmError> {
    value
        .and_then(Value::as_str)
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| {
            VmError::host(
                "EINVAL",
                format!("IPC {label} must be a decimal u64 string"),
            )
        })
}

fn child(value: Option<&Value>) -> Result<Option<BoundedString>, VmError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => bounded_child_process_id(value).map(Some),
        _ => Err(VmError::host("EINVAL", "invalid IPC owned-child selector")),
    }
}

fn target(value: Option<&Value>) -> Result<ProcessIpcTarget, VmError> {
    let value = value
        .and_then(Value::as_object)
        .ok_or_else(|| VmError::host("EINVAL", "IPC target is required"))?;
    Ok(ProcessIpcTarget {
        child_id: child(value.get("childId"))?,
        generation: token(value.get("generation"), "generation")?,
    })
}

fn interests(
    value: Option<&Value>,
    max_bytes: usize,
) -> Result<BoundedVec<ProcessIpcInterest>, VmError> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| VmError::host("EINVAL", "IPC interests must be an array"))?;
    let limit = PayloadLimit::new("limits.reactor.maxBridgeRequestBytes", max_bytes / 64)
        .map_err(VmError::from)?;
    limit.admit(values.len()).map_err(VmError::from)?;
    let values = values
        .iter()
        .map(|value| {
            Ok(ProcessIpcInterest {
                target: target(Some(value))?,
                writable: match value.get("writable") {
                    None | Some(Value::Bool(false)) => false,
                    Some(Value::Bool(true)) => true,
                    _ => {
                        return Err(VmError::host(
                            "EINVAL",
                            "IPC writable interest must be boolean",
                        ))
                    }
                },
            })
        })
        .collect::<Result<Vec<_>, VmError>>()?;
    BoundedVec::try_new(values, &limit).map_err(VmError::from)
}

pub(super) fn decode(
    request: &HostRpcRequest,
    max_bytes: usize,
) -> Result<ProcessIpcOperation, VmError> {
    let args = &request.args;
    let limit = PayloadLimit::new(
        "limits.reactor.maxBridgeRequestBytes",
        max_bytes.min(64 * 1024),
    )
    .map_err(VmError::from)?;
    Ok(match request.method.as_str() {
        "process.ipc_info" => ProcessIpcOperation::Info {
            child_id: child(args.first())?,
        },
        "process.ipc_read" => ProcessIpcOperation::Read {
            target: target(args.first())?,
            max_bytes: BoundedUsize::try_new(
                usize::try_from(javascript_sync_rpc_arg_u64(args, 1, "IPC read bytes")?)
                    .map_err(|_| VmError::host("E2BIG", "IPC read size exceeds usize"))?,
                &limit,
            )
            .map_err(VmError::from)?,
        },
        "process.ipc_write" => ProcessIpcOperation::Write {
            target: target(args.first())?,
            chunk: BoundedBytes::try_new(
                javascript_sync_rpc_request_bytes_arg(request, 1, "IPC write bytes")?,
                &limit,
            )
            .map_err(VmError::from)?,
        },
        "process.ipc_close" => ProcessIpcOperation::Close {
            target: target(args.first())?,
        },
        "process.ipc_poll" => ProcessIpcOperation::Poll {
            poll_id: token(args.first(), "poll id")?,
            interests: interests(args.get(1), max_bytes)?,
        },
        "process.ipc_update" => ProcessIpcOperation::Update {
            poll_id: token(args.first(), "poll id")?,
            expected_revision: token(args.get(1), "interest revision")?,
            interests: interests(args.get(2), max_bytes)?,
        },
        _ => return Err(VmError::host("ENOSYS", "unknown private IPC operation")),
    })
}
