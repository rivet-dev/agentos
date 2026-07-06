//! Agent session actions: create an ACP agent session, send prompts,
//! and close it. Ports of [`AgentOs::create_session`] / `prompt` /
//! `close_session`.
//!
//! Session metadata is persisted to the actor's SQLite database
//! (`agent_os_sessions`, with streamed events in `agent_os_session_events`)
//! via `ctx.db_*`, so the set of sessions survives actor sleep/wake. The live
//! ACP session itself lives in the VM and is recreated on demand.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::host_ctx::HostCtx;
use agentos_client::{
    AgentExitEvent, AgentOs, CreateSessionOptions, JsonRpcResponse, PermissionReply,
};
use anyhow::{anyhow, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use super::Vars;
use crate::persistence::{
    insert_session_event, query_rows, reconstruct_transcript_to_file, run_stmt,
};

/// Options object for `createSession(agentType, options?)`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionOptionsDto {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub skip_os_instructions: bool,
    #[serde(default)]
    pub additional_instructions: Option<String>,
}

/// Result of `sendPrompt` exposed to the TS client.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResultDto {
    pub response: JsonRpcResponse,
    pub text: String,
}

/// One row of `listPersistedSessions`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSessionDto {
    pub session_id: String,
    pub agent_type: String,
    pub created_at: f64,
    /// Activity status derived from VM liveness: `"running"` when the ACP
    /// session is currently loaded in the VM, `"idle"` when it is persisted
    /// but hibernated (resumable).
    pub status: &'static str,
}

/// One row of `getSessionEvents` — the full persisted event record (not just
/// the bare JSON-RPC notification) so the client keeps `seq`/`createdAt`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSessionEventDto {
    pub session_id: String,
    pub seq: i64,
    /// The stored ACP JSON-RPC notification, parsed back to JSON.
    pub event: JsonValue,
    pub created_at: f64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Subscribe to the live `session/update` stream for `live_session_id` and
/// spawn a task that persists each event under `external_session_id` (spec §5).
///
/// The subscription is broadcast-backed, so aborting the spawned task — which
/// drops the stream — is the unsubscribe. The handle is tracked in
/// [`Vars::capture_tasks`] keyed by the live id so it can be cancelled on close
/// / sleep / destroy. Re-subscribing for the same live id first aborts any
/// existing pump so we never run two pumps for one session.
fn spawn_event_capture(
    ctx: &HostCtx,
    vm: &AgentOs,
    vars: &mut Vars,
    external_session_id: &str,
    live_session_id: &str,
) {
    let (mut stream, subscription) = match vm.on_session_event(live_session_id) {
        Ok(sub) => sub,
        Err(error) => {
            tracing::warn!(?error, live_session_id, "on_session_event subscribe failed");
            return;
        }
    };
    // Replace any existing pump for this live id.
    if let Some(old) = vars.capture_tasks.remove(live_session_id) {
        old.abort();
    }
    let ctx = ctx.clone();
    let external = external_session_id.to_owned();
    let handle = tokio::spawn(async move {
        // Keep the RAII guard alive for the lifetime of the pump; dropping the
        // stream (on abort / channel close) is the unsubscribe.
        let _subscription = subscription;
        while let Some(notification) = stream.next().await {
            let event_value = match serde_json::to_value(&notification) {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!(?error, "failed to encode captured session event");
                    continue;
                }
            };
            // Live-stream to connected clients (`conn.on("sessionEvent")`) before
            // persisting. The RivetKit event wire is CBOR, and the broadcast body
            // is the array of handler ARGUMENTS the client spreads into the
            // listener (`handler(...body)`). The quickstart listener is
            // `(data) => data.event`, so the single argument is `{"event": <…>}`
            // and the body is `[{"event": <notification>}]`. (A bare object trips
            // the client's "Spread syntax requires …iterable"; JSON bytes trip
            // "length over 4294967295".) Without this broadcast the events were
            // only written to SQLite and never reached live subscribers, so
            // `sessionEvent` streaming silently delivered nothing.
            match encode_session_event(&external, &event_value) {
                Ok(cbor) => {
                    let _ = ctx.broadcast(b"sessionEvent".to_vec(), cbor);
                }
                Err(error) => {
                    tracing::warn!(?error, "failed to encode session event broadcast");
                }
            }
            let event_json = event_value.to_string();
            if let Err(error) = insert_session_event(&ctx, &external, &event_json).await {
                tracing::warn!(?error, external, "failed to persist captured session event");
            }
        }
    });
    vars.capture_tasks
        .insert(live_session_id.to_owned(), handle);
}

/// Build the `permissionRequest` broadcast body for one request.
///
/// The RivetKit event wire is CBOR and the body is the array of handler
/// ARGUMENTS the client spreads into the listener (`handler(...body)`). The
/// documented listener is `(data) => …`, so the single argument is the TS
/// `PermissionRequestPayload` — `{ sessionId, request: { permissionId,
/// description?, params } }` — and the body is `[ <that object> ]`. The
/// `sessionId` is the client-facing external id (== live for native sessions).
pub(crate) fn session_event_payload(external_session_id: &str, event: &JsonValue) -> JsonValue {
    json!({
        "sessionId": external_session_id,
        "event": event,
    })
}

pub(crate) fn encode_session_event(
    external_session_id: &str,
    event: &JsonValue,
) -> Result<Vec<u8>> {
    super::encode_event_arg(&session_event_payload(external_session_id, event))
}

fn permission_event_payload(
    external_session_id: &str,
    permission_id: &str,
    description: Option<&str>,
    params: &JsonValue,
) -> JsonValue {
    json!({
        "sessionId": external_session_id,
        "request": {
            "permissionId": permission_id,
            "description": description,
            "params": params,
        },
    })
}

pub(crate) fn encode_permission_request_event(
    external_session_id: &str,
    permission_id: &str,
    description: Option<&str>,
    params: &JsonValue,
) -> Result<Vec<u8>> {
    super::encode_event_arg(&permission_event_payload(
        external_session_id,
        permission_id,
        description,
        params,
    ))
}

/// Map the wire reply string to a [`PermissionReply`] (`"once"` / `"always"` /
/// `"reject"`), matching the TS `PermissionReply` union.
fn parse_permission_reply(reply: &str) -> Result<PermissionReply> {
    match reply {
        "once" => Ok(PermissionReply::Once),
        "always" => Ok(PermissionReply::Always),
        "reject" => Ok(PermissionReply::Reject),
        other => Err(anyhow!(
            "invalid permission reply {other:?} (expected \"once\" | \"always\" | \"reject\")"
        )),
    }
}

/// Subscribe to the session's permission-request stream and spawn a task that
/// broadcasts each request to connected clients as a `permissionRequest` event
/// (`conn.on("permissionRequest", …)`).
///
/// Mirrors [`spawn_event_capture`]. A subscriber MUST exist before the guest
/// agent raises a permission request, otherwise the client auto-rejects it
/// (`deliver_sidecar_permission_request` checks `receiver_count() == 0`) — so
/// this is started at session-create time. Clients answer via the
/// `respondPermission` action (→ [`respond_permission`]), which resolves the
/// pending reply slot; this pump only fans the request out, so dropping the
/// broadcast item's responder clone here is harmless.
fn spawn_permission_pump(
    ctx: &HostCtx,
    vm: &AgentOs,
    vars: &mut Vars,
    external_session_id: &str,
    live_session_id: &str,
) {
    let (mut stream, subscription) = match vm.on_permission_request(live_session_id) {
        Ok(sub) => sub,
        Err(error) => {
            tracing::warn!(
                ?error,
                live_session_id,
                "on_permission_request subscribe failed"
            );
            return;
        }
    };
    if let Some(old) = vars.permission_tasks.remove(live_session_id) {
        old.abort();
    }
    let ctx = ctx.clone();
    let external = external_session_id.to_owned();
    let handle = tokio::spawn(async move {
        // Keep the RAII guard alive for the pump's lifetime; dropping the stream
        // (on abort / channel close) is the unsubscribe.
        let _subscription = subscription;
        while let Some(request) = stream.next().await {
            let body = encode_permission_request_event(
                &external,
                &request.permission_id,
                request.description.as_deref(),
                &request.params,
            );
            match body {
                Ok(cbor) => {
                    let _ = ctx.broadcast(b"permissionRequest".to_vec(), cbor);
                }
                Err(error) => {
                    tracing::warn!(?error, "failed to encode permission request broadcast");
                }
            }
        }
    });
    vars.permission_tasks
        .insert(live_session_id.to_owned(), handle);
}

/// Answer a permission request raised by the session's guest agent
/// (`respondPermission`). Resolves the pending reply slot through the client's
/// `respond_permission`, keyed by the live session id.
pub async fn respond_permission(
    vm: &AgentOs,
    vars: &Vars,
    session_id: &str,
    permission_id: &str,
    reply: &str,
) -> Result<()> {
    let reply = parse_permission_reply(reply)?;
    let live_session_id = vars.live_id(session_id).to_owned();
    vm.respond_permission(&live_session_id, permission_id, reply)
        .await?;
    Ok(())
}

/// Exit-capture task key for [`Vars::capture_tasks`]: distinct from the
/// session/update pump key so both tasks are tracked (and cancelled)
/// independently for one live session.
fn exit_capture_key(live_session_id: &str) -> String {
    format!("{live_session_id}#exit")
}

/// Subscribe to unexpected adapter process exits (crashes) for
/// `live_session_id` and spawn a task that live-broadcasts each event to
/// connected clients (`conn.on("agentCrashed")`), including the sidecar's
/// auto-restart outcome — the actor-side counterpart of the core
/// `onAgentExit` hook. Broadcast-only: the durable transcript stays limited to
/// real session events. Tracked in [`Vars::capture_tasks`] under
/// [`exit_capture_key`] so it shares the close/sleep/destroy cancellation path
/// with the session/update pump.
fn spawn_exit_capture(
    ctx: &HostCtx,
    vm: &AgentOs,
    vars: &mut Vars,
    external_session_id: &str,
    live_session_id: &str,
) {
    let (mut stream, subscription) = match vm.on_agent_exit(live_session_id) {
        Ok(sub) => sub,
        Err(error) => {
            tracing::warn!(?error, live_session_id, "on_agent_exit subscribe failed");
            return;
        }
    };
    let key = exit_capture_key(live_session_id);
    if let Some(old) = vars.capture_tasks.remove(&key) {
        old.abort();
    }
    let ctx = ctx.clone();
    let external = external_session_id.to_owned();
    let handle = tokio::spawn(async move {
        // Keep the RAII guard alive for the lifetime of the pump; dropping the
        // stream (on abort / channel close) is the unsubscribe.
        let _subscription = subscription;
        while let Some(event) = stream.next().await {
            tracing::warn!(
                external,
                agent_type = event.agent_type,
                exit_code = ?event.exit_code,
                restart = event.restart,
                restart_count = event.restart_count,
                max_restarts = event.max_restarts,
                "agent adapter exited unexpectedly",
            );
            let event_value = match serde_json::to_value(&event) {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!(?error, "failed to encode agent exit event");
                    continue;
                }
            };
            match encode_agent_crashed_event_value(&external, event_value) {
                Ok(cbor) => {
                    let _ = ctx.broadcast(b"agentCrashed".to_vec(), cbor);
                }
                Err(error) => {
                    tracing::warn!(?error, "failed to encode agent crash broadcast");
                }
            }
        }
    });
    vars.capture_tasks.insert(key, handle);
}

pub async fn create_session(
    ctx: &HostCtx,
    vm: &AgentOs,
    vars: &mut Vars,
    agent_type: &str,
    dto: CreateSessionOptionsDto,
) -> Result<String> {
    let options = CreateSessionOptions {
        cwd: dto.cwd,
        env: dto.env,
        skip_os_instructions: dto.skip_os_instructions,
        additional_instructions: dto.additional_instructions,
        ..CreateSessionOptions::default()
    };
    let session_id = vm.create_session(agent_type, options).await?.session_id;
    // Persist session metadata so the set of sessions survives sleep/wake. Capture the REAL
    // agent capabilities + info (not a `"{}"` placeholder) so the resume path can capability-gate
    // the native `session/load` tier after a wake, when the live session is gone. See
    // `resume_session` for how these are read back.
    let capabilities = vm
        .get_session_capabilities(&session_id)
        .and_then(|caps| serde_json::to_string(&caps).ok())
        .unwrap_or_else(|| "{}".to_owned());
    let agent_info = vm
        .get_session_agent_info(&session_id)
        .and_then(|info| serde_json::to_string(&info).ok());
    run_stmt(
        ctx,
        "INSERT OR REPLACE INTO agent_os_sessions \
		 (session_id, agent_type, capabilities, agent_info, created_at) \
		 VALUES (?, ?, ?, ?, ?)",
        &[
            json!(session_id),
            json!(agent_type),
            json!(capabilities),
            agent_info.map(JsonValue::String).unwrap_or(JsonValue::Null),
            json!(now_ms()),
        ],
    )
    .await?;
    // At create time `external == live`; capture every `session/update` for this
    // session under the external id (spec §3/§5), and start fanning the guest's
    // permission requests out to connected clients. The permission pump must be
    // subscribed before the agent runs, or requests would auto-reject.
    spawn_event_capture(ctx, vm, vars, &session_id, &session_id);
    spawn_permission_pump(ctx, vm, vars, &session_id, &session_id);
    // Live adapter-crash notifications for connected clients (`agentCrashed`).
    spawn_exit_capture(ctx, vm, vars, &session_id, &session_id);
    // The TS mirror (`actor-actions.ts`) types this as `Promise<string>`, so the
    // reply must be the bare session id, not a `{ sessionId }` wrapper.
    Ok(session_id)
}

fn agent_crashed_event_payload_value(external_session_id: &str, mut event: JsonValue) -> JsonValue {
    if let JsonValue::Object(map) = &mut event {
        map.insert("sessionId".to_owned(), json!(external_session_id));
    }
    json!({
        "sessionId": external_session_id,
        "event": event,
    })
}

fn encode_agent_crashed_event_value(
    external_session_id: &str,
    event: JsonValue,
) -> Result<Vec<u8>> {
    super::encode_event_arg(&agent_crashed_event_payload_value(
        external_session_id,
        event,
    ))
}

pub(crate) fn encode_agent_crashed_event(
    external_session_id: &str,
    event: &AgentExitEvent,
) -> Result<Vec<u8>> {
    let event = serde_json::to_value(event)?;
    encode_agent_crashed_event_value(external_session_id, event)
}

pub async fn send_prompt(
    ctx: &HostCtx,
    vm: &AgentOs,
    vars: &mut Vars,
    session_id: &str,
    text: &str,
) -> Result<PromptResultDto> {
    // Lazy-resume trigger (spec §8): a prompt for a session that is persisted in
    // `agent_os_sessions` but absent from `Vars.live_sessions` means the VM was
    // recreated since the session was last live — resume it before forwarding.
    // `session_id` here is the client-facing `external_session_id`.
    //
    // Canonical resume state-machine documentation lives on the sidecar handler
    // in `crates/agentos-sidecar/src/acp_extension.rs` (spec §6); this is just
    // the actor-side trigger that drives it.
    if !vars.live_sessions.contains_key(session_id)
        && !is_session_live(vm, session_id)
        && session_is_persisted(ctx, session_id).await?
    {
        resume_session(ctx, vm, vars, session_id).await?;
    }

    // Record the outbound prompt text as a synthetic `user_prompt` event BEFORE
    // the prompt streams, so the transcript turn ordering is correct (the prompt
    // row precedes the agent `session/update` rows for this turn). Stored under
    // the stable external id (spec §4/§5).
    let prompt_event = json!({
        "method": "user_prompt",
        "params": { "text": text },
    });
    if let Err(error) = insert_session_event(ctx, session_id, &prompt_event.to_string()).await {
        tracing::warn!(?error, session_id, "failed to persist user_prompt event");
    }

    // Forward to the live id (== external for native/not-yet-resumed sessions).
    let live_session_id = vars.live_id(session_id).to_owned();
    let result = vm.prompt(&live_session_id, text).await?;
    Ok(PromptResultDto {
        response: result.response,
        text: result.text,
    })
}

pub async fn close_session(
    ctx: &HostCtx,
    vm: &AgentOs,
    vars: &mut Vars,
    session_id: &str,
) -> Result<()> {
    // Stop event capture + the permission pump + drop the remap for this session.
    let live_session_id = vars.live_id(session_id).to_owned();
    if let Some(task) = vars.capture_tasks.remove(&live_session_id) {
        task.abort();
    }
    if let Some(task) = vars.permission_tasks.remove(&live_session_id) {
        task.abort();
    }
    if let Some(task) = vars
        .capture_tasks
        .remove(&exit_capture_key(&live_session_id))
    {
        task.abort();
    }
    vars.live_sessions.remove(session_id);
    vm.close_session(&live_session_id).map_err(|e| anyhow!(e))?;
    // Drop persisted metadata + events (explicit, since SQLite FK cascade is
    // only enforced when `PRAGMA foreign_keys = ON`).
    run_stmt(
        ctx,
        "DELETE FROM agent_os_session_events WHERE session_id = ?",
        &[json!(session_id)],
    )
    .await?;
    run_stmt(
        ctx,
        "DELETE FROM agent_os_sessions WHERE session_id = ?",
        &[json!(session_id)],
    )
    .await?;
    Ok(())
}

/// List the sessions persisted for this actor (`listPersistedSessions`), each
/// tagged with its live/hibernated activity status.
pub async fn list_persisted_sessions(
    ctx: &HostCtx,
    vm: &AgentOs,
) -> Result<Vec<PersistedSessionDto>> {
    let rows = query_rows(
        ctx,
        "SELECT session_id, agent_type, created_at FROM agent_os_sessions \
		 ORDER BY created_at",
        &[],
    )
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let session_id = row
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned();
            let status = if is_session_live(vm, &session_id) {
                "running"
            } else {
                "idle"
            };
            PersistedSessionDto {
                agent_type: row
                    .get("agent_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                created_at: row.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0) as f64,
                status,
                session_id,
            }
        })
        .collect())
}

/// Return the persisted ACP events for a session, ordered by sequence
/// (`getSessionEvents`). Each row is the full `PersistedSessionEvent`
/// (`seq`/`createdAt` + the parsed JSON-RPC notification), not just the bare
/// notification, so the client can key/order/timestamp them.
pub async fn get_session_events(
    ctx: &HostCtx,
    session_id: &str,
) -> Result<Vec<PersistedSessionEventDto>> {
    let rows = query_rows(
        ctx,
        "SELECT session_id, seq, event, created_at FROM agent_os_session_events \
		 WHERE session_id = ? ORDER BY seq",
        &[json!(session_id)],
    )
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let event = row
                .get("event")
                .and_then(|v| v.as_str())
                .and_then(|raw| serde_json::from_str::<JsonValue>(raw).ok())?;
            Some(PersistedSessionEventDto {
                session_id: row
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                seq: row.get("seq").and_then(|v| v.as_i64()).unwrap_or(0),
                event,
                created_at: row.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0) as f64,
            })
        })
        .collect())
}

/// True when an ACP session with this id is currently live in the VM.
fn is_session_live(vm: &AgentOs, session_id: &str) -> bool {
    vm.list_sessions()
        .iter()
        .any(|info| info.session_id == session_id)
}

/// True when `external_session_id` has a persisted registry row in
/// `agent_os_sessions` (so it is resumable).
async fn session_is_persisted(ctx: &HostCtx, external_session_id: &str) -> Result<bool> {
    let rows = query_rows(
        ctx,
        "SELECT session_id FROM agent_os_sessions WHERE session_id = ? LIMIT 1",
        &[json!(external_session_id)],
    )
    .await?;
    Ok(!rows.is_empty())
}

/// Read the persisted `(agent_type, capabilities)` for a session from the
/// registry, returning the parsed capabilities JSON (`{}` if absent/unparsable).
async fn read_session_registry(
    ctx: &HostCtx,
    external_session_id: &str,
) -> Result<(String, JsonValue)> {
    let rows = query_rows(
        ctx,
        "SELECT agent_type, capabilities FROM agent_os_sessions WHERE session_id = ? LIMIT 1",
        &[json!(external_session_id)],
    )
    .await?;
    let row = rows
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no persisted session {external_session_id} to resume"))?;
    let agent_type = row
        .get("agent_type")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned();
    let capabilities = row
        .get("capabilities")
        .and_then(|v| v.as_str())
        .and_then(|raw| serde_json::from_str::<JsonValue>(raw).ok())
        .unwrap_or_else(|| json!({}));
    Ok((agent_type, capabilities))
}

/// Resume a persisted-but-not-live session in the freshly recreated VM
/// (spec §6/§8). Reads the registry caps, reconstructs the transcript file from
/// `agent_os_session_events`, calls the sidecar resume orchestration via the
/// client, records the `external -> live` remap, and starts event capture for
/// the live session.
///
/// The canonical resume state machine (native `session/load`/`resume` tier with
/// the `unknown_session` fallthrough, then the universal `session/new` +
/// transcript-preamble fallback) lives on the sidecar handler in
/// `crates/agentos-sidecar/src/acp_extension.rs` (spec §6). This actor function
/// only supplies the durable inputs (caps + transcript path) and records the
/// remap the sidecar returns.
pub async fn resume_session(
    ctx: &HostCtx,
    vm: &AgentOs,
    vars: &mut Vars,
    external_session_id: &str,
) -> Result<()> {
    let (agent_type, _capabilities) = read_session_registry(ctx, external_session_id).await?;

    // Disposable on-demand render of the canonical event log; handed to the
    // sidecar so a fallback agent can read prior context with its file tools.
    let transcript_path = reconstruct_transcript_to_file(ctx, external_session_id).await?;

    // Call the sidecar resume orchestration through the client. The contract is
    // `AcpResumeSessionRequest { sessionId, agentType, transcriptPath?, cwd, env }`
    // (spec §6); it returns the live session id (== external for the native tier,
    // a new id for the `session/new` fallback). The actor records the remap.
    //
    // TODO(session-resume): the `agentos_client::AgentOs::resume_session` method
    // is being implemented in parallel against the same spec §6 contract and is
    // not present in the pinned client yet. Once it lands, replace the error
    // below with the real call + remap:
    //
    //     let live_session_id = vm
    //         .resume_session(external_session_id, &agent_type, Some(&transcript_path))
    //         .await?
    //         .session_id;
    //     // The remap lives SOLELY in the actor (spec §3): record external -> live
    //     // and capture the live session's events under the stable external id.
    //     vars.live_sessions
    //         .insert(external_session_id.to_owned(), live_session_id.clone());
    //     spawn_event_capture(ctx, vm, vars, external_session_id, &live_session_id);
    //     return Ok(());
    let _ = (&agent_type, vm, vars);
    Err(anyhow!(
        "resume_session: client `resume_session` not yet available \
		 (transcript reconstructed at {transcript_path}); blocked on the \
		 parallel sidecar/client implementation of the spec §6 \
		 AcpResumeSessionRequest contract"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_client::PermissionReply;

    #[test]
    fn permission_event_body_matches_ts_payload_shape() {
        // The TS client listener is `(data) => …` where data is
        // `PermissionRequestPayload { sessionId, request: { permissionId,
        // description?, params } }`, delivered as the single broadcast arg.
        let params = json!({
            "toolCall": { "title": "Bash", "kind": "execute" },
            "options": [{ "optionId": "allow_once" }],
        });
        let data = permission_event_payload("sess-1", "perm-7", Some("run a command"), &params);

        assert_eq!(data["sessionId"], json!("sess-1"));
        assert_eq!(data["request"]["permissionId"], json!("perm-7"));
        assert_eq!(data["request"]["description"], json!("run a command"));
        // params are forwarded verbatim so the client can inspect the tool/paths.
        assert_eq!(data["request"]["params"], params);
    }

    #[test]
    fn permission_event_body_serializes_absent_description_as_null() {
        let body = permission_event_payload("sess-1", "perm-1", None, &json!({}));
        assert_eq!(body["request"]["description"], JsonValue::Null);
    }

    #[test]
    fn parse_permission_reply_maps_each_wire_value() {
        assert_eq!(
            parse_permission_reply("once").unwrap(),
            PermissionReply::Once
        );
        assert_eq!(
            parse_permission_reply("always").unwrap(),
            PermissionReply::Always
        );
        assert_eq!(
            parse_permission_reply("reject").unwrap(),
            PermissionReply::Reject
        );
    }

    #[test]
    fn parse_permission_reply_rejects_unknown_value() {
        let err = parse_permission_reply("maybe").unwrap_err().to_string();
        assert!(err.contains("invalid permission reply"), "got: {err}");
        assert!(err.contains("maybe"), "names the bad value: {err}");
    }
}
