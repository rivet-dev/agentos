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
use rivetkit::{Action, Actor, Ctx, Handler};

#[cfg(feature = "contract")]
pub mod schema;

mod limits;
pub use limits::{warn_near_budget, ActionBudgetViolation, ActionCodecLimits};

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
    H: Actor + Handler<A>,
    A: Action,
{
    dispatch_typed_with_limits::<H, A>(
        actor,
        ctx,
        args,
        ActionCodecLimits::for_message_size(message_size_limit),
    )
}

pub fn dispatch_typed_with_limits<H, A>(
    actor: Arc<H>,
    ctx: Ctx<H>,
    args: &[u8],
    limits: ActionCodecLimits,
) -> DispatchFuture
where
    H: Actor + Handler<A>,
    A: Action,
{
    let args = args.to_vec();
    Box::pin(async move {
        let action = decode_action_input_with_limits::<A>(&args, limits)
            .with_context(|| format!("decode action '{}' args", A::NAME))
            .map_err(classify_public_error)?;
        let output = <H as Handler<A>>::handle(actor, ctx, action)
            .await
            .map_err(classify_public_error)?;
        encode_action_output_with_limits(&output, limits).map_err(classify_public_error)
    })
}

/// Decode RivetKit's JSON-compatible scalar tags before typed CBOR decoding.
pub fn decode_action_input<A: serde::de::DeserializeOwned>(
    args: &[u8],
    message_size_limit: usize,
) -> Result<A> {
    decode_action_input_with_limits(
        args,
        ActionCodecLimits::for_message_size(message_size_limit),
    )
}

pub fn decode_action_input_with_limits<A: serde::de::DeserializeOwned>(
    args: &[u8],
    limits: ActionCodecLimits,
) -> Result<A> {
    let message_size_limit = limits.message_size_limit;
    if args.len() > message_size_limit {
        if limits.hosted_actor {
            return Err(ActionBudgetViolation {
                setting: "actor.transport.maxMessageBytes",
                observed: args.len(),
                limit: message_size_limit,
                units: "bytes",
                reason: "encoded action input exceeds the transport ceiling".into(),
            }
            .into());
        }
        anyhow::bail!(
            "limit_exceeded: encoded action input exceeds the {message_size_limit}-byte actor message limit"
        );
    }
    if limits.hosted_actor {
        warn_near_budget(
            "actor.transport.maxMessageBytes",
            args.len(),
            message_size_limit,
            "bytes",
        );
    } else if args.len() >= message_size_limit.saturating_mul(4) / 5 {
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
    let mut remaining_nodes = limits.max_input_values;
    let mut peak_depth = 0;
    normalize_client_tags_tracked(&mut value, 0, &mut remaining_nodes, limits, &mut peak_depth)?;
    if limits.hosted_actor {
        warn_near_budget(
            "actor.codec.maxInputValues",
            limits.max_input_values.saturating_sub(remaining_nodes),
            limits.max_input_values,
            "values",
        );
    } else if remaining_nodes <= limits.max_input_values / 5 {
        tracing::warn!(
            limit = "actor_action_input_values",
            observed = limits.max_input_values - remaining_nodes,
            capacity = limits.max_input_values,
            "action input approaches the value-count limit; simplify the payload"
        );
    }
    let mut normalized = Vec::new();
    ciborium::into_writer(&value, &mut normalized)
        .context("invalid_input: encode normalized action input")?;
    let decoded = rivetkit::action::decode_positional(&normalized)
        .context("invalid_input: decode typed action input")?;
    if limits.hosted_actor {
        warn_near_budget(
            "actor.codec.maxInputDepth",
            peak_depth,
            limits.max_input_depth,
            "levels",
        );
    }
    Ok(decoded)
}

#[doc(hidden)]
pub fn normalize_client_tags(
    value: &mut ciborium::Value,
    depth: usize,
    remaining_nodes: &mut usize,
) -> Result<()> {
    normalize_client_tags_with_limits(
        value,
        depth,
        remaining_nodes,
        ActionCodecLimits::for_message_size(usize::MAX),
    )
}

#[doc(hidden)]
pub fn normalize_client_tags_with_limits(
    value: &mut ciborium::Value,
    depth: usize,
    remaining_nodes: &mut usize,
    limits: ActionCodecLimits,
) -> Result<()> {
    normalize_client_tags_tracked(value, depth, remaining_nodes, limits, &mut 0)
}

fn normalize_client_tags_tracked(
    value: &mut ciborium::Value,
    depth: usize,
    remaining_nodes: &mut usize,
    limits: ActionCodecLimits,
    peak_depth: &mut usize,
) -> Result<()> {
    use ciborium::Value;
    *peak_depth = (*peak_depth).max(depth);
    if depth > limits.max_input_depth || *remaining_nodes == 0 {
        if limits.hosted_actor {
            let (setting, observed, limit, units) = if depth > limits.max_input_depth {
                (
                    "actor.codec.maxInputDepth",
                    depth,
                    limits.max_input_depth,
                    "levels",
                )
            } else {
                (
                    "actor.codec.maxInputValues",
                    limits.max_input_values.saturating_add(1),
                    limits.max_input_values,
                    "values",
                )
            };
            return Err(ActionBudgetViolation {
                setting,
                observed,
                limit,
                units,
                reason: "action input exceeds the configured structural budget".into(),
            }
            .into());
        }
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
                    normalize_client_tags_tracked(
                        item,
                        depth + 1,
                        remaining_nodes,
                        limits,
                        peak_depth,
                    )?;
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
                    let encoded_limit = limits.max_binary_bytes.div_ceil(3).checked_mul(4)
                        .ok_or_else(|| anyhow::anyhow!("invalid_input: codec.maxBinaryBytes base64 expansion exceeds platform usize"))?;
                    if raw.len() > encoded_limit {
                        if limits.hosted_actor {
                            return Err(ActionBudgetViolation { setting: "actor.codec.maxBinaryBytes", observed: raw.len(), limit: encoded_limit, units: "base64 bytes", reason: "encoded binary representation exceeds the binary budget expansion".into() }.into());
                        }
                        anyhow::bail!("limit_exceeded: {tag} exceeds the 768 KiB binary input limit; split the payload");
                    }
                    let decoded = base64::engine::general_purpose::STANDARD
                        .decode(raw)
                        .with_context(|| format!("invalid_input: invalid {tag} base64"))?;
                    if decoded.len() > limits.max_binary_bytes {
                        if limits.hosted_actor {
                            return Err(ActionBudgetViolation {
                                setting: "actor.codec.maxBinaryBytes",
                                observed: decoded.len(),
                                limit: limits.max_binary_bytes,
                                units: "bytes",
                                reason: "decoded binary input exceeds the configured ceiling"
                                    .into(),
                            }
                            .into());
                        }
                        anyhow::bail!("limit_exceeded: {tag} exceeds the 768 KiB binary input limit; split the payload");
                    }
                    if limits.hosted_actor {
                        warn_near_budget(
                            "actor.codec.maxBinaryBytes",
                            decoded.len(),
                            limits.max_binary_bytes,
                            "bytes",
                        );
                    }
                    Value::Bytes(decoded)
                };
            } else {
                for item in values {
                    normalize_client_tags_tracked(
                        item,
                        depth + 1,
                        remaining_nodes,
                        limits,
                        peak_depth,
                    )?;
                }
            }
        }
        Value::Map(entries) => {
            let mut retained = 0;
            for index in 0..entries.len() {
                let (key, value) = &mut entries[index];
                let omitted = matches!(value, Value::Array(values)
                    if values.first().and_then(Value::as_text) == Some("$Undefined"));
                normalize_client_tags_tracked(key, depth + 1, remaining_nodes, limits, peak_depth)?;
                normalize_client_tags_tracked(
                    value,
                    depth + 1,
                    remaining_nodes,
                    limits,
                    peak_depth,
                )?;
                if !omitted {
                    entries.swap(retained, index);
                    retained += 1;
                }
            }
            entries.truncate(retained);
        }
        Value::Bytes(bytes) if limits.hosted_actor => {
            if bytes.len() > limits.max_binary_bytes {
                return Err(ActionBudgetViolation {
                    setting: "actor.codec.maxBinaryBytes",
                    observed: bytes.len(),
                    limit: limits.max_binary_bytes,
                    units: "bytes",
                    reason: "native CBOR binary input exceeds the configured ceiling".into(),
                }
                .into());
            }
            warn_near_budget(
                "actor.codec.maxBinaryBytes",
                bytes.len(),
                limits.max_binary_bytes,
                "bytes",
            );
        }
        Value::Tag(_, inner) => {
            normalize_client_tags_tracked(inner, depth + 1, remaining_nodes, limits, peak_depth)?
        }
        _ => {}
    }
    Ok(())
}

#[derive(Default)]
struct BoundedActionOutput {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
    observed: usize,
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
        self.observed = self.bytes.len().saturating_add(bytes.len());
        if self.observed > self.limit {
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
    encode_action_output_with_limits(
        output,
        ActionCodecLimits::for_message_size(message_size_limit),
    )
}

pub fn encode_action_output_with_limits(
    output: &impl serde::Serialize,
    limits: ActionCodecLimits,
) -> Result<Vec<u8>> {
    let message_size_limit = limits.message_size_limit;
    let output_size_limit = message_size_limit.saturating_sub(ACTION_ENVELOPE_RESERVE_BYTES);
    let mut encoded = BoundedActionOutput::new(output_size_limit);
    let result = ciborium::into_writer(output, &mut encoded);
    require_output_capacity(&encoded, output_size_limit, limits)?;
    result.context("encode action response as cbor")?;

    let mut measured_output_size = encoded.bytes.len();
    if let Ok(value) = ciborium::from_reader::<serde_json::Value, _>(encoded.bytes.as_slice()) {
        let mut json = BoundedActionOutput::new(output_size_limit);
        let result = serde_json::to_writer(&mut json, &value);
        require_output_capacity(&json, output_size_limit, limits)?;
        result.context("measure JSON action response")?;
        measured_output_size = measured_output_size.max(json.bytes.len());
    }
    if limits.hosted_actor {
        warn_near_budget(
            "actor.transport.maxMessageBytes",
            measured_output_size.saturating_add(ACTION_ENVELOPE_RESERVE_BYTES),
            message_size_limit,
            "bytes including reserved envelope",
        );
    }
    Ok(encoded.bytes)
}

fn require_output_capacity(
    output: &BoundedActionOutput,
    output_size_limit: usize,
    limits: ActionCodecLimits,
) -> Result<()> {
    let message_size_limit = limits.message_size_limit;
    if output.exceeded {
        if limits.hosted_actor {
            return Err(ActionBudgetViolation {
                setting: "actor.transport.maxMessageBytes",
                observed: output
                    .observed
                    .saturating_add(ACTION_ENVELOPE_RESERVE_BYTES),
                limit: message_size_limit,
                units: "bytes including reserved envelope",
                reason: "encoded action response exceeds the bounded CBOR/JSON transport budget"
                    .into(),
            }
            .into());
        }
        anyhow::bail!(
            "limit_exceeded: encoded action response exceeds {output_size_limit} bytes; reduce the requested batch or maxBytes to fit the {message_size_limit}-byte actor message limit"
        );
    }
    Ok(())
}

// Only the trusted filesystem RPC's complete typed quota contract is public.
// Native wire counters remain u64; hosted JSON/CBOR uses exact decimal strings.
fn public_filesystem_quota(
    error: &anyhow::Error,
) -> Option<(&'static str, u64, u64, &'static str)> {
    let (code, details) = error.chain().find_map(|cause| {
        match cause.downcast_ref::<agentos_client::ClientError>()? {
            agentos_client::ClientError::ResourceLimit { code, details, .. } => {
                Some((code, details))
            }
            _ => None,
        }
    })?;
    if code != "ERR_AGENTOS_RESOURCE_LIMIT"
        || details.errno.as_deref() != Some("ENOSPC")
        || details.current_usage.is_some()
    {
        return None;
    }
    let (setting, units) = match (
        details.limit_name.as_deref(),
        details.configuration_path.as_deref(),
        details.unit.as_deref(),
    ) {
        (
            Some("limits.resources.maxFilesystemBytes"),
            Some("limits.resources.maxFilesystemBytes"),
            Some("bytes"),
        ) => ("limits.resources.maxFilesystemBytes", "bytes"),
        (
            Some("limits.resources.maxInodeCount"),
            Some("limits.resources.maxInodeCount"),
            Some("inodes"),
        ) => ("limits.resources.maxInodeCount", "inodes"),
        _ => return None,
    };
    let limit = details.configured_limit?;
    let observed = details.requested?;
    (observed > limit).then_some((setting, limit, observed, units))
}

pub fn classify_public_error(error: anyhow::Error) -> anyhow::Error {
    if error.chain().any(|cause| cause.is::<RivetError>()) {
        return error;
    }

    // Only an owning typed budget diagnostic is deliberately public. Generic
    // string classification below may contain private context and stays masked.
    if let Some(budget) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<ActionBudgetViolation>())
    {
        let remediation = budget.remediation();
        let message = format!(
            "limit_exceeded: {}: observed {} {}, limit {} {}; {}",
            budget.setting, budget.observed, budget.units, budget.limit, budget.units, remediation
        );
        let metadata = match serde_json::value::to_raw_value(&serde_json::json!({"budget": {
            "setting": budget.setting, "observed": budget.observed, "limit": budget.limit,
            "units": budget.units, "remediation": remediation,
        }})) {
            Ok(metadata) => metadata,
            Err(error) => {
                return anyhow::Error::new(error).context("serialize public actor budget")
            }
        };
        return anyhow::Error::new(RivetError {
            kind: RivetErrorKind::Dynamic {
                group: String::from("user"),
                code: String::from("limit_exceeded"),
                default_message: message.clone(),
            },
            meta: Some(metadata),
            message: Some(message),
            actor: None,
        });
    }

    if let Some((setting, limit, observed, units)) = public_filesystem_quota(&error) {
        let remediation =
            format!("reduce filesystem usage or raise {setting} in the VM configuration");
        let message = format!("limit_exceeded: {setting}: observed {observed} {units}, limit {limit} {units}; {remediation}");
        let metadata = match serde_json::value::to_raw_value(&serde_json::json!({"limit": {
            "setting": setting, "limit": limit.to_string(), "observed": observed.to_string(),
            "units": units, "remediation": remediation,
        }})) {
            Ok(metadata) => metadata,
            Err(error) => {
                return anyhow::Error::new(error).context("serialize public filesystem quota")
            }
        };
        return anyhow::Error::new(RivetError {
            kind: RivetErrorKind::Dynamic {
                group: String::from("user"),
                code: String::from("limit_exceeded"),
                default_message: message.clone(),
            },
            meta: Some(metadata),
            message: Some(message),
            actor: None,
        });
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
    if let Some(budget) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<ActionBudgetViolation>())
    {
        metadata["budget"] = serde_json::json!({"setting": budget.setting, "observed": budget.observed, "limit": budget.limit, "units": budget.units});
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
