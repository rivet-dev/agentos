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
    CloseAgentSessionRequest, CreateSessionRequest, GetSessionStateRequest, GuestRuntimeKind,
    OwnershipScope, RequestPayload, ResponsePayload, SessionCreatedResponse, SessionRequest,
    SessionStateResponse,
};

use crate::agent_os::{AgentOs, SessionEntry};
use crate::error::ClientError;
use crate::json_rpc::{
    JsonRpcError, JsonRpcId, JsonRpcNotification, JsonRpcResponse, SequencedEvent,
};
use crate::stream::{subscribe_with_replay, Subscription};
use crate::{ACP_SESSION_EVENT_RETENTION_LIMIT, CLOSED_SESSION_ID_RETENTION_LIMIT};

/// ACP method name for legacy permission requests/responses.
const LEGACY_PERMISSION_METHOD: &str = "request/permission";
/// ACP method name for `session/request_permission` (newer ACP).
const ACP_PERMISSION_METHOD: &str = "session/request_permission";

pub type SessionEventStream = Pin<Box<dyn Stream<Item = JsonRpcNotification> + Send>>;
pub type SessionEventSubscription = (SessionEventStream, Subscription);
pub type PermissionRequestStream = Pin<Box<dyn Stream<Item = PermissionRequest> + Send>>;
pub type PermissionRequestSubscription = (PermissionRequestStream, Subscription);

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

/// Built-in agent ids (mirrors the keys of TS `AGENT_CONFIGS`).
const BUILTIN_AGENT_IDS: [&str; 5] = ["pi", "pi-cli", "opencode", "claude", "codex"];

/// opencode context-file paths injected via `OPENCODE_CONTEXTPATHS` (port of TS `OPENCODE_CONTEXT_PATHS`).
const OPENCODE_CONTEXT_PATHS: [&str; 12] = [
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
    "/etc/agentos/instructions.md",
];

/// A built-in agent configuration (port of a TS `AGENT_CONFIGS` entry). `prepareInstructions` is a
/// documented nuance not yet ported.
struct AgentConfigDef {
    acp_adapter: &'static str,
    agent_package: &'static str,
    default_env: &'static [(&'static str, &'static str)],
}

/// Resolve a built-in agent type to its config (port of TS `AGENT_CONFIGS`).
fn agent_config(agent_type: &str) -> Option<AgentConfigDef> {
    Some(match agent_type {
        "pi" => AgentConfigDef {
            acp_adapter: "@rivet-dev/agent-os-pi",
            agent_package: "@mariozechner/pi-coding-agent",
            default_env: &[],
        },
        "pi-cli" => AgentConfigDef {
            acp_adapter: "pi-acp",
            agent_package: "@mariozechner/pi-coding-agent",
            default_env: &[],
        },
        "opencode" => AgentConfigDef {
            acp_adapter: "@rivet-dev/agent-os-opencode",
            agent_package: "@rivet-dev/agent-os-opencode",
            default_env: &[
                ("OPENCODE_DISABLE_CONFIG_DEP_INSTALL", "1"),
                ("OPENCODE_DISABLE_EMBEDDED_WEB_UI", "1"),
            ],
        },
        "claude" => AgentConfigDef {
            acp_adapter: "@rivet-dev/agent-os-claude",
            agent_package: "@anthropic-ai/claude-agent-sdk",
            default_env: &[
                ("CLAUDE_AGENT_SDK_CLIENT_APP", "@rivet-dev/agent-os"),
                ("CLAUDE_CODE_SIMPLE", "1"),
                ("CLAUDE_CODE_FORCE_AGENT_OS_RIPGREP", "1"),
                ("CLAUDE_CODE_DEFER_GROWTHBOOK_INIT", "1"),
                ("CLAUDE_CODE_DISABLE_CWD_PERSIST", "1"),
                ("CLAUDE_CODE_DISABLE_DEV_NULL_REDIRECT", "1"),
                ("CLAUDE_CODE_NODE_SHELL_WRAPPER", "1"),
                ("CLAUDE_CODE_DISABLE_STREAM_JSON_HOOK_EVENTS", "1"),
                ("CLAUDE_CODE_SHELL", "/bin/sh"),
                ("CLAUDE_CODE_SKIP_INITIAL_MESSAGES", "1"),
                ("CLAUDE_CODE_SKIP_SANDBOX_INIT", "1"),
                ("CLAUDE_CODE_SIMPLE_SHELL_EXEC", "1"),
                ("CLAUDE_CODE_SWAP_STDIO", "0"),
                ("CLAUDE_CODE_USE_PIPE_OUTPUT", "1"),
                ("DISABLE_TELEMETRY", "1"),
                ("SHELL", "/bin/sh"),
                ("USE_BUILTIN_RIPGREP", "0"),
            ],
        },
        "codex" => AgentConfigDef {
            acp_adapter: "@rivet-dev/agent-os-codex-agent",
            agent_package: "@rivet-dev/agent-os-codex",
            default_env: &[],
        },
        _ => return None,
    })
}

/// Resolve a package's VM bin entrypoint from the host `node_modules` (port of TS
/// `_resolvePackageBin`, using `module_access_cwd` rather than software roots). Returns the
/// guest-visible path `/root/node_modules/<package>/<bin>`.
fn resolve_package_bin(
    module_access_cwd: &str,
    package_name: &str,
    bin_name: Option<&str>,
) -> std::result::Result<String, ClientError> {
    let pkg_json_path = std::path::Path::new(module_access_cwd)
        .join("node_modules")
        .join(package_name)
        .join("package.json");
    let contents = std::fs::read_to_string(&pkg_json_path).map_err(|error| {
        ClientError::Sidecar(format!("cannot read {}: {error}", pkg_json_path.display()))
    })?;
    let pkg: Value = serde_json::from_str(&contents).map_err(|error| {
        ClientError::Sidecar(format!("invalid package.json for {package_name}: {error}"))
    })?;
    let bin_entry: Option<String> = match &pkg["bin"] {
        Value::String(bin) => Some(bin.clone()),
        Value::Object(map) => bin_name
            .and_then(|name| map.get(name))
            .or_else(|| map.get(package_name))
            .or_else(|| map.values().next())
            .and_then(|value| value.as_str())
            .map(|bin| bin.to_string()),
        _ => None,
    };
    let bin_entry = bin_entry.ok_or_else(|| {
        ClientError::Sidecar(format!("No bin entry found in {package_name}/package.json"))
    })?;
    Ok(format!("/root/node_modules/{package_name}/{bin_entry}"))
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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
///
/// `currentModeId` and `availableModes` default so a loosely-shaped modes object (one missing either
/// field) still deserializes and is stored. Mirrors TS `toSessionModes`, which returns ANY non-array
/// object as `SessionModeState` with no field check.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionModeState {
    #[serde(default, rename = "currentModeId")]
    pub current_mode_id: String,
    #[serde(default, rename = "availableModes")]
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
///
/// `id` defaults so a partial entry missing `id` still deserializes and is kept (rather than dropped),
/// narrowing the gap with TS `toSessionConfigOptions`, which casts the whole array verbatim. Truly
/// non-object entries still cannot be stored in this typed Vec; see the parity audit minor note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionConfigOption {
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(
        default,
        rename = "currentValue",
        skip_serializing_if = "Option::is_none"
    )]
    pub current_value: Option<String>,
    #[serde(
        default,
        rename = "allowedValues",
        skip_serializing_if = "Option::is_none"
    )]
    pub allowed_values: Option<Vec<ConfigAllowedValue>>,
    #[serde(default, rename = "readOnly", skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
}

/// Prompt capabilities sub-object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PromptCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,
    #[serde(
        default,
        rename = "embeddedContext",
        skip_serializing_if = "Option::is_none"
    )]
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
    #[serde(
        default,
        rename = "tool_calls",
        skip_serializing_if = "Option::is_none"
    )]
    pub tool_calls: Option<bool>,
    #[serde(
        default,
        rename = "text_messages",
        skip_serializing_if = "Option::is_none"
    )]
    pub text_messages: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<bool>,
    #[serde(
        default,
        rename = "file_attachments",
        skip_serializing_if = "Option::is_none"
    )]
    pub file_attachments: Option<bool>,
    #[serde(
        default,
        rename = "session_lifecycle",
        skip_serializing_if = "Option::is_none"
    )]
    pub session_lifecycle: Option<bool>,
    #[serde(
        default,
        rename = "error_events",
        skip_serializing_if = "Option::is_none"
    )]
    pub error_events: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<bool>,
    #[serde(
        default,
        rename = "streaming_deltas",
        skip_serializing_if = "Option::is_none"
    )]
    pub streaming_deltas: Option<bool>,
    #[serde(default, rename = "mcp_tools", skip_serializing_if = "Option::is_none")]
    pub mcp_tools: Option<bool>,
    #[serde(
        default,
        rename = "promptCapabilities",
        skip_serializing_if = "Option::is_none"
    )]
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
    #[serde(
        default,
        rename = "configOptions",
        skip_serializing_if = "Option::is_none"
    )]
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
    inner:
        std::sync::Arc<parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<PermissionReply>>>>,
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
/// subscriber can call [`PermissionResponder::respond`], while the reachable notification path sends
/// replies with [`AgentOs::respond_permission`].
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

/// Whether a cached [`AgentCapabilities`] is empty in the TS sense (`Object.keys(caps).length === 0`):
/// every modeled field is `None` and there are no extra keys. `toAgentCapabilities` stores `{}` for
/// any non-object/empty state, and `getSessionCapabilities` returns `null` for that empty object.
fn agent_capabilities_is_empty(caps: &AgentCapabilities) -> bool {
    caps.permissions.is_none()
        && caps.plan_mode.is_none()
        && caps.questions.is_none()
        && caps.tool_calls.is_none()
        && caps.text_messages.is_none()
        && caps.images.is_none()
        && caps.file_attachments.is_none()
        && caps.session_lifecycle.is_none()
        && caps.error_events.is_none()
        && caps.reasoning.is_none()
        && caps.status.is_none()
        && caps.streaming_deltas.is_none()
        && caps.mcp_tools.is_none()
        && caps.prompt_capabilities.is_none()
        && caps.extra.is_empty()
}

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
fn next_highest_sequence_number(
    current: Option<i64>,
    ring: &VecDeque<SequencedEvent>,
) -> Option<i64> {
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
            if let Ok(parsed) = serde_json::from_value::<Vec<SessionConfigOption>>(Value::Array(
                config_options.clone(),
            )) {
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

    // Permission-from-notification delivery (mirrors the permission branch of
    // `_recordSessionNotification`). When a recorded notification is a legacy `request/permission`
    // or ACP `session/request_permission` with a string/number `permissionId`, deliver a
    // [`PermissionRequest`] to subscribers. This is the notification path: it broadcasts the request
    // with params verbatim, as TS does here, and replies are sent through `respond_permission`.
    if notification.method == LEGACY_PERMISSION_METHOD
        || notification.method == ACP_PERMISSION_METHOD
    {
        let params = notification.params.clone().unwrap_or(Value::Null);
        let permission_id = match params.get("permissionId") {
            Some(Value::String(id)) => Some(id.clone()),
            Some(Value::Number(num)) => Some(num.to_string()),
            _ => None,
        };
        if let Some(permission_id) = permission_id {
            let description = params
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string);
            // The notification path has no reply slot, so the responder resolves to nothing.
            let (responder, _receiver) = PermissionResponder::new();
            let request = PermissionRequest {
                permission_id,
                description,
                params,
                responder,
            };
            let _ = entry.permission_tx.send(request);
        }
    }
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
fn apply_codex_config_fallback(
    entry: &SessionEntry,
    category: &str,
    value: &str,
) -> JsonRpcResponse {
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
    async fn hydrate_session_state(
        &self,
        session_id: &str,
    ) -> std::result::Result<(), ClientError> {
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
            self.require_session(session_id, |entry| {
                augment_prompt_params(entry, params.clone())
            })?
        } else {
            params
        };

        // Register a pending-resolver slot so cancel/close can resolve this request locally. The
        // resolver carries the intended [`JsonRpcResponse`] (close -> `-32000 Session closed`,
        // cancel -> `{stopReason: cancelled}`); whichever completes first wins. Mirrors the TS
        // resolver `{ method, resolve: (response) => void }`.
        let resolver_id = self.inner().request_counter.fetch_add(1, Ordering::SeqCst);
        let (resolve_tx, resolve_rx) = tokio::sync::oneshot::channel::<JsonRpcResponse>();
        self.require_session(session_id, |entry| {
            let _ = entry
                .pending_prompt_resolvers
                .insert(resolver_id, resolve_tx);
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
            resolved = resolve_rx => {
                // A cancel/close resolved this request locally before the sidecar replied. The
                // resolver carries the intended response (cancel vs close), set at the abort/cancel
                // site, so it is returned verbatim rather than re-derived from the method.
                self.cleanup_pending_resolver(session_id, resolver_id);
                match resolved {
                    Ok(response) => return Ok(response),
                    Err(_) => return Ok(session_closed_response(session_id)),
                }
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
                if let Some(mode_id) = params.and_then(|p| p.get("modeId")).and_then(Value::as_str)
                {
                    let mut modes = entry.modes.lock();
                    if let Some(modes) = modes.as_mut() {
                        modes.current_mode_id = mode_id.to_string();
                    }
                }
            }
            if method == "session/set_config_option" {
                let config_id = params
                    .and_then(|p| p.get("configId"))
                    .and_then(Value::as_str);
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
        let module_access_cwd = self
            .config()
            .module_access_cwd
            .clone()
            .unwrap_or_else(|| ".".to_string());
        BUILTIN_AGENT_IDS
            .iter()
            .filter_map(|id| {
                let config = agent_config(id)?;
                let installed =
                    resolve_package_bin(&module_access_cwd, config.acp_adapter, None).is_ok();
                Some(AgentRegistryEntry {
                    id: (*id).to_string(),
                    acp_adapter: config.acp_adapter.to_string(),
                    agent_package: config.agent_package.to_string(),
                    installed,
                })
            })
            .collect()
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
        agent_type: &str,
        options: CreateSessionOptions,
    ) -> Result<SessionId> {
        let config = agent_config(agent_type)
            .ok_or_else(|| ClientError::Sidecar(format!("Unknown agent type: {agent_type}")))?;
        let module_access_cwd = self
            .config()
            .module_access_cwd
            .clone()
            .unwrap_or_else(|| ".".to_string());

        // Resolve the ACP adapter's VM bin entrypoint from the host node_modules (mirrors TS
        // `_resolveAdapterBin` / `_resolvePackageBin`).
        let adapter_entrypoint = resolve_package_bin(&module_access_cwd, config.acp_adapter, None)?;

        // prepareInstructions (per-agent OS-instruction injection): appended-prompt launch args for
        // pi/pi-cli/claude/codex, OPENCODE_CONTEXTPATHS env for opencode.
        let (args, prepared_env) = self.prepare_instructions(agent_type, &options).await?;

        // Merge env: agent default_env (lowest) -> prepareInstructions env -> user env (wins).
        let mut env: BTreeMap<String, String> = config
            .default_env
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        for (key, value) in prepared_env {
            env.insert(key, value);
        }
        for (key, value) in &options.env {
            env.insert(key.clone(), value.clone());
        }
        if (agent_type == "pi" || agent_type == "pi-cli") && !env.contains_key("PI_ACP_PI_COMMAND")
        {
            if let Ok(pi_command) =
                resolve_package_bin(&module_access_cwd, config.agent_package, Some("pi"))
            {
                env.insert("PI_ACP_PI_COMMAND".to_string(), pi_command);
            }
        }

        let cwd = options
            .cwd
            .clone()
            .unwrap_or_else(|| "/home/user".to_string());
        let mcp_servers: Vec<Value> = options
            .mcp_servers
            .iter()
            .filter_map(|server| serde_json::to_value(server).ok())
            .collect();
        let client_capabilities = json!({
            "fs": { "readTextFile": true, "writeTextFile": true },
            "terminal": true,
        });

        let response = self
            .transport()
            .request(
                self.session_ownership(),
                RequestPayload::CreateSession(CreateSessionRequest {
                    agent_type: agent_type.to_string(),
                    runtime: GuestRuntimeKind::JavaScript,
                    adapter_entrypoint,
                    args,
                    env,
                    cwd,
                    mcp_servers,
                    protocol_version: crate::ACP_PROTOCOL_VERSION,
                    client_capabilities,
                }),
            )
            .await?;
        let created: SessionCreatedResponse = match response {
            ResponsePayload::SessionCreated(created) => created,
            ResponsePayload::Rejected(rejected) => {
                return Err(ClientError::Kernel {
                    code: rejected.code,
                    message: rejected.message,
                }
                .into());
            }
            other => {
                return Err(ClientError::Sidecar(format!(
                    "unexpected create_session response: {other:?}"
                ))
                .into());
            }
        };

        // Seed local state from the create response, then register + hydrate (re-fetches the
        // authoritative state from the sidecar). `process_id` is filled by the hydrate snapshot.
        let state = SessionStateResponse {
            session_id: created.session_id.clone(),
            agent_type: agent_type.to_string(),
            process_id: String::new(),
            pid: created.pid,
            closed: false,
            modes: created.modes,
            config_options: created.config_options,
            agent_capabilities: created.agent_capabilities,
            agent_info: created.agent_info,
            events: Vec::new(),
        };
        self.register_session(&created.session_id, agent_type, &state)
            .await?;

        Ok(SessionId {
            session_id: created.session_id,
        })
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

        let (event_tx, _) =
            tokio::sync::broadcast::channel(ACP_SESSION_EVENT_RETENTION_LIMIT.max(1));
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
        let _ = self.inner().sessions.insert(session_id.to_string(), entry);

        match self.hydrate_session_state(session_id).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = self.inner().sessions.remove(session_id);
                Err(error)
            }
        }
    }

    /// Read OS instructions from `/etc/agentos/instructions.md` inside the VM, optionally appending
    /// session-level additional instructions. Port of TS `readVmInstructions` (tool-reference
    /// injection is a noted nuance not yet wired).
    async fn read_vm_instructions(
        &self,
        additional: Option<&str>,
        skip_base: bool,
    ) -> Result<String> {
        let mut parts: Vec<String> = Vec::new();
        if !skip_base {
            let data = self.read_file("/etc/agentos/instructions.md").await?;
            parts.push(String::from_utf8_lossy(&data).into_owned());
        }
        if let Some(additional) = additional {
            if !additional.is_empty() {
                parts.push(additional.to_string());
            }
        }
        if parts.is_empty() {
            return Ok(String::new());
        }
        // Horizontal rule so agents can distinguish the injected prompt from host-appended content.
        parts.push("---".to_string());
        Ok(parts.join("\n\n"))
    }

    /// Per-agent `prepareInstructions` (port of TS `AGENT_CONFIGS[*].prepareInstructions`). Returns
    /// the launch args and env additions to apply. pi/pi-cli/claude/codex append the OS+session
    /// instructions as a prompt arg; opencode injects them as `OPENCODE_CONTEXTPATHS`.
    async fn prepare_instructions(
        &self,
        agent_type: &str,
        options: &CreateSessionOptions,
    ) -> Result<(Vec<String>, BTreeMap<String, String>)> {
        let skip_base = options.skip_os_instructions;
        match agent_type {
            "pi" | "pi-cli" | "claude" | "codex" => {
                let flag = if agent_type == "codex" {
                    "--append-developer-instructions"
                } else {
                    "--append-system-prompt"
                };
                if !skip_base || options.additional_instructions.is_some() {
                    let instructions = self
                        .read_vm_instructions(options.additional_instructions.as_deref(), skip_base)
                        .await?;
                    if !instructions.is_empty() {
                        return Ok((vec![flag.to_string(), instructions], BTreeMap::new()));
                    }
                }
                Ok((Vec::new(), BTreeMap::new()))
            }
            "opencode" => {
                let mut context_paths: Vec<String> = if skip_base {
                    Vec::new()
                } else {
                    OPENCODE_CONTEXT_PATHS
                        .iter()
                        .map(|path| (*path).to_string())
                        .collect()
                };
                if let Some(additional) = options.additional_instructions.as_deref() {
                    if !additional.is_empty() {
                        let path = "/tmp/agentos-additional-instructions.md";
                        self.write_file(path, crate::fs::FileContent::Text(additional.to_string()))
                            .await?;
                        context_paths.push(path.to_string());
                    }
                }
                if context_paths.is_empty() {
                    return Ok((Vec::new(), BTreeMap::new()));
                }
                let mut env = BTreeMap::new();
                env.insert(
                    "OPENCODE_CONTEXTPATHS".to_string(),
                    serde_json::to_string(&context_paths).unwrap_or_default(),
                );
                Ok((Vec::new(), env))
            }
            _ => Ok((Vec::new(), BTreeMap::new())),
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

        let mut agent_text = String::new();
        let mut last_delivered = start_after;
        // Accumulate `agent_message_chunk` text from an event into the running buffer (dedup by
        // sequence number). Mirrors the synchronous `SessionEventHandler` invoked inline by TS
        // `_flushSessionEventHandlers`.
        let mut accumulate = |event: &SequencedEvent, agent_text: &mut String| {
            if event.sequence_number <= last_delivered {
                return;
            }
            last_delivered = event.sequence_number;
            let params = event.notification.params.clone().unwrap_or(Value::Null);
            let update = params.get("update").cloned().unwrap_or(Value::Null);
            if update.get("sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk") {
                if let Some(chunk) = update
                    .get("content")
                    .and_then(|content| content.get("text"))
                    .and_then(Value::as_str)
                {
                    agent_text.push_str(chunk);
                }
            }
        };

        let request = self.send_session_request(
            session_id,
            "session/prompt",
            Some(json!({ "prompt": [{ "type": "text", "text": text }] })),
        );
        tokio::pin!(request);

        // Drive the request to completion while concurrently draining broadcast chunks, so the
        // bounded broadcast buffer never lags during a long prompt.
        let response = loop {
            tokio::select! {
                biased;
                result = &mut request => break result,
                event = rx.recv() => {
                    match event {
                        Ok(event) => accumulate(&event, &mut agent_text),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            // Channel closed; finish the request without further chunks.
                            break (&mut request).await;
                        }
                    }
                }
            }
        };

        // The prompt has resolved; `send_session_request` re-hydrates state (recording any final
        // chunks) before returning. Drain every remaining buffered broadcast event with `try_recv`
        // until empty before unsubscribing (= the TS `finally { unsubscribe() }`), so chunks emitted
        // late (during the final hydrate) but not yet received are not dropped.
        loop {
            match rx.try_recv() {
                Ok(event) => accumulate(&event, &mut agent_text),
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::TryRecvError::Empty)
                | Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            }
        }
        drop(rx);

        let response = response?;
        Ok(PromptResult {
            response,
            text: agent_text,
        })
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
                    // Mirrors `_cancelPendingPromptRequests`: resolve prompt resolvers with the
                    // synthetic `{ result: { stopReason: "cancelled" } }` response.
                    let _ = resolver.send(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: Some(JsonRpcId::Null),
                        result: Some(json!({ "stopReason": "cancelled" })),
                        error: None,
                    });
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
            entry.pending_prompt_resolvers.scan(|id, _| ids.push(*id));
            for id in ids {
                if let Some((_, resolver)) = entry.pending_prompt_resolvers.remove(&id) {
                    // Mirrors `_abortPendingSessionRequests`: resolve EVERY pending resolver
                    // (prompt or otherwise) with the `-32000` `Session closed: <id>` error.
                    let _ = resolver.send(session_closed_response(session_id));
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

    /// Close a session. SYNC fire-and-forget. Errors only if unknown across sessions / closed-ids /
    /// in-flight closes. Aborts pending, rejects pending permissions, records the closed id (bounded
    /// 2048). Mirrors `closeSession`, whose known-check spans `_sessions`, `_closedSessionIds`, and
    /// `_sessionClosePromises`.
    pub fn close_session(&self, session_id: &str) -> std::result::Result<(), ClientError> {
        let known = self.inner().sessions.contains(session_id)
            || self.inner().closing_session_ids.contains(session_id)
            || self
                .inner()
                .closed_session_ids
                .lock()
                .iter()
                .any(|id| id == session_id);
        if !known {
            return Err(ClientError::SessionNotFound(session_id.to_string()));
        }

        // Synchronously mark the close in-flight (mirrors setting `_sessionClosePromises`) so a
        // second `close_session` / close-after-destroy issued during the detached close still sees
        // the id as known.
        let _ = self
            .inner()
            .closing_session_ids
            .insert(session_id.to_string());

        let this = self.clone();
        let session_id_owned = session_id.to_string();
        tokio::spawn(async move {
            let _ = this.close_session_internal(&session_id_owned).await;
            let _ = this.inner().closing_session_ids.remove(&session_id_owned);
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

        // Session processes live entirely inside the VM, so the only safe teardown is the sidecar
        // `CloseAgentSession` RPC (and the `kill_process` RPC if ever added here), which targets the
        // guest process by its in-VM session/process handle.
        //
        // NEVER fall back to a host `kill()` here. A session/process pid is a guest/kernel display
        // PID, not a host PID. Passing it to the host signal API would SIGKILL whatever unrelated
        // host process happens to share that number -- and a negative PID kills the entire host
        // process *group* with that id. In the TypeScript client that has in practice killed the host
        // tmux session, the test launcher, and even the user systemd manager. This client holds no
        // host handle for guest processes, so there is nothing host-side to signal; `CloseAgentSession`
        // remains the authoritative teardown path.
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

    /// Get cached capabilities. Mirrors `getSessionCapabilities`: returns `null` (`None`) when the
    /// stored capabilities object has no keys (`Object.keys(caps).length === 0`).
    pub fn get_session_capabilities(&self, session_id: &str) -> Option<AgentCapabilities> {
        self.require_session(session_id, |entry| entry.capabilities.lock().clone())
            .ok()
            .flatten()
            .filter(|caps| !agent_capabilities_is_empty(caps))
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
        Ok(self
            .send_session_request(session_id, method, params)
            .await?)
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
    ) -> std::result::Result<SessionEventSubscription, ClientError> {
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

    /// Subscribe to recorded session permission notifications. Both `request/permission` (legacy)
    /// and `session/request_permission` (ACP) method names are handled; the host answers via
    /// `respond_permission`.
    pub fn on_permission_request(
        &self,
        session_id: &str,
    ) -> std::result::Result<PermissionRequestSubscription, ClientError> {
        let rx = self.require_session(session_id, |entry| entry.permission_tx.subscribe())?;

        // Pass broadcast items straight through. Each item carries a cloneable
        // [`PermissionResponder`] for API parity, while reachable replies are sent with
        // `respond_permission`.
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
}

/// A settled permission outcome carrying both the resolved [`PermissionReply`] and a JSON-RPC
/// handler result.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionDelivery {
    /// The settled reply.
    pub reply: PermissionReply,
    /// The handler result to return on the wire (ACP outcome vs bare `{ reply }`).
    pub result: Value,
}
