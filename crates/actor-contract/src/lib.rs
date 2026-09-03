#![forbid(unsafe_code)]

//! Transport support for the generated agentOS RivetKit contract.
//!
//! This crate owns compatibility decoding, bounded response encoding, and
//! stable public error conversion. Actor handlers should only apply hosted
//! policy and forward typed operations to Core.

use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context, Result};
use base64::Engine;
use rivet_error::{RivetError, RivetErrorKind};
use rivetkit::{Action, Actor, Ctx, Handles};

pub mod config;
pub mod cron;
pub mod events;
pub mod filesystem;
pub mod language;
pub mod lifecycle;
pub mod merge_patch;
pub mod network;
pub mod process;
#[cfg(feature = "contract")]
pub mod schema;
pub mod software;

pub type DispatchFuture = Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send>>;

pub const MAX_ACTION_INPUT_DEPTH: usize = 64;
pub const MAX_ACTION_INPUT_NODES: usize = 128 * 1024;
const MAX_BINARY_BYTES: usize = 768 * 1024;
const ACTION_ENVELOPE_RESERVE_BYTES: usize = 1024;

pub fn dispatch_typed<H, A>(
    actor: Arc<H>,
    ctx: Ctx<H>,
    args: &[u8],
    message_size_limit: usize,
) -> DispatchFuture
where
    H: Actor + Handles<A>,
    A: Action,
{
    let args = args.to_vec();
    Box::pin(async move {
        let action = decode_action_input::<A>(&args, message_size_limit)
            .with_context(|| format!("decode action '{}' args", A::NAME))
            .map_err(classify_public_error)?;
        let output = <H as Handles<A>>::handle(actor, ctx, action)
            .await
            .map_err(classify_public_error)?;
        encode_action_output(&output, message_size_limit).map_err(classify_public_error)
    })
}

/// Decode RivetKit's JSON-compatible scalar tags before typed CBOR decoding.
pub fn decode_action_input<A: serde::de::DeserializeOwned>(
    args: &[u8],
    message_size_limit: usize,
) -> Result<A> {
    if args.len() > message_size_limit {
        anyhow::bail!(
            "limit_exceeded: encoded action input exceeds the {message_size_limit}-byte actor message limit"
        );
    }
    if args.len() >= message_size_limit.saturating_mul(4) / 5 {
        tracing::warn!(
            limit = "actor_action_input_bytes",
            observed = args.len(),
            capacity = message_size_limit,
            "action input approaches the message limit; split the payload"
        );
    }
    if args.is_empty() {
        return rivetkit::action::decode_positional(args)
            .context("invalid_input: decode typed action input");
    }
    let mut value: ciborium::Value =
        ciborium::from_reader(args).context("invalid_input: decode action input CBOR")?;
    let mut remaining_nodes = MAX_ACTION_INPUT_NODES;
    normalize_client_tags(&mut value, 0, &mut remaining_nodes)?;
    if remaining_nodes <= MAX_ACTION_INPUT_NODES / 5 {
        tracing::warn!(
            limit = "actor_action_input_values",
            observed = MAX_ACTION_INPUT_NODES - remaining_nodes,
            capacity = MAX_ACTION_INPUT_NODES,
            "action input approaches the value-count limit; simplify the payload"
        );
    }
    let mut normalized = Vec::new();
    ciborium::into_writer(&value, &mut normalized)
        .context("invalid_input: encode normalized action input")?;
    rivetkit::action::decode_positional(&normalized)
        .context("invalid_input: decode typed action input")
}

#[doc(hidden)]
pub fn normalize_client_tags(
    value: &mut ciborium::Value,
    depth: usize,
    remaining_nodes: &mut usize,
) -> Result<()> {
    use ciborium::Value;
    if depth > MAX_ACTION_INPUT_DEPTH || *remaining_nodes == 0 {
        anyhow::bail!(
            "limit_exceeded: action input exceeds nesting depth {MAX_ACTION_INPUT_DEPTH} or {MAX_ACTION_INPUT_NODES} values; simplify the payload"
        );
    }
    *remaining_nodes -= 1;
    match value {
        Value::Array(values) => {
            let tag = values.first().and_then(Value::as_text);
            if values.len() == 2 && tag.is_some_and(|tag| tag.starts_with("$$")) {
                values[0] = Value::Text(tag.expect("escaped literal tag")[1..].to_owned());
                for item in values {
                    normalize_client_tags(item, depth + 1, remaining_nodes)?;
                }
            } else if matches!(tag, Some("$Undefined")) {
                if values.len() != 2 || values[1].as_integer().map(i128::from) != Some(0) {
                    anyhow::bail!("invalid_input: $Undefined requires the numeric sentinel 0");
                }
                *value = Value::Null;
            } else if matches!(tag, Some("$BigInt" | "$Uint8Array" | "$ArrayBuffer")) {
                let tag = tag.expect("recognized scalar tag").to_owned();
                if values.len() != 2 {
                    anyhow::bail!("invalid_input: {tag} requires exactly one string value");
                }
                let raw = values[1].as_text().ok_or_else(|| {
                    anyhow::anyhow!("invalid_input: {tag} requires a string value")
                })?;
                *value = if tag == "$BigInt" {
                    let digits = raw.strip_prefix('-').unwrap_or(raw);
                    if raw.len() > 21
                        || digits.is_empty()
                        || !digits.bytes().all(|byte| byte.is_ascii_digit())
                    {
                        anyhow::bail!(
                            "invalid_input: $BigInt must contain a decimal 64-bit integer"
                        );
                    }
                    let integer = raw
                        .parse::<i128>()
                        .context("invalid_input: invalid $BigInt integer")?;
                    Value::Integer(integer.try_into().map_err(|_| {
                        anyhow::anyhow!(
                            "invalid_input: $BigInt exceeds the CBOR 64-bit integer range"
                        )
                    })?)
                } else {
                    if raw.len() > MAX_BINARY_BYTES.div_ceil(3) * 4 {
                        anyhow::bail!("limit_exceeded: {tag} exceeds the 768 KiB binary input limit; split the payload");
                    }
                    let decoded = base64::engine::general_purpose::STANDARD
                        .decode(raw)
                        .with_context(|| format!("invalid_input: invalid {tag} base64"))?;
                    if decoded.len() > MAX_BINARY_BYTES {
                        anyhow::bail!("limit_exceeded: {tag} exceeds the 768 KiB binary input limit; split the payload");
                    }
                    Value::Bytes(decoded)
                };
            } else {
                for item in values {
                    normalize_client_tags(item, depth + 1, remaining_nodes)?;
                }
            }
        }
        Value::Map(entries) => {
            let mut retained = 0;
            for index in 0..entries.len() {
                let (key, value) = &mut entries[index];
                let omitted = matches!(value, Value::Array(values)
                    if values.first().and_then(Value::as_text) == Some("$Undefined"));
                normalize_client_tags(key, depth + 1, remaining_nodes)?;
                normalize_client_tags(value, depth + 1, remaining_nodes)?;
                if !omitted {
                    entries.swap(retained, index);
                    retained += 1;
                }
            }
            entries.truncate(retained);
        }
        Value::Tag(_, inner) => normalize_client_tags(inner, depth + 1, remaining_nodes)?,
        _ => {}
    }
    Ok(())
}

#[derive(Default)]
struct BoundedActionOutput {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl BoundedActionOutput {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }
}

impl Write for BoundedActionOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            self.exceeded = true;
            return Err(std::io::Error::other("actor output size limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn encode_action_output(
    output: &impl serde::Serialize,
    message_size_limit: usize,
) -> Result<Vec<u8>> {
    let output_size_limit = message_size_limit.saturating_sub(ACTION_ENVELOPE_RESERVE_BYTES);
    let mut encoded = BoundedActionOutput::new(output_size_limit);
    let result = ciborium::into_writer(output, &mut encoded);
    require_output_capacity(&encoded, output_size_limit, message_size_limit)?;
    result.context("encode action response as cbor")?;

    if let Ok(value) = ciborium::from_reader::<serde_json::Value, _>(encoded.bytes.as_slice()) {
        let mut json = BoundedActionOutput::new(output_size_limit);
        let result = serde_json::to_writer(&mut json, &value);
        require_output_capacity(&json, output_size_limit, message_size_limit)?;
        result.context("measure JSON action response")?;
    }
    Ok(encoded.bytes)
}

fn require_output_capacity(
    output: &BoundedActionOutput,
    output_size_limit: usize,
    message_size_limit: usize,
) -> Result<()> {
    if output.exceeded {
        anyhow::bail!(
            "limit_exceeded: encoded action response exceeds {output_size_limit} bytes; reduce the requested batch or maxBytes to fit the {message_size_limit}-byte actor message limit"
        );
    }
    Ok(())
}

pub fn classify_public_error(error: anyhow::Error) -> anyhow::Error {
    if error.chain().any(|cause| cause.is::<RivetError>()) {
        return error;
    }

    let code = error.chain().find_map(|cause| {
        if let Some(error) = cause.downcast_ref::<agentos_client::ClientError>() {
            use agentos_client::ClientError;
            let code = match error {
                ClientError::ExecutionTimedOut { .. } | ClientError::OperationTimedOut { .. } => {
                    Some("timeout")
                }
                ClientError::TerminationFailed { .. } => Some("termination_failed"),
                ClientError::ProcessNotFound(_)
                | ClientError::ShellNotFound(_)
                | ClientError::SoftwareNotFound(_) => Some("not_found"),
                ClientError::Kernel { code, .. } if code == "ENOENT" || code == "ESRCH" => {
                    Some("not_found")
                }
                ClientError::PathNotAbsolute(_)
                | ClientError::PathNotNormalized(_)
                | ClientError::InvalidSchedule(_)
                | ClientError::PastSchedule(_)
                | ClientError::InvalidPackageSource(_)
                | ClientError::InvalidPackageFormat(_)
                | ClientError::PackageDigestMismatch { .. } => Some("invalid_input"),
                ClientError::ResourceLimit { .. }
                | ClientError::PackageTooLarge { .. }
                | ClientError::PackageCacheCapacity { .. }
                | ClientError::PackageCacheEntryCapacity { .. }
                | ClientError::PackageCachePendingLimit { .. } => Some("limit_exceeded"),
                ClientError::PackageAcquisition { code, .. }
                    if matches!(
                        code.as_str(),
                        "invalid_package_source"
                            | "invalid_package_format"
                            | "package_digest_mismatch"
                    ) =>
                {
                    Some("invalid_input")
                }
                _ => None,
            };
            if code.is_some() {
                return code;
            }
        }
        let prefix = cause.to_string();
        let prefix = prefix
            .split_once(':')
            .map_or(prefix.as_str(), |(head, _)| head);
        match prefix {
            "config_conflict" | "revision_conflict" => Some("revision_conflict"),
            "invalid_input" => Some("invalid_input"),
            "limit_exceeded" => Some("limit_exceeded"),
            "stale_generation" => Some("stale_generation"),
            "not_found" | "software_not_found" => Some("not_found"),
            "not_ready" | "vm_not_ready" | "runtime_not_ready" => Some("not_ready"),
            "timeout" | "config_package_resolution_timeout" => Some("timeout"),
            "fetch_stream_expired" => Some("expired"),
            "termination_failed" => Some("termination_failed"),
            _ => None,
        }
    });
    let Some(code) = code else {
        return error;
    };

    let message = error.to_string();
    let mut metadata = serde_json::json!({
        "cause": format!("{error:#}"),
    });
    if let Some(details) = error.chain().find_map(|cause| {
        let client_error = cause.downcast_ref::<agentos_client::ClientError>()?;
        match client_error {
            agentos_client::ClientError::OperationTimedOut { details, .. }
            | agentos_client::ClientError::ResourceLimit { details, .. } => Some(details),
            _ => None,
        }
    }) {
        metadata["limit"] = serde_json::json!(details);
    }
    if let Some((package_code, details)) = error.chain().find_map(|cause| {
        let client_error = cause.downcast_ref::<agentos_client::ClientError>()?;
        match client_error {
            agentos_client::ClientError::PackageAcquisition { code, details, .. } => {
                Some((code, details))
            }
            _ => None,
        }
    }) {
        metadata["package"] = serde_json::json!(details);
        metadata["package"]["code"] = serde_json::json!(package_code);
    }
    let metadata = serde_json::value::to_raw_value(&metadata).ok();
    anyhow::Error::new(RivetError {
        kind: RivetErrorKind::Dynamic {
            group: String::from("agentos"),
            code: String::from(code),
            default_message: message.clone(),
        },
        meta: metadata,
        message: Some(message),
        actor: None,
    })
}
