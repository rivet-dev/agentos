//! Agent sessions (ACP) methods + supporting types.
//!
//! Ported from `packages/core/src/agent-os.ts` (session methods) and `agent-session-types.ts`
//! (session/mode/config/capability/permission types). Agent types are resolved dynamically
//! from the configured `/opt/agentos` package manifests (keyed by manifest `name`), exactly
//! as the TS client does — there is no hardcoded agent registry.
//!
//! ACP = JSON-RPC 2.0 over stdio. Sessions are referenced by string ID and return JSON-serializable
//! data only. JSON-RPC errors are NOT Rust `Err`; methods that issue requests return a
//! [`JsonRpcResponse`] whose `error` field may be set.

use std::collections::BTreeMap;
use std::pin::Pin;

use anyhow::Result;
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use agentos_protocol::generated::v1::{
    AcpCloseSessionRequest, AcpCreateSessionRequest, AcpGetSessionStateRequest,
    AcpListAgentsRequest, AcpListSessionsRequest, AcpRequest, AcpResponse, AcpResumeSessionRequest,
    AcpSessionCreatedResponse, AcpSessionRequest, AcpSessionStateResponse,
    AcpSetSessionConfigRequest,
};
use agentos_protocol::ACP_EXTENSION_NAMESPACE;
use agentos_sidecar_client::wire;

use crate::agent_os::{AgentOs, SessionEntry};
use crate::error::ClientError;
use crate::json_rpc::{JsonRpcId, JsonRpcNotification, JsonRpcResponse};
use crate::stream::Subscription;

/// ACP method name for legacy permission requests/responses.
const LEGACY_PERMISSION_METHOD: &str = "request/permission";

pub(crate) struct PermissionRouteRequest {
    pub(crate) session_id: String,
    pub(crate) permission_id: String,
    pub(crate) params: Value,
    pub(crate) timeout_ms: u64,
}

pub(crate) struct PermissionRouteResult {
    pub(crate) reply: Option<String>,
}

struct SessionCreatedResponse {
    session_id: String,
}

struct SessionRpcResult {
    response: JsonRpcResponse,
    text: Option<String>,
}

pub(crate) struct SessionStateResponse {
    modes: Option<Value>,
    config_options: Vec<Value>,
    agent_capabilities: Option<Value>,
    agent_info: Option<Value>,
}

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

/// A registry agent entry from `list_agents`. Mirrors the TS `AgentRegistryEntry`.
/// The client is npm-agnostic and parses no manifests: `list_agents` is a sidecar
/// ACP RPC that enumerates the projected `/opt/agentos` packages. The entry is just
/// the agent `id`; `installed` is always `true` (the package is materialized into
/// the VM at boot).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRegistryEntry {
    pub id: String,
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
/// sidecar-supplied timeout; an absent host reply is returned to the ACP sidecar,
/// which owns the default permission outcome.
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
// Host-event helpers
// ---------------------------------------------------------------------------

/// Whether an [`AgentCapabilities`] response is empty in the TS sense
/// (`Object.keys(caps).length === 0`).
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
    if should_dispatch_to_session_event_handlers(&notification) {
        let _ = entry.event_tx.send(notification);
    }
}

fn session_created_from_acp(
    response: AcpSessionCreatedResponse,
) -> std::result::Result<SessionCreatedResponse, ClientError> {
    Ok(SessionCreatedResponse {
        session_id: response.session_id,
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

    /// Read the authoritative session state from the sidecar.
    async fn get_session_state(
        &self,
        session_id: &str,
    ) -> std::result::Result<SessionStateResponse, ClientError> {
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
        session_state_from_acp(state)
    }

    /// Forward one session request and return the adapter's JSON-RPC response.
    pub(crate) async fn send_session_request(
        &self,
        session_id: &str,
        method: &str,
        params: Option<Value>,
    ) -> std::result::Result<JsonRpcResponse, ClientError> {
        Ok(self
            .send_session_request_with_text(session_id, method, params)
            .await?
            .response)
    }

    async fn send_session_request_with_text(
        &self,
        session_id: &str,
        method: &str,
        params: Option<Value>,
    ) -> std::result::Result<SessionRpcResult, ClientError> {
        let response = self
            .send_acp_request(AcpRequest::AcpSessionRequest(AcpSessionRequest {
                session_id: session_id.to_string(),
                method: method.to_string(),
                params: params
                    .map(|params| serde_json::to_string(&params))
                    .transpose()
                    .map_err(|error| {
                        ClientError::Sidecar(format!("failed to encode session params: {error}"))
                    })?,
            }))
            .await
            .map_err(|error| match error {
                ClientError::Kernel { ref code, .. } if code == "session_not_found" => {
                    ClientError::SessionNotFound(session_id.to_string())
                }
                error => error,
            })?;

        match response {
            AcpResponse::AcpSessionRpcResponse(rpc) => {
                let response =
                    serde_json::from_str::<JsonRpcResponse>(&rpc.response).map_err(|err| {
                        ClientError::Sidecar(format!("malformed session rpc response: {err}"))
                    })?;
                Ok(SessionRpcResult {
                    response,
                    text: rpc.text,
                })
            }
            other => Err(unexpected_acp_response("AcpSessionRequest", other)),
        }
    }

    /// Forward a category-based config selection to the ACP sidecar adapter.
    async fn set_session_config_by_category(
        &self,
        session_id: &str,
        category: &str,
        value: &str,
    ) -> std::result::Result<JsonRpcResponse, ClientError> {
        let response = self
            .send_acp_request(AcpRequest::AcpSetSessionConfigRequest(
                AcpSetSessionConfigRequest {
                    session_id: session_id.to_string(),
                    category: category.to_string(),
                    value: value.to_string(),
                },
            ))
            .await?;
        let AcpResponse::AcpSessionRpcResponse(response) = response else {
            return Err(unexpected_acp_response(
                "AcpSetSessionConfigRequest",
                response,
            ));
        };
        serde_json::from_str(&response.response).map_err(|error| {
            ClientError::Sidecar(format!("malformed session config response: {error}"))
        })
    }

    /// List the sidecar's authoritative live sessions for this connection.
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let response = self
            .send_acp_request(AcpRequest::AcpListSessionsRequest(AcpListSessionsRequest {
                reserved: false,
            }))
            .await?;
        let AcpResponse::AcpListSessionsResponse(listed) = response else {
            return Err(unexpected_acp_response("AcpListSessionsRequest", response).into());
        };
        Ok(listed
            .sessions
            .into_iter()
            .map(|session| SessionInfo {
                session_id: session.session_id,
                agent_type: session.agent_type,
            })
            .collect())
    }

    /// List available agents. A thin forwarder: sends `AcpListAgentsRequest` and
    /// maps the sidecar's response. The sidecar enumerates the projected
    /// `/opt/agentos` packages (client parses no manifests). Every such agent is a
    /// package materialized into the VM at boot, so `installed` is always `true`.
    pub async fn list_agents(&self) -> Result<Vec<AgentRegistryEntry>> {
        let response = self
            .send_acp_request(AcpRequest::AcpListAgentsRequest(AcpListAgentsRequest {
                reserved: false,
            }))
            .await?;
        let AcpResponse::AcpListAgentsResponse(listed) = response else {
            return Err(unexpected_acp_response("AcpListAgentsRequest", response).into());
        };
        Ok(listed
            .agents
            .into_iter()
            .map(|agent| AgentRegistryEntry {
                id: agent.id,
                installed: agent.installed,
            })
            .collect())
    }

    /// Create an ACP session. Forwards explicit options to the sidecar, which owns runtime,
    /// working-directory, protocol, capability, MCP, environment, and flag defaults. The sidecar
    /// owns base-prompt and registered-tool reference assembly plus agent-specific injection.
    /// Returns the session id only.
    pub async fn create_session(
        &self,
        agent_type: &str,
        options: CreateSessionOptions,
    ) -> Result<SessionId> {
        // The client is npm-agnostic: it sends only the agent name. The sidecar
        // resolves the name -> package -> entrypoint/env/launchArgs from the
        // projected `/opt/agentos/<name>/current/agentos-package.json` and spawns.
        let env = (!options.env.is_empty()).then(|| options.env.clone().into_iter().collect());
        let mcp_servers = if options.mcp_servers.is_empty() {
            None
        } else {
            let values: Vec<Value> = options
                .mcp_servers
                .iter()
                .filter_map(|server| serde_json::to_value(server).ok())
                .collect();
            Some(serde_json::to_string(&values).map_err(|error| {
                ClientError::Sidecar(format!("failed to encode MCP servers: {error}"))
            })?)
        };
        let response = self
            .send_acp_request(AcpRequest::AcpCreateSessionRequest(
                AcpCreateSessionRequest {
                    agent_type: agent_type.to_string(),
                    runtime: None,
                    args: None,
                    env,
                    cwd: options.cwd.clone(),
                    mcp_servers,
                    protocol_version: None,
                    client_capabilities: None,
                    additional_instructions: options.additional_instructions.clone(),
                    skip_os_instructions: options.skip_os_instructions.then_some(true),
                },
            ))
            .await?;
        let AcpResponse::AcpSessionCreatedResponse(created) = response else {
            return Err(unexpected_acp_response("AcpCreateSessionRequest", response).into());
        };
        let created = session_created_from_acp(created)?;
        self.register_session(&created.session_id);

        Ok(SessionId {
            session_id: created.session_id,
        })
    }

    /// Register only the host-side callback/event routes for a live sidecar session.
    pub(crate) fn register_session(&self, session_id: &str) {
        let (event_tx, _) = tokio::sync::broadcast::channel(1024);
        let (permission_tx, _) = tokio::sync::broadcast::channel(64);
        let (agent_exit_tx, _) = tokio::sync::broadcast::channel(16);
        let entry = SessionEntry {
            event_tx,
            permission_tx,
            agent_exit_tx,
            pending_permission_replies: scc::HashMap::new(),
        };
        let _ = self.inner().sessions.insert(session_id.to_string(), entry);
    }

    /// Resume a session that exists in durable storage but is not live in this VM
    /// (e.g. after a Rivet actor slept and woke with a fresh VM). Thin forwarder:
    /// resolves the agent config + adapter entrypoint exactly as `create_session`
    /// does, then forwards a single [`AcpResumeSessionRequest`] to the sidecar,
    /// which owns the resume state machine (native `session/load` when supported,
    /// else `session/new` + transcript-continuation preamble). The returned
    /// `session_id` is the live id in this VM (equal to `session_id` for native
    /// loads, freshly assigned for the fallback); the caller remaps
    /// `external -> live`. The new live session is registered locally only for host callback/event
    /// routing; authoritative state remains in the sidecar.
    ///
    /// Resume depends on a durable root; on a non-durable (default in-memory) root
    /// there is no surviving store and the fallback tier always runs.
    pub async fn resume_session(
        &self,
        session_id: &str,
        agent_type: &str,
        options: ResumeSessionOptions,
    ) -> Result<ResumeSessionResult> {
        // The client is npm-agnostic: it sends only the agent name. The sidecar
        // resolves the name -> package -> entrypoint/env/launchArgs from the
        // projected manifest, exactly as `create_session` does.
        let env = (!options.env.is_empty()).then(|| options.env.clone().into_iter().collect());

        let response = self
            .send_acp_request(AcpRequest::AcpResumeSessionRequest(
                AcpResumeSessionRequest {
                    session_id: session_id.to_string(),
                    agent_type: agent_type.to_string(),
                    transcript_path: options.transcript_path.clone(),
                    cwd: options.cwd.clone(),
                    env,
                },
            ))
            .await?;
        let AcpResponse::AcpSessionResumedResponse(resumed) = response else {
            return Err(unexpected_acp_response("AcpResumeSessionRequest", response).into());
        };

        self.register_session(&resumed.session_id);

        Ok(ResumeSessionResult {
            session_id: resumed.session_id,
            mode: resumed.mode,
        })
    }

    /// Destroy a session through the sidecar-owned graceful close path.
    pub async fn destroy_session(&self, session_id: &str) -> Result<()> {
        self.close_session(session_id).await?;
        Ok(())
    }

    /// Prompt a session. The sidecar returns the bounded text accumulated while
    /// it streams live `session/update` events to host subscribers.
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<PromptResult> {
        let result = self
            .send_session_request_with_text(
                session_id,
                "session/prompt",
                Some(json!({ "prompt": [{ "type": "text", "text": text }] })),
            )
            .await?;
        let agent_text = result.text.ok_or_else(|| {
            ClientError::Sidecar(String::from(
                "sidecar prompt response is missing accumulated text",
            ))
        })?;
        Ok(PromptResult {
            response: result.response,
            text: agent_text,
        })
    }

    /// Cancel through the sidecar, whose transport interrupts a blocking prompt
    /// and returns the authoritative prompt and cancel responses.
    pub async fn cancel_session(&self, session_id: &str) -> Result<JsonRpcResponse> {
        Ok(self
            .send_session_request(session_id, "session/cancel", None)
            .await?)
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

    /// Close a session through the sidecar. The sidecar makes repeated or unknown
    /// closes idempotent, so the client keeps no closed-id or in-flight-close state.
    pub async fn close_session(&self, session_id: &str) -> std::result::Result<(), ClientError> {
        self.reject_pending_permission_replies(session_id);
        let _ = self.inner().sessions.remove(session_id);

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
        let pending = self
            .inner()
            .sessions
            .read(session_id, |_, entry| {
                entry
                    .pending_permission_replies
                    .remove(permission_id)
                    .map(|(_, responder)| responder)
            })
            .flatten();

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

    /// Set the session mode (`session/set_mode`).
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

    /// Read session mode state from the sidecar.
    pub async fn get_session_modes(&self, session_id: &str) -> Result<Option<SessionModeState>> {
        let state = self.get_session_state(session_id).await?;
        Ok(state
            .modes
            .filter(Value::is_object)
            .and_then(|value| serde_json::from_value(value).ok()))
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

    /// Read session config options from the sidecar.
    pub async fn get_session_config_options(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionConfigOption>> {
        Ok(self
            .get_session_state(session_id)
            .await?
            .config_options
            .into_iter()
            .filter_map(|value| serde_json::from_value(value).ok())
            .collect())
    }

    /// Read capabilities from the sidecar. Returns `None` for an empty object.
    pub async fn get_session_capabilities(
        &self,
        session_id: &str,
    ) -> Result<Option<AgentCapabilities>> {
        let capabilities = self
            .get_session_state(session_id)
            .await?
            .agent_capabilities
            .filter(Value::is_object)
            .and_then(|value| serde_json::from_value(value).ok())
            .filter(|caps| !agent_capabilities_is_empty(caps));
        Ok(capabilities)
    }

    /// Read agent info from the sidecar.
    pub async fn get_session_agent_info(&self, session_id: &str) -> Result<Option<AgentInfo>> {
        Ok(self
            .get_session_state(session_id)
            .await?
            .agent_info
            .filter(Value::is_object)
            .and_then(|value| serde_json::from_value(value).ok()))
    }

    /// Raw passthrough to `send_session_request`.
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
    /// host). With no subscribers the client returns no explicit answer; subscribers reply via
    /// the carried [`PermissionResponder`] or [`AgentOs::respond_permission`], bounded by the
    /// sidecar-supplied timeout. The ACP sidecar owns the missing-answer default.
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
    /// - no subscribers -> no reply, so the ACP sidecar applies its default
    /// - otherwise registers the `pending_permission_replies` slot, delivers the request, and waits
    ///   up to the sidecar-supplied timeout for `respond_permission` / the responder; timeout
    ///   removes the slot and returns `error: "Timed out waiting for permission reply: <id>"`.
    pub(crate) async fn deliver_sidecar_permission_request(
        &self,
        request: PermissionRouteRequest,
    ) -> PermissionRouteResult {
        let PermissionRouteRequest {
            session_id,
            permission_id,
            params,
            timeout_ms,
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

        // Register the reply slot and broadcast under the same session lookup. No subscribers
        // means no explicit host answer; the ACP sidecar owns the default.
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
                return PermissionRouteResult { reply: None };
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

        let timeout = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms));
        tokio::pin!(timeout);
        tokio::select! {
            reply = slot_rx => match reply {
                Ok(reply) => PermissionRouteResult {
                    reply: Some(permission_reply_wire(reply).to_string()),
                },
                // The slot sender dropped without an explicit host answer.
                Err(_) => PermissionRouteResult { reply: None },
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
