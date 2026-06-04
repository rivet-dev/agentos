//! Agent sessions (ACP) methods + supporting types.
//!
//! Ported from `packages/core/src/agent-os.ts` (session methods), `agent-session-types.ts`
//! (session/mode/config/capability/permission types), and `agents.ts` (`AgentType`, `AgentConfig`).
//!
//! ACP = JSON-RPC 2.0 over stdio. Sessions are referenced by string ID and return JSON-serializable
//! data only. JSON-RPC errors are NOT Rust `Err`; methods that issue requests return a
//! [`JsonRpcResponse`] whose `error` field may be set.

use std::collections::{BTreeMap, VecDeque};

use std::pin::Pin;
use std::sync::atomic::Ordering;

use anyhow::Result;
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use agent_os_sidecar::protocol::{
    CloseAgentSessionRequest, GetSessionStateRequest, OwnershipScope, RequestPayload,
    ResponsePayload, SessionRequest, SessionStateResponse,
};

use crate::agent_os::{AgentOs, SessionEntry};
use crate::error::ClientError;
use crate::json_rpc::{JsonRpcError, JsonRpcId, JsonRpcNotification, JsonRpcResponse, SequencedEvent};
use crate::stream::{subscribe_with_replay, Subscription};
use crate::{ACP_SESSION_EVENT_RETENTION_LIMIT, CLOSED_SESSION_ID_RETENTION_LIMIT, PERMISSION_TIMEOUT_MS};

/// ACP method name for legacy permission requests/responses.
const LEGACY_PERMISSION_METHOD: &str = "request/permission";
/// ACP method name for `session/request_permission` (newer ACP).
const ACP_PERMISSION_METHOD: &str = "session/request_permission";

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// In-memory session registry entry summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "agentType")]
    pub agent_type: String,
}

/// A registry agent entry from `list_agents`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRegistryEntry {
    pub id: String,
    #[serde(rename = "acpAdapter")]
    pub acp_adapter: String,
    #[serde(rename = "agentPackage")]
    pub agent_package: String,
    pub installed: bool,
}

/// MCP server config used by `create_session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpServerConfig {
    Local {
        command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        env: BTreeMap<String, String>,
    },
    Remote {
        url: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        headers: BTreeMap<String, String>,
    },
}

/// Options for `create_session`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateSessionOptions {
    /// Default `"/home/user"`.
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    /// Default `[]`.
    pub mcp_servers: Vec<McpServerConfig>,
    /// Default false.
    pub skip_os_instructions: bool,
    pub additional_instructions: Option<String>,
}

impl Default for CreateSessionOptions {
    fn default() -> Self {
        Self {
            cwd: None,
            env: BTreeMap::new(),
            mcp_servers: Vec::new(),
            skip_os_instructions: false,
            additional_instructions: None,
        }
    }
}

/// The id returned by `create_session` / `resume_session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionId {
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

/// Result of `prompt`.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptResult {
    pub response: JsonRpcResponse,
    pub text: String,
}

/// Options for `get_session_events`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GetEventsOptions {
    pub since: Option<i64>,
    pub method: Option<String>,
}

/// A single session mode (`{ id; name?; label?; description?; [k]: unknown }`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMode {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Additional unmodeled fields.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Session mode state (`{ currentModeId; availableModes }`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionModeState {
    #[serde(rename = "currentModeId")]
    pub current_mode_id: String,
    #[serde(rename = "availableModes")]
    pub available_modes: Vec<SessionMode>,
}

/// An allowed value for a config option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigAllowedValue {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// A session config option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionConfigOption {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, rename = "currentValue", skip_serializing_if = "Option::is_none")]
    pub current_value: Option<String>,
    #[serde(default, rename = "allowedValues", skip_serializing_if = "Option::is_none")]
    pub allowed_values: Option<Vec<ConfigAllowedValue>>,
    #[serde(default, rename = "readOnly", skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
}

/// Prompt capabilities sub-object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PromptCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,
    #[serde(default, rename = "embeddedContext", skip_serializing_if = "Option::is_none")]
    pub embedded_context: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<bool>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Agent capabilities (all optional booleans + prompt capabilities + extras).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<bool>,
    #[serde(default, rename = "plan_mode", skip_serializing_if = "Option::is_none")]
    pub plan_mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questions: Option<bool>,
    #[serde(default, rename = "tool_calls", skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<bool>,
    #[serde(default, rename = "text_messages", skip_serializing_if = "Option::is_none")]
    pub text_messages: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<bool>,
    #[serde(default, rename = "file_attachments", skip_serializing_if = "Option::is_none")]
    pub file_attachments: Option<bool>,
    #[serde(default, rename = "session_lifecycle", skip_serializing_if = "Option::is_none")]
    pub session_lifecycle: Option<bool>,
    #[serde(default, rename = "error_events", skip_serializing_if = "Option::is_none")]
    pub error_events: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<bool>,
    #[serde(default, rename = "streaming_deltas", skip_serializing_if = "Option::is_none")]
    pub streaming_deltas: Option<bool>,
    #[serde(default, rename = "mcp_tools", skip_serializing_if = "Option::is_none")]
    pub mcp_tools: Option<bool>,
    #[serde(default, rename = "promptCapabilities", skip_serializing_if = "Option::is_none")]
    pub prompt_capabilities: Option<PromptCapabilities>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Agent info (`{ name; title?; version?; [k]: unknown }`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Initial hydration data for a session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionInitData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<SessionModeState>,
    #[serde(default, rename = "configOptions", skip_serializing_if = "Option::is_none")]
    pub config_options: Option<Vec<SessionConfigOption>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<AgentCapabilities>,
    #[serde(default, rename = "agentInfo", skip_serializing_if = "Option::is_none")]
    pub agent_info: Option<AgentInfo>,
}

/// A Clone-able one-shot responder for a permission request.
///
/// [`PermissionRequest`] is delivered over a [`tokio::sync::broadcast`] channel, which requires the
/// item to be `Clone`. A raw `oneshot::Sender` is not `Clone`, so the sender is held behind a shared
/// `Arc<Mutex<Option<..>>>`; the first [`PermissionResponder::respond`] call takes the sender out
/// and resolves it. Subsequent calls (or other broadcast clones) are no-ops.
#[derive(Clone)]
pub struct PermissionResponder {
    inner: std::sync::Arc<parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<PermissionReply>>>>,
}

impl PermissionResponder {
    /// Create a responder paired with the receiving end.
    pub fn new() -> (Self, tokio::sync::oneshot::Receiver<PermissionReply>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (
            Self {
                inner: std::sync::Arc::new(parking_lot::Mutex::new(Some(tx))),
            },
            rx,
        )
    }

    /// Resolve the request with `reply`. The first call wins; later calls are no-ops.
    pub fn respond(&self, reply: PermissionReply) {
        if let Some(tx) = self.inner.lock().take() {
            let _ = tx.send(reply);
        }
    }
}

/// A permission request delivered to a subscriber. Carries a Clone-able one-shot responder.
///
/// The TS handler is `(request) => void`; in Rust this is the request/responder pattern: the
/// subscriber resolves the request by calling [`PermissionResponder::respond`], or the 120s timeout
/// / no-subscriber path auto-rejects.
#[derive(Clone)]
pub struct PermissionRequest {
    pub permission_id: String,
    pub description: Option<String>,
    pub params: Value,
    pub responder: PermissionResponder,
}

impl std::fmt::Debug for PermissionRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PermissionRequest")
            .field("permission_id", &self.permission_id)
            .field("description", &self.description)
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}

/// A permission reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionReply {
    Once,
    Always,
    Reject,
}

// ---------------------------------------------------------------------------
// Local-state helpers (operate on a `SessionEntry`; mirror the TS private helpers)
// ---------------------------------------------------------------------------

/// Whether a notification should be delivered to `on_session_event` subscribers (`session/update`
/// only). Mirrors `shouldDispatchToSessionEventHandlers`.
fn should_dispatch_to_session_event_handlers(notification: &JsonRpcNotification) -> bool {
    notification.method == "session/update"
}

/// Merge incoming sequenced events into the bounded ring, deduped by sequence number, sorted
/// ascending, and truncated to [`ACP_SESSION_EVENT_RETENTION_LIMIT`]. Mirrors `mergeSequencedEvents`.
fn merge_sequenced_events(ring: &mut VecDeque<SequencedEvent>, incoming: Vec<SequencedEvent>) {
    let mut by_sequence: BTreeMap<i64, SequencedEvent> = BTreeMap::new();
    for event in ring.drain(..) {
        by_sequence.insert(event.sequence_number, event);
    }
    for event in incoming {
        by_sequence.insert(event.sequence_number, event);
    }
    let mut merged: Vec<SequencedEvent> = by_sequence.into_values().collect();
    if merged.len() > ACP_SESSION_EVENT_RETENTION_LIMIT {
        let start = merged.len() - ACP_SESSION_EVENT_RETENTION_LIMIT;
        merged.drain(0..start);
    }
    *ring = merged.into();
}

/// Compute the next highest acknowledged sequence number. Mirrors `nextHighestSequenceNumber`.
fn next_highest_sequence_number(current: Option<i64>, ring: &VecDeque<SequencedEvent>) -> Option<i64> {
    let Some(latest) = ring.back().map(|event| event.sequence_number) else {
        return current;
    };
    match current {
        None => Some(latest),
        Some(current) => Some(current.max(latest)),
    }
}

/// The smallest synthetic (negative) sequence number for a session: `min(0, all seqs) - 1`. Mirrors
/// `_nextSyntheticSequenceNumber`.
fn next_synthetic_sequence_number(ring: &VecDeque<SequencedEvent>) -> i64 {
    let min = ring
        .iter()
        .map(|event| event.sequence_number)
        .fold(0i64, i64::min);
    min - 1
}

/// Apply a `session/update` notification's local cache side effects (`current_mode_update`,
/// `config_option(s)_update`). Mirrors `_applySessionUpdate`. Holds the entry's per-field guards
/// briefly.
fn apply_session_update(entry: &SessionEntry, notification: &JsonRpcNotification) {
    if notification.method != "session/update" {
        return;
    }
    let params = notification.params.clone().unwrap_or(Value::Null);
    let update = params
        .get("update")
        .cloned()
        .unwrap_or_else(|| params.clone());
    let session_update = update.get("sessionUpdate").and_then(Value::as_str);

    if session_update == Some("current_mode_update") {
        if let Some(current_mode_id) = update.get("currentModeId").and_then(Value::as_str) {
            let mut modes = entry.modes.lock();
            if let Some(modes) = modes.as_mut() {
                modes.current_mode_id = current_mode_id.to_string();
            }
        }
    }

    if matches!(
        session_update,
        Some("config_option_update") | Some("config_options_update")
    ) {
        if let Some(config_options) = update.get("configOptions").and_then(Value::as_array) {
            if let Ok(parsed) =
                serde_json::from_value::<Vec<SessionConfigOption>>(Value::Array(config_options.clone()))
            {
                *entry.config_options.lock() = parsed;
            }
        }
    }
}

/// Re-apply synthetic config overrides onto the cached config options. Mirrors
/// `_applySyntheticConfigOverrides`.
fn apply_synthetic_config_overrides(entry: &SessionEntry) {
    let overrides = entry.config_overrides.lock().clone();
    if overrides.is_empty() {
        return;
    }
    let mut options = entry.config_options.lock();
    for option in options.iter_mut() {
        // Skip internal pending-request method markers (see `send_session_request`); they share the
        // override map but are never real config option ids/categories.
        let override_value = overrides
            .get(&option.id)
            .filter(|_| !option.id.starts_with(PENDING_METHOD_PREFIX))
            .cloned()
            .or_else(|| {
                option
                    .category
                    .as_ref()
                    .and_then(|category| overrides.get(category).cloned())
            });
        if let Some(value) = override_value {
            option.current_value = Some(value);
        }
    }
}

/// Prefix for the internal per-resolver method markers stored in `config_overrides` (so cancel can
/// distinguish `session/prompt` resolvers without an extra `SessionEntry` field).
const PENDING_METHOD_PREFIX: &str = "__pending_method::";

/// Record a sequenced notification into the session ring and run the cache/permission side effects.
/// Mirrors `_recordSessionNotification` (without the host-side event-handler microtask dispatch,
/// which the broadcast channel covers).
fn record_session_notification(
    entry: &SessionEntry,
    sequence_number: i64,
    notification: JsonRpcNotification,
) {
    {
        let mut ring = entry.event_ring.lock();
        merge_sequenced_events(
            &mut ring,
            vec![SequencedEvent {
                sequence_number,
                notification: notification.clone(),
            }],
        );
        let next = next_highest_sequence_number(
            Some(entry.highest_sequence_number.load(Ordering::SeqCst)),
            &ring,
        );
        if let Some(next) = next {
            entry.highest_sequence_number.store(next, Ordering::SeqCst);
        }
    }
    apply_session_update(entry, &notification);

    if should_dispatch_to_session_event_handlers(&notification) {
        let _ = entry.event_tx.send(SequencedEvent {
            sequence_number,
            notification: notification.clone(),
        });
    }

    // The permission-from-notification delivery path (legacy `request/permission` /
    // `session/request_permission`) needs an `AgentOs` handle to register the reply slot + 120s
    // timeout, so it is exposed as [`AgentOs::deliver_permission_request`] and invoked by the
    // sidecar event/request handler (in `agent_os.rs`/`transport`, which this module does not own).
}

/// Build a [`PermissionRequest`] from a legacy/ACP permission notification and broadcast it to
/// subscribers, registering its reply slot into `pending_permission_replies` with a 120s timeout.
/// Mirrors the permission branch of `_recordSessionNotification` plus the request/responder wiring.
///
/// `register_pending` registers the resolution slot keyed by permission id and arms the timeout; it
/// is supplied by the caller because it needs an [`AgentOs`] handle for the timeout cleanup.
fn build_permission_request(notification: &JsonRpcNotification) -> Option<(String, PermissionRequest, tokio::sync::oneshot::Receiver<PermissionReply>)> {
    let params = notification.params.clone().unwrap_or(Value::Null);
    let permission_id = match params.get("permissionId") {
        Some(Value::String(id)) => id.clone(),
        Some(Value::Number(num)) => num.to_string(),
        _ => return None,
    };
    let description = params
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_string);

    let (responder, receiver) = PermissionResponder::new();
    let request = PermissionRequest {
        permission_id: permission_id.clone(),
        description,
        params,
        responder,
    };
    Some((permission_id, request, receiver))
}

/// Apply the local cache mutations of `_syncSessionState`: modes, config options, capabilities,
/// agent info, and merged events from a sidecar [`SessionStateResponse`].
fn sync_session_state(entry: &SessionEntry, state: &SessionStateResponse) {
    *entry.modes.lock() = state
        .modes
        .as_ref()
        .filter(|value| value.is_object())
        .and_then(|value| serde_json::from_value(value.clone()).ok());

    *entry.config_options.lock() = state
        .config_options
        .iter()
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect();

    apply_synthetic_config_overrides(entry);

    *entry.capabilities.lock() = state
        .agent_capabilities
        .as_ref()
        .filter(|value| value.is_object())
        .and_then(|value| serde_json::from_value(value.clone()).ok());

    *entry.agent_info.lock() = state
        .agent_info
        .as_ref()
        .filter(|value| value.is_object())
        .and_then(|value| serde_json::from_value(value.clone()).ok());

    let incoming: Vec<SequencedEvent> = state
        .events
        .iter()
        .filter_map(|event| {
            serde_json::from_value::<JsonRpcNotification>(event.notification.clone())
                .ok()
                .map(|notification| SequencedEvent {
                    sequence_number: event.sequence_number as i64,
                    notification,
                })
        })
        .collect();

    let mut ring = entry.event_ring.lock();
    merge_sequenced_events(&mut ring, incoming);
    let next = next_highest_sequence_number(
        Some(entry.highest_sequence_number.load(Ordering::SeqCst)),
        &ring,
    );
    if let Some(next) = next {
        entry.highest_sequence_number.store(next, Ordering::SeqCst);
    }
}

/// Synthesize the unsupported-config JSON-RPC error response (`-32601`). Mirrors
/// `_unsupportedConfigResponse`.
fn unsupported_config_response(agent_type: &str, category: &str) -> JsonRpcResponse {
    let message = if agent_type == "opencode" && category == "model" {
        "OpenCode reports available models, but model switching must be configured before createSession() because ACP session/set_config_option is not implemented.".to_string()
    } else {
        format!("The {category} config option is read-only for {agent_type} sessions.")
    };
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: Some(JsonRpcId::Null),
        result: None,
        error: Some(JsonRpcError {
            code: -32601,
            message,
            data: None,
        }),
    }
}

/// Apply the codex config fallback: record overrides, re-apply, synthesize a negative-seq
/// `config_option_update`, and return a `via: "codex-config-fallback"` response. Mirrors
/// `_applyCodexConfigFallback`.
fn apply_codex_config_fallback(entry: &SessionEntry, category: &str, value: &str) -> JsonRpcResponse {
    {
        let options = entry.config_options.lock();
        let matching_id = options
            .iter()
            .find(|option| option.category.as_deref() == Some(category))
            .map(|option| option.id.clone());
        drop(options);
        let mut overrides = entry.config_overrides.lock();
        if let Some(id) = matching_id {
            overrides.insert(id, value.to_string());
        }
        overrides.insert(category.to_string(), value.to_string());
    }
    apply_synthetic_config_overrides(entry);

    let config_options = entry.config_options.lock().clone();
    let synthetic_seq = {
        let ring = entry.event_ring.lock();
        next_synthetic_sequence_number(&ring)
    };
    record_session_notification(
        entry,
        synthetic_seq,
        JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "session/update".to_string(),
            params: Some(json!({
                "update": {
                    "sessionUpdate": "config_option_update",
                    "configOptions": config_options,
                }
            })),
        },
    );

    let config_options = entry.config_options.lock().clone();
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: Some(JsonRpcId::Null),
        result: Some(json!({
            "configOptions": config_options,
            "via": "codex-config-fallback",
        })),
        error: None,
    }
}

/// Augment `session/prompt` params for codex sessions with the cached model/thought-level overrides
/// under `_meta.agentOsCodexConfig`. Mirrors `_augmentPromptParams`.
fn augment_prompt_params(entry: &SessionEntry, params: Option<Value>) -> Option<Value> {
    if entry.agent_type != "codex" {
        return params;
    }
    let (model, thought_level) = {
        let options = entry.config_options.lock();
        let model = options
            .iter()
            .find(|option| option.category.as_deref() == Some("model"))
            .and_then(|option| option.current_value.clone());
        let thought_level = options
            .iter()
            .find(|option| option.category.as_deref() == Some("thought_level"))
            .and_then(|option| option.current_value.clone());
        (model, thought_level)
    };
    if model.is_none() && thought_level.is_none() {
        return params;
    }

    let mut meta = match params.as_ref().and_then(|p| p.get("_meta")) {
        Some(Value::Object(existing)) => existing.clone(),
        _ => serde_json::Map::new(),
    };
    let mut codex_config = serde_json::Map::new();
    if let Some(model) = model {
        codex_config.insert("model".to_string(), Value::String(model));
    }
    if let Some(thought_level) = thought_level {
        codex_config.insert("thought_level".to_string(), Value::String(thought_level));
    }
    meta.insert(
        "agentOsCodexConfig".to_string(),
        Value::Object(codex_config),
    );

    let mut object = match params {
        Some(Value::Object(existing)) => existing,
        _ => serde_json::Map::new(),
    };
    object.insert("_meta".to_string(), Value::Object(meta));
    Some(Value::Object(object))
}

/// Build the closed-session abort response (`-32000`). Mirrors `_abortPendingSessionRequests`.
fn session_closed_response(session_id: &str) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: Some(JsonRpcId::Null),
        result: None,
        error: Some(JsonRpcError {
            code: -32000,
            message: format!("Session closed: {session_id}"),
            data: None,
        }),
    }
}

// ---------------------------------------------------------------------------
// Methods
// ---------------------------------------------------------------------------

impl AgentOs {
    /// VM-scoped ownership for session RPCs.
    fn session_ownership(&self) -> OwnershipScope {
        OwnershipScope::vm(
            self.connection_id().to_string(),
            self.wire_session_id().to_string(),
            self.vm_id().to_string(),
        )
    }

    /// Look up a session entry or return [`ClientError::SessionNotFound`]. Mirrors `_requireSession`.
    fn require_session<R>(
        &self,
        session_id: &str,
        f: impl FnOnce(&SessionEntry) -> R,
    ) -> std::result::Result<R, ClientError> {
        self.inner()
            .sessions
            .read(session_id, |_, entry| f(entry))
            .ok_or_else(|| ClientError::SessionNotFound(session_id.to_string()))
    }

    /// Re-hydrate cached session state from the sidecar `GetSessionState` snapshot, acknowledging the
    /// highest seen sequence number. Mirrors `_hydrateSessionState`.
    async fn hydrate_session_state(&self, session_id: &str) -> std::result::Result<(), ClientError> {
        let acknowledged = self.require_session(session_id, |entry| {
            let highest = entry.highest_sequence_number.load(Ordering::SeqCst);
            if highest >= 0 {
                Some(highest as u64)
            } else {
                None
            }
        })?;

        let response = self
            .transport()
            .request(
                self.session_ownership(),
                RequestPayload::GetSessionState(GetSessionStateRequest {
                    session_id: session_id.to_string(),
                    acknowledged_sequence_number: acknowledged,
                }),
            )
            .await?;

        let state = match response {
            ResponsePayload::SessionState(state) => state,
            ResponsePayload::Rejected(rejected) => {
                return Err(ClientError::Kernel {
                    code: rejected.code,
                    message: rejected.message,
                });
            }
            other => {
                return Err(ClientError::Sidecar(format!(
                    "unexpected response to GetSessionState: {other:?}"
                )));
            }
        };

        self.require_session(session_id, |entry| sync_session_state(entry, &state))?;
        Ok(())
    }

    /// Core request helper: every session request routes through this. Tracks pending resolvers per
    /// session (cancel prompt-fallback + abort-on-close), augments `session/prompt` for codex, calls
    /// the sidecar, re-hydrates state, and applies local cache updates for `set_mode` /
    /// `set_config_option`.
    pub(crate) async fn send_session_request(
        &self,
        session_id: &str,
        method: &str,
        params: Option<Value>,
    ) -> std::result::Result<JsonRpcResponse, ClientError> {
        let request_params = if method == "session/prompt" {
            self.require_session(session_id, |entry| augment_prompt_params(entry, params.clone()))?
        } else {
            params
        };

        // Register a pending-resolver slot so cancel/close can resolve this request locally. The
        // resolver fires `()` to short-circuit the awaited oneshot; whichever completes first wins.
        let resolver_id = self.inner().request_counter.fetch_add(1, Ordering::SeqCst);
        let (resolve_tx, resolve_rx) = tokio::sync::oneshot::channel::<()>();
        self.require_session(session_id, |entry| {
            let _ = entry.pending_prompt_resolvers.insert(resolver_id, resolve_tx);
            // Track the method so prompt-fallback can target only `session/prompt` resolvers.
            entry
                .config_overrides
                .lock()
                .entry(format!("{PENDING_METHOD_PREFIX}{resolver_id}"))
                .or_insert_with(|| method.to_string());
        })?;

        let transport = self.transport();
        let ownership = self.session_ownership();
        let session_request = SessionRequest {
            session_id: session_id.to_string(),
            method: method.to_string(),
            params: request_params.clone(),
        };

        let rpc = transport.request(ownership, RequestPayload::SessionRequest(session_request));
        tokio::pin!(rpc);

        let response = tokio::select! {
            biased;
            _ = resolve_rx => {
                // A cancel/close resolved this request locally before the sidecar replied.
                self.cleanup_pending_resolver(session_id, resolver_id);
                return Ok(session_closed_or_cancelled_placeholder(session_id, method));
            }
            result = &mut rpc => {
                self.cleanup_pending_resolver(session_id, resolver_id);
                result?
            }
        };

        let response = match response {
            ResponsePayload::SessionRpc(rpc) => {
                serde_json::from_value::<JsonRpcResponse>(rpc.response).map_err(|err| {
                    ClientError::Sidecar(format!("malformed session rpc response: {err}"))
                })?
            }
            ResponsePayload::Rejected(rejected) => {
                return Err(ClientError::Kernel {
                    code: rejected.code,
                    message: rejected.message,
                });
            }
            other => {
                return Err(ClientError::Sidecar(format!(
                    "unexpected response to SessionRequest: {other:?}"
                )));
            }
        };

        // Re-hydrate state regardless of outcome (best-effort; ignore errors).
        let _ = self.hydrate_session_state(session_id).await;

        if response.error.is_none() {
            self.apply_post_send_cache_updates(session_id, method, request_params.as_ref())?;
        }

        Ok(response)
    }

    /// Drop a pending-resolver slot and its tracked method marker.
    fn cleanup_pending_resolver(&self, session_id: &str, resolver_id: i64) {
        let _ = self.require_session(session_id, |entry| {
            let _ = entry.pending_prompt_resolvers.remove(&resolver_id);
            entry
                .config_overrides
                .lock()
                .remove(&format!("{PENDING_METHOD_PREFIX}{resolver_id}"));
        });
    }

    /// Apply local cache updates for successful `session/set_mode` / `session/set_config_option`.
    fn apply_post_send_cache_updates(
        &self,
        session_id: &str,
        method: &str,
        params: Option<&Value>,
    ) -> std::result::Result<(), ClientError> {
        self.require_session(session_id, |entry| {
            if method == "session/set_mode" {
                if let Some(mode_id) = params.and_then(|p| p.get("modeId")).and_then(Value::as_str) {
                    let mut modes = entry.modes.lock();
                    if let Some(modes) = modes.as_mut() {
                        modes.current_mode_id = mode_id.to_string();
                    }
                }
            }
            if method == "session/set_config_option" {
                let config_id = params.and_then(|p| p.get("configId")).and_then(Value::as_str);
                let value = params.and_then(|p| p.get("value")).and_then(Value::as_str);
                if let (Some(config_id), Some(value)) = (config_id, value) {
                    let mut options = entry.config_options.lock();
                    for option in options.iter_mut() {
                        if option.id == config_id {
                            option.current_value = Some(value.to_string());
                        }
                    }
                }
            }
        })
    }

    /// Set a config option by its category (model/thought_level). Mirrors
    /// `_setSessionConfigByCategory`: readonly -> error response, codex `-32601` fallback.
    async fn set_session_config_by_category(
        &self,
        session_id: &str,
        category: &str,
        value: &str,
    ) -> std::result::Result<JsonRpcResponse, ClientError> {
        let (read_only, config_id, agent_type) = self.require_session(session_id, |entry| {
            let options = entry.config_options.lock();
            let option = options
                .iter()
                .find(|option| option.category.as_deref() == Some(category));
            (
                option.and_then(|option| option.read_only).unwrap_or(false),
                option.map(|option| option.id.clone()),
                entry.agent_type.clone(),
            )
        })?;

        if read_only {
            return Ok(unsupported_config_response(&agent_type, category));
        }

        let config_id = config_id.unwrap_or_else(|| category.to_string());
        let response = self
            .send_session_request(
                session_id,
                "session/set_config_option",
                Some(json!({ "configId": config_id, "value": value })),
            )
            .await?;

        let is_codex_method_not_found = agent_type == "codex"
            && response
                .error
                .as_ref()
                .map(|error| {
                    error.code == -32601
                        && error
                            .data
                            .as_ref()
                            .and_then(|data| data.get("method"))
                            .and_then(Value::as_str)
                            == Some("session/set_config_option")
                })
                .unwrap_or(false);

        if is_codex_method_not_found {
            return self.require_session(session_id, |entry| {
                apply_codex_config_fallback(entry, category, value)
            });
        }

        Ok(response)
    }

    /// List in-memory sessions.
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        let mut sessions = Vec::new();
        self.inner().sessions.scan(|session_id, entry| {
            sessions.push(SessionInfo {
                session_id: session_id.clone(),
                agent_type: entry.agent_type.clone(),
            });
        });
        sessions
    }

    /// List available agents (host FS). Unions package agent ids + the built-in `AGENT_CONFIGS`
    /// keys; `installed` is determined by reading the adapter `package.json` (host FS, try/catch).
    ///
    /// PARITY GAP: the agent-config registry (`AGENT_CONFIGS`, package agent configs, software
    /// roots, adapter `package.json` resolution) does not exist in the client scaffold and lives in
    /// shared modules this task may not edit. Returns an empty list until that infrastructure is
    /// added. See `todosLeft`.
    pub fn list_agents(&self) -> Vec<AgentRegistryEntry> {
        Vec::new()
    }

    /// Create an ACP session. Resolves the agent config, prepares instructions, merges env (user
    /// wins), creates the session via the sidecar (`runtime: java_script`, protocol v1, default
    /// client caps), and hydrates state. On hydration failure the session is removed and the error
    /// rethrown. Returns the session id only.
    ///
    /// PARITY GAP: agent-config resolution + adapter-bin resolution + `prepareInstructions` live in
    /// shared modules (`AgentConfig`/`AGENT_CONFIGS`/software roots) that are not present in the
    /// scaffold and out of scope to edit. The local registration + hydration flow is implemented
    /// against `register_session`, which the create path must call once that infra exists. See
    /// `todosLeft`.
    pub async fn create_session(
        &self,
        _agent_type: &str,
        _options: CreateSessionOptions,
    ) -> Result<SessionId> {
        anyhow::bail!(
            "create_session requires agent-config resolution infrastructure (AgentConfig / \
             AGENT_CONFIGS / software roots / adapter-bin resolution) that is not present in the \
             client scaffold; see todosLeft"
        )
    }

    /// Register a freshly created session entry and hydrate it. Used by the create path once
    /// agent-config resolution exists; exposed so the create flow stays a 1:1 port of the local
    /// registration + hydrate + on-failure-remove behavior.
    pub(crate) async fn register_session(
        &self,
        session_id: &str,
        agent_type: &str,
        state: &SessionStateResponse,
    ) -> std::result::Result<(), ClientError> {
        {
            let mut closed = self.inner().closed_session_ids.lock();
            closed.retain(|id| id != session_id);
        }

        let (event_tx, _) = tokio::sync::broadcast::channel(ACP_SESSION_EVENT_RETENTION_LIMIT.max(1));
        let (permission_tx, _) = tokio::sync::broadcast::channel(64);
        let entry = SessionEntry {
            agent_type: agent_type.to_string(),
            modes: parking_lot::Mutex::new(None),
            config_options: parking_lot::Mutex::new(Vec::new()),
            capabilities: parking_lot::Mutex::new(None),
            agent_info: parking_lot::Mutex::new(None),
            config_overrides: parking_lot::Mutex::new(BTreeMap::new()),
            event_ring: parking_lot::Mutex::new(VecDeque::new()),
            highest_sequence_number: std::sync::atomic::AtomicI64::new(-1),
            event_tx,
            permission_tx,
            pending_permission_replies: scc::HashMap::new(),
            pending_prompt_resolvers: scc::HashMap::new(),
        };
        sync_session_state(&entry, state);
        let _ = self
            .inner()
            .sessions
            .insert(session_id.to_string(), entry);

        match self.hydrate_session_state(session_id).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = self.inner().sessions.remove(session_id);
                Err(error)
            }
        }
    }

    /// Resume an existing session. SYNC. Existence check + echo; no sidecar call.
    pub fn resume_session(&self, session_id: &str) -> std::result::Result<SessionId, ClientError> {
        self.require_session(session_id, |_| ())?;
        Ok(SessionId {
            session_id: session_id.to_string(),
        })
    }

    /// Destroy a session. Best-effort `cancel_session` then internal close.
    pub async fn destroy_session(&self, session_id: &str) -> Result<()> {
        self.require_session(session_id, |_| ())?;
        let _ = self.cancel_session(session_id).await;
        self.close_session_internal(session_id).await?;
        Ok(())
    }

    /// Prompt a session. Uses a NO-REPLAY internal subscribe; accumulates `agent_message_chunk`
    /// text; sends `session/prompt`; unsubscribes by dropping the receiver. The `response` may
    /// itself be an error.
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<PromptResult> {
        // No-replay subscription: start after the current highest sequence so only future chunks
        // accumulate. Mirrors `_subscribeSessionEvents(..., { replayBuffered: false })`.
        let (mut rx, start_after) = self.require_session(session_id, |entry| {
            let ring = entry.event_ring.lock();
            let latest = ring
                .iter()
                .rev()
                .find(|event| should_dispatch_to_session_event_handlers(&event.notification))
                .map(|event| event.sequence_number)
                .unwrap_or(i64::MIN);
            (entry.event_tx.subscribe(), latest)
        })?;

        let agent_text = std::sync::Arc::new(parking_lot::Mutex::new(String::new()));
        let agent_text_task = agent_text.clone();
        let accumulator = tokio::spawn(async move {
            let mut last_delivered = start_after;
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if event.sequence_number <= last_delivered {
                            continue;
                        }
                        last_delivered = event.sequence_number;
                        let params = event.notification.params.clone().unwrap_or(Value::Null);
                        let update = params.get("update").cloned().unwrap_or(Value::Null);
                        if update.get("sessionUpdate").and_then(Value::as_str)
                            == Some("agent_message_chunk")
                        {
                            if let Some(chunk) = update
                                .get("content")
                                .and_then(|content| content.get("text"))
                                .and_then(Value::as_str)
                            {
                                agent_text_task.lock().push_str(chunk);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let response = self
            .send_session_request(
                session_id,
                "session/prompt",
                Some(json!({ "prompt": [{ "type": "text", "text": text }] })),
            )
            .await;

        // The prompt has resolved; `send_session_request` re-hydrates state (recording any final
        // chunks) before returning, so all `agent_message_chunk` broadcasts have been emitted. Stop
        // the accumulator (= the TS `finally { unsubscribe() }`) and read the buffered text.
        accumulator.abort();
        let _ = accumulator.await;
        let text = std::mem::take(&mut *agent_text.lock());

        let response = response?;
        Ok(PromptResult { response, text })
    }

    /// Cancel a session. If prompt requests are pending, resolves locally + background
    /// `session/cancel` and returns a synthetic `{ via: "prompt-fallback" }`; else real
    /// `session/cancel`.
    pub async fn cancel_session(&self, session_id: &str) -> Result<JsonRpcResponse> {
        self.require_session(session_id, |_| ())?;
        let cancelled_pending_prompt = self.cancel_pending_prompt_requests(session_id)?;
        if cancelled_pending_prompt {
            // Forward the real cancel in the background (best effort); return the synthetic
            // prompt-fallback response immediately.
            let this = self.clone();
            let session_id_owned = session_id.to_string();
            tokio::spawn(async move {
                let _ = this
                    .send_session_request(&session_id_owned, "session/cancel", None)
                    .await;
            });
            return Ok(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: Some(JsonRpcId::Null),
                result: Some(json!({
                    "cancelled": true,
                    "requested": true,
                    "via": "prompt-fallback",
                })),
                error: None,
            });
        }
        Ok(self
            .send_session_request(session_id, "session/cancel", None)
            .await?)
    }

    /// Resolve any pending `session/prompt` resolvers with a synthetic `stopReason: cancelled`
    /// result. Returns whether a prompt was cancelled. Mirrors `_cancelPendingPromptRequests`.
    fn cancel_pending_prompt_requests(
        &self,
        session_id: &str,
    ) -> std::result::Result<bool, ClientError> {
        self.require_session(session_id, |entry| {
            let mut prompt_resolver_ids = Vec::new();
            {
                let overrides = entry.config_overrides.lock();
                for (key, method) in overrides.iter() {
                    if let Some(id) = key.strip_prefix(PENDING_METHOD_PREFIX) {
                        if method == "session/prompt" {
                            if let Ok(id) = id.parse::<i64>() {
                                prompt_resolver_ids.push(id);
                            }
                        }
                    }
                }
            }
            let mut cancelled = false;
            for id in prompt_resolver_ids {
                if let Some((_, resolver)) = entry.pending_prompt_resolvers.remove(&id) {
                    let _ = resolver.send(());
                    cancelled = true;
                }
                entry
                    .config_overrides
                    .lock()
                    .remove(&format!("{PENDING_METHOD_PREFIX}{id}"));
            }
            cancelled
        })
    }

    /// Abort all pending session requests with a `-32000 Session closed` response. Mirrors
    /// `_abortPendingSessionRequests`.
    fn abort_pending_session_requests(&self, session_id: &str) {
        let _ = self.require_session(session_id, |entry| {
            let mut ids = Vec::new();
            entry
                .pending_prompt_resolvers
                .scan(|id, _| ids.push(*id));
            for id in ids {
                if let Some((_, resolver)) = entry.pending_prompt_resolvers.remove(&id) {
                    let _ = resolver.send(());
                }
                entry
                    .config_overrides
                    .lock()
                    .remove(&format!("{PENDING_METHOD_PREFIX}{id}"));
            }
        });
    }

    /// Reject all pending permission replies. The TS path clears their 120s timers and rejects them;
    /// here dropping the responder side closes the awaiting channel. Mirrors
    /// `_rejectPendingPermissionReplies`.
    fn reject_pending_permission_replies(&self, session_id: &str) {
        let _ = self.require_session(session_id, |entry| {
            let mut ids = Vec::new();
            entry
                .pending_permission_replies
                .scan(|id, _| ids.push(id.clone()));
            for id in ids {
                let _ = entry.pending_permission_replies.remove(&id);
            }
        });
    }

    /// Close a session. SYNC fire-and-forget. Errors only if unknown across sessions / closed-ids.
    /// Aborts pending, rejects pending permissions, records the closed id (bounded 2048).
    pub fn close_session(&self, session_id: &str) -> std::result::Result<(), ClientError> {
        let known = self.inner().sessions.contains(session_id)
            || self
                .inner()
                .closed_session_ids
                .lock()
                .iter()
                .any(|id| id == session_id);
        if !known {
            return Err(ClientError::SessionNotFound(session_id.to_string()));
        }

        let this = self.clone();
        let session_id_owned = session_id.to_string();
        tokio::spawn(async move {
            let _ = this.close_session_internal(&session_id_owned).await;
        });
        Ok(())
    }

    /// Internal close: abort pending requests, reject pending permissions, deregister the session,
    /// record the closed id (bounded), and best-effort `CloseAgentSession`. Mirrors
    /// `_closeSessionInternal`.
    pub(crate) async fn close_session_internal(
        &self,
        session_id: &str,
    ) -> std::result::Result<(), ClientError> {
        if self
            .inner()
            .closed_session_ids
            .lock()
            .iter()
            .any(|id| id == session_id)
        {
            return Ok(());
        }

        self.abort_pending_session_requests(session_id);
        self.reject_pending_permission_replies(session_id);

        // Require existence before removal, matching `_requireSession` in `_closeSessionInternal`.
        if !self.inner().sessions.contains(session_id) {
            return Err(ClientError::SessionNotFound(session_id.to_string()));
        }
        let _ = self.inner().sessions.remove(session_id);
        {
            let mut closed = self.inner().closed_session_ids.lock();
            closed.push_back(session_id.to_string());
            while closed.len() > CLOSED_SESSION_ID_RETENTION_LIMIT {
                closed.pop_front();
            }
        }

        let response = self
            .transport()
            .request(
                self.session_ownership(),
                RequestPayload::CloseAgentSession(CloseAgentSessionRequest {
                    session_id: session_id.to_string(),
                }),
            )
            .await?;
        match response {
            ResponsePayload::AgentSessionClosed(_) => Ok(()),
            ResponsePayload::Rejected(rejected) => Err(ClientError::Kernel {
                code: rejected.code,
                message: rejected.message,
            }),
            other => Err(ClientError::Sidecar(format!(
                "unexpected response to CloseAgentSession: {other:?}"
            ))),
        }
    }

    /// Get buffered session events (bounded ring), filtered by `since`/`method`. Synthetic events
    /// use negative sequence numbers.
    pub fn get_session_events(
        &self,
        session_id: &str,
        options: GetEventsOptions,
    ) -> std::result::Result<Vec<SequencedEvent>, ClientError> {
        self.require_session(session_id, |entry| {
            let ring = entry.event_ring.lock();
            ring.iter()
                .filter(|event| {
                    options
                        .since
                        .map(|since| event.sequence_number > since)
                        .unwrap_or(true)
                })
                .filter(|event| {
                    options
                        .method
                        .as_ref()
                        .map(|method| &event.notification.method == method)
                        .unwrap_or(true)
                })
                .cloned()
                .collect()
        })
    }

    /// Respond to a permission request. If a pending reply slot exists, resolves it and returns a
    /// synthetic `{ via: "sidecar-request" }`; else the legacy `request/permission` RPC. Mirrors
    /// `respondPermission`.
    pub async fn respond_permission(
        &self,
        session_id: &str,
        permission_id: &str,
        reply: PermissionReply,
    ) -> Result<JsonRpcResponse> {
        let pending = self.require_session(session_id, |entry| {
            entry
                .pending_permission_replies
                .remove(permission_id)
                .map(|(_, responder)| responder)
        })?;

        if let Some(responder) = pending {
            let _ = responder.send(reply);
            return Ok(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: Some(JsonRpcId::Null),
                result: Some(json!({
                    "permissionId": permission_id,
                    "reply": reply,
                    "via": "sidecar-request",
                })),
                error: None,
            });
        }

        Ok(self
            .send_session_request(
                session_id,
                LEGACY_PERMISSION_METHOD,
                Some(json!({ "permissionId": permission_id, "reply": reply })),
            )
            .await?)
    }

    /// Set the session mode (`session/set_mode`). Updates cached `current_mode_id` on success.
    pub async fn set_session_mode(
        &self,
        session_id: &str,
        mode_id: &str,
    ) -> Result<JsonRpcResponse> {
        Ok(self
            .send_session_request(
                session_id,
                "session/set_mode",
                Some(json!({ "modeId": mode_id })),
            )
            .await?)
    }

    /// Get cached session mode state.
    pub fn get_session_modes(&self, session_id: &str) -> Option<SessionModeState> {
        self.require_session(session_id, |entry| entry.modes.lock().clone())
            .ok()
            .flatten()
    }

    /// Set the session model. Uses `set_config_option` with category `model`; readonly -> error
    /// response; codex `-32601` fallback synthesizes a negative-seq update.
    pub async fn set_session_model(
        &self,
        session_id: &str,
        model: &str,
    ) -> Result<JsonRpcResponse> {
        Ok(self
            .set_session_config_by_category(session_id, "model", model)
            .await?)
    }

    /// Set the session thought level. Same as model with category `thought_level`.
    pub async fn set_session_thought_level(
        &self,
        session_id: &str,
        level: &str,
    ) -> Result<JsonRpcResponse> {
        Ok(self
            .set_session_config_by_category(session_id, "thought_level", level)
            .await?)
    }

    /// Get cached config options (shallow copy).
    pub fn get_session_config_options(&self, session_id: &str) -> Vec<SessionConfigOption> {
        self.require_session(session_id, |entry| entry.config_options.lock().clone())
            .unwrap_or_default()
    }

    /// Get cached capabilities (empty -> None).
    pub fn get_session_capabilities(&self, session_id: &str) -> Option<AgentCapabilities> {
        self.require_session(session_id, |entry| entry.capabilities.lock().clone())
            .ok()
            .flatten()
    }

    /// Get cached agent info.
    pub fn get_session_agent_info(&self, session_id: &str) -> Option<AgentInfo> {
        self.require_session(session_id, |entry| entry.agent_info.lock().clone())
            .ok()
            .flatten()
    }

    /// Raw passthrough to `send_session_request` (which already re-hydrates + applies set_mode /
    /// set_config_option cache updates). Mirrors `rawSessionSend`.
    pub async fn raw_session_send(
        &self,
        session_id: &str,
        method: &str,
        params: Option<Value>,
    ) -> Result<JsonRpcResponse> {
        Ok(self.send_session_request(session_id, method, params).await?)
    }

    /// Thin alias for `raw_session_send`.
    pub async fn raw_send(
        &self,
        session_id: &str,
        method: &str,
        params: Option<Value>,
    ) -> Result<JsonRpcResponse> {
        self.raw_session_send(session_id, method, params).await
    }

    /// Subscribe to a session's `session/update` events. Only `session/update` notifications are
    /// delivered. Replay-on-subscribe defaults true (buffered events replayed first), then live
    /// events with `seq > last_delivered` per subscriber.
    pub fn on_session_event(
        &self,
        session_id: &str,
    ) -> std::result::Result<
        (Pin<Box<dyn Stream<Item = JsonRpcNotification> + Send>>, Subscription),
        ClientError,
    > {
        let (buffered, rx) = self.require_session(session_id, |entry| {
            let ring = entry.event_ring.lock();
            let buffered: VecDeque<SequencedEvent> = ring
                .iter()
                .filter(|event| should_dispatch_to_session_event_handlers(&event.notification))
                .cloned()
                .collect();
            (buffered, entry.event_tx.subscribe())
        })?;

        let stream = subscribe_with_replay(buffered, rx, i64::MIN, true);
        let mapped = futures::StreamExt::map(stream, |event| event.notification);
        Ok((Box::pin(mapped), Subscription::noop()))
    }

    /// Subscribe to a session's permission requests (request/responder). No subscribers -> auto
    /// reject; 120s timeout; both `request/permission` (legacy) and `session/request_permission`
    /// (ACP) method names are handled; the host answers via `respond_permission`.
    ///
    /// Each emitted [`PermissionRequest`] carries a `responder` oneshot. The matching
    /// `pending_permission_replies` slot is registered with a 120s timeout that auto-removes the
    /// entry on expiry. The constant is [`PERMISSION_TIMEOUT_MS`].
    pub fn on_permission_request(
        &self,
        session_id: &str,
    ) -> std::result::Result<
        (Pin<Box<dyn Stream<Item = PermissionRequest> + Send>>, Subscription),
        ClientError,
    > {
        let rx = self.require_session(session_id, |entry| entry.permission_tx.subscribe())?;

        // Pass broadcast items straight through. Each item carries a Clone-able
        // [`PermissionResponder`]; the reply slot + 120s timeout are armed by
        // [`AgentOs::deliver_permission_request`] at ingestion time, and `respond_permission`
        // resolves the same slot.
        let stream = futures::stream::unfold(rx, move |mut rx| async move {
            loop {
                match rx.recv().await {
                    Ok(request) => return Some((request, rx)),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        });

        Ok((Box::pin(stream), Subscription::noop()))
    }

    /// Deliver an inbound permission request to a session's subscribers, registering its reply slot
    /// into `pending_permission_replies` with a 120s ([`PERMISSION_TIMEOUT_MS`]) timeout that
    /// auto-rejects on expiry. When there are no subscribers the request auto-rejects immediately.
    ///
    /// Invoked by the sidecar event/request handler for both `request/permission` (legacy) and
    /// `session/request_permission` (ACP). The returned future resolves to the [`PermissionReply`]
    /// the host (or timeout / no-subscriber path) settles on, mirroring `_handleAcpPermissionRequest`
    /// / `_handlePermissionSidecarRequest`.
    pub(crate) async fn deliver_permission_request(
        &self,
        session_id: &str,
        notification: &JsonRpcNotification,
    ) -> PermissionReply {
        let Some((permission_id, request, responder_rx)) = build_permission_request(notification)
        else {
            return PermissionReply::Reject;
        };

        // Register the reply slot so `respond_permission` can resolve it directly.
        let (slot_tx, slot_rx) = tokio::sync::oneshot::channel::<PermissionReply>();
        let registered = self.require_session(session_id, |entry| {
            // No subscribers -> auto-reject (mirrors `permissionHandlers.size === 0`).
            if entry.permission_tx.receiver_count() == 0 {
                return false;
            }
            let _ = entry
                .pending_permission_replies
                .insert(permission_id.clone(), slot_tx);
            let _ = entry.permission_tx.send(request);
            true
        });

        match registered {
            Ok(true) => {}
            Ok(false) | Err(_) => return PermissionReply::Reject,
        }

        // Bridge the subscriber's `responder.respond(..)` into the same reply slot.
        let this = self.clone();
        let session_owned = session_id.to_string();
        let permission_owned = permission_id.clone();
        tokio::spawn(async move {
            if let Ok(reply) = responder_rx.await {
                let _ = this
                    .respond_permission(&session_owned, &permission_owned, reply)
                    .await;
            }
        });

        // Await the host reply, the subscriber responder (via the bridge above), or the 120s
        // timeout, whichever fires first.
        let timeout =
            tokio::time::sleep(std::time::Duration::from_millis(PERMISSION_TIMEOUT_MS));
        tokio::pin!(timeout);
        tokio::select! {
            reply = slot_rx => reply.unwrap_or(PermissionReply::Reject),
            _ = &mut timeout => {
                let _ = self.require_session(session_id, |entry| {
                    let _ = entry.pending_permission_replies.remove(&permission_id);
                });
                PermissionReply::Reject
            }
        }
    }
}

/// Placeholder response returned when a pending request was resolved locally (cancel/close) before
/// the sidecar replied. The exact shape depends on whether it was a prompt-cancel or a close.
fn session_closed_or_cancelled_placeholder(session_id: &str, method: &str) -> JsonRpcResponse {
    if method == "session/prompt" {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(JsonRpcId::Null),
            result: Some(json!({ "stopReason": "cancelled" })),
            error: None,
        }
    } else {
        session_closed_response(session_id)
    }
}
