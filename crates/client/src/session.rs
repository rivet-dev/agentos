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
use crate::stream::{RoutedStreamEvent, StreamRouteFailure};

pub(crate) struct PermissionRouteRequest {
    pub(crate) session_id: String,
    pub(crate) permission_id: String,
    pub(crate) params: Value,
    pub(crate) cleanup_after_ms: u64,
}

#[cfg(test)]
mod stream_tests {
    use super::{
        broadcast_result_stream, failed_result_stream, finalize_acp_session_close,
        send_bridged_permission_reply, wait_for_permission_reply, AgentExitEvent,
        PendingPermissionOutcome, PermissionReply, PermissionRequest,
    };
    use crate::agent_os::SessionEntry;
    use crate::error::ClientError;
    use crate::json_rpc::JsonRpcNotification;
    use crate::stream::{RoutedStreamEvent, StreamRouteFailure};
    use agentos_protocol::generated::v1::{
        AcpListAgentsResponse, AcpResponse, AcpSessionClosedResponse,
    };
    use futures::StreamExt;

    #[tokio::test]
    async fn session_fanout_streams_surface_lag_instead_of_skipping() {
        let (tx, rx) = tokio::sync::broadcast::channel(1);
        let mut stream = broadcast_result_stream(rx);
        tx.send(RoutedStreamEvent::Data(1u8)).expect("first event");
        tx.send(RoutedStreamEvent::Data(2u8)).expect("second event");
        tx.send(RoutedStreamEvent::Data(3u8)).expect("third event");

        assert!(matches!(
            stream.next().await,
            Some(Err(ClientError::EventStreamLagged { skipped: 2 }))
        ));
        assert!(stream.next().await.is_none(), "lag terminates the stream");
    }

    #[tokio::test]
    async fn session_fanout_streams_surface_upstream_route_failure() {
        let (tx, rx) = tokio::sync::broadcast::channel(2);
        let mut stream = broadcast_result_stream::<u8>(rx);
        tx.send(RoutedStreamEvent::Lagged { skipped: 5 })
            .expect("route failure");

        assert!(matches!(
            stream.next().await,
            Some(Err(ClientError::EventStreamLagged { skipped: 5 }))
        ));
        assert!(
            stream.next().await.is_none(),
            "route failure terminates stream"
        );
    }

    #[tokio::test]
    async fn late_session_subscriber_receives_retained_control_failure() {
        let mut stream = failed_result_stream::<u8>(StreamRouteFailure::Lagged { skipped: 9 });
        assert!(matches!(
            stream.next().await,
            Some(Err(ClientError::EventStreamLagged { skipped: 9 }))
        ));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn permission_responder_bridge_only_uses_a_live_pending_slot() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        assert!(send_bridged_permission_reply(
            Some(tx),
            PermissionReply::Once,
            "permission-live"
        ));
        assert_eq!(
            rx.await.expect("pending bridge reply"),
            PermissionReply::Once
        );

        // Control-route failure and timeout both clear the slot before a late responder resolves.
        // The bridge must stop here; it must never fall back to a new legacy protocol request.
        assert!(!send_bridged_permission_reply(
            None,
            PermissionReply::Always,
            "permission-cleared"
        ));
    }

    #[tokio::test]
    async fn permission_reply_wait_uses_only_the_later_cleanup_deadline() {
        let (_sender, receiver) = tokio::sync::oneshot::channel();
        let (retained_responder, responder_receiver) = super::PermissionResponder::new();
        let mut pending = Box::pin(wait_for_permission_reply(receiver, responder_receiver, 25));

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(5), &mut pending)
                .await
                .is_err(),
            "the client must retain the route before the supplied cleanup deadline"
        );
        assert!(matches!(
            tokio::time::timeout(std::time::Duration::from_secs(1), pending)
                .await
                .expect("cleanup deadline must remain bounded"),
            PendingPermissionOutcome::CleanupElapsed
        ));
        assert!(
            retained_responder
                .inner
                .lock()
                .as_ref()
                .is_some_and(tokio::sync::oneshot::Sender::is_closed),
            "cleanup must drop the responder receiver instead of leaving a bridge task alive"
        );
    }

    #[tokio::test]
    async fn permission_responder_reply_is_joined_without_a_detached_task() {
        let (pending_sender, pending_receiver) = tokio::sync::oneshot::channel();
        let (responder, responder_receiver) = super::PermissionResponder::new();
        responder.respond(PermissionReply::Once);

        let PendingPermissionOutcome::ResponderReply {
            reply,
            pending_reply,
        } = wait_for_permission_reply(pending_receiver, responder_receiver, 1_000).await
        else {
            panic!("responder must win the permission reply wait");
        };
        assert_eq!(reply, PermissionReply::Once);

        pending_sender
            .send(reply)
            .expect("pending route receiver remains available to join the reply");
        assert_eq!(
            pending_reply.await.expect("joined pending reply"),
            PermissionReply::Once
        );
    }

    #[tokio::test]
    async fn acp_close_routes_survive_every_failed_confirmation_until_retry_succeeds() {
        const SESSION_ID: &str = "session-retry";

        let sessions = scc::HashMap::new();
        let (event_tx, mut event_rx) =
            tokio::sync::broadcast::channel::<RoutedStreamEvent<JsonRpcNotification>>(1);
        let (permission_tx, mut permission_rx) =
            tokio::sync::broadcast::channel::<RoutedStreamEvent<PermissionRequest>>(1);
        let (agent_exit_tx, mut agent_exit_rx) =
            tokio::sync::broadcast::channel::<RoutedStreamEvent<AgentExitEvent>>(1);
        let pending_permission_replies = scc::HashMap::new();
        let (pending_tx, mut pending_rx) = tokio::sync::oneshot::channel();
        pending_permission_replies
            .insert(String::from("permission-1"), pending_tx)
            .expect("insert pending permission route");
        assert!(
            sessions
                .insert(
                    String::from(SESSION_ID),
                    SessionEntry {
                        event_tx,
                        permission_tx,
                        agent_exit_tx,
                        pending_permission_replies,
                    },
                )
                .is_ok(),
            "insert session route"
        );

        let failures = [
            Err(ClientError::Sidecar(String::from("transport unavailable"))),
            Err(ClientError::Kernel {
                code: String::from("acp_close_rejected"),
                message: String::from("close rejected"),
            }),
            Ok(AcpResponse::AcpListAgentsResponse(AcpListAgentsResponse {
                agents: Vec::new(),
            })),
            Ok(AcpResponse::AcpSessionClosedResponse(
                AcpSessionClosedResponse {
                    session_id: String::from("wrong-session"),
                },
            )),
        ];

        for failure in failures {
            assert!(finalize_acp_session_close(&sessions, SESSION_ID, failure).is_err());
            assert!(
                sessions.read(SESSION_ID, |_, _| ()).is_some(),
                "a failed close must retain the complete session route for retry"
            );
            assert!(matches!(
                pending_rx.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
            assert!(matches!(
                event_rx.try_recv(),
                Err(tokio::sync::broadcast::error::TryRecvError::Empty)
            ));
            assert!(matches!(
                permission_rx.try_recv(),
                Err(tokio::sync::broadcast::error::TryRecvError::Empty)
            ));
            assert!(matches!(
                agent_exit_rx.try_recv(),
                Err(tokio::sync::broadcast::error::TryRecvError::Empty)
            ));
        }

        finalize_acp_session_close(
            &sessions,
            SESSION_ID,
            Ok(AcpResponse::AcpSessionClosedResponse(
                AcpSessionClosedResponse {
                    session_id: String::from(SESSION_ID),
                },
            )),
        )
        .expect("matching close confirmation finalizes the session route");

        assert!(sessions.read(SESSION_ID, |_, _| ()).is_none());
        assert!(matches!(
            pending_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        ));
        assert!(matches!(
            event_rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Closed)
        ));
        assert!(matches!(
            permission_rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Closed)
        ));
        assert!(matches!(
            agent_exit_rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Closed)
        ));
    }
}

pub(crate) struct PermissionRouteResult {
    pub(crate) reply: Option<String>,
}

enum PendingPermissionOutcome {
    Reply(Option<PermissionReply>),
    ResponderReply {
        reply: PermissionReply,
        pending_reply: tokio::sync::oneshot::Receiver<PermissionReply>,
    },
    CleanupElapsed,
}

async fn wait_for_permission_reply(
    mut pending_reply: tokio::sync::oneshot::Receiver<PermissionReply>,
    responder_reply: tokio::sync::oneshot::Receiver<PermissionReply>,
    cleanup_after_ms: u64,
) -> PendingPermissionOutcome {
    let responder_reply = async move {
        match responder_reply.await {
            Ok(reply) => reply,
            Err(_) => futures::future::pending::<PermissionReply>().await,
        }
    };
    tokio::pin!(responder_reply);
    let cleanup = tokio::time::sleep(std::time::Duration::from_millis(cleanup_after_ms));
    tokio::pin!(cleanup);
    tokio::select! {
        reply = &mut pending_reply => PendingPermissionOutcome::Reply(reply.ok()),
        reply = &mut responder_reply => PendingPermissionOutcome::ResponderReply {
            reply,
            pending_reply,
        },
        _ = &mut cleanup => PendingPermissionOutcome::CleanupElapsed,
    }
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

pub type SessionEventStream =
    Pin<Box<dyn Stream<Item = std::result::Result<JsonRpcNotification, ClientError>> + Send>>;
pub type SessionEventSubscription = (SessionEventStream, Subscription);
pub type PermissionRequestStream =
    Pin<Box<dyn Stream<Item = std::result::Result<PermissionRequest, ClientError>> + Send>>;
pub type PermissionRequestSubscription = (PermissionRequestStream, Subscription);
pub type AgentExitStream =
    Pin<Box<dyn Stream<Item = std::result::Result<AgentExitEvent, ClientError>> + Send>>;
pub type AgentExitSubscription = (AgentExitStream, Subscription);

fn broadcast_result_stream<T: Clone + Send + 'static>(
    rx: tokio::sync::broadcast::Receiver<RoutedStreamEvent<T>>,
) -> Pin<Box<dyn Stream<Item = std::result::Result<T, ClientError>> + Send>> {
    Box::pin(futures::stream::unfold(Some(rx), move |state| async move {
        let mut rx = state?;
        match rx.recv().await {
            Ok(RoutedStreamEvent::Data(value)) => Some((Ok(value), Some(rx))),
            Ok(RoutedStreamEvent::Lagged { skipped }) => {
                Some((Err(ClientError::EventStreamLagged { skipped }), None))
            }
            Ok(RoutedStreamEvent::Closed { context }) => {
                Some((Err(ClientError::EventStreamClosed { context }), None))
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                Some((Err(ClientError::EventStreamLagged { skipped }), None))
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
        }
    }))
}

fn failed_result_stream<T: Send + 'static>(
    failure: StreamRouteFailure,
) -> Pin<Box<dyn Stream<Item = std::result::Result<T, ClientError>> + Send>> {
    Box::pin(futures::stream::once(async move {
        Err(match failure {
            StreamRouteFailure::Lagged { skipped } => ClientError::EventStreamLagged { skipped },
            StreamRouteFailure::Closed { context } => ClientError::EventStreamClosed { context },
        })
    }))
}

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
/// sidecar owns the permission deadline/default and supplies only a later host-route cleanup
/// deadline to keep client bookkeeping bounded.
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

fn send_bridged_permission_reply(
    sender: Option<tokio::sync::oneshot::Sender<PermissionReply>>,
    reply: PermissionReply,
    permission_id: &str,
) -> bool {
    let Some(sender) = sender else {
        return false;
    };
    if sender.send(reply).is_err() {
        tracing::warn!(
            permission_id,
            "permission callback completed after its sidecar route closed"
        );
    }
    true
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
        let _ = entry.event_tx.send(RoutedStreamEvent::Data(notification));
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

/// Validate the sidecar's ACP close response before releasing any host callback/event routes.
/// Passing the request result into this one finalization point keeps transport failures and typed
/// rejections retryable as well as rejecting unrelated or wrong-session success responses.
fn finalize_acp_session_close(
    sessions: &scc::HashMap<String, SessionEntry>,
    requested_session_id: &str,
    response: std::result::Result<AcpResponse, ClientError>,
) -> std::result::Result<(), ClientError> {
    let response = response?;
    match response {
        AcpResponse::AcpSessionClosedResponse(closed)
            if closed.session_id == requested_session_id =>
        {
            // Removing the complete entry closes its event/permission routes and drops every
            // pending permission responder only after the authoritative close is confirmed.
            let _ = sessions.remove(requested_session_id);
            Ok(())
        }
        AcpResponse::AcpSessionClosedResponse(closed) => Err(ClientError::Sidecar(format!(
            "ACP close returned session {} for requested session {requested_session_id}",
            closed.session_id
        ))),
        other => Err(unexpected_acp_response("AcpCloseSessionRequest", other)),
    }
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

    /// Close a session through the sidecar. The sidecar makes repeated or unknown
    /// closes idempotent, so the client keeps no closed-id or in-flight-close state. Host callback,
    /// event, and permission routes remain live until a matching close response is validated; every
    /// failure retains them so callers can retry without losing correlation state.
    pub async fn close_session(&self, session_id: &str) -> std::result::Result<(), ClientError> {
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
            .await;
        finalize_acp_session_close(&self.inner().sessions, session_id, response)
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
    /// synthetic `{ via: "sidecar-request" }`. Replies to unknown or expired routes fail instead
    /// of becoming a separate legacy RPC. Mirrors `respondPermission`.
    pub async fn respond_permission(
        &self,
        session_id: &str,
        permission_id: &str,
        reply: PermissionReply,
    ) -> Result<JsonRpcResponse> {
        let pending = self.take_pending_permission_sender(session_id, permission_id);

        if let Some(responder) = pending {
            responder.send(reply.clone()).map_err(|_| {
                ClientError::Sidecar(format!(
                    "permission reply route closed before response: {permission_id}"
                ))
            })?;
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

        Err(ClientError::Sidecar(format!(
            "permission request is not pending: {permission_id}"
        ))
        .into())
    }

    fn take_pending_permission_sender(
        &self,
        session_id: &str,
        permission_id: &str,
    ) -> Option<tokio::sync::oneshot::Sender<PermissionReply>> {
        self.inner()
            .sessions
            .read(session_id, |_, entry| {
                entry
                    .pending_permission_replies
                    .remove(permission_id)
                    .map(|(_, responder)| responder)
            })
            .flatten()
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
        let stream = match *self.inner().control_route_failure.lock() {
            Some(failure) => failed_result_stream(failure),
            None => broadcast_result_stream(rx),
        };
        Ok((stream, Subscription::noop()))
    }

    /// Subscribe to permission requests raised by the session's guest agent. Requests originate
    /// from the typed sidecar permission callback. With no subscribers the client returns no
    /// explicit answer; subscribers reply via the carried [`PermissionResponder`] or
    /// [`AgentOs::respond_permission`]. The ACP sidecar owns the decision deadline and
    /// missing-answer default; its later cleanup deadline only bounds this host route.
    pub fn on_permission_request(
        &self,
        session_id: &str,
    ) -> std::result::Result<PermissionRequestSubscription, ClientError> {
        let rx = self.require_session(session_id, |entry| entry.permission_tx.subscribe())?;

        // Pass broadcast items straight through. Each item carries a cloneable
        // [`PermissionResponder`] that resolves the pending reply slot registered by
        // `deliver_sidecar_permission_request`.
        let stream = match *self.inner().control_route_failure.lock() {
            Some(failure) => failed_result_stream(failure),
            None => broadcast_result_stream(rx),
        };
        Ok((stream, Subscription::noop()))
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
        let stream = match *self.inner().control_route_failure.lock() {
            Some(failure) => failed_result_stream(failure),
            None => broadcast_result_stream(rx),
        };
        Ok((stream, Subscription::noop()))
    }

    /// Answer an ACP permission callback by fanning a [`PermissionRequest`] out to
    /// `on_permission_request` subscribers and waiting for the reply. Mirrors TS
    /// `_handlePermissionSidecarRequest`:
    /// - unknown session -> `error: "Session not found: <id>"`
    /// - no subscribers -> no reply, so the ACP sidecar applies its default
    /// - otherwise registers the `pending_permission_replies` slot, delivers the request, and waits
    ///   until `respond_permission`, the responder, or the sidecar-supplied post-decision cleanup
    ///   deadline closes the host route. The sidecar alone owns the earlier timeout and default.
    pub(crate) async fn deliver_sidecar_permission_request(
        &self,
        request: PermissionRouteRequest,
    ) -> PermissionRouteResult {
        let PermissionRouteRequest {
            session_id,
            permission_id,
            params,
            cleanup_after_ms,
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
        let registered = {
            let control_route_failure = self.inner().control_route_failure.lock();
            if control_route_failure.is_some() {
                return PermissionRouteResult { reply: None };
            }
            self.require_session(&session_id, |entry| {
                if entry.permission_tx.receiver_count() == 0 {
                    return false;
                }
                let _ = entry
                    .pending_permission_replies
                    .insert(permission_id.clone(), slot_tx);
                let _ = entry.permission_tx.send(RoutedStreamEvent::Data(delivered));
                true
            })
        };
        match registered {
            Ok(true) => {}
            Ok(false) => {
                return PermissionRouteResult { reply: None };
            }
            Err(_) => {
                return PermissionRouteResult { reply: None };
            }
        }

        match wait_for_permission_reply(slot_rx, responder_rx, cleanup_after_ms).await {
            PendingPermissionOutcome::Reply(reply) => match reply {
                Some(reply) => PermissionRouteResult {
                    reply: Some(permission_reply_wire(reply).to_string()),
                },
                // The slot sender dropped without an explicit host answer.
                None => PermissionRouteResult { reply: None },
            },
            PendingPermissionOutcome::ResponderReply {
                reply,
                pending_reply,
            } => {
                let sender = self.take_pending_permission_sender(&session_id, &permission_id);
                if send_bridged_permission_reply(sender, reply, &permission_id) {
                    PermissionRouteResult {
                        reply: Some(permission_reply_wire(reply).to_string()),
                    }
                } else {
                    PermissionRouteResult {
                        reply: pending_reply
                            .await
                            .ok()
                            .map(permission_reply_wire)
                            .map(str::to_string),
                    }
                }
            }
            PendingPermissionOutcome::CleanupElapsed => {
                let _ = self.require_session(&session_id, |entry| {
                    let _ = entry.pending_permission_replies.remove(&permission_id);
                });
                PermissionRouteResult { reply: None }
            }
        }
    }
}
