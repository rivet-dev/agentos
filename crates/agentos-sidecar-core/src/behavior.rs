//! Pure ACP behavior shared by native and browser sidecar adapters.
//!
//! This module deliberately contains no host I/O or scheduling. Adapters remain
//! responsible for process execution and event delivery while using these
//! helpers for parsing and state transitions that must not drift by backend.

use std::collections::BTreeMap;

use agentos_protocol::generated::v1::AcpSessionEvent;
use serde_json::{json, Map, Value};

use crate::session::AcpSessionRecord;
use crate::AcpCoreError;

pub const ACP_SESSION_CANCEL_METHOD: &str = "session/cancel";
/// Default maximum bytes retained for one newline-delimited adapter message.
/// Native and browser adapters consume this shared default; explicit VM limits
/// may lower or raise the value at their host boundary.
pub const DEFAULT_ACP_MAX_READ_LINE_BYTES: usize = 16 * 1024 * 1024;
pub const AGENTOS_SYSTEM_PROMPT: &str = include_str!("AGENTOS_SYSTEM_PROMPT.md");
pub const OPENCODE_SYSTEM_PROMPT_PATH: &str = "/tmp/agentos-system-prompt.md";
pub const OPENCODE_CONTEXT_PATHS_ENV: &str = "OPENCODE_CONTEXTPATHS";

/// Semantic JSON-RPC message classes used by every ACP stdio driver.
///
/// The order is intentional: a message containing both `id` and `method` is an
/// agent-to-host request, never a notification, even if its id does not match the
/// host's currently pending request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpJsonRpcMessageKind {
    InboundRequest,
    Response,
    Notification,
}

pub fn classify_json_rpc_message(message: &Value) -> Result<AcpJsonRpcMessageKind, AcpCoreError> {
    match (
        message.get("id").is_some(),
        message.get("method").and_then(Value::as_str).is_some(),
    ) {
        (true, true) => Ok(AcpJsonRpcMessageKind::InboundRequest),
        (true, false) => Ok(AcpJsonRpcMessageKind::Response),
        (false, true) => Ok(AcpJsonRpcMessageKind::Notification),
        (false, false) if message.get("result").is_some() || message.get("error").is_some() => {
            Err(AcpCoreError::InvalidState(String::from(
                "ACP adapter emitted invalid JSON-RPC response: response is missing id",
            )))
        }
        (false, false) => Err(AcpCoreError::InvalidState(String::from(
            "ACP adapter emitted invalid JSON-RPC envelope: message has neither a string method nor an id",
        ))),
    }
}

/// Canonical response when an ACP host cannot execute an inbound adapter request.
pub fn unsupported_inbound_request_response(request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32601,
            "message": format!("method not found: {method}"),
            "data": { "method": method },
        },
    })
}

const OPENCODE_DEFAULT_CONTEXT_PATHS: [&str; 11] = [
    ".github/copilot-instructions.md",
    ".cursorrules",
    ".cursor/rules/",
    "CLAUDE.md",
    "CLAUDE.local.md",
    "opencode.md",
    "opencode.local.md",
    "OpenCode.md",
    "OpenCode.local.md",
    "OPENCODE.md",
    "OPENCODE.local.md",
];

/// A guest-file write required to deliver an adapter launch prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPromptFileWrite {
    pub path: String,
    pub contents: String,
}

/// Backend-neutral prompt changes for one ACP adapter launch.
///
/// Adapters apply exactly one variant to their host-specific spawn request. The
/// plan contains no host handles and performs no filesystem access itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpLaunchPromptPlan {
    None,
    AppendArgument {
        flag: String,
        prompt: String,
    },
    OpenCodeContext {
        env_key: String,
        env_value: String,
        file: AcpPromptFileWrite,
    },
}

/// Build the adapter-specific launch plan for the shared agentOS prompt.
///
/// `base_system_prompt` is explicit so tests and alternative distributions may
/// supply another base while production adapters share [`AGENTOS_SYSTEM_PROMPT`].
/// Prompt ordering and separators match the native ACP adapter: base, caller
/// instructions, host-tool reference, then `\n\n---`.
pub fn plan_acp_launch_prompt(
    agent_type: &str,
    base_system_prompt: &str,
    skip_base: bool,
    additional_instructions: Option<&str>,
    host_tool_reference: &str,
    env: &BTreeMap<String, String>,
) -> Result<AcpLaunchPromptPlan, AcpCoreError> {
    let prompt = assemble_system_prompt(
        base_system_prompt,
        skip_base,
        additional_instructions,
        host_tool_reference,
    );
    if prompt.is_empty() {
        return Ok(AcpLaunchPromptPlan::None);
    }

    let plan = match agent_type {
        "pi" | "pi-cli" | "claude" => AcpLaunchPromptPlan::AppendArgument {
            flag: String::from("--append-system-prompt"),
            prompt,
        },
        "codex" => AcpLaunchPromptPlan::AppendArgument {
            flag: String::from("--append-developer-instructions"),
            prompt,
        },
        "opencode" if !env.contains_key(OPENCODE_CONTEXT_PATHS_ENV) => {
            let mut context_paths = OPENCODE_DEFAULT_CONTEXT_PATHS
                .iter()
                .map(|path| path.to_string())
                .collect::<Vec<_>>();
            context_paths.push(OPENCODE_SYSTEM_PROMPT_PATH.to_string());
            let env_value = serde_json::to_string(&context_paths).map_err(|error| {
                AcpCoreError::InvalidState(format!(
                    "failed to serialize OpenCode context paths: {error}"
                ))
            })?;
            AcpLaunchPromptPlan::OpenCodeContext {
                env_key: OPENCODE_CONTEXT_PATHS_ENV.to_string(),
                env_value,
                file: AcpPromptFileWrite {
                    path: OPENCODE_SYSTEM_PROMPT_PATH.to_string(),
                    contents: prompt,
                },
            }
        }
        _ => AcpLaunchPromptPlan::None,
    };
    Ok(plan)
}

fn assemble_system_prompt(
    base_system_prompt: &str,
    skip_base: bool,
    additional_instructions: Option<&str>,
    host_tool_reference: &str,
) -> String {
    let mut parts = Vec::new();
    if !skip_base {
        let base = base_system_prompt.trim_end();
        if !base.is_empty() {
            parts.push(base);
        }
    }
    if let Some(additional) = additional_instructions {
        if !additional.is_empty() {
            parts.push(additional);
        }
    }
    if !host_tool_reference.is_empty() {
        parts.push(host_tool_reference);
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{}\n\n---", parts.join("\n\n"))
    }
}

/// Incremental, bounded parser for newline-delimited ACP JSON-RPC messages.
///
/// The limit has the native adapter's semantics: it applies to bytes before the
/// newline, an exactly-at-limit line is accepted, whitespace-only lines are
/// ignored, and an unterminated partial line may not exceed the limit.
#[derive(Debug, Default)]
pub struct AcpJsonLineAccumulator {
    buffer: String,
}

impl AcpJsonLineAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_buffer(buffer: String) -> Self {
        Self { buffer }
    }

    pub fn retained(&self) -> &str {
        &self.buffer
    }

    pub fn into_retained(self) -> String {
        self.buffer
    }

    /// Append one output chunk and return every complete JSON value it contains.
    /// Invalid complete JSON is a typed error rather than an ignored adapter line.
    pub fn push_json(
        &mut self,
        chunk: &[u8],
        max_line_bytes: usize,
    ) -> Result<Vec<Value>, AcpCoreError> {
        self.buffer.push_str(&String::from_utf8_lossy(chunk));

        let mut messages = Vec::new();
        while let Some(index) = self.buffer.find('\n') {
            if index > max_line_bytes {
                return Err(line_limit_error(max_line_bytes));
            }
            let line = self.buffer[..index].trim().to_owned();
            self.buffer = self.buffer[index + 1..].to_owned();
            if line.is_empty() {
                continue;
            }
            messages.push(serde_json::from_str(&line).map_err(|error| {
                AcpCoreError::InvalidState(format!("ACP adapter emitted invalid JSON-RPC: {error}"))
            })?);
        }

        if self.buffer.len() > max_line_bytes {
            return Err(line_limit_error(max_line_bytes));
        }
        Ok(messages)
    }
}

fn line_limit_error(max_line_bytes: usize) -> AcpCoreError {
    AcpCoreError::InvalidState(format!(
        "ACP adapter emitted a line longer than {max_line_bytes} bytes"
    ))
}

/// Backend-neutral session fields derived from initialize + session/new/load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpBootstrapFields {
    pub modes: Option<String>,
    pub config_options: Vec<String>,
    pub agent_capabilities: Option<String>,
    pub agent_info: Option<String>,
}

/// Derive the canonical state returned by create/resume handshakes.
///
/// Session results override initialize results. If neither result supplies a
/// model config option, adapters that expose the older `models` shape receive a
/// derived model option matching native behavior.
pub fn derive_bootstrap_fields(
    agent_type: &str,
    init_result: &Map<String, Value>,
    session_result: &Map<String, Value>,
    agent_capabilities: Option<&Value>,
) -> Result<AcpBootstrapFields, AcpCoreError> {
    let mut config_options = config_option_values(init_result, "initialize")?;
    if let Some(session_options) = session_result.get("configOptions") {
        match session_options {
            // Native treats an absent/null session override as no override, so
            // initialize-time options remain authoritative in this case.
            Value::Null => {}
            Value::Array(options) => config_options = options.clone(),
            _ => {
                return Err(AcpCoreError::InvalidState(String::from(
                    "ACP session configOptions must be an array",
                )))
            }
        }
    }
    if !config_options
        .iter()
        .enumerate()
        .map(|(index, option)| is_model_config_option(option, index))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .any(|is_model| is_model)
    {
        config_options.extend(derive_model_config_options(agent_type, session_result)?);
    }

    Ok(AcpBootstrapFields {
        modes: serialize_optional_json(
            session_result
                .get("modes")
                .or_else(|| init_result.get("modes")),
        )?,
        config_options: serialize_json_values(&config_options, "ACP config option")?,
        agent_capabilities: serialize_optional_json(agent_capabilities)?,
        agent_info: serialize_optional_json(init_result.get("agentInfo"))?,
    })
}

fn config_option_values(
    source: &Map<String, Value>,
    source_name: &str,
) -> Result<Vec<Value>, AcpCoreError> {
    match source.get("configOptions") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(options)) => Ok(options.clone()),
        Some(_) => Err(AcpCoreError::InvalidState(format!(
            "ACP {source_name} configOptions must be an array"
        ))),
    }
}

fn is_model_config_option(value: &Value, index: usize) -> Result<bool, AcpCoreError> {
    let object = value.as_object().ok_or_else(|| {
        AcpCoreError::InvalidState(format!("ACP config option {index} must be an object"))
    })?;
    let id = optional_string_field(object, "id", &format!("ACP config option {index}"))?;
    let category =
        optional_string_field(object, "category", &format!("ACP config option {index}"))?;
    Ok(id == Some("model") || category == Some("model"))
}

fn derive_model_config_options(
    agent_type: &str,
    session_result: &Map<String, Value>,
) -> Result<Vec<Value>, AcpCoreError> {
    let Some(models_value) = session_result.get("models") else {
        return Ok(Vec::new());
    };
    if models_value.is_null() {
        return Ok(Vec::new());
    }
    let models = models_value.as_object().ok_or_else(|| {
        AcpCoreError::InvalidState(String::from("ACP session models must be an object"))
    })?;
    let current_model_id = optional_string_field(models, "currentModelId", "ACP session models")?;
    let available_models = match models.get("availableModels") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(models)) => models.as_slice(),
        Some(_) => {
            return Err(AcpCoreError::InvalidState(String::from(
                "ACP session models.availableModels must be an array",
            )))
        }
    };
    let mut allowed_values = Vec::with_capacity(available_models.len());
    for (index, model) in available_models.iter().enumerate() {
        let model = model.as_object().ok_or_else(|| {
            AcpCoreError::InvalidState(format!(
                "ACP session available model {index} must be an object"
            ))
        })?;
        let model_id = model
            .get("modelId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                AcpCoreError::InvalidState(format!(
                    "ACP session available model {index} missing modelId"
                ))
            })?;
        let mut allowed =
            Map::from_iter([(String::from("id"), Value::String(model_id.to_string()))]);
        if let Some(name) = optional_string_field(
            model,
            "name",
            &format!("ACP session available model {index}"),
        )? {
            allowed.insert(String::from("label"), Value::String(name.to_string()));
        }
        allowed_values.push(Value::Object(allowed));
    }
    if current_model_id.is_none() && allowed_values.is_empty() {
        return Ok(Vec::new());
    }

    let mut option = Map::from_iter([
        (String::from("id"), Value::String(String::from("model"))),
        (
            String::from("category"),
            Value::String(String::from("model")),
        ),
        (String::from("label"), Value::String(String::from("Model"))),
        (String::from("allowedValues"), Value::Array(allowed_values)),
        (
            String::from("readOnly"),
            Value::Bool(agent_type == "opencode"),
        ),
    ]);
    if let Some(current_model_id) = current_model_id {
        option.insert(
            String::from("currentValue"),
            Value::String(current_model_id.to_string()),
        );
    }
    if agent_type == "opencode" {
        option.insert(
            String::from("description"),
            Value::String(String::from(
                "Available models reported by OpenCode. Model switching must be configured before createSession() because ACP session/set_config_option is not implemented.",
            )),
        );
    }
    Ok(vec![Value::Object(option)])
}

fn optional_string_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<Option<&'a str>, AcpCoreError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(AcpCoreError::InvalidState(format!(
            "{label}.{field} must be a string"
        ))),
    }
}

fn serialize_optional_json(value: Option<&Value>) -> Result<Option<String>, AcpCoreError> {
    value
        .map(|value| {
            serde_json::to_string(value).map_err(|error| {
                AcpCoreError::InvalidState(format!("failed to serialize ACP JSON field: {error}"))
            })
        })
        .transpose()
}

fn serialize_json_values(values: &[Value], label: &str) -> Result<Vec<String>, AcpCoreError> {
    values
        .iter()
        .map(|value| {
            serde_json::to_string(value).map_err(|error| {
                AcpCoreError::InvalidState(format!("failed to serialize {label}: {error}"))
            })
        })
        .collect()
}

/// One decoded, session-scoped adapter notification.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpSessionNotification {
    pub session_id: String,
    pub notification: Value,
}

impl AcpSessionNotification {
    /// Decode a wire session event without silently discarding malformed JSON.
    pub fn from_wire(event: &AcpSessionEvent) -> Result<Self, AcpCoreError> {
        let notification = serde_json::from_str(&event.notification).map_err(|error| {
            AcpCoreError::InvalidState(format!("invalid ACP session notification JSON: {error}"))
        })?;
        Ok(Self {
            session_id: event.session_id.clone(),
            notification,
        })
    }
}

/// Synthetic event required when an adapter accepted a state change but did not
/// emit the corresponding `session/update` notification itself.
#[derive(Debug, Clone, PartialEq)]
pub enum AcpSyntheticSessionUpdate {
    CurrentMode { mode_id: String },
    ConfigOptions { config_options: Vec<Value> },
}

impl AcpSyntheticSessionUpdate {
    pub fn notification(&self) -> Value {
        match self {
            Self::CurrentMode { mode_id } => json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "update": {
                        "sessionUpdate": "current_mode_update",
                        "currentModeId": mode_id,
                    },
                },
            }),
            Self::ConfigOptions { config_options } => json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "update": {
                        "sessionUpdate": "config_option_update",
                        "configOptions": config_options,
                    },
                },
            }),
        }
    }
}

/// Apply local state after a successful adapter request and return a synthetic
/// update only when the adapter did not already emit the matching update.
pub fn apply_successful_session_request(
    session: &mut AcpSessionRecord,
    method: &str,
    params: &Map<String, Value>,
    adapter_notifications: &[AcpSessionNotification],
) -> Result<Option<AcpSyntheticSessionUpdate>, AcpCoreError> {
    apply_successful_session_fields(
        &session.session_id,
        &mut session.modes,
        &mut session.config_options,
        method,
        params,
        adapter_notifications,
    )
}

/// Backend-neutral form used by native adapter records that carry additional
/// host-only restart/terminal state around these shared behavioral fields.
pub fn apply_successful_session_fields(
    session_id: &str,
    modes: &mut Option<String>,
    config_options: &mut Vec<String>,
    method: &str,
    params: &Map<String, Value>,
    adapter_notifications: &[AcpSessionNotification],
) -> Result<Option<AcpSyntheticSessionUpdate>, AcpCoreError> {
    if method == "session/set_mode" {
        let Some(mode_id) = params.get("modeId").and_then(Value::as_str) else {
            return Ok(None);
        };
        apply_local_mode_update(modes, mode_id)?;
        if !has_matching_session_update(adapter_notifications, session_id, |update| {
            update.get("sessionUpdate").and_then(Value::as_str) == Some("current_mode_update")
                && update.get("currentModeId").and_then(Value::as_str) == Some(mode_id)
        }) {
            return Ok(Some(AcpSyntheticSessionUpdate::CurrentMode {
                mode_id: mode_id.to_string(),
            }));
        }
    }

    if method == "session/set_config_option" {
        let Some(config_id) = params.get("configId").and_then(Value::as_str) else {
            return Ok(None);
        };
        let Some(value) = params.get("value") else {
            return Ok(None);
        };
        apply_local_config_update(config_options, config_id, value)?;
        if !has_matching_session_update(adapter_notifications, session_id, |update| {
            update
                .get("sessionUpdate")
                .and_then(Value::as_str)
                .is_some_and(|kind| {
                    kind == "config_option_update" || kind == "config_options_update"
                })
        }) {
            let config_options = config_options
                .iter()
                .enumerate()
                .map(|(index, option)| {
                    serde_json::from_str(option).map_err(|error| {
                        AcpCoreError::InvalidState(format!(
                            "malformed ACP config option {index}: {error}"
                        ))
                    })
                })
                .collect::<Result<Vec<Value>, _>>()?;
            return Ok(Some(AcpSyntheticSessionUpdate::ConfigOptions {
                config_options,
            }));
        }
    }

    Ok(None)
}

fn apply_local_mode_update(modes: &mut Option<String>, mode_id: &str) -> Result<(), AcpCoreError> {
    let Some(modes) = modes.as_mut() else {
        return Ok(());
    };
    let mut value: Value = serde_json::from_str(modes)
        .map_err(|error| AcpCoreError::InvalidState(format!("invalid ACP modes JSON: {error}")))?;
    if let Value::Object(object) = &mut value {
        object.insert(
            String::from("currentModeId"),
            Value::String(mode_id.to_string()),
        );
        *modes = serde_json::to_string(&value).map_err(|error| {
            AcpCoreError::InvalidState(format!("failed to serialize ACP modes: {error}"))
        })?;
    }
    Ok(())
}

fn apply_local_config_update(
    config_options: &mut Vec<String>,
    config_id: &str,
    value: &Value,
) -> Result<(), AcpCoreError> {
    let mut updated = false;
    let mut next = Vec::with_capacity(config_options.len());
    for (index, option) in config_options.iter().enumerate() {
        let mut option: Value = serde_json::from_str(option).map_err(|error| {
            AcpCoreError::InvalidState(format!("malformed ACP config option {index}: {error}"))
        })?;
        let object = option.as_object_mut().ok_or_else(|| {
            AcpCoreError::InvalidState(format!("ACP config option {index} must be an object"))
        })?;
        let option_id = object.get("id").and_then(Value::as_str).ok_or_else(|| {
            AcpCoreError::InvalidState(format!("ACP config option {index} missing id"))
        })?;
        if option_id == config_id {
            object.insert(String::from("currentValue"), value.clone());
            updated = true;
        }
        next.push(serde_json::to_string(&option).map_err(|error| {
            AcpCoreError::InvalidState(format!("failed to serialize ACP config option: {error}"))
        })?);
    }
    if !updated {
        return Err(AcpCoreError::InvalidState(format!(
            "unknown ACP config option {config_id}"
        )));
    }
    *config_options = next;
    Ok(())
}

fn has_matching_session_update(
    notifications: &[AcpSessionNotification],
    session_id: &str,
    predicate: impl Fn(&Map<String, Value>) -> bool,
) -> bool {
    notifications.iter().any(|event| {
        if event.session_id != session_id
            || event.notification.get("method").and_then(Value::as_str) != Some("session/update")
        {
            return false;
        }
        let Some(params) = event.notification.get("params").and_then(Value::as_object) else {
            return false;
        };
        let update = params
            .get("update")
            .and_then(Value::as_object)
            .unwrap_or(params);
        predicate(update)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpCancelFallbackDecision {
    ReturnAdapterResponse,
    SendNotification,
}

/// Decide whether an unsupported `session/cancel` request should fall back to
/// the ACP cancellation notification. Other methods and errors pass through.
pub fn cancel_fallback_decision(
    request_method: &str,
    response: &Value,
) -> AcpCancelFallbackDecision {
    if request_method != ACP_SESSION_CANCEL_METHOD {
        return AcpCancelFallbackDecision::ReturnAdapterResponse;
    }
    let Some(error) = response.get("error").and_then(Value::as_object) else {
        return AcpCancelFallbackDecision::ReturnAdapterResponse;
    };
    if error.get("code").and_then(Value::as_i64) != Some(-32601) {
        return AcpCancelFallbackDecision::ReturnAdapterResponse;
    }
    let names_cancel = error
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("method"))
        .and_then(Value::as_str)
        .is_some_and(|method| method == ACP_SESSION_CANCEL_METHOD)
        || error
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains(ACP_SESSION_CANCEL_METHOD));
    if names_cancel {
        AcpCancelFallbackDecision::SendNotification
    } else {
        AcpCancelFallbackDecision::ReturnAdapterResponse
    }
}

pub fn cancel_notification(session_id: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": ACP_SESSION_CANCEL_METHOD,
        "params": { "sessionId": session_id },
    })
}

pub fn cancel_notification_fallback_response(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "cancelled": false,
            "requested": true,
            "via": "notification-fallback",
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> AcpSessionRecord {
        AcpSessionRecord {
            session_id: String::from("session-1"),
            owner_connection_id: String::from("connection-1"),
            agent_type: String::from("pi"),
            process_id: String::from("process-1"),
            pid: Some(7),
            modes: Some(
                json!({
                    "currentModeId": "ask",
                    "availableModes": [{"id": "ask"}, {"id": "plan"}],
                })
                .to_string(),
            ),
            config_options: vec![json!({
                "id": "model",
                "category": "model",
                "currentValue": "small",
            })
            .to_string()],
            agent_capabilities: None,
            agent_info: None,
            stdout_buffer: String::new(),
            next_request_id: 3,
            closed: false,
            exit_code: None,
            pending_preamble: None,
            restart: None,
        }
    }

    fn notification(session_id: &str, update: Value) -> AcpSessionNotification {
        AcpSessionNotification {
            session_id: session_id.to_string(),
            notification: json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": { "update": update },
            }),
        }
    }

    #[test]
    fn launch_prompt_plan_assembles_native_prompt_order_for_argument_adapters() {
        let env = BTreeMap::new();
        let expected_prompt = "base prompt\n\nsession guidance\n\n## Host tools\n\n---";
        for agent_type in ["pi", "pi-cli", "claude"] {
            assert_eq!(
                plan_acp_launch_prompt(
                    agent_type,
                    "base prompt\n\n",
                    false,
                    Some("session guidance"),
                    "## Host tools",
                    &env,
                )
                .unwrap(),
                AcpLaunchPromptPlan::AppendArgument {
                    flag: String::from("--append-system-prompt"),
                    prompt: expected_prompt.to_string(),
                },
                "{agent_type} must use the system-prompt launch flag",
            );
        }
        assert_eq!(
            plan_acp_launch_prompt(
                "codex",
                "base prompt\n",
                false,
                Some("session guidance"),
                "## Host tools",
                &env,
            )
            .unwrap(),
            AcpLaunchPromptPlan::AppendArgument {
                flag: String::from("--append-developer-instructions"),
                prompt: expected_prompt.to_string(),
            }
        );
    }

    #[test]
    fn launch_prompt_plan_respects_skip_and_empty_components() {
        let env = BTreeMap::new();
        assert_eq!(
            plan_acp_launch_prompt(
                "pi",
                "ignored base",
                true,
                Some("session only"),
                "tools only",
                &env,
            )
            .unwrap(),
            AcpLaunchPromptPlan::AppendArgument {
                flag: String::from("--append-system-prompt"),
                prompt: String::from("session only\n\ntools only\n\n---"),
            }
        );

        for agent_type in ["pi", "pi-cli", "claude", "codex", "opencode", "custom"] {
            assert_eq!(
                plan_acp_launch_prompt(agent_type, "ignored base", true, Some(""), "", &env,)
                    .unwrap(),
                AcpLaunchPromptPlan::None,
                "{agent_type} must not receive an empty launch prompt",
            );
        }
    }

    #[test]
    fn launch_prompt_plan_builds_opencode_context_file_and_env() {
        let plan = plan_acp_launch_prompt(
            "opencode",
            "base",
            false,
            Some("session"),
            "tools",
            &BTreeMap::new(),
        )
        .unwrap();
        let AcpLaunchPromptPlan::OpenCodeContext {
            env_key,
            env_value,
            file,
        } = plan
        else {
            panic!("OpenCode without an explicit context path must receive a file plan");
        };
        assert_eq!(env_key, OPENCODE_CONTEXT_PATHS_ENV);
        assert_eq!(file.path, OPENCODE_SYSTEM_PROMPT_PATH);
        assert_eq!(file.contents, "base\n\nsession\n\ntools\n\n---");
        let paths: Vec<String> = serde_json::from_str(&env_value).unwrap();
        assert_eq!(
            paths,
            [
                ".github/copilot-instructions.md",
                ".cursorrules",
                ".cursor/rules/",
                "CLAUDE.md",
                "CLAUDE.local.md",
                "opencode.md",
                "opencode.local.md",
                "OpenCode.md",
                "OpenCode.local.md",
                "OPENCODE.md",
                "OPENCODE.local.md",
                OPENCODE_SYSTEM_PROMPT_PATH,
            ]
            .map(String::from)
        );
    }

    #[test]
    fn launch_prompt_plan_preserves_explicit_opencode_context_and_ignores_unknown_agents() {
        let env = BTreeMap::from_iter([(OPENCODE_CONTEXT_PATHS_ENV.to_string(), String::new())]);
        assert_eq!(
            plan_acp_launch_prompt("opencode", "base", false, None, "", &env).unwrap(),
            AcpLaunchPromptPlan::None,
            "key presence, including an empty value, suppresses OpenCode injection",
        );
        assert_eq!(
            plan_acp_launch_prompt(
                "custom-adapter",
                "base",
                false,
                Some("session"),
                "tools",
                &BTreeMap::new(),
            )
            .unwrap(),
            AcpLaunchPromptPlan::None,
        );
    }

    #[test]
    fn bounded_json_lines_preserve_partials_and_reject_bad_input() {
        let mut lines = AcpJsonLineAccumulator::new();
        assert!(lines.push_json(br#" {"id":1}"#, 16).unwrap().is_empty());
        assert_eq!(lines.retained(), r#" {"id":1}"#);
        assert_eq!(
            lines.push_json(b"\r\n\n{\"id\":2}\n", 16).unwrap(),
            vec![json!({"id": 1}), json!({"id": 2})]
        );
        assert_eq!(lines.retained(), "");

        let mut exact = AcpJsonLineAccumulator::new();
        assert_eq!(exact.push_json(b"null\n", 4).unwrap(), vec![Value::Null]);
        assert!(exact
            .push_json(b"12345", 4)
            .unwrap_err()
            .to_string()
            .contains("longer than 4 bytes"));

        let mut malformed = AcpJsonLineAccumulator::new();
        assert!(malformed
            .push_json(b"not-json\n", 16)
            .unwrap_err()
            .to_string()
            .contains("invalid JSON-RPC"));
    }

    #[test]
    fn json_rpc_classifier_rejects_complete_invalid_envelopes() {
        assert_eq!(
            classify_json_rpc_message(&json!({
                "jsonrpc": "2.0",
                "id": "host-1",
                "method": "host/read",
            }))
            .expect("inbound request"),
            AcpJsonRpcMessageKind::InboundRequest
        );
        assert_eq!(
            classify_json_rpc_message(&json!({"jsonrpc": "2.0", "id": 1, "result": {}}))
                .expect("response"),
            AcpJsonRpcMessageKind::Response
        );
        assert_eq!(
            classify_json_rpc_message(&json!({"jsonrpc": "2.0", "method": "session/update"}))
                .expect("notification"),
            AcpJsonRpcMessageKind::Notification
        );

        let missing_id = classify_json_rpc_message(&json!({
            "jsonrpc": "2.0",
            "result": {"ok": true},
        }))
        .expect_err("response without id must fail closed");
        assert_eq!(missing_id.code(), "invalid_state");
        assert!(missing_id.to_string().contains("response is missing id"));

        for invalid in [json!({"jsonrpc": "2.0"}), Value::Null] {
            let error = classify_json_rpc_message(&invalid)
                .expect_err("complete non-protocol JSON must fail closed");
            assert_eq!(error.code(), "invalid_state");
            assert!(error
                .to_string()
                .contains("neither a string method nor an id"));
        }
    }

    #[test]
    fn unsupported_inbound_request_response_preserves_request_identity() {
        let response = unsupported_inbound_request_response(&json!({
            "jsonrpc": "2.0",
            "id": "host-1",
            "method": "host/read",
        }));
        assert_eq!(response["id"], "host-1");
        assert_eq!(response["error"]["code"], -32601);
        assert_eq!(response["error"]["data"]["method"], "host/read");
    }

    #[test]
    fn bootstrap_derives_models_and_honors_session_overrides() {
        let init = json!({
            "modes": {"currentModeId": "ask"},
            "configOptions": [{"id": "theme", "category": "theme"}],
            "agentCapabilities": {"loadSession": true},
            "agentInfo": {"name": "Pi"},
        })
        .as_object()
        .unwrap()
        .clone();
        let result = json!({
            "modes": {"currentModeId": "plan"},
            "configOptions": [{"id": "temperature", "category": "temperature"}],
            "models": {
                "currentModelId": "large",
                "availableModels": [{"modelId": "small", "name": "Small"}, {"modelId": "large"}],
            },
        })
        .as_object()
        .unwrap()
        .clone();
        let fields =
            derive_bootstrap_fields("opencode", &init, &result, init.get("agentCapabilities"))
                .unwrap();

        assert_eq!(
            fields.modes,
            Some(json!({"currentModeId": "plan"}).to_string())
        );
        assert_eq!(fields.config_options.len(), 2);
        let derived: Value = serde_json::from_str(&fields.config_options[1]).unwrap();
        assert_eq!(derived["id"], "model");
        assert_eq!(derived["currentValue"], "large");
        assert_eq!(derived["readOnly"], true);
        assert!(derived["description"]
            .as_str()
            .unwrap()
            .contains("before createSession()"));
        assert_eq!(
            fields.agent_capabilities,
            Some(json!({"loadSession": true}).to_string())
        );
        assert_eq!(fields.agent_info, Some(json!({"name": "Pi"}).to_string()));
    }

    #[test]
    fn bootstrap_rejects_malformed_config_and_model_shapes() {
        let init = Map::from_iter([(String::from("configOptions"), json!({}))]);
        assert!(derive_bootstrap_fields("pi", &init, &Map::new(), None)
            .unwrap_err()
            .to_string()
            .contains("must be an array"));

        let result = Map::from_iter([(String::from("models"), json!({"availableModels": [null]}))]);
        assert!(derive_bootstrap_fields("pi", &Map::new(), &result, None)
            .unwrap_err()
            .to_string()
            .contains("must be an object"));
    }

    #[test]
    fn successful_mode_updates_state_and_synthesizes_only_when_needed() {
        let mut record = session();
        let params =
            Map::from_iter([(String::from("modeId"), Value::String(String::from("plan")))]);
        let synthetic =
            apply_successful_session_request(&mut record, "session/set_mode", &params, &[])
                .unwrap()
                .unwrap();
        assert_eq!(
            synthetic.notification()["params"]["update"]["currentModeId"],
            "plan"
        );
        let modes: Value = serde_json::from_str(record.modes.as_deref().unwrap()).unwrap();
        assert_eq!(modes["currentModeId"], "plan");

        let existing = [notification(
            "session-1",
            json!({"sessionUpdate": "current_mode_update", "currentModeId": "ask"}),
        )];
        let params = Map::from_iter([(String::from("modeId"), Value::String(String::from("ask")))]);
        assert!(apply_successful_session_request(
            &mut record,
            "session/set_mode",
            &params,
            &existing,
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn successful_config_updates_state_and_rejects_malformed_options() {
        let mut record = session();
        let params = Map::from_iter([
            (
                String::from("configId"),
                Value::String(String::from("model")),
            ),
            (String::from("value"), Value::String(String::from("large"))),
        ]);
        let synthetic = apply_successful_session_request(
            &mut record,
            "session/set_config_option",
            &params,
            &[],
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            synthetic.notification()["params"]["update"]["configOptions"][0]["currentValue"],
            "large"
        );

        let existing = [notification(
            "session-1",
            json!({"sessionUpdate": "config_options_update"}),
        )];
        assert!(apply_successful_session_request(
            &mut record,
            "session/set_config_option",
            &params,
            &existing,
        )
        .unwrap()
        .is_none());

        let boolean_params = Map::from_iter([
            (
                String::from("configId"),
                Value::String(String::from("model")),
            ),
            (String::from("value"), Value::Bool(true)),
        ]);
        let synthetic = apply_successful_session_request(
            &mut record,
            "session/set_config_option",
            &boolean_params,
            &[],
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            synthetic.notification()["params"]["update"]["configOptions"][0]["currentValue"],
            Value::Bool(true)
        );
        let stored: Value = serde_json::from_str(&record.config_options[0]).unwrap();
        assert_eq!(stored["currentValue"], Value::Bool(true));

        record.config_options = vec![String::from("not-json")];
        assert!(apply_successful_session_request(
            &mut record,
            "session/set_config_option",
            &params,
            &[],
        )
        .unwrap_err()
        .to_string()
        .contains("malformed ACP config option"));
    }

    #[test]
    fn wire_notifications_and_cancel_fallback_are_strict() {
        let event = AcpSessionEvent {
            session_id: String::from("session-1"),
            notification: String::from("not-json"),
        };
        assert!(AcpSessionNotification::from_wire(&event).is_err());

        for response in [
            json!({"error": {"code": -32601, "data": {"method": "session/cancel"}}}),
            json!({"error": {"code": -32601, "message": "unknown session/cancel"}}),
        ] {
            assert_eq!(
                cancel_fallback_decision("session/cancel", &response),
                AcpCancelFallbackDecision::SendNotification
            );
            assert_eq!(
                cancel_fallback_decision("session/prompt", &response),
                AcpCancelFallbackDecision::ReturnAdapterResponse
            );
        }
        assert_eq!(
            cancel_fallback_decision(
                "session/cancel",
                &json!({"error": {"code": -32603, "message": "session/cancel"}}),
            ),
            AcpCancelFallbackDecision::ReturnAdapterResponse
        );
        assert_eq!(
            cancel_notification("session-1"),
            json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": "session-1" },
            })
        );
        assert_eq!(
            cancel_notification_fallback_response(json!(4))["result"]["via"],
            "notification-fallback"
        );
    }
}
