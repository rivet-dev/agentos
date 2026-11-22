//! Loading executable images through the kernel, for the initial module and
//! for exec replacements.

use crate::backend::{HostCallReply, HostServiceError, PayloadLimit};
use crate::host::{
    BoundedString, BoundedUsize, ExecutableImageSource, HostOperation, ProcessOperation,
};
use crate::WasmHostClient;
use agentos_executor_wasm_abi::StartWasmExecutionRequest;
use base64::Engine as _;
use serde_json::Value;

const MODULE_READ_CHUNK_BYTES: usize = 512 * 1024;
pub const DEFAULT_MAX_MODULE_FILE_BYTES: usize = 256 * 1024 * 1024;
/// Module paths with this prefix are trusted initial images resolved by the
/// sidecar rather than through the guest's view of the filesystem.
pub const TRUSTED_INITIAL_MODULE_PREFIX: &str = "agentos-trusted-initial:";

pub async fn load_module(
    host: &WasmHostClient,
    module_path: &str,
    request: &StartWasmExecutionRequest,
) -> Result<Vec<u8>, HostServiceError> {
    let path_limit = PayloadLimit::new("runtime.filesystem.maxPathBytes", 4096)?;
    let source = if let Some(path) = module_path.strip_prefix(TRUSTED_INITIAL_MODULE_PREFIX) {
        ExecutableImageSource::TrustedInitialPath(BoundedString::try_new(
            path.to_owned(),
            &path_limit,
        )?)
    } else {
        ExecutableImageSource::Path(BoundedString::try_new(module_path.to_owned(), &path_limit)?)
    };
    let maximum = request
        .limits
        .max_module_file_bytes
        .map(usize::try_from)
        .transpose()
        .map_err(|_| HostServiceError::new("EFBIG", "module byte limit does not fit usize"))?
        .unwrap_or(DEFAULT_MAX_MODULE_FILE_BYTES);
    load_executable_image(host, source, maximum).await
}

pub async fn load_executable_image(
    host: &WasmHostClient,
    source: ExecutableImageSource,
    maximum: usize,
) -> Result<Vec<u8>, HostServiceError> {
    let retained_request_bytes = match &source {
        ExecutableImageSource::TrustedInitialPath(path) | ExecutableImageSource::Path(path) => {
            path.as_str().len()
        }
        ExecutableImageSource::Descriptor(_) => std::mem::size_of::<u32>(),
    };
    let open = host
        .submit(
            HostOperation::Process(ProcessOperation::OpenExecutableImage {
                source,
                resolution: None,
            }),
            retained_request_bytes,
        )
        .await?;
    let (bytes, _) = read_open_executable_image(host, open, maximum).await?;
    Ok(bytes)
}

/// Both replacement launchers use the kernel's commit acknowledgment. A missing
/// tier is a protocol error after commit, so callers must retire the old image.
pub fn committed_exec_tier(
    reply: HostCallReply,
) -> Result<agentos_executor_wasm_abi::WasmPermissionTier, HostServiceError> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Commit {
        committed: bool,
        permission_tier: agentos_executor_wasm_abi::WasmPermissionTier,
    }
    if let HostCallReply::Json(value) = reply {
        if let Ok(Commit {
            committed: true,
            permission_tier,
        }) = serde_json::from_value(value)
        {
            return Ok(permission_tier);
        }
    }
    Err(HostServiceError::new(
        "EIO",
        "exec commit omitted confirmation or a valid kernel permission tier",
    ))
}

#[cfg(test)]
#[test]
fn replacement_uses_only_a_confirmed_kernel_tier() {
    use agentos_executor_wasm_abi::WasmPermissionTier::*;
    use serde_json::json;
    for (label, tier) in [
        ("isolated", Isolated),
        ("read-only", ReadOnly),
        ("read-write", ReadWrite),
        ("full", Full),
    ] {
        assert_eq!(
            committed_exec_tier(HostCallReply::Json(
                json!({"committed": true, "permissionTier": label})
            ))
            .unwrap(),
            tier
        );
    }
    for incomplete in [
        json!({"committed": true}),
        json!({"committed": false, "permissionTier": "full"}),
        json!({"committed": true, "permissionTier": "unknown"}),
    ] {
        assert_eq!(
            committed_exec_tier(HostCallReply::Json(incomplete))
                .unwrap_err()
                .code,
            "EIO"
        );
    }
}

pub async fn read_open_executable_image(
    host: &WasmHostClient,
    open: HostCallReply,
    maximum: usize,
) -> Result<(Vec<u8>, Option<Vec<String>>), HostServiceError> {
    read_open_executable_image_for_exec(host, open, maximum, false).await
}

pub async fn read_open_executable_image_for_exec(
    host: &WasmHostClient,
    open: HostCallReply,
    maximum: usize,
    retain: bool,
) -> Result<(Vec<u8>, Option<Vec<String>>), HostServiceError> {
    let (handle, size, argv) = decode_open_image(open)?;
    let result = read_module_image(host, handle, size, maximum).await;
    if retain && result.is_ok() {
        return result.map(|bytes| (bytes, argv));
    }
    let close = host
        .submit(
            HostOperation::Process(ProcessOperation::CloseExecutableImage { handle }),
            std::mem::size_of::<u64>(),
        )
        .await;
    match (result, close) {
        (Ok(bytes), Ok(_)) => Ok((bytes, argv)),
        (Err(error), Ok(_)) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(close_error)) => {
            eprintln!(
                "ERR_AGENTOS_WASMTIME_IMAGE_CLOSE: primary module-load error {}; image-close error {}",
                error, close_error
            );
            Err(error)
        }
    }
}

fn decode_open_image(
    reply: HostCallReply,
) -> Result<(u64, usize, Option<Vec<String>>), HostServiceError> {
    let HostCallReply::Json(value) = reply else {
        return Err(HostServiceError::new(
            "ERR_AGENTOS_WASMTIME_IMAGE_REPLY",
            "executable-image open returned a non-JSON reply",
        ));
    };
    let handle = value
        .get("handle")
        .and_then(Value::as_str)
        .ok_or_else(|| HostServiceError::new("EIO", "image reply is missing handle"))?
        .parse::<u64>()
        .map_err(|_| HostServiceError::new("EIO", "image handle is not a u64"))?;
    let size = value
        .get("size")
        .and_then(Value::as_u64)
        .and_then(|size| usize::try_from(size).ok())
        .ok_or_else(|| HostServiceError::new("EFBIG", "image size does not fit this platform"))?;
    let argv = value
        .get("argv")
        .filter(|value| !value.is_null())
        .map(|value| {
            serde_json::from_value::<Vec<String>>(value.clone()).map_err(|error| {
                HostServiceError::new("EIO", format!("executable-image argv is invalid: {error}"))
            })
        })
        .transpose()?;
    Ok((handle, size, argv))
}

async fn read_module_image(
    host: &WasmHostClient,
    handle: u64,
    size: usize,
    maximum: usize,
) -> Result<Vec<u8>, HostServiceError> {
    if size > maximum {
        return Err(HostServiceError::new(
            "ERR_AGENTOS_WASMTIME_MODULE_FILE_LIMIT",
            "WebAssembly module exceeds the configured executable-image limit",
        )
        .with_details(serde_json::json!({
            "limitName": "limits.resources.maxWasmModuleFileBytes",
            "limit": maximum,
            "observed": size,
        })));
    }
    // This bound describes the fixed chunk size selected by the runtime, not
    // guest consumption of a configurable resource. Using the normal warning
    // hook here would warn on every full-sized internal transfer.
    let read_limit = PayloadLimit::with_warning_hook(
        "limits.wasm.moduleReadChunkBytes",
        MODULE_READ_CHUNK_BYTES,
        None,
    )?;
    let mut bytes = Vec::with_capacity(size);
    while bytes.len() < size {
        let requested = (size - bytes.len()).min(MODULE_READ_CHUNK_BYTES);
        let reply = host
            .submit(
                HostOperation::Process(ProcessOperation::ReadExecutableImage {
                    handle,
                    offset: bytes.len() as u64,
                    max_bytes: BoundedUsize::try_new(requested, &read_limit)?,
                }),
                std::mem::size_of::<u64>() * 2 + std::mem::size_of::<usize>(),
            )
            .await?;
        let chunk = decode_image_bytes(reply)?;
        if chunk.is_empty() {
            return Err(HostServiceError::new(
                "EIO",
                "executable-image read returned EOF before its declared size",
            ));
        }
        if chunk.len() > requested || bytes.len().saturating_add(chunk.len()) > size {
            return Err(HostServiceError::new(
                "EIO",
                "executable-image read exceeded its requested or declared size",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn decode_image_bytes(reply: HostCallReply) -> Result<Vec<u8>, HostServiceError> {
    match reply {
        HostCallReply::Raw(bytes) => Ok(bytes),
        HostCallReply::Json(value) => {
            let encoded = value
                .get("base64")
                .and_then(Value::as_str)
                .filter(|_| value.get("__agentOSType").and_then(Value::as_str) == Some("bytes"))
                .ok_or_else(|| {
                    HostServiceError::new("EIO", "image read returned invalid encoded bytes")
                })?;
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| HostServiceError::new("EIO", "image read returned invalid base64"))
        }
        HostCallReply::Empty => Err(HostServiceError::new(
            "EIO",
            "image read returned an empty reply envelope",
        )),
    }
}
