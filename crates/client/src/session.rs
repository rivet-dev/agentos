//! Agent sessions (ACP) methods + supporting types.
//!
//! Ported from `packages/core/src/agent-os.ts` (session methods), `agent-session-types.ts`
//! (session/mode/config/capability/permission types), and `agents.ts` (`AgentType`, `AgentConfig`).
//!
//! ACP = JSON-RPC 2.0 over stdio. Sessions are referenced by string ID and return JSON-serializable
//! data only. JSON-RPC errors are NOT Rust `Err`; methods that issue requests return a
//! [`JsonRpcResponse`] whose `error` field may be set.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::Ordering;

use anyhow::Result;
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use agentos_protocol::generated::v1::{
    AcpCloseSessionRequest, AcpCreateSessionRequest, AcpGetSessionStateRequest, AcpRequest,
    AcpResponse, AcpResumeSessionRequest, AcpRuntimeKind, AcpSessionCreatedResponse,
    AcpSessionRequest, AcpSessionStateResponse,
};
use agentos_protocol::ACP_EXTENSION_NAMESPACE;
use secure_exec_client::wire;

use crate::agent_os::{AgentOs, SessionEntry};
use crate::config::{AgentOsConfig, BindingGroup, MountConfig};
use crate::error::ClientError;
use crate::json_rpc::{JsonRpcError, JsonRpcId, JsonRpcNotification, JsonRpcResponse};
use crate::stream::Subscription;
use crate::{CLOSED_SESSION_ID_RETENTION_LIMIT, PERMISSION_TIMEOUT_MS};

/// ACP method name for legacy permission requests/responses.
const LEGACY_PERMISSION_METHOD: &str = "request/permission";

/// Reserved `env` key on `AcpResumeSessionRequest` carrying the resolved adapter
/// bin entrypoint. The resume wire request omits a dedicated `adapterEntrypoint`
/// field; the sidecar reads the entrypoint from this key and strips it before
/// launching the adapter. Must stay in sync with the sidecar constant of the same
/// name in `crates/agentos-sidecar/src/acp_extension.rs`.
const RESUME_ADAPTER_ENTRYPOINT_ENV: &str = "AGENT_OS_RESUME_ADAPTER_ENTRYPOINT";

/// ACP method name for permission requests issued by the agent to the host (TS
/// `ACP_PERMISSION_METHOD`). Used by the host-request ACP dispatcher in `agent_os.rs`.
pub(crate) const ACP_PERMISSION_METHOD: &str = "session/request_permission";

/// Maximum in-flight session RPC requests per session.
const SESSION_PENDING_REQUEST_LIMIT: usize = 1024;

pub(crate) struct PermissionRouteRequest {
    pub(crate) session_id: String,
    pub(crate) permission_id: String,
    pub(crate) params: Value,
}

pub(crate) struct PermissionRouteResult {
    pub(crate) reply: Option<String>,
}

struct SessionCreatedResponse {
    session_id: String,
    modes: Option<Value>,
    config_options: Vec<Value>,
    agent_capabilities: Option<Value>,
    agent_info: Option<Value>,
}

pub(crate) struct SessionStateResponse {
    modes: Option<Value>,
    config_options: Vec<Value>,
    agent_capabilities: Option<Value>,
    agent_info: Option<Value>,
}

/// Maximum bytes accumulated into `PromptResult.text`.
const PROMPT_TEXT_CAPTURE_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// Maximum agent-message chunks tracked per prompt call.
const PROMPT_DELIVERED_CHUNK_LIMIT: usize = 262_144;

pub type SessionEventStream = Pin<Box<dyn Stream<Item = JsonRpcNotification> + Send>>;
pub type SessionEventSubscription = (SessionEventStream, Subscription);
pub type PermissionRequestStream = Pin<Box<dyn Stream<Item = PermissionRequest> + Send>>;
pub type PermissionRequestSubscription = (PermissionRequestStream, Subscription);
pub type AgentExitStream = Pin<Box<dyn Stream<Item = AgentExitEvent> + Send>>;
pub type AgentExitSubscription = (AgentExitStream, Subscription);

/// An unexpected ACP adapter process exit — a crash from the host's
/// perspective (any spontaneous exit without `close_session`, including exit
/// code 0) — plus the sidecar's bounded auto-restart outcome. Mirrors the wire
/// `AcpAgentExitedEvent` and the TS `AgentExitEvent`.
///
/// `restart` is one of `"restarted"` (adapter respawned and the session
/// natively re-attached under the same id; still usable), `"unsupported"`
/// (adapter lacks `loadSession`/`resume`; session evicted), `"failed"`
/// (respawn/re-attach errored; evicted), or `"exhausted"` (restart budget
/// spent; evicted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExitEvent {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "agentType")]
    pub agent_type: String,
    #[serde(rename = "processId")]
    pub process_id: String,
    /// Adapter exit code; `None` when the exit was observed indirectly.
    #[serde(rename = "exitCode")]
    pub exit_code: Option<i32>,
    pub restart: String,
    #[serde(rename = "restartCount")]
    pub restart_count: u32,
    #[serde(rename = "maxRestarts")]
    pub max_restarts: u32,
}

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
const BUILTIN_AGENT_IDS: [&str; 4] = ["pi", "pi-cli", "opencode", "claude"];

/// A built-in agent configuration (port of a TS `AGENT_CONFIGS` entry). System-prompt assembly and
/// injection are owned by the sidecar.
struct AgentConfigDef {
    acp_adapter: &'static str,
    agent_package: &'static str,
    default_env: &'static [(&'static str, &'static str)],
}

/// Resolve a built-in agent type to its config (port of TS `AGENT_CONFIGS`).
fn agent_config(agent_type: &str) -> Option<AgentConfigDef> {
    Some(match agent_type {
        "pi" => AgentConfigDef {
            acp_adapter: "@agentos-software/pi",
            agent_package: "@mariozechner/pi-coding-agent",
            default_env: &[],
        },
        "pi-cli" => AgentConfigDef {
            acp_adapter: "pi-acp",
            agent_package: "@mariozechner/pi-coding-agent",
            default_env: &[],
        },
        "opencode" => AgentConfigDef {
            acp_adapter: "@agentos-software/opencode",
            agent_package: "@agentos-software/opencode",
            default_env: &[
                ("OPENCODE_DISABLE_CONFIG_DEP_INSTALL", "1"),
                ("OPENCODE_DISABLE_EMBEDDED_WEB_UI", "1"),
            ],
        },
        "claude" => AgentConfigDef {
            acp_adapter: "@agentos-software/claude-code",
            agent_package: "@anthropic-ai/claude-agent-sdk",
            default_env: &[
                ("CLAUDE_AGENT_SDK_CLIENT_APP", "@rivet-dev/agentos"),
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
        _ => return None,
    })
}

/// Resolve a package's VM bin entrypoint from the host `node_modules` (port of
/// TS `_resolvePackageBin`). Prefer legacy `module_access_cwd/node_modules`,
/// then fall back to the host directory backing a native `/root/node_modules`
/// mount. The latter is the RivetKit actor path: the TS shim no longer forwards
/// `moduleAccessCwd`; callers explicitly mount the desired `node_modules`
/// directory instead.
fn resolve_package_bin(
    config: &AgentOsConfig,
    package_name: &str,
    bin_name: Option<&str>,
) -> std::result::Result<String, ClientError> {
    let mut candidates = Vec::new();
    let module_access_cwd = config
        .module_access_cwd
        .clone()
        .unwrap_or_else(|| ".".to_string());
    candidates.push(
        Path::new(&module_access_cwd)
            .join("node_modules")
            .join(package_name)
            .join("package.json"),
    );
    candidates.extend(node_modules_mount_package_json_paths(config, package_name));

    let contents = candidates
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .ok_or_else(|| {
            let looked = candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            ClientError::Sidecar(format!(
                "cannot resolve package {package_name}: no package.json found (looked in {looked})"
            ))
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

fn node_modules_mount_package_json_paths(
    config: &AgentOsConfig,
    package_name: &str,
) -> Vec<PathBuf> {
    config
        .mounts
        .iter()
        .filter_map(|mount| {
            let MountConfig::Native {
                path,
                plugin,
                read_only: _,
            } = mount
            else {
                return None;
            };
            if path != "/root/node_modules" || plugin.id != "host_dir" {
                return None;
            }
            let host_path = plugin
                .config
                .as_ref()
                .and_then(|config| config.get("hostPath"))
                .and_then(Value::as_str)?;
            Some(Path::new(host_path).join(package_name).join("package.json"))
        })
        .collect()
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
    /// Default `"/workspace"`.
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    /// Default `[]`.
    pub mcp_servers: Vec<McpServerConfig>,
    /// Default false.
    pub skip_os_instructions: bool,
    pub additional_instructions: Option<String>,
}

/// The id returned by `create_session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionId {
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

/// Result of `resume_session`. `session_id` is the live ACP session id in the
/// fresh VM: equal to the requested id for native loads, or a freshly assigned id
/// for the fallback tier — the caller (e.g. the actor) remaps `external -> live`.
/// `mode` is `"native"` or `"fallback"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeSessionResult {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub mode: String,
}

/// Options for `resume_session`. Mirrors the durability-dependent fields the
/// sidecar fallback tier needs to re-launch the adapter, plus the transcript
/// pointer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResumeSessionOptions {
    /// Guest-readable path to the reconstructed transcript. When present, the
    /// fallback tier arms a continuation preamble pointing the agent at it.
    pub transcript_path: Option<String>,
    /// Default `"/workspace"`.
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
}

/// Result of `prompt`.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptResult {
    pub response: JsonRpcResponse,
    pub text: String,
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
/// Requests are delivered by the sidecar permission-request path
/// ([`AgentOs::deliver_sidecar_permission_request`]). The subscriber resolves the request via
/// [`PermissionResponder::respond`] or [`AgentOs::respond_permission`]; the
/// [`crate::PERMISSION_TIMEOUT_MS`] timeout and the no-subscriber path auto-reject.
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

/// The wire string for a [`PermissionReply`] (`"once"` / `"always"` / `"reject"`), matching the
/// serde `lowercase` rename and the TS `PermissionReply` union.
fn permission_reply_wire(reply: PermissionReply) -> &'static str {
    match reply {
        PermissionReply::Once => "once",
        PermissionReply::Always => "always",
        PermissionReply::Reject => "reject",
    }
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

pub(crate) fn record_live_session_event(entry: &SessionEntry, notification: JsonRpcNotification) {
    apply_session_update(entry, &notification);
    if should_dispatch_to_session_event_handlers(&notification) {
        let _ = entry.event_tx.send(notification);
    }
}

fn apply_session_update(entry: &SessionEntry, notification: &JsonRpcNotification) {
    if notification.method != "session/update" {
        return;
    }
    let Some(params) = notification.params.as_ref().and_then(Value::as_object) else {
        return;
    };
    let update = params
        .get("update")
        .and_then(Value::as_object)
        .unwrap_or(params);
    match update.get("sessionUpdate").and_then(Value::as_str) {
        Some("current_mode_update") => {
            let Some(mode_id) = update.get("currentModeId").and_then(Value::as_str) else {
                return;
            };
            let mut modes = entry.modes.lock();
            if let Some(modes) = modes.as_mut() {
                modes.current_mode_id = mode_id.to_string();
            }
        }
        Some("config_option_update") | Some("config_options_update") => {
            let Some(options) = update.get("configOptions").and_then(Value::as_array) else {
                return;
            };
            let parsed = options
                .iter()
                .filter_map(|value| serde_json::from_value(value.clone()).ok())
                .collect();
            *entry.config_options.lock() = parsed;
            apply_synthetic_config_overrides(entry);
        }
        Some("agent_message_chunk") | None | Some(_) => {}
    }
}

fn accumulate_agent_message_chunk(
    notification: &JsonRpcNotification,
    delivered_chunks: &mut usize,
    agent_text: &mut String,
) -> std::result::Result<(), ClientError> {
    let params = notification.params.clone().unwrap_or(Value::Null);
    let update = params.get("update").cloned().unwrap_or(Value::Null);
    if update.get("sessionUpdate").and_then(Value::as_str) != Some("agent_message_chunk") {
        return Ok(());
    }
    if let Some(chunk) = update
        .get("content")
        .and_then(|content| content.get("text"))
        .and_then(Value::as_str)
    {
        if *delivered_chunks >= PROMPT_DELIVERED_CHUNK_LIMIT {
            return Err(prompt_chunk_limit_error());
        }
        let next_len = agent_text
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| prompt_text_limit_error(usize::MAX))?;
        if next_len > PROMPT_TEXT_CAPTURE_LIMIT_BYTES {
            return Err(prompt_text_limit_error(next_len));
        }
        agent_text.push_str(chunk);
        *delivered_chunks += 1;
    }
    Ok(())
}

fn pending_session_request_count(entry: &SessionEntry) -> usize {
    let mut count = 0;
    entry.pending_prompt_resolvers.scan(|_, _| {
        count += 1;
    });
    count
}

fn prompt_text_limit_error(size: usize) -> ClientError {
    ClientError::Sidecar(format!(
        "prompt text capture is {size} bytes, limit is {PROMPT_TEXT_CAPTURE_LIMIT_BYTES}"
    ))
}

fn prompt_chunk_limit_error() -> ClientError {
    ClientError::Sidecar(format!(
        "prompt chunk tracking limit exceeded: at most {PROMPT_DELIVERED_CHUNK_LIMIT} chunks can be captured per prompt"
    ))
}

struct PendingSessionRequestGuard<'a> {
    os: &'a AgentOs,
    session_id: &'a str,
    resolver_id: i64,
    active: bool,
}

impl<'a> PendingSessionRequestGuard<'a> {
    fn new(os: &'a AgentOs, session_id: &'a str, resolver_id: i64) -> Self {
        Self {
            os,
            session_id,
            resolver_id,
            active: true,
        }
    }

    fn cleanup(&mut self) {
        if self.active {
            self.os
                .cleanup_pending_resolver(self.session_id, self.resolver_id);
            self.active = false;
        }
    }
}

impl Drop for PendingSessionRequestGuard<'_> {
    fn drop(&mut self) {
        self.cleanup();
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

/// Apply the local cache mutations of `_syncSessionState`: modes, config options, capabilities,
/// and agent info from a sidecar [`SessionStateResponse`].
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

fn session_created_from_acp(
    response: AcpSessionCreatedResponse,
) -> std::result::Result<SessionCreatedResponse, ClientError> {
    Ok(SessionCreatedResponse {
        session_id: response.session_id,
        modes: parse_optional_json(response.modes, "modes")?,
        config_options: parse_json_vec(response.config_options, "configOptions")?,
        agent_capabilities: parse_optional_json(response.agent_capabilities, "agentCapabilities")?,
        agent_info: parse_optional_json(response.agent_info, "agentInfo")?,
    })
}

fn session_state_from_acp(
    response: AcpSessionStateResponse,
) -> std::result::Result<SessionStateResponse, ClientError> {
    Ok(SessionStateResponse {
        modes: parse_optional_json(response.modes, "modes")?,
        config_options: parse_json_vec(response.config_options, "configOptions")?,
        agent_capabilities: parse_optional_json(response.agent_capabilities, "agentCapabilities")?,
        agent_info: parse_optional_json(response.agent_info, "agentInfo")?,
    })
}

fn parse_optional_json(
    value: Option<String>,
    label: &str,
) -> std::result::Result<Option<Value>, ClientError> {
    value
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| {
                ClientError::Sidecar(format!("malformed ACP {label} JSON: {error}"))
            })
        })
        .transpose()
}

fn parse_json_vec(
    values: Vec<String>,
    label: &str,
) -> std::result::Result<Vec<Value>, ClientError> {
    values
        .into_iter()
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| {
                ClientError::Sidecar(format!("malformed ACP {label} JSON: {error}"))
            })
        })
        .collect()
}

fn unexpected_acp_response(operation: &str, response: AcpResponse) -> ClientError {
    ClientError::Sidecar(format!("unexpected response to {operation}: {response:?}"))
}

fn combine_instructions(additional: Option<&str>, tool_reference: &str) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(additional) = additional.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(additional.to_string());
    }
    let tool_reference = tool_reference.trim();
    if !tool_reference.is_empty() {
        parts.push(tool_reference.to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

fn build_host_binding_reference(binding_groups: &[BindingGroup]) -> String {
    if binding_groups.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        String::from("## Available Host Bindings"),
        String::new(),
        String::from("Run `agentos list-bindings` to see all available bindings."),
        String::new(),
    ];

    for kit in binding_groups {
        lines.push(format!("### {}", kit.name));
        lines.push(String::new());
        lines.push(kit.description.clone());
        lines.push(String::new());
        for binding in &kit.bindings {
            let signature = build_binding_flag_signature(&binding.input_schema);
            let suffix = if signature.is_empty() {
                String::new()
            } else {
                format!(" {signature}")
            };
            lines.push(format!(
                "- `agentos-{} {}{}` — {}",
                kit.name, binding.name, suffix, binding.description
            ));
        }
        lines.push(String::new());
        lines.push(format!(
            "Run `agentos-{} <binding> --help` for details.",
            kit.name
        ));
        lines.push(String::new());
    }

    lines.join("\n")
}

fn build_binding_flag_signature(schema: &Value) -> String {
    describe_binding_flags(schema)
        .into_iter()
        .map(|flag| {
            if flag.required {
                format!("{} <{}>", flag.name, flag.value_type)
            } else {
                format!("[{} <{}>]", flag.name, flag.value_type)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

struct BindingFlagDescription {
    name: String,
    value_type: String,
    required: bool,
}

fn describe_binding_flags(schema: &Value) -> Vec<BindingFlagDescription> {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    properties
        .into_iter()
        .map(|(field_name, field_schema)| BindingFlagDescription {
            name: format!("--{}", camel_to_kebab(&field_name)),
            value_type: describe_binding_flag_type(&field_schema),
            required: required.contains(&field_name),
        })
        .collect()
}

fn describe_binding_flag_type(schema: &Value) -> String {
    match json_schema_type(schema) {
        Some("array") => {
            let item_type = schema
                .get("items")
                .and_then(json_schema_type)
                .unwrap_or("string");
            format!("{item_type}[]")
        }
        Some("string") => schema
            .get("enum")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .filter(|values| !values.is_empty())
            .map(|values| values.join("|"))
            .unwrap_or_else(|| String::from("string")),
        Some(other) => other.to_string(),
        None => String::from("string"),
    }
}

fn json_schema_type(schema: &Value) -> Option<&str> {
    schema.get("type").and_then(Value::as_str)
}

fn camel_to_kebab(value: &str) -> String {
    let mut output = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() && index > 0 {
            output.push('-');
        }
        output.push(ch.to_ascii_lowercase());
    }
    output
}

// ---------------------------------------------------------------------------
// Methods
// ---------------------------------------------------------------------------

impl AgentOs {
    /// VM-scoped ownership for session RPCs.
    fn session_ownership(&self) -> wire::OwnershipScope {
        wire::OwnershipScope::VmOwnership(wire::VmOwnership {
            connection_id: self.connection_id().to_string(),
            session_id: self.wire_session_id().to_string(),
            vm_id: self.vm_id().to_string(),
        })
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

    /// Re-hydrate cached session state from the sidecar `AcpGetSessionStateRequest` snapshot.
    /// Mirrors `_hydrateSessionState`.
    async fn hydrate_session_state(
        &self,
        session_id: &str,
    ) -> std::result::Result<(), ClientError> {
        self.require_session(session_id, |_| ())?;
        let response = self
            .send_acp_request(AcpRequest::AcpGetSessionStateRequest(
                AcpGetSessionStateRequest {
                    session_id: session_id.to_string(),
                },
            ))
            .await?;
        let AcpResponse::AcpSessionStateResponse(state) = response else {
            return Err(unexpected_acp_response(
                "AcpGetSessionStateRequest",
                response,
            ));
        };
        let state = session_state_from_acp(state)?;

        self.require_session(session_id, |entry| sync_session_state(entry, &state))?;
        Ok(())
    }

    /// Core request helper: every session request routes through this. Tracks pending resolvers per
    /// session (cancel prompt-fallback + abort-on-close), calls the sidecar, re-hydrates state, and
    /// applies local cache updates for `set_mode` / `set_config_option`.
    pub(crate) async fn send_session_request(
        &self,
        session_id: &str,
        method: &str,
        params: Option<Value>,
    ) -> std::result::Result<JsonRpcResponse, ClientError> {
        let request_params = params;

        // Register a pending-resolver slot so cancel/close can resolve this request locally. The
        // resolver carries the intended [`JsonRpcResponse`] (close -> `-32000 Session closed`,
        // cancel -> `{stopReason: cancelled}`); whichever completes first wins. Mirrors the TS
        // resolver `{ method, resolve: (response) => void }`.
        let resolver_id = self.inner().request_counter.fetch_add(1, Ordering::SeqCst);
        let (resolve_tx, resolve_rx) = tokio::sync::oneshot::channel::<JsonRpcResponse>();
        self.require_session(session_id, |entry| {
            let _guard = entry.pending_session_request_lock.lock();
            if pending_session_request_count(entry) >= SESSION_PENDING_REQUEST_LIMIT {
                return Err(ClientError::Sidecar(format!(
                    "session pending request limit exceeded: at most {SESSION_PENDING_REQUEST_LIMIT} requests can be in flight per session"
                )));
            }
            let _ = entry
                .pending_prompt_resolvers
                .insert(resolver_id, resolve_tx);
            // Track the method so prompt-fallback can target only `session/prompt` resolvers.
            entry
                .config_overrides
                .lock()
                .entry(format!("{PENDING_METHOD_PREFIX}{resolver_id}"))
                .or_insert_with(|| method.to_string());
            Ok(())
        })??;
        let mut pending_request_guard =
            PendingSessionRequestGuard::new(self, session_id, resolver_id);

        let rpc = self.send_acp_request(AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: session_id.to_string(),
            method: method.to_string(),
            params: request_params
                .clone()
                .map(|params| serde_json::to_string(&params))
                .transpose()
                .map_err(|error| {
                    ClientError::Sidecar(format!("failed to encode session params: {error}"))
                })?,
        }));
        tokio::pin!(rpc);

        let response = tokio::select! {
            biased;
            resolved = resolve_rx => {
                // A cancel/close resolved this request locally before the sidecar replied. The
                // resolver carries the intended response (cancel vs close), set at the abort/cancel
                // site, so it is returned verbatim rather than re-derived from the method.
                pending_request_guard.cleanup();
                match resolved {
                    Ok(response) => return Ok(response),
                    Err(_) => return Ok(session_closed_response(session_id)),
                }
            }
            result = &mut rpc => {
                pending_request_guard.cleanup();
                result?
            }
        };

        let response = match response {
            AcpResponse::AcpSessionRpcResponse(rpc) => {
                serde_json::from_str::<JsonRpcResponse>(&rpc.response).map_err(|err| {
                    ClientError::Sidecar(format!("malformed session rpc response: {err}"))
                })?
            }
            other => return Err(unexpected_acp_response("AcpSessionRequest", other)),
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
    /// `_setSessionConfigByCategory`: readonly -> error response.
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
        BUILTIN_AGENT_IDS
            .iter()
            .filter_map(|id| {
                let config = agent_config(id)?;
                let installed =
                    resolve_package_bin(self.config(), config.acp_adapter, None).is_ok();
                Some(AgentRegistryEntry {
                    id: (*id).to_string(),
                    acp_adapter: config.acp_adapter.to_string(),
                    agent_package: config.agent_package.to_string(),
                    installed,
                })
            })
            .collect()
    }

    /// Create an ACP session. Resolves the agent config, merges env (user wins), creates the session
    /// via the sidecar (`runtime: java_script`, protocol v1, default client caps), and hydrates
    /// state. Agent OS owns dynamic binding-reference instructions and forwards them as additional
    /// instructions; the sidecar owns final base-prompt assembly and agent-specific injection. On
    /// hydration failure the session is removed and the error rethrown. Returns the session id only.
    pub async fn create_session(
        &self,
        agent_type: &str,
        options: CreateSessionOptions,
    ) -> Result<SessionId> {
        let config = agent_config(agent_type)
            .ok_or_else(|| ClientError::Sidecar(format!("Unknown agent type: {agent_type}")))?;

        // Resolve the ACP adapter's VM bin entrypoint from the host node_modules (mirrors TS
        // `_resolveAdapterBin` / `_resolvePackageBin`).
        let adapter_entrypoint = resolve_package_bin(self.config(), config.acp_adapter, None)?;

        // Merge env: agent default_env (lowest) -> user env (wins).
        let mut env: BTreeMap<String, String> = config
            .default_env
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        for (key, value) in &options.env {
            env.insert(key.clone(), value.clone());
        }
        if (agent_type == "pi" || agent_type == "pi-cli") && !env.contains_key("PI_ACP_PI_COMMAND")
        {
            if let Ok(pi_command) =
                resolve_package_bin(self.config(), config.agent_package, Some("pi"))
            {
                env.insert("PI_ACP_PI_COMMAND".to_string(), pi_command);
            }
        }

        let cwd = options
            .cwd
            .clone()
            .unwrap_or_else(|| "/workspace".to_string());
        let mcp_servers: Vec<Value> = options
            .mcp_servers
            .iter()
            .filter_map(|server| serde_json::to_value(server).ok())
            .collect();
        let client_capabilities = json!({
            "fs": { "readTextFile": true, "writeTextFile": true },
            "terminal": true,
        });
        let tool_reference = build_host_binding_reference(&self.config().binding_groups);
        let additional_instructions =
            combine_instructions(options.additional_instructions.as_deref(), &tool_reference);

        let response = self
            .send_acp_request(AcpRequest::AcpCreateSessionRequest(
                AcpCreateSessionRequest {
                    agent_type: agent_type.to_string(),
                    runtime: AcpRuntimeKind::JavaScript,
                    adapter_entrypoint,
                    args: Vec::new(),
                    env: env.into_iter().collect(),
                    cwd,
                    mcp_servers: serde_json::to_string(&mcp_servers).map_err(|error| {
                        ClientError::Sidecar(format!("failed to encode MCP servers: {error}"))
                    })?,
                    protocol_version: crate::ACP_PROTOCOL_VERSION as i32,
                    client_capabilities: serde_json::to_string(&client_capabilities).map_err(
                        |error| {
                            ClientError::Sidecar(format!(
                                "failed to encode client capabilities: {error}"
                            ))
                        },
                    )?,
                    additional_instructions,
                    skip_os_instructions: options.skip_os_instructions,
                },
            ))
            .await?;
        let AcpResponse::AcpSessionCreatedResponse(created) = response else {
            return Err(unexpected_acp_response("AcpCreateSessionRequest", response).into());
        };
        let created = session_created_from_acp(created)?;

        // Seed local state from the create response, then register + hydrate from the authoritative
        // sidecar state.
        let state = SessionStateResponse {
            modes: created.modes,
            config_options: created.config_options,
            agent_capabilities: created.agent_capabilities,
            agent_info: created.agent_info,
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

        let (event_tx, _) = tokio::sync::broadcast::channel(1024);
        let (permission_tx, _) = tokio::sync::broadcast::channel(64);
        let (agent_exit_tx, _) = tokio::sync::broadcast::channel(16);
        let entry = SessionEntry {
            agent_type: agent_type.to_string(),
            modes: parking_lot::Mutex::new(None),
            config_options: parking_lot::Mutex::new(Vec::new()),
            capabilities: parking_lot::Mutex::new(None),
            agent_info: parking_lot::Mutex::new(None),
            config_overrides: parking_lot::Mutex::new(BTreeMap::new()),
            event_tx,
            permission_tx,
            agent_exit_tx,
            pending_permission_replies: scc::HashMap::new(),
            pending_session_request_lock: parking_lot::Mutex::new(()),
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

    /// Resume a session that exists in durable storage but is not live in this VM
    /// (e.g. after a Rivet actor slept and woke with a fresh VM). Thin forwarder:
    /// resolves the agent config + adapter entrypoint exactly as `create_session`
    /// does, then forwards a single [`AcpResumeSessionRequest`] to the sidecar,
    /// which owns the resume state machine (native `session/load` when supported,
    /// else `session/new` + transcript-continuation preamble). The returned
    /// `session_id` is the live id in this VM (equal to `session_id` for native
    /// loads, freshly assigned for the fallback); the caller remaps
    /// `external -> live`. The new live session is registered + hydrated locally so
    /// subsequent prompts route to it.
    ///
    /// Resume depends on a durable root; on a non-durable (default in-memory) root
    /// there is no surviving store and the fallback tier always runs.
    pub async fn resume_session(
        &self,
        session_id: &str,
        agent_type: &str,
        options: ResumeSessionOptions,
    ) -> Result<ResumeSessionResult> {
        let config = agent_config(agent_type)
            .ok_or_else(|| ClientError::Sidecar(format!("Unknown agent type: {agent_type}")))?;
        let adapter_entrypoint = resolve_package_bin(self.config(), config.acp_adapter, None)?;

        // Merge env: agent default_env (lowest) -> user env (wins), then carry the
        // resolved adapter entrypoint under the sidecar's reserved key (the resume
        // wire request has no dedicated `adapterEntrypoint` field).
        let mut env: BTreeMap<String, String> = config
            .default_env
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        for (key, value) in &options.env {
            env.insert(key.clone(), value.clone());
        }
        if (agent_type == "pi" || agent_type == "pi-cli") && !env.contains_key("PI_ACP_PI_COMMAND")
        {
            if let Ok(pi_command) =
                resolve_package_bin(self.config(), config.agent_package, Some("pi"))
            {
                env.insert("PI_ACP_PI_COMMAND".to_string(), pi_command);
            }
        }
        env.insert(
            RESUME_ADAPTER_ENTRYPOINT_ENV.to_string(),
            adapter_entrypoint,
        );

        let cwd = options
            .cwd
            .clone()
            .unwrap_or_else(|| "/workspace".to_string());

        let response = self
            .send_acp_request(AcpRequest::AcpResumeSessionRequest(
                AcpResumeSessionRequest {
                    session_id: session_id.to_string(),
                    agent_type: agent_type.to_string(),
                    transcript_path: options.transcript_path.clone(),
                    cwd,
                    env: env.into_iter().collect(),
                },
            ))
            .await?;
        let AcpResponse::AcpSessionResumedResponse(resumed) = response else {
            return Err(unexpected_acp_response("AcpResumeSessionRequest", response).into());
        };

        // Register + hydrate the live session so subsequent prompts route to it.
        let empty_state = SessionStateResponse {
            modes: None,
            config_options: Vec::new(),
            agent_capabilities: None,
            agent_info: None,
        };
        self.register_session(&resumed.session_id, agent_type, &empty_state)
            .await?;

        Ok(ResumeSessionResult {
            session_id: resumed.session_id,
            mode: resumed.mode,
        })
    }

    /// Destroy a session. Best-effort `cancel_session` then internal close.
    pub async fn destroy_session(&self, session_id: &str) -> Result<()> {
        self.require_session(session_id, |_| ())?;
        let _ = self.cancel_session(session_id).await;
        self.close_session_internal(session_id).await?;
        Ok(())
    }

    /// Prompt a session. Subscribes to live `session/update` events, accumulates
    /// `agent_message_chunk` text, sends `session/prompt`, and unsubscribes by dropping the
    /// receiver. The `response` may itself be an error.
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<PromptResult> {
        let mut rx = self.require_session(session_id, |entry| entry.event_tx.subscribe())?;

        let mut agent_text = String::new();
        let mut delivered_chunks = 0;
        let mut prompt_text_error: Option<ClientError> = None;

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
                        Ok(event) => accumulate_agent_message_chunk(
                            &event,
                            &mut delivered_chunks,
                            &mut agent_text,
                        )
                        .unwrap_or_else(|error| {
                            prompt_text_error.get_or_insert(error);
                        }),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            // Channel closed; finish the request without further chunks.
                            break (&mut request).await;
                        }
                    }
                }
            }
        };

        // Drain already-buffered live events before unsubscribing.
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    accumulate_agent_message_chunk(&event, &mut delivered_chunks, &mut agent_text)
                        .unwrap_or_else(|error| {
                            prompt_text_error.get_or_insert(error);
                        })
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::TryRecvError::Empty)
                | Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            }
        }
        drop(rx);

        let response = response?;
        if let Some(error) = prompt_text_error {
            return Err(error.into());
        }

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
    /// record the closed id (bounded), and best-effort `AcpCloseSessionRequest`. Mirrors
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

        // Session processes live entirely inside the VM, so the only safe teardown is the ACP close
        // request, which targets the guest process by its in-VM session/process handle.
        //
        // NEVER fall back to a host `kill()` here. A session/process pid is a guest/kernel display
        // PID, not a host PID. Passing it to the host signal API would SIGKILL whatever unrelated
        // host process happens to share that number -- and a negative PID kills the entire host
        // process *group* with that id. In the TypeScript client that has in practice killed the host
        // tmux session, the test launcher, and even the user systemd manager. This client holds no
        // host handle for guest processes, so there is nothing host-side to signal; the ACP close
        // request remains the authoritative teardown path.
        let response = self
            .send_acp_request(AcpRequest::AcpCloseSessionRequest(AcpCloseSessionRequest {
                session_id: session_id.to_string(),
            }))
            .await?;
        match response {
            AcpResponse::AcpSessionClosedResponse(_) => Ok(()),
            other => Err(unexpected_acp_response("AcpCloseSessionRequest", other)),
        }
    }

    async fn send_acp_request(
        &self,
        request: AcpRequest,
    ) -> std::result::Result<AcpResponse, ClientError> {
        let payload = serde_bare::to_vec(&request).map_err(|error| {
            ClientError::Sidecar(format!("failed to encode ACP request: {error}"))
        })?;
        let response = self
            .transport()
            .request_wire(
                self.session_ownership(),
                wire::RequestPayload::ExtEnvelope(wire::ExtEnvelope {
                    namespace: ACP_EXTENSION_NAMESPACE.to_string(),
                    payload,
                }),
            )
            .await?;
        let envelope = match response {
            wire::ResponsePayload::ExtEnvelope(envelope) => envelope,
            wire::ResponsePayload::RejectedResponse(rejected) => {
                return Err(ClientError::Kernel {
                    code: rejected.code,
                    message: rejected.message,
                });
            }
            other => {
                return Err(ClientError::Sidecar(format!(
                    "unexpected ACP Ext response: {other:?}"
                )));
            }
        };
        if envelope.namespace != ACP_EXTENSION_NAMESPACE {
            return Err(ClientError::Sidecar(format!(
                "unexpected ACP Ext namespace: {}",
                envelope.namespace
            )));
        }
        let response: AcpResponse = serde_bare::from_slice(&envelope.payload).map_err(|error| {
            ClientError::Sidecar(format!("failed to decode ACP response: {error}"))
        })?;
        match response {
            AcpResponse::AcpErrorResponse(error) => Err(ClientError::Kernel {
                code: error.code,
                message: error.message,
            }),
            response => Ok(response),
        }
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
    /// response.
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

    /// Subscribe to live `session/update` events. Only events emitted after subscription are
    /// delivered.
    pub fn on_session_event(
        &self,
        session_id: &str,
    ) -> std::result::Result<SessionEventSubscription, ClientError> {
        let rx = self.require_session(session_id, |entry| entry.event_tx.subscribe())?;
        let stream = futures::stream::unfold(rx, move |mut rx| async move {
            loop {
                match rx.recv().await {
                    Ok(notification) => return Some((notification, rx)),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        });
        Ok((Box::pin(stream), Subscription::noop()))
    }

    /// Subscribe to permission requests raised by the session's guest agent. Requests originate
    /// from the sidecar `permission_request` callback (the sidecar normalizes both the legacy
    /// `request/permission` and ACP `session/request_permission` method names before invoking the
    /// host). With no subscribers a request auto-rejects; subscribers reply via the carried
    /// [`PermissionResponder`] or [`AgentOs::respond_permission`], bounded by the
    /// [`crate::PERMISSION_TIMEOUT_MS`] timeout.
    pub fn on_permission_request(
        &self,
        session_id: &str,
    ) -> std::result::Result<PermissionRequestSubscription, ClientError> {
        let rx = self.require_session(session_id, |entry| entry.permission_tx.subscribe())?;

        // Pass broadcast items straight through. Each item carries a cloneable
        // [`PermissionResponder`] that resolves the pending reply slot registered by
        // `deliver_sidecar_permission_request`.
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

    /// Subscribe to unexpected adapter process exits (crashes) for a session,
    /// including the sidecar's bounded auto-restart outcome. Only events
    /// emitted after subscription are delivered; only `restart == "restarted"`
    /// leaves the session usable. Mirrors the TS `onAgentExit` option.
    pub fn on_agent_exit(
        &self,
        session_id: &str,
    ) -> std::result::Result<AgentExitSubscription, ClientError> {
        let rx = self.require_session(session_id, |entry| entry.agent_exit_tx.subscribe())?;
        let stream = futures::stream::unfold(rx, move |mut rx| async move {
            loop {
                match rx.recv().await {
                    Ok(event) => return Some((event, rx)),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        });
        Ok((Box::pin(stream), Subscription::noop()))
    }

    /// Answer an ACP permission callback by fanning a [`PermissionRequest`] out to
    /// `on_permission_request` subscribers and waiting for the reply. Mirrors TS
    /// `_handlePermissionSidecarRequest`:
    /// - unknown session -> `error: "Session not found: <id>"`
    /// - no subscribers -> `reply: "reject"`
    /// - otherwise registers the `pending_permission_replies` slot, delivers the request, and waits
    ///   up to [`crate::PERMISSION_TIMEOUT_MS`] for `respond_permission` / the responder; timeout
    ///   removes the slot and returns `error: "Timed out waiting for permission reply: <id>"`.
    pub(crate) async fn deliver_sidecar_permission_request(
        &self,
        request: PermissionRouteRequest,
    ) -> PermissionRouteResult {
        let PermissionRouteRequest {
            session_id,
            permission_id,
            params,
        } = request;

        let (slot_tx, slot_rx) = tokio::sync::oneshot::channel::<PermissionReply>();
        let (responder, responder_rx) = PermissionResponder::new();
        let description = params
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string);
        let delivered = PermissionRequest {
            permission_id: permission_id.clone(),
            description,
            params,
            responder,
        };

        // Register the reply slot and broadcast under the same session lookup. No subscribers ->
        // auto-reject (mirrors `permissionHandlers.size === 0`).
        let registered = self.require_session(&session_id, |entry| {
            if entry.permission_tx.receiver_count() == 0 {
                return false;
            }
            let _ = entry
                .pending_permission_replies
                .insert(permission_id.clone(), slot_tx);
            let _ = entry.permission_tx.send(delivered);
            true
        });
        match registered {
            Ok(true) => {}
            Ok(false) => {
                return PermissionRouteResult {
                    reply: Some(permission_reply_wire(PermissionReply::Reject).to_string()),
                };
            }
            Err(_) => {
                return PermissionRouteResult { reply: None };
            }
        }

        // Bridge the subscriber's `responder.respond(..)` into the same reply slot.
        let this = self.clone();
        let bridge_session_id = session_id.clone();
        let bridge_permission_id = permission_id.clone();
        tokio::spawn(async move {
            if let Ok(reply) = responder_rx.await {
                let _ = this
                    .respond_permission(&bridge_session_id, &bridge_permission_id, reply)
                    .await;
            }
        });

        let timeout = tokio::time::sleep(std::time::Duration::from_millis(PERMISSION_TIMEOUT_MS));
        tokio::pin!(timeout);
        tokio::select! {
            reply = slot_rx => match reply {
                Ok(reply) => PermissionRouteResult {
                    reply: Some(permission_reply_wire(reply).to_string()),
                },
                // The slot sender dropped without a reply (session closed / replies rejected).
                Err(_) => PermissionRouteResult {
                    reply: Some(permission_reply_wire(PermissionReply::Reject).to_string()),
                },
            },
            _ = &mut timeout => {
                let _ = self.require_session(&session_id, |entry| {
                    let _ = entry.pending_permission_replies.remove(&permission_id);
                });
                PermissionRouteResult {
                    reply: None,
                }
            }
        }
    }
}

// Private accumulator coverage stays inline because integration tests cannot construct the missed
// broadcast plus hydrated-ring ordering without exposing client internals.
#[cfg(test)]
mod prompt_accumulation_tests {
    use super::*;

    fn notification(update: Value) -> JsonRpcNotification {
        JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "session/update".to_string(),
            params: Some(json!({ "update": update })),
        }
    }

    #[test]
    fn non_chunk_events_do_not_affect_prompt_text() {
        let chunk = notification(json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "text": "hello" },
        }));
        let non_chunk = notification(json!({
            "sessionUpdate": "current_mode_update",
            "currentModeId": "default",
        }));

        let mut delivered_chunks = 0;
        let mut text = String::new();
        accumulate_agent_message_chunk(&non_chunk, &mut delivered_chunks, &mut text)
            .expect("non-chunk");
        accumulate_agent_message_chunk(&chunk, &mut delivered_chunks, &mut text).expect("chunk");

        assert_eq!(text, "hello");
    }

    #[test]
    fn prompt_text_capture_limit_rejects_overflowing_chunk() {
        let chunk = notification(json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "text": "abcd" },
        }));
        let mut delivered_chunks = 0;
        let mut text = "x".repeat(PROMPT_TEXT_CAPTURE_LIMIT_BYTES - 3);
        let error = accumulate_agent_message_chunk(&chunk, &mut delivered_chunks, &mut text)
            .expect_err("chunk should exceed prompt text cap");
        assert!(
            error.to_string().contains("prompt text capture is"),
            "unexpected error: {error}"
        );
        assert_eq!(text.len(), PROMPT_TEXT_CAPTURE_LIMIT_BYTES - 3);
    }

    #[test]
    fn prompt_chunk_limit_rejects_more_tracked_chunks() {
        let chunk = notification(json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "text": "x" },
        }));
        let mut delivered_chunks = PROMPT_DELIVERED_CHUNK_LIMIT;
        let mut text = String::new();
        let error = accumulate_agent_message_chunk(&chunk, &mut delivered_chunks, &mut text)
            .expect_err("chunk should exceed chunk tracking cap");
        assert!(
            error
                .to_string()
                .contains("prompt chunk tracking limit exceeded"),
            "unexpected error: {error}"
        );
        assert!(text.is_empty());
    }

    #[test]
    fn pending_session_request_count_tracks_registered_resolvers() {
        let (event_tx, _) = tokio::sync::broadcast::channel(1);
        let (permission_tx, _) = tokio::sync::broadcast::channel(1);
        let (agent_exit_tx, _) = tokio::sync::broadcast::channel(1);
        let entry = SessionEntry {
            agent_type: "pi".to_string(),
            modes: parking_lot::Mutex::new(None),
            config_options: parking_lot::Mutex::new(Vec::new()),
            capabilities: parking_lot::Mutex::new(None),
            agent_info: parking_lot::Mutex::new(None),
            config_overrides: parking_lot::Mutex::new(BTreeMap::new()),
            event_tx,
            permission_tx,
            agent_exit_tx,
            pending_permission_replies: scc::HashMap::new(),
            pending_session_request_lock: parking_lot::Mutex::new(()),
            pending_prompt_resolvers: scc::HashMap::new(),
        };
        let (first_tx, _first_rx) = tokio::sync::oneshot::channel();
        let (second_tx, _second_rx) = tokio::sync::oneshot::channel();
        let _ = entry.pending_prompt_resolvers.insert(1, first_tx);
        let _ = entry.pending_prompt_resolvers.insert(2, second_tx);

        assert_eq!(pending_session_request_count(&entry), 2);
    }
}
