use std::collections::{BTreeMap, HashMap};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use agentos_protocol::generated::v1::{
    AcpAgentEntry, AcpAgentExitedEvent, AcpAgentStderrEvent, AcpCallback, AcpCallbackResponse,
    AcpCloseSessionRequest,
    AcpCreateSessionRequest, AcpErrorResponse, AcpEvent, AcpGetSessionStateRequest,
    AcpHostRequestCallback, AcpListAgentsResponse, AcpPermissionCallback, AcpRequest, AcpResponse,
    AcpResumeSessionRequest, AcpRuntimeKind, AcpSessionClosedResponse, AcpSessionCreatedResponse,
    AcpSessionEvent, AcpSessionRequest, AcpSessionResumedResponse, AcpSessionStateResponse,
};
use agentos_protocol::ACP_EXTENSION_NAMESPACE;
use secure_exec_sidecar::extension::ExtensionSnapshot;
use secure_exec_sidecar::limits::DEFAULT_ACP_MAX_READ_LINE_BYTES;
use secure_exec_sidecar::wire::{
    CloseStdinRequest, EventPayload, ExecuteRequest, GuestFilesystemCallRequest,
    GuestFilesystemOperation, GuestRuntimeKind, KillProcessRequest, OwnershipScope, StreamChannel,
    WriteStdinRequest,
};
use secure_exec_sidecar::{
    Extension, ExtensionContext, ExtensionFuture, ExtensionInterruptRequest,
    ExtensionInterruptResponse, ExtensionResponse, SidecarError,
};
use serde_json::{json, Map, Value};
use tokio::sync::Mutex;

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const SESSION_NEW_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
// While an ACP request is in flight the stdio loop is inside the extension
// dispatch, so this wait loop becomes the cooperative VM I/O pump. Keep it at
// the same cadence as secure-exec's outer event pump so adapter fetches and
// process output keep moving mid-turn.
const ACP_JSON_RPC_POLL_INTERVAL: Duration = Duration::from_micros(250);
const ACP_CANCEL_METHOD: &str = "session/cancel";
/// Transcript-continuation preamble prepended (once) to the first prompt after a
/// fallback resume. Lossy-but-universal floor: the agent is handed a *pointer* to
/// the rendered transcript and reads it on demand with its own file tools. `{path}`
/// is substituted with the guest-readable transcript path. Tunable; see spec §6.
const CONTINUATION_PREAMBLE: &str = "You are continuing an earlier session. The full prior transcript is at `{path}`. Read it with your file tools if you need context before answering.";
const ACP_TRACE_PATH_ENV: &str = "AGENT_OS_ACP_TRACE_PATH";
/// ACP protocol version used for the resume handshake. Lockstep single version.
const ACP_RESUME_PROTOCOL_VERSION: i32 = 1;
/// Client capabilities advertised during the resume `initialize`. Mirrors the
/// client's `defaultAcpClientCapabilities()` so resumed sessions behave like
/// freshly created ones.
const DEFAULT_RESUME_CLIENT_CAPABILITIES: &str =
    "{\"fs\":{\"readTextFile\":true,\"writeTextFile\":true},\"terminal\":true}";
const OPENCODE_SYSTEM_PROMPT_PATH: &str = "/tmp/agentos-system-prompt.md";
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
// Embedded next to this source so `cargo publish` packages it (an out-of-crate
// `include_str!` path breaks the isolated package-verify build). The TypeScript
// side reads the same file from this location for its sanity check.
const AGENTOS_SYSTEM_PROMPT: &str = include_str!("AGENTOS_SYSTEM_PROMPT.md");
/// Hard ceiling on the `stdout_buffer` retained on an `AcpSessionRecord` between
/// requests. The buffer only ever holds the partial trailing line not yet parsed
/// into a complete JSON-RPC message, so this also bounds the per-session record
/// against a runaway or hostile adapter that streams bytes without ever emitting
/// a newline. Mirrors the per-line cap enforced while reading
/// (`DEFAULT_ACP_MAX_READ_LINE_BYTES`); a record's stdout must never outgrow it.
const MAX_SESSION_STDOUT_BUFFER_BYTES: usize = DEFAULT_ACP_MAX_READ_LINE_BYTES;
/// Substring identifying the `send_json_rpc_request` error raised when the
/// adapter process exits before answering. `session_request` matches on it to
/// evict the now-dead session record instead of leaking it until an explicit
/// `close_session` that the client may never send.
const ADAPTER_EXITED_ERROR_MARKER: &str = "exited with code";
/// Substring of the secure-exec process-table error returned when an operation
/// targets a process that already exited ("VM <vm> has no active process <id>").
/// Writing a request to an adapter that crashed while *idle* surfaces this way
/// (the exit is observed lazily, on the next stdin write), so it is classified
/// as an adapter-gone failure alongside `ADAPTER_EXITED_ERROR_MARKER`.
const ADAPTER_NO_ACTIVE_PROCESS_MARKER: &str = "has no active process";
/// Bounded auto-restart budget per ACP session. Each unexpected adapter exit
/// consumes one attempt (respawn + native `session/load`/`session/resume` under
/// the same session id); once spent, further exits evict the session record,
/// matching the pre-restart behavior. Every attempt and the exhaustion of the
/// budget are logged at `warn` and surfaced to the host via
/// `AcpAgentExitedEvent`, so the cap is observable rather than a silent retry
/// loop.
const MAX_ADAPTER_RESTARTS: u32 = 3;
/// `AcpAgentExitedEvent.restart` outcome strings. Keep in sync with the schema
/// doc on `agent_os_acp_v1.bare` and the TypeScript `AgentRestartOutcome` type.
const ADAPTER_RESTART_OUTCOME_RESTARTED: &str = "restarted";
const ADAPTER_RESTART_OUTCOME_UNSUPPORTED: &str = "unsupported";
const ADAPTER_RESTART_OUTCOME_FAILED: &str = "failed";
const ADAPTER_RESTART_OUTCOME_EXHAUSTED: &str = "exhausted";

#[derive(Debug, Default)]
pub struct AcpExtension {
    next_process_id: AtomicUsize,
    sessions: Mutex<BTreeMap<String, AcpSessionRecord>>,
}

#[derive(Debug, Clone)]
struct AcpSessionRecord {
    session_id: String,
    /// Connection that created this session. Used to enforce per-connection
    /// ownership so one connection cannot read or drive another connection's
    /// ACP session by its `session_id`.
    owner_connection_id: String,
    agent_type: String,
    process_id: String,
    pid: Option<u32>,
    modes: Option<String>,
    config_options: Vec<String>,
    agent_capabilities: Option<String>,
    agent_info: Option<String>,
    stdout_buffer: String,
    next_request_id: i64,
    closed: bool,
    exit_code: Option<i32>,
    /// Set by the resume fallback tier (`session/new` instead of native
    /// `session/load`). The transcript-continuation preamble is prepended, once,
    /// as a leading text content block on this session's next `session/prompt`,
    /// then cleared. See `CONTINUATION_PREAMBLE` and the resume state machine on
    /// `AcpExtension::resume_session`.
    pending_preamble: Option<String>,
    /// Launch + handshake parameters retained for adapter auto-restart. See
    /// `AdapterRestartState`.
    restart: AdapterRestartState,
}

/// Adapter launch + handshake parameters retained on the session record so the
/// sidecar can auto-restart a crashed adapter (see
/// `AcpExtension::handle_adapter_exit`). `args`/`env` are the FINAL values that
/// went into the original `ExecuteRequest` — post prompt-injection, including
/// `SECURE_EXEC_KEEP_STDIN_OPEN` — so a restart relaunches exactly what
/// create/resume launched. `count` is the restarts already consumed for this
/// session, bounded by `MAX_ADAPTER_RESTARTS`.
#[derive(Debug, Clone)]
struct AdapterRestartState {
    runtime: AcpRuntimeKind,
    entrypoint: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: String,
    protocol_version: i32,
    client_capabilities: String,
    count: u32,
}

/// Why an adapter auto-restart attempt did not leave the session live. Maps to
/// the `restart` outcome string on `AcpAgentExitedEvent`.
#[derive(Debug)]
enum AdapterRestartError {
    /// The respawned adapter does not advertise a native resume capability
    /// (`loadSession`/`resume`), so the session cannot be re-attached under its
    /// existing id.
    Unsupported,
    /// The respawn, handshake, or native re-attach failed.
    Failed(SidecarError),
}

impl AdapterRestartError {
    fn detail(&self) -> String {
        match self {
            Self::Unsupported => {
                String::from("adapter does not advertise loadSession/resume")
            }
            Self::Failed(error) => error.to_string(),
        }
    }
}

impl AcpExtension {
    pub fn new() -> Self {
        Self::default()
    }

    async fn handle_payload(
        &self,
        ctx: ExtensionContext<'_>,
        payload: &[u8],
    ) -> Result<ExtensionResponse, SidecarError> {
        use tracing::Instrument as _;
        let request = decode_request(payload)?;
        let kind = Self::acp_request_kind(&request);
        let start = std::time::Instant::now();
        tracing::info!(target: "agentos_sidecar::acp_extension", kind, "ext request received");

        let work = async move {
            match request {
                AcpRequest::AcpCreateSessionRequest(request) => {
                    self.create_session(ctx, request).await
                }
                AcpRequest::AcpGetSessionStateRequest(request) => {
                    AcpHandlerOutput::response(self.get_session_state(ctx, request).await)
                }
                AcpRequest::AcpCloseSessionRequest(request) => {
                    AcpHandlerOutput::response(self.close_session(ctx, request).await)
                }
                AcpRequest::AcpSessionRequest(request) => self.session_request(ctx, request).await,
                AcpRequest::AcpResumeSessionRequest(request) => {
                    self.resume_session(ctx, request).await
                }
                AcpRequest::AcpListAgentsRequest(_) => self.list_agents(ctx).await,
                AcpRequest::AcpDeliverAgentOutputRequest(_) => AcpHandlerOutput::response(Err(
                    SidecarError::InvalidState(
                        "AcpDeliverAgentOutputRequest is dispatched by the engine/browser resumable path, not the native ACP extension".to_string(),
                    ),
                )),
            }
        }
        .instrument(tracing::info_span!(
            target: "agentos_sidecar::acp_extension",
            "ext.request",
            kind
        ));

        // Stall watchdog: while the request is in flight, warn periodically so a
        // hang surfaces as a breadcrumb long before the host's 120s frame
        // timeout. This never interrupts the work itself.
        tokio::pin!(work);
        let response = loop {
            tokio::select! {
                result = &mut work => break result,
                _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                    tracing::warn!(
                        target: "agentos_sidecar::acp_extension",
                        kind,
                        elapsed_ms = start.elapsed().as_millis() as u64,
                        "ext request still pending — possible stall before response frame",
                    );
                }
            }
        };

        tracing::info!(
            target: "agentos_sidecar::acp_extension",
            kind,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "ext request handled",
        );
        let payload = encode_response(response.response.unwrap_or_else(error_response))?;
        ExtensionResponse::with_wire_events(payload, response.events)
    }

    /// Stable label for an ACP request kind, used as a tracing field.
    fn acp_request_kind(request: &AcpRequest) -> &'static str {
        match request {
            AcpRequest::AcpCreateSessionRequest(_) => "create_session",
            AcpRequest::AcpGetSessionStateRequest(_) => "get_session_state",
            AcpRequest::AcpCloseSessionRequest(_) => "close_session",
            AcpRequest::AcpSessionRequest(_) => "session_request",
            AcpRequest::AcpResumeSessionRequest(_) => "resume_session",
            AcpRequest::AcpListAgentsRequest(_) => "list_agents",
            AcpRequest::AcpDeliverAgentOutputRequest(_) => "deliver_agent_output",
        }
    }

    async fn create_session(
        &self,
        mut ctx: ExtensionContext<'_>,
        request: AcpCreateSessionRequest,
    ) -> AcpHandlerOutput {
        let __t0 = Instant::now();
        // Resolve the agent name -> package entrypoint/env/launchArgs from the
        // projected `/opt/agentos/<name>/current/agentos-package.json`. The client
        // is npm-agnostic and sends only the agent name; the sidecar owns this.
        let resolved = match resolve_agent(&mut ctx, &request.agent_type).await {
            Ok(resolved) => resolved,
            Err(error) => return AcpHandlerOutput::response(Err(error)),
        };
        let process_id = self.allocate_process_id("acp-agent");
        // Manifest launch args first, then any caller-supplied args.
        let mut args = resolved.launch_args.clone();
        args.extend(request.args.iter().cloned());
        let mut env = hash_to_btree(request.env.clone());
        env.insert(
            String::from("SECURE_EXEC_KEEP_STDIN_OPEN"),
            String::from("1"),
        );
        // Manifest env applies as DEFAULTS; caller/base env wins on conflicts.
        for (key, value) in &resolved.env {
            env.entry(key.clone()).or_insert_with(|| value.clone());
        }
        if let Err(error) = self
            .apply_prompt_injection(&mut ctx, &request, &mut args, &mut env)
            .await
        {
            return AcpHandlerOutput::response(Err(error));
        }
        tracing::info!(target: "agentos_sidecar::perf", phase = "prompt_injection", elapsed_ms = __t0.elapsed().as_millis() as u64, "create_session phase");

        // Retain the final launch + handshake parameters for adapter
        // auto-restart before they are moved into the spawn request.
        let restart_state = AdapterRestartState {
            runtime: request.runtime.clone(),
            entrypoint: resolved.entrypoint.clone(),
            args: args.clone(),
            env: env.clone(),
            cwd: request.cwd.clone(),
            protocol_version: request.protocol_version,
            client_capabilities: request.client_capabilities.clone(),
            count: 0,
        };

        let started = match ctx
            .spawn_process_wire(ExecuteRequest {
                process_id: process_id.clone(),
                command: None,
                runtime: Some(convert_runtime(request.runtime.clone())),
                entrypoint: Some(resolved.entrypoint.clone()),
                args,
                env: env.into_iter().collect(),
                cwd: Some(request.cwd.clone()),
                wasm_permission_tier: None,
            })
            .await
        {
            Ok(started) => started,
            Err(error) => return AcpHandlerOutput::response(Err(error)),
        };
        tracing::info!(target: "agentos_sidecar::perf", phase = "spawn_process", elapsed_ms = __t0.elapsed().as_millis() as u64, "create_session phase");

        let bootstrap = self
            .create_session_inner(&mut ctx, &request, &process_id)
            .await;
        tracing::info!(target: "agentos_sidecar::perf", phase = "session_inner_done", elapsed_ms = __t0.elapsed().as_millis() as u64, "create_session phase");
        if bootstrap.is_err() {
            kill_process_best_effort(&mut ctx, &process_id).await;
        }
        let bootstrap = match bootstrap {
            Ok(bootstrap) => bootstrap,
            Err(error) => return AcpHandlerOutput::response(Err(error)),
        };

        let session = AcpSessionRecord {
            session_id: bootstrap.session_id.clone(),
            owner_connection_id: ownership_connection_id(ctx.ownership()),
            agent_type: request.agent_type.clone(),
            process_id: process_id.clone(),
            pid: started.pid,
            modes: bootstrap.modes,
            config_options: bootstrap.config_options,
            agent_capabilities: bootstrap.agent_capabilities,
            agent_info: bootstrap.agent_info,
            stdout_buffer: bootstrap.stdout_buffer,
            next_request_id: 3,
            closed: false,
            exit_code: None,
            pending_preamble: None,
            restart: restart_state,
        };

        let mut events = Vec::new();
        for notification in bootstrap.notifications {
            let event = match encode_event(AcpEvent::AcpSessionEvent(AcpSessionEvent {
                session_id: session.session_id.clone(),
                notification,
            })) {
                Ok(event) => event,
                Err(error) => {
                    kill_process_best_effort(&mut ctx, &process_id).await;
                    return AcpHandlerOutput::response(Err(error));
                }
            };
            match ctx.ext_event_wire(event) {
                Ok(event) => events.push(event),
                Err(error) => {
                    kill_process_best_effort(&mut ctx, &process_id).await;
                    return AcpHandlerOutput::response(Err(error));
                }
            }
        }

        if let Err(error) = ctx
            .bind_process_to_session(&session.session_id, &process_id)
            .await
        {
            kill_process_best_effort(&mut ctx, &process_id).await;
            return AcpHandlerOutput::response(Err(error));
        }
        self.sessions
            .lock()
            .await
            .insert(session.session_id.clone(), session.clone());

        AcpHandlerOutput {
            response: Ok(AcpResponse::AcpSessionCreatedResponse(
                session.created_response(),
            )),
            events,
        }
    }

    /// Enumerate the agents available in this VM from the ALREADY-PROJECTED
    /// `/opt/agentos` packages. Lists `/opt/agentos`, skips the `bin` symlink farm,
    /// and for each package dir reads `<name>/current/agentos-package.json`; a dir
    /// whose manifest carries a non-empty `agent.acpEntrypoint` is an agent. The
    /// client parses no manifests — the sidecar owns agent enumeration too. Sorted
    /// by id.
    async fn list_agents(&self, mut ctx: ExtensionContext<'_>) -> AcpHandlerOutput {
        let listing = ctx
            .guest_filesystem_call_wire(GuestFilesystemCallRequest {
                operation: GuestFilesystemOperation::ReadDir,
                path: String::from("/opt/agentos"),
                destination_path: None,
                target: None,
                content: None,
                encoding: None,
                recursive: false,
                max_depth: None,
                mode: None,
                uid: None,
                gid: None,
                atime_ms: None,
                mtime_ms: None,
                len: None,
                offset: None,
            })
            .await;
        // No projection (e.g. no packages) => no agents.
        let entries = match listing {
            Ok(result) => result.entries.unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        let mut agents = Vec::new();
        for entry in entries {
            if entry.name == "bin" {
                continue;
            }
            if read_projected_agent_block(&mut ctx, &entry.name)
                .await
                .is_some()
            {
                agents.push(AcpAgentEntry {
                    id: entry.name,
                    installed: true,
                });
            }
        }
        agents.sort_by(|a, b| a.id.cmp(&b.id));
        AcpHandlerOutput::response(Ok(AcpResponse::AcpListAgentsResponse(
            AcpListAgentsResponse { agents },
        )))
    }

    async fn create_session_inner(
        &self,
        ctx: &mut ExtensionContext<'_>,
        request: &AcpCreateSessionRequest,
        process_id: &str,
    ) -> Result<CreateSessionBootstrap, SidecarError> {
        let __ti = Instant::now();
        let mut stdout = String::new();
        let mut notifications = Vec::new();
        let client_capabilities =
            parse_json_text(&request.client_capabilities, "clientCapabilities")?;
        let mcp_servers = parse_json_text(&request.mcp_servers, "mcpServers")?;

        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": request.protocol_version,
                "clientCapabilities": client_capabilities,
            },
        });
        let initialize_response = send_json_rpc_request(
            ctx,
            process_id,
            &request.agent_type,
            initialize,
            1,
            INITIALIZE_TIMEOUT,
            &mut stdout,
            None,
        )
        .await?;
        notifications.extend(initialize_response.notifications);
        let init_result = response_result(initialize_response.response, "ACP initialize")?;
        validate_initialize_result(&init_result, request.protocol_version)?;
        tracing::info!(target: "agentos_sidecar::perf", phase = "acp_initialize", elapsed_ms = __ti.elapsed().as_millis() as u64, "create_session_inner phase");

        let session_new = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/new",
            "params": {
                "cwd": request.cwd,
                "mcpServers": mcp_servers,
            },
        });
        let session_response = send_json_rpc_request(
            ctx,
            process_id,
            &request.agent_type,
            session_new,
            2,
            SESSION_NEW_TIMEOUT,
            &mut stdout,
            None,
        )
        .await?;
        notifications.extend(session_response.notifications);
        let session_result = response_result(session_response.response, "ACP session/new")?;
        let session_id = session_id_from_session_result(&session_result, process_id);
        tracing::info!(target: "agentos_sidecar::perf", phase = "acp_session_new", elapsed_ms = __ti.elapsed().as_millis() as u64, "create_session_inner phase");

        let mut config_options = init_result
            .get("configOptions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if let Some(overrides) = session_result
            .get("configOptions")
            .and_then(Value::as_array)
        {
            config_options = overrides.clone();
        }
        if !config_options.iter().any(is_model_config_option) {
            config_options.extend(derive_config_options(&request.agent_type, &session_result));
        }

        Ok(CreateSessionBootstrap {
            session_id,
            modes: json_field(&session_result, &init_result, "modes")?,
            config_options: json_array_to_strings(config_options)?,
            agent_capabilities: json_optional_string(init_result.get("agentCapabilities"))?,
            agent_info: json_optional_string(init_result.get("agentInfo"))?,
            stdout_buffer: stdout,
            notifications,
        })
    }

    async fn apply_prompt_injection(
        &self,
        ctx: &mut ExtensionContext<'_>,
        request: &AcpCreateSessionRequest,
        args: &mut Vec<String>,
        env: &mut BTreeMap<String, String>,
    ) -> Result<(), SidecarError> {
        let prompt = assemble_system_prompt(
            request.skip_os_instructions,
            request.additional_instructions.as_deref(),
        );
        if prompt.is_empty() {
            return Ok(());
        }

        match request.agent_type.as_str() {
            "pi" | "pi-cli" | "claude" => {
                args.push(String::from("--append-system-prompt"));
                args.push(prompt);
            }
            "codex" => {
                args.push(String::from("--append-developer-instructions"));
                args.push(prompt);
            }
            "opencode" if !env.contains_key("OPENCODE_CONTEXTPATHS") => {
                ctx.guest_filesystem_call_wire(GuestFilesystemCallRequest {
                    operation: GuestFilesystemOperation::WriteFile,
                    path: String::from(OPENCODE_SYSTEM_PROMPT_PATH),
                    destination_path: None,
                    target: None,
                    content: Some(prompt),
                    encoding: None,
                    recursive: false,
                    max_depth: None,
                    mode: None,
                    uid: None,
                    gid: None,
                    atime_ms: None,
                    mtime_ms: None,
                    len: None,
                    offset: None,
                })
                .await?;
                let mut context_paths = OPENCODE_DEFAULT_CONTEXT_PATHS
                    .iter()
                    .map(|path| path.to_string())
                    .collect::<Vec<_>>();
                context_paths.push(OPENCODE_SYSTEM_PROMPT_PATH.to_string());
                env.insert(
                    String::from("OPENCODE_CONTEXTPATHS"),
                    serde_json::to_string(&context_paths).expect("serialize context paths"),
                );
            }
            _ => {}
        }

        Ok(())
    }

    async fn get_session_state(
        &self,
        ctx: ExtensionContext<'_>,
        request: AcpGetSessionStateRequest,
    ) -> Result<AcpResponse, SidecarError> {
        let caller_connection_id = ownership_connection_id(ctx.ownership());
        let sessions = self.sessions.lock().await;
        let unknown =
            || SidecarError::InvalidState(format!("unknown ACP session {}", request.session_id));
        let session = sessions.get(&request.session_id).ok_or_else(unknown)?;
        // Enforce per-connection ownership: a session may only be read by the
        // connection that created it. Fail closed with the same error a missing
        // session produces so existence of another connection's session is not
        // leaked across the connection tenant boundary.
        if session.owner_connection_id != caller_connection_id {
            return Err(unknown());
        }
        Ok(AcpResponse::AcpSessionStateResponse(
            session.state_response(),
        ))
    }

    async fn close_session(
        &self,
        mut ctx: ExtensionContext<'_>,
        request: AcpCloseSessionRequest,
    ) -> Result<AcpResponse, SidecarError> {
        // Enforce per-connection ownership before tearing anything down: only the
        // connection that created the session may close it. A non-owner (or a
        // missing session) fails closed with the same error, so a cross-connection
        // close neither succeeds nor reveals that another connection's session
        // exists — preventing a cross-tenant DoS. Mirrors the ownership check in
        // `get_session_state`.
        let caller_connection_id = ownership_connection_id(ctx.ownership());
        let session = {
            let mut sessions = self.sessions.lock().await;
            let owned_by_caller = sessions
                .get(&request.session_id)
                .is_some_and(|session| session.owner_connection_id == caller_connection_id);
            if owned_by_caller {
                sessions.remove(&request.session_id)
            } else {
                None
            }
        };
        let Some(session) = session else {
            return Err(SidecarError::InvalidState(format!(
                "unknown ACP session {}",
                request.session_id
            )));
        };
        let _ = ctx
            .close_stdin_wire(CloseStdinRequest {
                process_id: session.process_id.clone(),
            })
            .await;
        let _ = ctx
            .kill_process_wire(KillProcessRequest {
                process_id: session.process_id.clone(),
                signal: String::from("SIGTERM"),
            })
            .await;
        if !wait_for_process_exit(&mut ctx, &session.process_id, SESSION_CLOSE_TIMEOUT).await {
            let _ = ctx
                .kill_process_wire(KillProcessRequest {
                    process_id: session.process_id.clone(),
                    signal: String::from("SIGKILL"),
                })
                .await;
            let _ =
                wait_for_process_exit(&mut ctx, &session.process_id, SESSION_CLOSE_TIMEOUT).await;
        }
        let _ = ctx
            .dispose_session_resources_wire(&request.session_id)
            .await;
        tracing::info!(
            target: "agentos_sidecar::acp_extension",
            session_id = request.session_id,
            agent_type = session.agent_type,
            process_id = session.process_id,
            "ACP session closed; adapter process terminated",
        );
        Ok(AcpResponse::AcpSessionClosedResponse(
            AcpSessionClosedResponse {
                session_id: request.session_id,
            },
        ))
    }

    async fn session_request(
        &self,
        mut ctx: ExtensionContext<'_>,
        request: AcpSessionRequest,
    ) -> AcpHandlerOutput {
        let params = match request
            .params
            .as_deref()
            .map(|params| parse_json_text(params, "session request params"))
            .transpose()
        {
            Ok(Some(params)) => to_record(params),
            Ok(None) => Map::new(),
            Err(error) => return AcpHandlerOutput::response(Err(error)),
        };
        let request_params = params.clone();
        let mut outbound_params = params;
        outbound_params.insert(
            String::from("sessionId"),
            Value::String(request.session_id.clone()),
        );

        let caller_connection_id = ownership_connection_id(ctx.ownership());
        let (process_id, agent_type, rpc_id, mut stdout_buffer, pending_preamble) = {
            let mut sessions = self.sessions.lock().await;
            let Some(session) = sessions.get_mut(&request.session_id) else {
                return AcpHandlerOutput::response(Err(SidecarError::InvalidState(format!(
                    "unknown ACP session {}",
                    request.session_id
                ))));
            };
            // Enforce per-connection ownership: a non-owner must not be able to
            // drive (prompt/cancel/set_mode/etc.) another connection's adapter.
            // Fail closed with the same unknown-session error BEFORE mutating any
            // session state, so the attempt has no side effect on the victim (no
            // request id consumed, no stdout drained) and does not leak the
            // session's existence. Mirrors `get_session_state`.
            if session.owner_connection_id != caller_connection_id {
                return AcpHandlerOutput::response(Err(SidecarError::InvalidState(format!(
                    "unknown ACP session {}",
                    request.session_id
                ))));
            }
            let rpc_id = session.next_request_id;
            session.next_request_id += 1;
            // Take (and clear) any armed transcript-continuation preamble. It is
            // consumed once, on this session's first `session/prompt` after a
            // fallback resume; non-prompt methods leave it untouched.
            let pending_preamble = if request.method == "session/prompt" {
                session.pending_preamble.take()
            } else {
                None
            };
            (
                session.process_id.clone(),
                session.agent_type.clone(),
                rpc_id,
                std::mem::take(&mut session.stdout_buffer),
                pending_preamble,
            )
        };
        if let Some(preamble) = pending_preamble.as_deref() {
            prepend_prompt_preamble(&mut outbound_params, preamble);
        }
        let method = request.method.clone();
        let timeout = request_timeout(&method);
        let outbound = json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "method": method,
            "params": Value::Object(outbound_params),
        });
        let mut exchange = match send_json_rpc_request(
            &mut ctx,
            &process_id,
            &agent_type,
            outbound,
            rpc_id,
            timeout,
            &mut stdout_buffer,
            Some(&request.session_id),
        )
        .await
        {
            Ok(exchange) => exchange,
            Err(error) => {
                // Adapter process exit is a teardown signal. Log it, surface it
                // as an `AcpAgentExitedEvent`, and attempt a bounded auto-restart
                // (respawn + native `session/load`/`session/resume` under the
                // same session id). Only a successful restart keeps the record;
                // otherwise it is evicted (incl. `stdout_buffer`) rather than
                // leaked until a `close_session` that may never come. Other
                // (transient) failures keep the session so the armed preamble
                // can ride a retried prompt.
                if is_adapter_gone_error(&error) {
                    let exit_code = adapter_exit_code_from_error(&error);
                    let (event_frame, outcome, error) = self
                        .handle_adapter_exit(&mut ctx, &request.session_id, exit_code, error)
                        .await;
                    if outcome == ADAPTER_RESTART_OUTCOME_RESTARTED {
                        // The restarted session is live again; re-arm the
                        // continuation preamble so a retried prompt still
                        // carries the transcript pointer.
                        if let Some(preamble) = pending_preamble {
                            if let Some(session) =
                                self.sessions.lock().await.get_mut(&request.session_id)
                            {
                                if session.pending_preamble.is_none() {
                                    session.pending_preamble = Some(preamble);
                                }
                            }
                        }
                    }
                    let mut events = Vec::new();
                    if let Some(frame) = event_frame {
                        // Best-effort: event delivery must not mask the
                        // underlying adapter failure.
                        let _ = deliver_event(&ctx, &mut events, frame);
                    }
                    return AcpHandlerOutput {
                        response: Err(error),
                        events,
                    };
                } else if let Some(preamble) = pending_preamble {
                    if let Some(session) = self.sessions.lock().await.get_mut(&request.session_id) {
                        if session.pending_preamble.is_none() {
                            session.pending_preamble = Some(preamble);
                        }
                    }
                }
                return AcpHandlerOutput::response(Err(error));
            }
        };

        if let Some(session) = self.sessions.lock().await.get_mut(&request.session_id) {
            cap_stdout_buffer(&mut stdout_buffer);
            session.stdout_buffer = stdout_buffer;
        }

        if request.method == ACP_CANCEL_METHOD && is_cancel_method_not_found(&exchange.response) {
            if let Err(error) =
                write_session_cancel_notification(&mut ctx, &process_id, &request.session_id).await
            {
                return AcpHandlerOutput::response(Err(error));
            }
            let id = exchange
                .response
                .get("id")
                .cloned()
                .unwrap_or_else(|| Value::Number(rpc_id.into()));
            exchange.response = cancel_notification_fallback_response(id);
        }

        if exchange.response.get("error").is_none() {
            let synthetic = {
                let mut sessions = self.sessions.lock().await;
                match sessions.get_mut(&request.session_id) {
                    Some(session) => session.apply_request_success(
                        &request.method,
                        &request_params,
                        &exchange.events,
                    ),
                    None => Ok(None),
                }
            };
            match synthetic {
                Ok(Some(notification)) => {
                    let event = match encode_event(AcpEvent::AcpSessionEvent(AcpSessionEvent {
                        session_id: request.session_id.clone(),
                        notification,
                    })) {
                        Ok(event) => event,
                        Err(error) => return AcpHandlerOutput::response(Err(error)),
                    };
                    match ctx.ext_event_wire(event) {
                        Ok(frame) => {
                            if let Err(error) = deliver_event(&ctx, &mut exchange.events, frame) {
                                return AcpHandlerOutput::response(Err(error));
                            }
                        }
                        Err(error) => return AcpHandlerOutput::response(Err(error)),
                    }
                }
                Ok(None) => {}
                Err(error) => return AcpHandlerOutput::response(Err(error)),
            }
        }

        AcpHandlerOutput {
            response: Ok(AcpResponse::AcpSessionRpcResponse(
                agentos_protocol::generated::v1::AcpSessionRpcResponse {
                    session_id: request.session_id,
                    response: match serde_json::to_string(&exchange.response) {
                        Ok(response) => response,
                        Err(error) => {
                            return AcpHandlerOutput::response(Err(SidecarError::InvalidState(
                                format!("failed to serialize ACP session response: {error}"),
                            )));
                        }
                    },
                },
            )),
            events: exchange.events,
        }
    }

    // -----------------------------------------------------------------------
    // Resume state machine (spec §6 / §8) — CANONICAL doc comment.
    //
    // `resume_session` is the *stateless* orchestration that re-attaches a session
    // which exists in the actor's durable storage but is not live in this VM
    // (e.g. after a Rivet actor slept and woke with a fresh VM). The actor is the
    // lazy-resume trigger: a prompt arrives for an `external_session_id` that is
    // known to `agent_os_sessions` but absent from `Vars.live_sessions`, so the
    // actor reconstructs the transcript, calls the client `resume_session`, then
    // remaps `external -> live` and forwards the (preamble-prefixed) prompt.
    //
    // This handler holds NO event state and NO durable remap: it only knows live
    // ids for the current VM lifetime, which keeps the "ACP session events are
    // live-only" invariant intact (no event buffer / cursor replay is added).
    //
    //   resume(sessionId, agentType, transcriptPath?, cwd, env):
    //     # Launch a fresh adapter and probe its real capabilities via `initialize`
    //     # (capabilities cannot be trusted across a wake; we re-probe here).
    //     caps = initialize(agentType)          # agentCapabilities from the adapter
    //
    //     # Tier 1 — native (capability-gated optimization).
    //     if caps.loadSession || caps.resume:
    //         r = session/load (sessionId)       # or session/resume
    //         ok               -> return { sessionId, mode: "native" }
    //         UNKNOWN_SESSION  -> fall through    # store didn't survive the wake
    //         other error      -> propagate
    //
    //     # Tier 2 — universal fallback (no adapter code, no capability needed).
    //     live = session/new(agentType, cwd, env)
    //     if transcriptPath present:
    //         arm CONTINUATION_PREAMBLE(transcriptPath) on `live`'s next prompt
    //     return { sessionId: live, mode: "fallback" }   # caller remaps external->live
    //
    // The `UNKNOWN_SESSION` discriminator is a JSON-RPC error with
    // `error.data.kind === "unknown_session"`, following the `acp_timeout`
    // convention; only it triggers fallthrough. Transport/timeout errors propagate.
    async fn resume_session(
        &self,
        mut ctx: ExtensionContext<'_>,
        request: AcpResumeSessionRequest,
    ) -> AcpHandlerOutput {
        // Resolve the agent name -> package entrypoint/env/launchArgs from the
        // projected manifest, exactly as create_session does. The client is
        // npm-agnostic and sends only the agent name.
        let resolved = match resolve_agent(&mut ctx, &request.agent_type).await {
            Ok(resolved) => resolved,
            Err(error) => return AcpHandlerOutput::response(Err(error)),
        };

        // Reconstruct a create-shaped request so we reuse the exact adapter launch
        // + initialize flow. Resume does not carry MCP servers or extra instructions
        // (the durable transcript, not re-injected instructions, carries context);
        // skip the base OS instructions for the same reason — they were already
        // delivered to the original session.
        let create_like = AcpCreateSessionRequest {
            agent_type: request.agent_type.clone(),
            runtime: AcpRuntimeKind::JavaScript,
            cwd: request.cwd.clone(),
            args: Vec::new(),
            env: request.env.clone(),
            protocol_version: ACP_RESUME_PROTOCOL_VERSION,
            client_capabilities: DEFAULT_RESUME_CLIENT_CAPABILITIES.to_string(),
            mcp_servers: "[]".to_string(),
            skip_os_instructions: true,
            additional_instructions: None,
        };

        let process_id = self.allocate_process_id("acp-agent");
        // Manifest launch args first, then any caller-supplied args.
        let mut args = resolved.launch_args.clone();
        args.extend(create_like.args.iter().cloned());
        let mut env = hash_to_btree(create_like.env.clone());
        env.insert(
            String::from("SECURE_EXEC_KEEP_STDIN_OPEN"),
            String::from("1"),
        );
        // Manifest env applies as DEFAULTS; caller/base env wins on conflicts.
        for (key, value) in &resolved.env {
            env.entry(key.clone()).or_insert_with(|| value.clone());
        }
        if let Err(error) = self
            .apply_prompt_injection(&mut ctx, &create_like, &mut args, &mut env)
            .await
        {
            return AcpHandlerOutput::response(Err(error));
        }

        // Retain the final launch + handshake parameters for adapter
        // auto-restart before they are moved into the spawn request.
        let restart_state = AdapterRestartState {
            runtime: create_like.runtime.clone(),
            entrypoint: resolved.entrypoint.clone(),
            args: args.clone(),
            env: env.clone(),
            cwd: create_like.cwd.clone(),
            protocol_version: create_like.protocol_version,
            client_capabilities: create_like.client_capabilities.clone(),
            count: 0,
        };

        let started = match ctx
            .spawn_process_wire(ExecuteRequest {
                process_id: process_id.clone(),
                command: None,
                runtime: Some(convert_runtime(create_like.runtime.clone())),
                entrypoint: Some(resolved.entrypoint.clone()),
                args,
                env: env.into_iter().collect(),
                cwd: Some(create_like.cwd.clone()),
                wasm_permission_tier: None,
            })
            .await
        {
            Ok(started) => started,
            Err(error) => return AcpHandlerOutput::response(Err(error)),
        };

        let outcome = self
            .resume_session_inner(&mut ctx, &request, &create_like, &process_id)
            .await;
        if outcome.is_err() {
            kill_process_best_effort(&mut ctx, &process_id).await;
        }
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => return AcpHandlerOutput::response(Err(error)),
        };

        let session = AcpSessionRecord {
            session_id: outcome.bootstrap.session_id.clone(),
            owner_connection_id: ownership_connection_id(ctx.ownership()),
            agent_type: request.agent_type.clone(),
            process_id: process_id.clone(),
            pid: started.pid,
            modes: outcome.bootstrap.modes,
            config_options: outcome.bootstrap.config_options,
            agent_capabilities: outcome.bootstrap.agent_capabilities,
            agent_info: outcome.bootstrap.agent_info,
            stdout_buffer: outcome.bootstrap.stdout_buffer,
            next_request_id: outcome.next_request_id,
            closed: false,
            exit_code: None,
            // Fallback arms the transcript-continuation preamble for the first prompt.
            pending_preamble: outcome.pending_preamble,
            restart: restart_state,
        };

        let mut events = Vec::new();
        for notification in outcome.bootstrap.notifications {
            let event = match encode_event(AcpEvent::AcpSessionEvent(AcpSessionEvent {
                session_id: session.session_id.clone(),
                notification,
            })) {
                Ok(event) => event,
                Err(error) => {
                    kill_process_best_effort(&mut ctx, &process_id).await;
                    return AcpHandlerOutput::response(Err(error));
                }
            };
            match ctx.ext_event_wire(event) {
                Ok(event) => events.push(event),
                Err(error) => {
                    kill_process_best_effort(&mut ctx, &process_id).await;
                    return AcpHandlerOutput::response(Err(error));
                }
            }
        }

        if let Err(error) = ctx
            .bind_process_to_session(&session.session_id, &process_id)
            .await
        {
            kill_process_best_effort(&mut ctx, &process_id).await;
            return AcpHandlerOutput::response(Err(error));
        }
        self.sessions
            .lock()
            .await
            .insert(session.session_id.clone(), session.clone());

        AcpHandlerOutput {
            response: Ok(AcpResponse::AcpSessionResumedResponse(
                AcpSessionResumedResponse {
                    session_id: session.session_id,
                    mode: outcome.mode,
                },
            )),
            events,
        }
    }

    /// Drive the resume handshake: `initialize`, then native `session/load` (when
    /// the adapter advertises it) or the `session/new` fallback. Returns the
    /// bootstrap state plus the chosen `mode` and any armed preamble.
    async fn resume_session_inner(
        &self,
        ctx: &mut ExtensionContext<'_>,
        request: &AcpResumeSessionRequest,
        create_like: &AcpCreateSessionRequest,
        process_id: &str,
    ) -> Result<ResumeOutcome, SidecarError> {
        let mut stdout = String::new();
        let mut notifications = Vec::new();
        let client_capabilities =
            parse_json_text(&create_like.client_capabilities, "clientCapabilities")?;

        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": create_like.protocol_version,
                "clientCapabilities": client_capabilities,
            },
        });
        let initialize_response = send_json_rpc_request(
            ctx,
            process_id,
            &create_like.agent_type,
            initialize,
            1,
            INITIALIZE_TIMEOUT,
            &mut stdout,
            None,
        )
        .await?;
        notifications.extend(initialize_response.notifications);
        let init_result = response_result(initialize_response.response, "ACP initialize")?;
        validate_initialize_result(&init_result, create_like.protocol_version)?;

        let agent_capabilities = init_result.get("agentCapabilities").cloned();

        // Tier 1 — native (capability-gated). Re-probed caps decide eligibility.
        if let Some(native_resume_method) = native_resume_method(agent_capabilities.as_ref()) {
            let load = json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": native_resume_method,
                "params": {
                    "sessionId": request.session_id,
                    "cwd": request.cwd,
                    "mcpServers": [],
                },
            });
            let mut load_response = send_json_rpc_request(
                ctx,
                process_id,
                &create_like.agent_type,
                load,
                2,
                SESSION_NEW_TIMEOUT,
                &mut stdout,
                None,
            )
            .await?;
            notifications.extend(load_response.notifications);
            trace_acp_response(native_resume_method, &load_response.response);
            normalize_unknown_session_error(&mut load_response.response);

            if load_response.response.get("error").is_none() {
                let load_result = response_result(
                    load_response.response,
                    &format!("ACP {native_resume_method}"),
                )?;
                let bootstrap = build_resume_bootstrap(
                    request.session_id.clone(),
                    &init_result,
                    &load_result,
                    &request.agent_type,
                    agent_capabilities.as_ref(),
                    stdout,
                    notifications,
                )?;
                return Ok(ResumeOutcome {
                    bootstrap,
                    mode: String::from("native"),
                    next_request_id: 3,
                    pending_preamble: None,
                });
            }

            // Native load failed. Only the `unknown_session` sentinel falls through
            // to the universal fallback; every other error propagates (surfaced
            // verbatim via `response_result`, which returns Err when `error` is set).
            if !is_unknown_session_error(&load_response.response) {
                return Err(response_result(
                    load_response.response,
                    &format!("ACP {native_resume_method}"),
                )
                .expect_err("native resume error object must map to a SidecarError"));
            }
            // fall through to Tier 2
        }

        // Tier 2 — universal fallback. A fresh session, plus the transcript pointer.
        let session_new = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/new",
            "params": {
                "cwd": request.cwd,
                "mcpServers": [],
            },
        });
        let session_response = send_json_rpc_request(
            ctx,
            process_id,
            &create_like.agent_type,
            session_new,
            2,
            SESSION_NEW_TIMEOUT,
            &mut stdout,
            None,
        )
        .await?;
        notifications.extend(session_response.notifications);
        let session_result = response_result(session_response.response, "ACP session/new")?;
        let live_session_id = session_id_from_session_result(&session_result, process_id);

        let pending_preamble = request
            .transcript_path
            .as_deref()
            .filter(|path| !path.is_empty())
            .map(|path| CONTINUATION_PREAMBLE.replace("{path}", path));

        let bootstrap = build_resume_bootstrap(
            live_session_id,
            &init_result,
            &session_result,
            &request.agent_type,
            agent_capabilities.as_ref(),
            stdout,
            notifications,
        )?;
        Ok(ResumeOutcome {
            bootstrap,
            mode: String::from("fallback"),
            next_request_id: 3,
            pending_preamble,
        })
    }

    fn allocate_process_id(&self, prefix: &str) -> String {
        let id = self.next_process_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("{prefix}-{id}")
    }

    /// Handle an unexpected adapter exit observed while driving `session_id`:
    /// record the exit on the session record, log it, attempt a bounded
    /// auto-restart, and build the `AcpAgentExitedEvent` describing the
    /// outcome. Returns the encoded event frame (when the session record still
    /// existed), the outcome string, and the error to surface for the
    /// in-flight request — augmented with the restart outcome so callers know
    /// whether a retry can succeed.
    ///
    /// Extension dispatch is serialized by the stdio loop (one ACP request is
    /// in flight at a time), so no second request can race the restart window
    /// between the lock scopes below.
    async fn handle_adapter_exit(
        &self,
        ctx: &mut ExtensionContext<'_>,
        session_id: &str,
        exit_code: Option<i32>,
        error: SidecarError,
    ) -> (
        Option<secure_exec_sidecar::wire::EventFrame>,
        &'static str,
        SidecarError,
    ) {
        // Snapshot + mark the record under the lock; run the (slow) restart
        // handshake outside it.
        let Some((agent_type, dead_process_id, restart)) = ({
            let mut sessions = self.sessions.lock().await;
            sessions.get_mut(session_id).map(|session| {
                session.closed = true;
                session.exit_code = exit_code;
                (
                    session.agent_type.clone(),
                    session.process_id.clone(),
                    session.restart.clone(),
                )
            })
        }) else {
            // Record already gone (e.g. connection cleanup won the race);
            // nothing to restart and no session to describe in an event.
            return (None, ADAPTER_RESTART_OUTCOME_FAILED, error);
        };

        tracing::warn!(
            target: "agentos_sidecar::acp_extension",
            session_id,
            agent_type,
            process_id = dead_process_id,
            exit_code = ?exit_code,
            restarts_used = restart.count,
            max_restarts = MAX_ADAPTER_RESTARTS,
            "ACP adapter process exited unexpectedly",
        );

        let mut restart_detail: Option<String> = None;
        let (outcome, restart_count) = if restart.count >= MAX_ADAPTER_RESTARTS {
            self.sessions.lock().await.remove(session_id);
            tracing::warn!(
                target: "agentos_sidecar::acp_extension",
                session_id,
                agent_type,
                max_restarts = MAX_ADAPTER_RESTARTS,
                "ACP adapter restart budget exhausted (raise MAX_ADAPTER_RESTARTS \
                 or investigate the crashing adapter); session evicted",
            );
            (ADAPTER_RESTART_OUTCOME_EXHAUSTED, restart.count)
        } else {
            let attempt = restart.count + 1;
            if let Some(session) = self.sessions.lock().await.get_mut(session_id) {
                session.restart.count = attempt;
            }
            match self
                .restart_adapter(ctx, session_id, &agent_type, &restart)
                .await
            {
                Ok(()) => {
                    tracing::warn!(
                        target: "agentos_sidecar::acp_extension",
                        session_id,
                        agent_type,
                        attempt,
                        max_restarts = MAX_ADAPTER_RESTARTS,
                        "ACP adapter auto-restarted; session re-attached natively",
                    );
                    (ADAPTER_RESTART_OUTCOME_RESTARTED, attempt)
                }
                Err(restart_error) => {
                    self.sessions.lock().await.remove(session_id);
                    let outcome = match &restart_error {
                        AdapterRestartError::Unsupported => ADAPTER_RESTART_OUTCOME_UNSUPPORTED,
                        AdapterRestartError::Failed(_) => ADAPTER_RESTART_OUTCOME_FAILED,
                    };
                    let detail = restart_error.detail();
                    tracing::warn!(
                        target: "agentos_sidecar::acp_extension",
                        session_id,
                        agent_type,
                        attempt,
                        outcome,
                        detail = %detail,
                        "ACP adapter auto-restart did not recover the session; session evicted",
                    );
                    restart_detail = Some(detail);
                    (outcome, attempt)
                }
            }
        };

        let frame = encode_event(AcpEvent::AcpAgentExitedEvent(AcpAgentExitedEvent {
            session_id: session_id.to_string(),
            agent_type,
            process_id: dead_process_id,
            exit_code,
            restart: outcome.to_string(),
            restart_count,
            max_restarts: MAX_ADAPTER_RESTARTS,
        }))
        .and_then(|payload| ctx.ext_event_wire(payload))
        .ok();

        let error = SidecarError::InvalidState(match outcome {
            ADAPTER_RESTART_OUTCOME_RESTARTED => format!(
                "{error}; ACP adapter was auto-restarted (attempt \
                 {restart_count}/{MAX_ADAPTER_RESTARTS}) and the session is live again — \
                 retry the request"
            ),
            ADAPTER_RESTART_OUTCOME_EXHAUSTED => format!(
                "{error}; ACP adapter restart budget exhausted \
                 ({MAX_ADAPTER_RESTARTS} restarts) — session evicted"
            ),
            _ => format!(
                "{error}; ACP adapter auto-restart {outcome} ({}) — session evicted",
                restart_detail.as_deref().unwrap_or("no detail")
            ),
        });
        (frame, outcome, error)
    }

    /// Respawn the adapter with the retained launch parameters and natively
    /// re-attach `session_id` (`initialize` + `session/load`/`session/resume`).
    /// On success the session record points at the new process and is live
    /// again; the caller owns eviction on failure.
    async fn restart_adapter(
        &self,
        ctx: &mut ExtensionContext<'_>,
        session_id: &str,
        agent_type: &str,
        restart: &AdapterRestartState,
    ) -> Result<(), AdapterRestartError> {
        let process_id = self.allocate_process_id("acp-agent");
        let started = ctx
            .spawn_process_wire(ExecuteRequest {
                process_id: process_id.clone(),
                command: None,
                runtime: Some(convert_runtime(restart.runtime.clone())),
                entrypoint: Some(restart.entrypoint.clone()),
                args: restart.args.clone(),
                env: restart.env.clone().into_iter().collect(),
                cwd: Some(restart.cwd.clone()),
                wasm_permission_tier: None,
            })
            .await
            .map_err(AdapterRestartError::Failed)?;

        let bootstrap =
            match restart_handshake(ctx, session_id, agent_type, restart, &process_id).await {
                Ok(bootstrap) => bootstrap,
                Err(error) => {
                    kill_process_best_effort(ctx, &process_id).await;
                    return Err(error);
                }
            };

        if let Err(error) = ctx.bind_process_to_session(session_id, &process_id).await {
            kill_process_best_effort(ctx, &process_id).await;
            return Err(AdapterRestartError::Failed(error));
        }

        {
            let mut sessions = self.sessions.lock().await;
            let Some(session) = sessions.get_mut(session_id) else {
                // The record vanished mid-restart (close/disconnect); don't
                // leak the fresh adapter process.
                drop(sessions);
                kill_process_best_effort(ctx, &process_id).await;
                return Err(AdapterRestartError::Failed(SidecarError::InvalidState(
                    format!("ACP session {session_id} was removed during adapter restart"),
                )));
            };
            session.process_id = process_id;
            session.pid = started.pid;
            session.closed = false;
            session.exit_code = None;
            session.modes = bootstrap.modes;
            session.config_options = bootstrap.config_options;
            session.agent_capabilities = bootstrap.agent_capabilities;
            session.agent_info = bootstrap.agent_info;
            session.stdout_buffer = bootstrap.stdout_buffer;
            session.next_request_id = 3;
        }
        Ok(())
    }

    /// Drop every session owned by `connection_id`, returning the adapter process
    /// ids of the removed records so the caller can reap them. This is the
    /// connection-teardown counterpart to the explicit `close_session` RPC: when
    /// a connection goes away (client disconnect / shutdown) without a
    /// `close_session` per live session, its records — including the potentially
    /// large `stdout_buffer` — must not outlive the connection.
    ///
    /// Invoked from `on_session_disposed` (the host's per-connection teardown
    /// callback, fired on `DisposeReason::ConnectionClosed`); the other live
    /// teardown paths are `on_dispose` (whole-extension) and the process-exit
    /// eviction in `session_request`. Covered by `connection_teardown_evicts_only_*`
    /// tests.
    async fn cleanup_sessions_for_connection(&self, connection_id: &str) -> Vec<String> {
        let mut sessions = self.sessions.lock().await;
        evict_sessions_for_connection(&mut sessions, connection_id)
    }
}

impl Extension for AcpExtension {
    fn namespace(&self) -> &str {
        ACP_EXTENSION_NAMESPACE
    }

    fn handle_request<'a>(
        &'a self,
        ctx: ExtensionContext<'a>,
        payload: Vec<u8>,
    ) -> ExtensionFuture<'a, ExtensionResponse> {
        Box::pin(async move {
            let response = self.handle_payload(ctx, &payload).await?;
            Ok(response)
        })
    }

    fn is_blocking_request(&self, payload: &[u8]) -> bool {
        matches!(
            decode_request(payload),
            Ok(AcpRequest::AcpSessionRequest(request)) if request.method == "session/prompt"
        )
    }

    fn on_dispose<'a>(&'a self) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            // Extension/sidecar teardown: drop every remaining session record so
            // no `stdout_buffer` survives the host process. The adapter processes
            // themselves are reaped by the host's own session/VM dispose; this
            // only frees the wrapper-side tracking map.
            self.sessions.lock().await.clear();
            Ok(())
        })
    }

    fn on_session_disposed<'a>(&'a self, ctx: ExtensionSnapshot) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            // The host invokes this only on DisposeReason::ConnectionClosed, i.e.
            // the client disconnected without sending `close_session` per live
            // session. Evict this connection's ACP session records — including
            // their potentially large `stdout_buffer` — so they don't outlive the
            // connection. This closes the disconnect path of H4 (the per-request
            // process-exit eviction and `on_dispose` cover the other paths).
            let connection_id = ownership_connection_id(ctx.ownership());
            self.cleanup_sessions_for_connection(&connection_id).await;
            Ok(())
        })
    }

    fn interrupt_blocking_request(
        &self,
        blocking_payload: &[u8],
        interrupt: ExtensionInterruptRequest<'_>,
    ) -> Option<ExtensionInterruptResponse> {
        let AcpRequest::AcpSessionRequest(blocking_request) =
            decode_request(blocking_payload).ok()?
        else {
            return None;
        };
        if blocking_request.method != "session/prompt" {
            return None;
        }

        let interrupted_response_payload =
            encode_interrupted_session_response(&blocking_request.session_id)?;
        match interrupt {
            ExtensionInterruptRequest::KillProcess => Some(ExtensionInterruptResponse {
                interrupted_response_payload,
                interrupting_response_payload: None,
            }),
            ExtensionInterruptRequest::ExtensionPayload(payload) => {
                let request = decode_request(payload).ok()?;
                match request {
                    AcpRequest::AcpCloseSessionRequest(request)
                        if request.session_id == blocking_request.session_id =>
                    {
                        Some(ExtensionInterruptResponse {
                            interrupted_response_payload,
                            interrupting_response_payload: None,
                        })
                    }
                    AcpRequest::AcpSessionRequest(request)
                        if request.session_id == blocking_request.session_id
                            && request.method == ACP_CANCEL_METHOD =>
                    {
                        Some(ExtensionInterruptResponse {
                            interrupted_response_payload,
                            interrupting_response_payload: Some(
                                encode_interrupted_cancel_response(&request.session_id)?,
                            ),
                        })
                    }
                    AcpRequest::AcpCreateSessionRequest(_)
                    | AcpRequest::AcpGetSessionStateRequest(_)
                    | AcpRequest::AcpCloseSessionRequest(_)
                    | AcpRequest::AcpResumeSessionRequest(_)
                    | AcpRequest::AcpSessionRequest(_)
                    | AcpRequest::AcpListAgentsRequest(_)
                    | AcpRequest::AcpDeliverAgentOutputRequest(_) => None,
                }
            }
        }
    }
}

struct AcpHandlerOutput {
    response: Result<AcpResponse, SidecarError>,
    events: Vec<secure_exec_sidecar::wire::EventFrame>,
}

impl AcpHandlerOutput {
    fn response(response: Result<AcpResponse, SidecarError>) -> Self {
        Self {
            response,
            events: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct CreateSessionBootstrap {
    session_id: String,
    modes: Option<String>,
    config_options: Vec<String>,
    agent_capabilities: Option<String>,
    agent_info: Option<String>,
    stdout_buffer: String,
    notifications: Vec<String>,
}

/// Result of the resume state machine (`resume_session_inner`).
#[derive(Debug)]
struct ResumeOutcome {
    bootstrap: CreateSessionBootstrap,
    /// `"native"` (session/load|resume) or `"fallback"` (session/new + preamble).
    mode: String,
    /// First request id available for post-resume RPCs (initialize=1, load/new=2).
    next_request_id: i64,
    /// Transcript-continuation preamble armed for the first prompt (fallback only).
    pending_preamble: Option<String>,
}

impl AcpSessionRecord {
    fn created_response(&self) -> AcpSessionCreatedResponse {
        AcpSessionCreatedResponse {
            session_id: self.session_id.clone(),
            pid: self.pid,
            modes: self.modes.clone(),
            config_options: self.config_options.clone(),
            agent_capabilities: self.agent_capabilities.clone(),
            agent_info: self.agent_info.clone(),
        }
    }

    fn state_response(&self) -> AcpSessionStateResponse {
        AcpSessionStateResponse {
            session_id: self.session_id.clone(),
            agent_type: self.agent_type.clone(),
            process_id: self.process_id.clone(),
            pid: self.pid,
            closed: self.closed,
            exit_code: self.exit_code,
            modes: self.modes.clone(),
            config_options: self.config_options.clone(),
            agent_capabilities: self.agent_capabilities.clone(),
            agent_info: self.agent_info.clone(),
        }
    }

    fn apply_request_success(
        &mut self,
        method: &str,
        params: &Map<String, Value>,
        events: &[secure_exec_sidecar::wire::EventFrame],
    ) -> Result<Option<String>, SidecarError> {
        if method == "session/set_mode" {
            let Some(mode_id) = params.get("modeId").and_then(Value::as_str) else {
                return Ok(None);
            };
            self.apply_local_mode_update(mode_id)?;
            if !has_matching_session_update(events, &self.session_id, |update| {
                update
                    .get("sessionUpdate")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == "current_mode_update")
                    && update
                        .get("currentModeId")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value == mode_id)
            }) {
                return serde_json::to_string(&synthetic_mode_update(mode_id))
                    .map(Some)
                    .map_err(|error| {
                        SidecarError::InvalidState(format!(
                            "failed to serialize synthetic mode update: {error}"
                        ))
                    });
            }
        }

        if method == "session/set_config_option" {
            let Some(config_id) = params.get("configId").and_then(Value::as_str) else {
                return Ok(None);
            };
            let Some(value) = params.get("value") else {
                return Ok(None);
            };
            self.apply_local_config_update(config_id, value)?;
            if !has_matching_session_update(events, &self.session_id, |update| {
                update
                    .get("sessionUpdate")
                    .and_then(Value::as_str)
                    .is_some_and(|value| {
                        value == "config_option_update" || value == "config_options_update"
                    })
            }) {
                return serde_json::to_string(&synthetic_config_update(&self.config_options))
                    .map(Some)
                    .map_err(|error| {
                        SidecarError::InvalidState(format!(
                            "failed to serialize synthetic config update: {error}"
                        ))
                    });
            }
        }

        Ok(None)
    }

    fn apply_local_mode_update(&mut self, mode_id: &str) -> Result<(), SidecarError> {
        let Some(modes) = self.modes.as_mut() else {
            return Ok(());
        };
        let mut modes_value = parse_json_text(modes, "ACP modes")?;
        if let Value::Object(map) = &mut modes_value {
            map.insert(
                String::from("currentModeId"),
                Value::String(String::from(mode_id)),
            );
            *modes = serde_json::to_string(&modes_value).map_err(|error| {
                SidecarError::InvalidState(format!("failed to serialize ACP modes: {error}"))
            })?;
        }
        Ok(())
    }

    fn apply_local_config_update(
        &mut self,
        config_id: &str,
        value: &Value,
    ) -> Result<(), SidecarError> {
        let mut updated = false;
        let mut config_options = Vec::with_capacity(self.config_options.len());
        for (index, option) in self.config_options.iter().enumerate() {
            let mut option_value = parse_json_text(option, "ACP config option")?;
            let Value::Object(map) = &mut option_value else {
                return Err(SidecarError::InvalidState(format!(
                    "ACP config option {index} must be an object"
                )));
            };
            let Some(option_id) = map.get("id").and_then(Value::as_str) else {
                return Err(SidecarError::InvalidState(format!(
                    "ACP config option {index} missing id"
                )));
            };
            if option_id == config_id {
                map.insert(String::from("currentValue"), value.clone());
                updated = true;
            }
            config_options.push(serde_json::to_string(&option_value).map_err(|error| {
                SidecarError::InvalidState(format!(
                    "failed to serialize ACP config option: {error}"
                ))
            })?);
        }
        if !updated {
            return Err(SidecarError::InvalidState(format!(
                "unknown ACP config option {config_id}"
            )));
        }
        self.config_options = config_options;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
/// Deliver an ACP event frame to the host. Streams it live through the sidecar's
/// event sink (the stdio path) the instant it is produced; only when no live sink
/// is configured (an in-process `NativeSidecar` with no stdout loop) does it fall
/// back to collecting the frame into `events` for the dispatch-result batch. This
/// is what makes `session/update`s arrive mid-turn instead of all arriving at
/// once when the `session/prompt` dispatch finally resolves.
fn deliver_event(
    ctx: &ExtensionContext<'_>,
    events: &mut Vec<secure_exec_sidecar::wire::EventFrame>,
    frame: secure_exec_sidecar::wire::EventFrame,
) -> Result<(), SidecarError> {
    if let Some(frame) = ctx.emit_event_wire(frame)? {
        events.push(frame);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn send_json_rpc_request(
    ctx: &mut ExtensionContext<'_>,
    process_id: &str,
    agent_type: &str,
    request: Value,
    response_id: i64,
    timeout: Duration,
    stdout: &mut String,
    event_session_id: Option<&str>,
) -> Result<JsonRpcExchange, SidecarError> {
    let mut line = serde_json::to_vec(&request).map_err(|error| {
        SidecarError::InvalidState(format!("failed to serialize ACP request: {error}"))
    })?;
    line.push(b'\n');
    ctx.write_stdin_wire(WriteStdinRequest {
        process_id: process_id.to_string(),
        chunk: line,
    })
    .await?;

    let deadline = Instant::now() + timeout;
    let mut events = Vec::new();
    let mut notifications = Vec::new();
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let mut recent_activity = Vec::new();
    let mut adapter_stderr = String::new();
    record_recent_activity(
        &mut recent_activity,
        format!("sent request {method} id={response_id}"),
    );
    loop {
        let now = Instant::now();
        if now >= deadline {
            let cancel_status = if let Some(session_id) = event_session_id {
                match write_session_cancel_notification(ctx, process_id, session_id).await {
                    Ok(()) => String::from("sent session/cancel notification"),
                    Err(error) => format!("failed to send session/cancel notification: {error}"),
                }
            } else {
                String::from("no session/cancel notification for bootstrap request")
            };
            if event_session_id.is_some() {
                return Ok(JsonRpcExchange {
                    response: timeout_error_response(
                        response_id,
                        &method,
                        timeout,
                        process_id,
                        &cancel_status,
                        recent_activity,
                    ),
                    events,
                    notifications,
                });
            }
            return Err(SidecarError::InvalidState(format!(
                "timed out waiting for ACP response id={response_id}; {cancel_status}"
            )));
        }
        let remaining = deadline.saturating_duration_since(now);
        let event = ctx
            .poll_event_wire(remaining.min(ACP_JSON_RPC_POLL_INTERVAL))
            .await?;
        let Some(event) = event else {
            continue;
        };

        match event.payload {
            EventPayload::ProcessOutputEvent(output)
                if output.process_id == process_id && output.channel == StreamChannel::Stdout =>
            {
                for line in
                    append_stdout_chunk(stdout, &output.chunk, DEFAULT_ACP_MAX_READ_LINE_BYTES)?
                {
                    let Ok(message) = serde_json::from_str::<Value>(&line) else {
                        record_recent_activity(
                            &mut recent_activity,
                            String::from("invalid_json_rpc code=-32700 Parse error"),
                        );
                        continue;
                    };
                    if message.get("id").is_some()
                        && message.get("method").and_then(Value::as_str).is_some()
                    {
                        if let Some(inbound_method) = message.get("method").and_then(Value::as_str)
                        {
                            record_recent_activity(
                                &mut recent_activity,
                                format!(
                                    "received request {inbound_method} id={}",
                                    json_rpc_id_label(message.get("id"))
                                ),
                            );
                        }
                        if let Some(session_id) = event_session_id {
                            handle_inbound_request(ctx, process_id, session_id, &message).await?;
                        }
                        continue;
                    }
                    if message.get("id").and_then(Value::as_i64) == Some(response_id) {
                        return Ok(JsonRpcExchange {
                            response: message,
                            events,
                            notifications,
                        });
                    }
                    if message.get("method").and_then(Value::as_str).is_some() {
                        if let Some(notification_method) =
                            message.get("method").and_then(Value::as_str)
                        {
                            record_recent_activity(
                                &mut recent_activity,
                                format!("received notification {notification_method}"),
                            );
                        }
                        if let Some(session_id) = event_session_id {
                            let frame = ctx.ext_event_wire(encode_event(
                                AcpEvent::AcpSessionEvent(AcpSessionEvent {
                                    session_id: session_id.to_string(),
                                    notification: serde_json::to_string(&message).map_err(
                                        |error| {
                                            SidecarError::InvalidState(format!(
                                                "failed to serialize ACP notification: {error}"
                                            ))
                                        },
                                    )?,
                                }),
                            )?)?;
                            deliver_event(ctx, &mut events, frame)?;
                        } else {
                            notifications.push(serde_json::to_string(&message).map_err(
                                |error| {
                                    SidecarError::InvalidState(format!(
                                        "failed to serialize ACP bootstrap notification: {error}"
                                    ))
                                },
                            )?);
                        }
                    }
                }
            }
            EventPayload::ProcessOutputEvent(output)
                if output.process_id == process_id && output.channel == StreamChannel::Stderr =>
            {
                // Accumulate stderr (borrow before the chunk is moved into the
                // frame) so a non-zero adapter exit can fold the tail into its
                // error for diagnostics.
                adapter_stderr.push_str(&String::from_utf8_lossy(&output.chunk));
                let frame = ctx.ext_event_wire(encode_event(AcpEvent::AcpAgentStderrEvent(
                    AcpAgentStderrEvent {
                        session_id: event_session_id.unwrap_or_default().to_string(),
                        agent_type: agent_type.to_string(),
                        process_id: process_id.to_string(),
                        chunk: output.chunk,
                    },
                ))?)?;
                // Stream live during an owned session turn (prompt/cancel), but
                // keep bootstrap stderr (initialize/session-new/load, which pass
                // no session id) in the batch so it still arrives for callers that
                // only subscribe after create/resume resolves.
                if event_session_id.is_some() {
                    deliver_event(ctx, &mut events, frame)?;
                } else {
                    events.push(frame);
                }
            }
            EventPayload::ProcessExitedEvent(exited) if exited.process_id == process_id => {
                // Embed ADAPTER_EXITED_ERROR_MARKER directly so is_adapter_exited_error()
                // stays coupled to this producer: changing the wording can't silently
                // disable session eviction (the H4 leak fix) without touching the const.
                let stderr_tail: String = adapter_stderr
                    .chars()
                    .rev()
                    .take(4000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                tracing::warn!(
                    target: "agentos_sidecar::acp_extension",
                    process_id,
                    agent_type,
                    session_id = ?event_session_id,
                    exit_code = exited.exit_code,
                    stderr_tail = %stderr_tail,
                    "ACP adapter process exited before answering request id={response_id}",
                );
                return Err(SidecarError::InvalidState(format!(
                    "ACP adapter process {process_id} {ADAPTER_EXITED_ERROR_MARKER} {} before response id={response_id}; recent_activity={:?}; adapter_stderr={:?}",
                    exited.exit_code, recent_activity, stderr_tail
                )));
            }
            EventPayload::ProcessOutputEvent(_)
            | EventPayload::ProcessExitedEvent(_)
            | EventPayload::VmLifecycleEvent(_)
            | EventPayload::StructuredEvent(_)
            | EventPayload::ExtEnvelope(_) => {}
        }
    }
}

async fn write_session_cancel_notification(
    ctx: &mut ExtensionContext<'_>,
    process_id: &str,
    session_id: &str,
) -> Result<(), SidecarError> {
    let mut line =
        serde_json::to_vec(&session_cancel_notification(session_id)).map_err(|error| {
            SidecarError::InvalidState(format!(
                "failed to serialize ACP cancel notification: {error}"
            ))
        })?;
    line.push(b'\n');
    ctx.write_stdin_wire(WriteStdinRequest {
        process_id: process_id.to_string(),
        chunk: line,
    })
    .await?;
    Ok(())
}

fn session_cancel_notification(session_id: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": ACP_CANCEL_METHOD,
        "params": {
            "sessionId": session_id,
        },
    })
}

fn cancel_notification_fallback_response(id: Value) -> Value {
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

fn encode_interrupted_session_response(session_id: &str) -> Option<Vec<u8>> {
    encode_session_rpc_response(
        session_id,
        json!({
            "jsonrpc": "2.0",
            "id": null,
            "result": {
                "stopReason": "cancelled",
            },
        }),
    )
}

fn encode_interrupted_cancel_response(session_id: &str) -> Option<Vec<u8>> {
    encode_session_rpc_response(
        session_id,
        json!({
            "jsonrpc": "2.0",
            "id": null,
            "result": {
                "cancelled": true,
                "requested": true,
                "via": "prompt-interrupt",
            },
        }),
    )
}

fn encode_session_rpc_response(session_id: &str, response: Value) -> Option<Vec<u8>> {
    let response = AcpResponse::AcpSessionRpcResponse(
        agentos_protocol::generated::v1::AcpSessionRpcResponse {
            session_id: session_id.to_string(),
            response: serde_json::to_string(&response).ok()?,
        },
    );
    encode_response(response).ok()
}

fn synthetic_mode_update(mode_id: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "update": {
                "sessionUpdate": "current_mode_update",
                "currentModeId": mode_id,
            },
        },
    })
}

fn synthetic_config_update(config_options: &[String]) -> Value {
    let config_options = config_options
        .iter()
        .filter_map(|option| serde_json::from_str::<Value>(option).ok())
        .collect::<Vec<_>>();
    json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "update": {
                "sessionUpdate": "config_option_update",
                "configOptions": config_options,
            },
        },
    })
}

fn has_matching_session_update(
    events: &[secure_exec_sidecar::wire::EventFrame],
    session_id: &str,
    predicate: impl Fn(&Map<String, Value>) -> bool,
) -> bool {
    events.iter().any(|event| {
        let EventPayload::ExtEnvelope(envelope) = &event.payload else {
            return false;
        };
        if envelope.namespace != ACP_EXTENSION_NAMESPACE {
            return false;
        }
        let Ok(AcpEvent::AcpSessionEvent(event)) =
            serde_bare::from_slice::<AcpEvent>(&envelope.payload)
        else {
            return false;
        };
        if event.session_id != session_id {
            return false;
        }
        let Ok(notification) = serde_json::from_str::<Value>(&event.notification) else {
            return false;
        };
        if notification.get("method").and_then(Value::as_str) != Some("session/update") {
            return false;
        }
        let Some(params) = notification.get("params").and_then(Value::as_object) else {
            return false;
        };
        let update = params
            .get("update")
            .and_then(Value::as_object)
            .unwrap_or(params);
        predicate(update)
    })
}

fn is_cancel_method_not_found(response: &Value) -> bool {
    let Some(error) = response.get("error").and_then(Value::as_object) else {
        return false;
    };
    if error.get("code").and_then(Value::as_i64) != Some(-32601) {
        return false;
    }
    if error
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("method"))
        .and_then(Value::as_str)
        .is_some_and(|method| method == ACP_CANCEL_METHOD)
    {
        return true;
    }
    error
        .get("message")
        .and_then(Value::as_str)
        .is_some_and(|message| message.contains(ACP_CANCEL_METHOD))
}

fn record_recent_activity(recent_activity: &mut Vec<String>, entry: String) {
    if recent_activity.len() == 16 {
        recent_activity.remove(0);
    }
    recent_activity.push(entry);
}

fn json_rpc_id_label(id: Option<&Value>) -> String {
    match id {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Null) => String::from("null"),
        Some(other) => other.to_string(),
        None => String::from("unknown"),
    }
}

fn timeout_error_response(
    response_id: i64,
    method: &str,
    timeout: Duration,
    process_id: &str,
    cancel_status: &str,
    recent_activity: Vec<String>,
) -> Value {
    let timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
    let data = json!({
        "kind": "acp_timeout",
        "method": method,
        "id": response_id,
        "timeoutMs": timeout_ms,
        "transportState": cancel_status,
        "recentActivity": recent_activity,
    });
    json!({
        "jsonrpc": "2.0",
        "id": response_id,
        "error": {
            "code": -32000,
            "message": timeout_error_message(method, response_id, timeout_ms, process_id, cancel_status, &data),
            "data": data,
        },
    })
}

fn timeout_error_message(
    method: &str,
    response_id: i64,
    timeout_ms: u64,
    process_id: &str,
    cancel_status: &str,
    data: &Value,
) -> String {
    let activity = data
        .get("recentActivity")
        .and_then(Value::as_array)
        .filter(|activity| !activity.is_empty())
        .map(|activity| {
            activity
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .unwrap_or_else(|| String::from("no recent ACP activity"));
    format!(
        "ACP request {method} (id={response_id}) timed out after {timeout_ms}ms. adapter process {process_id}. {cancel_status}. Recent ACP activity: {activity}"
    )
}

async fn wait_for_process_exit(
    ctx: &mut ExtensionContext<'_>,
    process_id: &str,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let Ok(event) = ctx
            .poll_event_wire(
                deadline
                    .saturating_duration_since(now)
                    .min(Duration::from_millis(50)),
            )
            .await
        else {
            return false;
        };
        let Some(event) = event else {
            continue;
        };
        if let EventPayload::ProcessExitedEvent(exited) = event.payload {
            if exited.process_id == process_id {
                return true;
            }
        }
    }
}

async fn handle_inbound_request(
    ctx: &mut ExtensionContext<'_>,
    process_id: &str,
    session_id: &str,
    message: &Value,
) -> Result<(), SidecarError> {
    let id = message.get("id").cloned().ok_or_else(|| {
        SidecarError::InvalidState(String::from("ACP inbound request missing id"))
    })?;
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Ok(());
    };
    let response = match method {
        "session/request_permission" => {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let permission_id = to_record(params.clone())
                .get("permissionId")
                .and_then(Value::as_str)
                .unwrap_or("permission")
                .to_string();
            let callback = AcpCallback::AcpPermissionCallback(AcpPermissionCallback {
                session_id: session_id.to_string(),
                permission_id: permission_id.clone(),
                params: serde_json::to_string(&params).map_err(|error| {
                    SidecarError::InvalidState(format!(
                        "failed to serialize ACP permission params: {error}"
                    ))
                })?,
            });
            let response =
                ctx.invoke_callback(encode_callback(callback)?, Duration::from_secs(120))?;
            let response: AcpCallbackResponse =
                serde_bare::from_slice(&response).map_err(|error| {
                    SidecarError::InvalidState(format!("invalid ACP callback response: {error}"))
                })?;
            let reply = match response {
                AcpCallbackResponse::AcpPermissionCallbackResponse(response) => response.reply,
                AcpCallbackResponse::AcpHostRequestCallbackResponse(_) => String::from("reject"),
            };
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": permission_result(&reply, &params),
            })
        }
        _ => forward_inbound_host_request(ctx, session_id, message, &id, method)?,
    };
    let mut line = serde_json::to_vec(&response).map_err(|error| {
        SidecarError::InvalidState(format!("failed to serialize ACP inbound response: {error}"))
    })?;
    line.push(b'\n');
    ctx.write_stdin_wire(WriteStdinRequest {
        process_id: process_id.to_string(),
        chunk: line,
    })
    .await?;
    Ok(())
}

fn forward_inbound_host_request(
    ctx: &ExtensionContext<'_>,
    session_id: &str,
    message: &Value,
    id: &Value,
    method: &str,
) -> Result<Value, SidecarError> {
    let callback = AcpCallback::AcpHostRequestCallback(AcpHostRequestCallback {
        session_id: session_id.to_string(),
        request: serde_json::to_string(message).map_err(|error| {
            SidecarError::InvalidState(format!("failed to serialize ACP host request: {error}"))
        })?,
    });
    let response = ctx.invoke_callback(encode_callback(callback)?, Duration::from_secs(120))?;
    let response: AcpCallbackResponse = serde_bare::from_slice(&response).map_err(|error| {
        SidecarError::InvalidState(format!("invalid ACP host request response: {error}"))
    })?;
    let AcpCallbackResponse::AcpHostRequestCallbackResponse(response) = response else {
        return Ok(method_not_found_response(id.clone(), method));
    };
    let Some(response) = response.response else {
        return Ok(method_not_found_response(id.clone(), method));
    };
    let response = parse_json_text(&response, "ACP host request response")?;
    if response.get("id") != Some(id) {
        return Err(SidecarError::InvalidState(format!(
            "ACP host request response id {} did not match request id {}",
            json_rpc_id_label(response.get("id")),
            json_rpc_id_label(Some(id))
        )));
    }
    Ok(response)
}

fn method_not_found_response(id: Value, method: &str) -> Value {
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

fn permission_result(reply: &str, params: &Value) -> Value {
    let option_id = match resolve_permission_option_id(params, reply) {
        Some(option_id) => option_id,
        None => match reply {
            "always" | "allow_always" => String::from("allow_always"),
            "once" | "allow_once" => String::from("allow_once"),
            "reject" | "reject_once" => String::from("reject_once"),
            _ => return json!({ "outcome": { "outcome": "cancelled" } }),
        },
    };
    json!({ "outcome": { "outcome": "selected", "optionId": option_id } })
}

fn resolve_permission_option_id(params: &Value, reply: &str) -> Option<String> {
    let targets = match reply {
        "always" | "allow_always" => (&["always", "allow_always"][..], "allow_always"),
        "once" | "allow_once" => (&["once", "allow_once"][..], "allow_once"),
        "reject" | "reject_once" => (&["reject", "reject_once"][..], "reject_once"),
        _ => return None,
    };
    let options = params.get("options")?.as_array()?;
    let matched = options.iter().find(|option| {
        let option_id_matches = option
            .get("optionId")
            .and_then(Value::as_str)
            .map(|value| targets.0.contains(&value))
            .unwrap_or(false);
        let kind_matches = option
            .get("kind")
            .and_then(Value::as_str)
            .map(|value| value == targets.1)
            .unwrap_or(false);
        option_id_matches || kind_matches
    })?;
    matched
        .get("optionId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

struct JsonRpcExchange {
    response: Value,
    events: Vec<secure_exec_sidecar::wire::EventFrame>,
    notifications: Vec<String>,
}

fn response_result(response: Value, label: &str) -> Result<Map<String, Value>, SidecarError> {
    if let Some(error) = response.get("error").and_then(Value::as_object) {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown ACP error");
        // Include `error.data` when present — adapters (e.g. Pi) put the real
        // failure detail there while `message` stays a generic "Internal error".
        let data = error
            .get("data")
            .map(|d| format!(" (data: {d})"))
            .unwrap_or_default();
        return Err(SidecarError::InvalidState(format!(
            "{label} failed: {message}{data}"
        )));
    }
    response
        .get("result")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| SidecarError::InvalidState(format!("{label} response missing result")))
}

fn validate_initialize_result(
    result: &Map<String, Value>,
    requested_protocol_version: i32,
) -> Result<(), SidecarError> {
    let reported = result
        .get("protocolVersion")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            SidecarError::InvalidState(String::from(
                "ACP initialize response missing protocolVersion",
            ))
        })?;
    if reported != i64::from(requested_protocol_version) {
        return Err(SidecarError::ProtocolVersionMismatch(format!(
            "ACP initialize protocolVersion mismatch: requested {requested_protocol_version}, agent reported {reported}"
        )));
    }
    Ok(())
}

fn assemble_system_prompt(skip_base: bool, additional: Option<&str>) -> String {
    let mut parts = Vec::new();
    if !skip_base {
        parts.push(AGENTOS_SYSTEM_PROMPT.trim_end());
    }
    if let Some(additional) = additional {
        if !additional.is_empty() {
            parts.push(additional);
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("{}\n\n---", parts.join("\n\n"))
}

fn convert_runtime(runtime: AcpRuntimeKind) -> GuestRuntimeKind {
    match runtime {
        AcpRuntimeKind::JavaScript => GuestRuntimeKind::JavaScript,
        AcpRuntimeKind::Python => GuestRuntimeKind::Python,
        AcpRuntimeKind::WebAssembly => GuestRuntimeKind::WebAssembly,
    }
}

fn hash_to_btree(map: HashMap<String, String>) -> BTreeMap<String, String> {
    map.into_iter().collect()
}

/// The agent launch parameters resolved from a projected `/opt/agentos` package
/// manifest. The npm-agnostic client sends only the agent name; the sidecar owns
/// this name -> package -> entrypoint/env/launchArgs resolution.
struct ResolvedAgent {
    entrypoint: String,
    env: BTreeMap<String, String>,
    launch_args: Vec<String>,
}

/// The `agent` block of an `agentos-package.json`, parsed from its JSON value: a
/// non-empty `acpEntrypoint` plus optional launch env/args.
struct AgentPackageAgentBlock {
    acp_entrypoint: String,
    env: BTreeMap<String, String>,
    launch_args: Vec<String>,
}

/// Read the projected manifest at `/opt/agentos/<name>/current/agentos-package.json`
/// from the guest filesystem and return its `agent` block iff it exists and carries
/// a non-empty `acpEntrypoint`. A missing/unreadable/malformed manifest, a missing
/// `agent` block, or an empty `acpEntrypoint` all yield `None`.
async fn read_projected_agent_block(
    ctx: &mut ExtensionContext<'_>,
    agent_type: &str,
) -> Option<AgentPackageAgentBlock> {
    let result = ctx
        .guest_filesystem_call_wire(GuestFilesystemCallRequest {
            operation: GuestFilesystemOperation::ReadFile,
            path: format!("/opt/agentos/{agent_type}/current/agentos-package.json"),
            destination_path: None,
            target: None,
            content: None,
            encoding: None,
            recursive: false,
            max_depth: None,
            mode: None,
            uid: None,
            gid: None,
            atime_ms: None,
            mtime_ms: None,
            len: None,
            offset: None,
        })
        .await
        .ok()?;
    let text = result.content?;
    let manifest: Value = serde_json::from_str(&text).ok()?;
    let agent = manifest.get("agent")?;
    let acp_entrypoint = agent.get("acpEntrypoint")?.as_str()?.to_string();
    if acp_entrypoint.is_empty() {
        return None;
    }
    // Optional manifest launch env/args (defaults empty).
    let env = agent
        .get("env")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    let launch_args = agent
        .get("launchArgs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some(AgentPackageAgentBlock {
        acp_entrypoint,
        env,
        launch_args,
    })
}

/// Resolve an agent name to its launch parameters from the projected manifest. A
/// missing file, a missing `agent` block, or an empty `agent.acpEntrypoint` all map
/// to a single typed "unknown agent" error naming the agent and how to fix it.
async fn resolve_agent(
    ctx: &mut ExtensionContext<'_>,
    agent_type: &str,
) -> Result<ResolvedAgent, SidecarError> {
    match read_projected_agent_block(ctx, agent_type).await {
        Some(agent) => Ok(ResolvedAgent {
            entrypoint: format!("/opt/agentos/bin/{}", agent.acp_entrypoint),
            env: agent.env,
            launch_args: agent.launch_args,
        }),
        None => Err(SidecarError::InvalidState(format!(
            "unknown agent type \"{agent_type}\": no projected /opt/agentos/{agent_type} package \
             with an agent.acpEntrypoint — pass its package to AgentOs software"
        ))),
    }
}

/// Extract the owning connection id from an ownership scope. Every scope carries
/// a connection id, which is the tenant boundary secure-exec enforces; ACP
/// session ownership is keyed off this same connection id.
fn ownership_connection_id(ownership: &OwnershipScope) -> String {
    match ownership {
        OwnershipScope::ConnectionOwnership(inner) => inner.connection_id.clone(),
        OwnershipScope::SessionOwnership(inner) => inner.connection_id.clone(),
        OwnershipScope::VmOwnership(inner) => inner.connection_id.clone(),
    }
}

/// Remove every session in `sessions` owned by `connection_id`, returning the
/// adapter process ids of the dropped records. Split out from
/// [`AcpExtension::cleanup_sessions_for_connection`] as a pure helper so the
/// connection-teardown eviction is unit-testable without locking the mutex.
fn evict_sessions_for_connection(
    sessions: &mut BTreeMap<String, AcpSessionRecord>,
    connection_id: &str,
) -> Vec<String> {
    let owned = sessions
        .iter()
        .filter(|(_, session)| session.owner_connection_id == connection_id)
        .map(|(session_id, _)| session_id.clone())
        .collect::<Vec<_>>();
    owned
        .into_iter()
        .filter_map(|session_id| {
            sessions
                .remove(&session_id)
                .map(|session| session.process_id)
        })
        .collect()
}

/// Trim a retained `stdout_buffer` so it never exceeds
/// [`MAX_SESSION_STDOUT_BUFFER_BYTES`], keeping the most recent (trailing) bytes
/// — the partial line still being assembled — and truncating at a UTF-8 char
/// boundary so the `String` stays valid.
fn cap_stdout_buffer(buffer: &mut String) {
    if buffer.len() <= MAX_SESSION_STDOUT_BUFFER_BYTES {
        return;
    }
    let mut start = buffer.len() - MAX_SESSION_STDOUT_BUFFER_BYTES;
    while start < buffer.len() && !buffer.is_char_boundary(start) {
        start += 1;
    }
    *buffer = buffer.split_off(start);
}

/// True when `error` is the `send_json_rpc_request` failure raised because the
/// adapter process exited before answering — the in-crate signal that a session
/// has torn down and its record can be evicted.
fn is_adapter_exited_error(error: &SidecarError) -> bool {
    matches!(error, SidecarError::InvalidState(message) if message.contains(ADAPTER_EXITED_ERROR_MARKER))
}

/// True when `error` means the adapter process is gone: either the in-pump exit
/// observation (`is_adapter_exited_error`) or a secure-exec process-table
/// lookup failure from operating on an adapter that already exited — the lazy
/// observation of an idle-time crash (`ADAPTER_NO_ACTIVE_PROCESS_MARKER`).
fn is_adapter_gone_error(error: &SidecarError) -> bool {
    if is_adapter_exited_error(error) {
        return true;
    }
    matches!(error, SidecarError::InvalidState(message) if message.contains(ADAPTER_NO_ACTIVE_PROCESS_MARKER))
}

/// Extract the adapter exit code from an `ADAPTER_EXITED_ERROR_MARKER` error
/// message (`"... exited with code <code> before response ..."`). Returns
/// `None` for indirect observations (e.g. a stdin write that failed because
/// the process was already gone), where no exit code was seen.
fn adapter_exit_code_from_error(error: &SidecarError) -> Option<i32> {
    let SidecarError::InvalidState(message) = error else {
        return None;
    };
    let tail = &message[message.find(ADAPTER_EXITED_ERROR_MARKER)? + ADAPTER_EXITED_ERROR_MARKER.len()..];
    tail.split_whitespace().next()?.parse().ok()
}

/// Drive the restart handshake against a freshly respawned adapter:
/// `initialize` (re-probing capabilities, which cannot be trusted across a
/// relaunch) followed by the native `session/load`/`session/resume` for
/// `session_id`. There is deliberately no `session/new` fallback tier here:
/// a fallback would hand back a *different* adapter session id, which an
/// in-place restart cannot remap transparently — callers that need the
/// fallback tier go through the actor-level lazy resume instead. Replayed
/// load notifications are dropped: the client already observed this session's
/// history live, so re-forwarding the transcript replay would duplicate it.
async fn restart_handshake(
    ctx: &mut ExtensionContext<'_>,
    session_id: &str,
    agent_type: &str,
    restart: &AdapterRestartState,
    process_id: &str,
) -> Result<CreateSessionBootstrap, AdapterRestartError> {
    let mut stdout = String::new();
    let client_capabilities = parse_json_text(&restart.client_capabilities, "clientCapabilities")
        .map_err(AdapterRestartError::Failed)?;

    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": restart.protocol_version,
            "clientCapabilities": client_capabilities,
        },
    });
    let initialize_response = send_json_rpc_request(
        ctx,
        process_id,
        agent_type,
        initialize,
        1,
        INITIALIZE_TIMEOUT,
        &mut stdout,
        None,
    )
    .await
    .map_err(AdapterRestartError::Failed)?;
    let init_result = response_result(initialize_response.response, "ACP initialize")
        .map_err(AdapterRestartError::Failed)?;
    validate_initialize_result(&init_result, restart.protocol_version)
        .map_err(AdapterRestartError::Failed)?;
    let agent_capabilities = init_result.get("agentCapabilities").cloned();

    let Some(native_resume_method) = native_resume_method(agent_capabilities.as_ref()) else {
        return Err(AdapterRestartError::Unsupported);
    };
    let load = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": native_resume_method,
        "params": {
            "sessionId": session_id,
            "cwd": restart.cwd,
            "mcpServers": [],
        },
    });
    let load_response = send_json_rpc_request(
        ctx,
        process_id,
        agent_type,
        load,
        2,
        SESSION_NEW_TIMEOUT,
        &mut stdout,
        None,
    )
    .await
    .map_err(AdapterRestartError::Failed)?;
    let replayed_notifications =
        initialize_response.notifications.len() + load_response.notifications.len();
    if replayed_notifications > 0 {
        tracing::debug!(
            target: "agentos_sidecar::acp_extension",
            session_id,
            replayed_notifications,
            "dropped adapter-restart replay notifications",
        );
    }
    let load_result = response_result(
        load_response.response,
        &format!("ACP {native_resume_method}"),
    )
    .map_err(AdapterRestartError::Failed)?;

    build_resume_bootstrap(
        session_id.to_string(),
        &init_result,
        &load_result,
        agent_type,
        agent_capabilities.as_ref(),
        stdout,
        Vec::new(),
    )
    .map_err(AdapterRestartError::Failed)
}

fn parse_json_text(text: &str, label: &str) -> Result<Value, SidecarError> {
    serde_json::from_str(text)
        .map_err(|error| SidecarError::InvalidState(format!("invalid {label} JSON: {error}")))
}

fn to_record(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        other => Map::from_iter([(String::from("value"), other)]),
    }
}

fn session_id_from_session_result(session_result: &Map<String, Value>, fallback: &str) -> String {
    session_result
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|session_id| !session_id.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback.to_string())
}

/// Prepend the transcript-continuation preamble as a leading text content block
/// on a `session/prompt`'s `prompt` array. This is the fallback-tier mechanism
/// for handing the agent a pointer to the prior transcript: it rides in-band on
/// the user's first post-resume prompt (a single turn) rather than as a separate
/// RPC, so the agent sees one coherent prompt. A missing/non-array `prompt` is
/// initialized to a single-element array so the preamble is still delivered.
fn prepend_prompt_preamble(params: &mut Map<String, Value>, preamble: &str) {
    let block = json!({ "type": "text", "text": preamble });
    match params.get_mut("prompt").and_then(Value::as_array_mut) {
        Some(prompt) => prompt.insert(0, block),
        None => {
            params.insert(String::from("prompt"), Value::Array(vec![block]));
        }
    }
}

async fn kill_process_best_effort(ctx: &mut ExtensionContext<'_>, process_id: &str) {
    let _ = ctx
        .kill_process_wire(KillProcessRequest {
            process_id: process_id.to_owned(),
            signal: String::from("SIGTERM"),
        })
        .await;
}

/// Return the adapter native-resume RPC method from re-probed
/// `agentCapabilities`. Prefer ACP `loadSession`/`session/load`; fall back to the
/// non-standard `resume`/`session/resume` capability some adapters expose.
fn native_resume_method(agent_capabilities: Option<&Value>) -> Option<&'static str> {
    let caps = agent_capabilities.and_then(Value::as_object)?;
    if caps
        .get("loadSession")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Some("session/load");
    }
    if caps.get("resume").and_then(Value::as_bool).unwrap_or(false) {
        return Some("session/resume");
    }
    None
}

fn trace_acp_response(method: &str, response: &Value) {
    // Test-only diagnostics for compatibility regressions: the OpenCode resume
    // test captures the raw native-resume response before normalization so we
    // notice upstream error-shape changes. The env var is sidecar-process
    // trusted input, not guest-controlled runtime surface.
    let Ok(path) = std::env::var(ACP_TRACE_PATH_ENV) else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let payload = json!({
        "method": method,
        "response": response,
    });
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(file, "{payload}");
}

/// Normalize adapter-specific "no such session" errors from `session/load` into
/// the shared `unknown_session` discriminator used by the resume state machine.
///
/// OpenCode currently reports a missing session as JSON-RPC `-32603` with
/// `error.data.details == "NotFoundError"`: its ACP server converts thrown
/// non-`RequestError` exceptions into `internalError({ details: error.message })`,
/// and `Session.get` throws a `NotFoundError` whose message is the class name.
/// Convert exactly that shape into `error.data.kind = "unknown_session"` before
/// fallback matching. Do not broaden this to message substrings or all
/// `-32603`/`-32602` errors; malformed `session/load` must still propagate.
fn normalize_unknown_session_error(response: &mut Value) {
    let Some(error) = response.get_mut("error").and_then(Value::as_object_mut) else {
        return;
    };
    let code = error.get("code").and_then(Value::as_i64);
    let Some(data) = error.get_mut("data").and_then(Value::as_object_mut) else {
        return;
    };
    let details = data.get("details").and_then(Value::as_str);
    if code == Some(-32603) && details == Some("NotFoundError") {
        data.insert(
            String::from("kind"),
            Value::String(String::from("unknown_session")),
        );
    }
}

/// Detect a normalized adapter "no such session" error from `session/load` and
/// treat it as the `unknown_session` fallthrough sentinel (the durable store did
/// not survive the VM teardown). Only this triggers the Tier 2 fallback;
/// transport/timeout errors propagate.
///
/// The matcher is intentionally strict: by the time it runs, adapter-specific
/// shapes must already be normalized by [`normalize_unknown_session_error`].
/// This prevents a malformed load request or unrelated internal error from
/// silently resetting the user's context via a fresh fallback session.
fn is_unknown_session_error(response: &Value) -> bool {
    response
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("data"))
        .and_then(Value::as_object)
        .and_then(|d| d.get("kind"))
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "unknown_session")
}

/// Build the post-resume bootstrap state from `initialize` + `session/load|new`
/// results. Mirrors the config-option / modes / capabilities derivation at the
/// tail of `create_session_inner` so a resumed session hydrates identically to a
/// freshly created one.
fn build_resume_bootstrap(
    session_id: String,
    init_result: &Map<String, Value>,
    session_result: &Map<String, Value>,
    agent_type: &str,
    agent_capabilities: Option<&Value>,
    stdout_buffer: String,
    notifications: Vec<String>,
) -> Result<CreateSessionBootstrap, SidecarError> {
    let mut config_options = init_result
        .get("configOptions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(overrides) = session_result
        .get("configOptions")
        .and_then(Value::as_array)
    {
        config_options = overrides.clone();
    }
    if !config_options.iter().any(is_model_config_option) {
        config_options.extend(derive_config_options(agent_type, session_result));
    }

    Ok(CreateSessionBootstrap {
        session_id,
        modes: json_field(session_result, init_result, "modes")?,
        config_options: json_array_to_strings(config_options)?,
        agent_capabilities: json_optional_string(agent_capabilities)?,
        agent_info: json_optional_string(init_result.get("agentInfo"))?,
        stdout_buffer,
        notifications,
    })
}

fn append_stdout_chunk(
    buffer: &mut String,
    chunk: &[u8],
    max_line_bytes: usize,
) -> Result<Vec<String>, SidecarError> {
    buffer.push_str(&String::from_utf8_lossy(chunk));

    let mut lines = Vec::new();
    while let Some(index) = buffer.find('\n') {
        if index > max_line_bytes {
            return Err(SidecarError::InvalidState(format!(
                "ACP adapter emitted a line longer than {max_line_bytes} bytes"
            )));
        }
        let line = buffer[..index].trim().to_owned();
        *buffer = buffer[index + 1..].to_owned();
        if !line.is_empty() {
            lines.push(line);
        }
    }

    if buffer.len() > max_line_bytes {
        return Err(SidecarError::InvalidState(format!(
            "ACP adapter emitted a line longer than {max_line_bytes} bytes"
        )));
    }

    Ok(lines)
}

fn request_timeout(method: &str) -> Duration {
    match method {
        "session/prompt" => Duration::from_secs(600),
        "initialize" => INITIALIZE_TIMEOUT,
        "session/new" => SESSION_NEW_TIMEOUT,
        _ => Duration::from_secs(120),
    }
}

fn json_field(
    primary: &Map<String, Value>,
    fallback: &Map<String, Value>,
    key: &str,
) -> Result<Option<String>, SidecarError> {
    match primary.get(key).or_else(|| fallback.get(key)) {
        Some(value) => json_optional_string(Some(value)),
        None => Ok(None),
    }
}

fn json_optional_string(value: Option<&Value>) -> Result<Option<String>, SidecarError> {
    value
        .map(|value| {
            serde_json::to_string(value).map_err(|error| {
                SidecarError::InvalidState(format!("failed to serialize ACP JSON field: {error}"))
            })
        })
        .transpose()
}

fn json_array_to_strings(values: Vec<Value>) -> Result<Vec<String>, SidecarError> {
    values
        .iter()
        .map(|value| {
            serde_json::to_string(value).map_err(|error| {
                SidecarError::InvalidState(format!("failed to serialize ACP JSON field: {error}"))
            })
        })
        .collect()
}

fn is_model_config_option(value: &Value) -> bool {
    value.as_object().is_some_and(|map| {
        map.get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == "model")
            || map
                .get("category")
                .and_then(Value::as_str)
                .is_some_and(|category| category == "model")
    })
}

fn derive_config_options(agent_type: &str, session_result: &Map<String, Value>) -> Vec<Value> {
    let Some(models) = session_result.get("models").and_then(Value::as_object) else {
        return Vec::new();
    };
    let current_model_id = models
        .get("currentModelId")
        .and_then(Value::as_str)
        .map(String::from);
    let allowed_values = models
        .get("availableModels")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(Value::as_object)
                .filter_map(|model| {
                    let model_id = model.get("modelId")?.as_str()?;
                    let mut item = Map::from_iter([(
                        String::from("id"),
                        Value::String(String::from(model_id)),
                    )]);
                    if let Some(name) = model.get("name").and_then(Value::as_str) {
                        item.insert(String::from("label"), Value::String(String::from(name)));
                    }
                    Some(Value::Object(item))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if current_model_id.is_none() && allowed_values.is_empty() {
        return Vec::new();
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
            Value::String(current_model_id),
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

    vec![Value::Object(option)]
}

fn decode_request(payload: &[u8]) -> Result<AcpRequest, SidecarError> {
    serde_bare::from_slice(payload)
        .map_err(|error| SidecarError::InvalidState(format!("invalid ACP request: {error}")))
}

fn encode_response(response: AcpResponse) -> Result<Vec<u8>, SidecarError> {
    serde_bare::to_vec(&response)
        .map_err(|error| SidecarError::InvalidState(format!("invalid ACP response: {error}")))
}

fn encode_event(event: AcpEvent) -> Result<Vec<u8>, SidecarError> {
    serde_bare::to_vec(&event)
        .map_err(|error| SidecarError::InvalidState(format!("invalid ACP event: {error}")))
}

fn encode_callback(callback: AcpCallback) -> Result<Vec<u8>, SidecarError> {
    serde_bare::to_vec(&callback)
        .map_err(|error| SidecarError::InvalidState(format!("invalid ACP callback: {error}")))
}

fn error_response(error: SidecarError) -> AcpResponse {
    AcpResponse::AcpErrorResponse(AcpErrorResponse {
        code: error_code(&error),
        message: error.to_string(),
    })
}

fn error_code(error: &SidecarError) -> String {
    let code = match error {
        SidecarError::InvalidState(_) => "invalid_state",
        SidecarError::ProtocolVersionMismatch(_) => "protocol_version_mismatch",
        SidecarError::BridgeVersionMismatch(_) => "bridge_version_mismatch",
        SidecarError::Conflict(_) => "conflict",
        SidecarError::Unauthorized(_) => "unauthorized",
        SidecarError::Unsupported(_) => "unsupported",
        SidecarError::FrameTooLarge(_) => "frame_too_large",
        SidecarError::Kernel(_) => "kernel",
        SidecarError::Plugin(_) => "plugin",
        SidecarError::Execution(_) => "execution",
        SidecarError::Bridge(_) => "bridge",
        SidecarError::Io(_) => "io",
    };
    String::from(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_protocol::PROTOCOL_VERSION;

    #[test]
    fn acp_extension_uses_agent_os_namespace() {
        assert_eq!(AcpExtension::new().namespace(), ACP_EXTENSION_NAMESPACE);
    }

    #[test]
    fn adapter_gone_classifier_matches_both_observation_paths() {
        // In-pump observation: the exchange loop saw the ProcessExitedEvent.
        let exited = SidecarError::InvalidState(format!(
            "ACP adapter process acp-agent-3 {ADAPTER_EXITED_ERROR_MARKER} 7 before response id=4"
        ));
        assert!(is_adapter_gone_error(&exited));
        assert_eq!(adapter_exit_code_from_error(&exited), Some(7));

        // Lazy observation: a request write to an already-reaped adapter fails
        // with secure-exec's process-table error (the exact production shape:
        // "VM vm-5 has no active process agent-6"). No exit code is observed.
        let gone = SidecarError::InvalidState(String::from(
            "VM vm-5 has no active process agent-6",
        ));
        assert!(is_adapter_gone_error(&gone));
        assert_eq!(adapter_exit_code_from_error(&gone), None);

        // Transient failures must NOT classify as adapter-gone, or the session
        // would be restarted/evicted on retryable errors.
        let transient = SidecarError::InvalidState(String::from(
            "timed out waiting for ACP response id=4; sent session/cancel notification",
        ));
        assert!(!is_adapter_gone_error(&transient));
        assert_eq!(adapter_exit_code_from_error(&transient), None);
    }

    #[test]
    fn unknown_session_normalization_pins_opencode_shape() {
        let mut opencode = serde_json::json!({
            "error": { "code": -32603, "message": "Internal error", "data": { "details": "NotFoundError" } }
        });
        normalize_unknown_session_error(&mut opencode);
        assert_eq!(
            opencode.pointer("/error/data/kind").and_then(Value::as_str),
            Some("unknown_session")
        );
        assert!(is_unknown_session_error(&opencode));

        let mut malformed = serde_json::json!({
            "error": { "code": -32602, "message": "Invalid params",
                       "data": { "_errors": [], "sessionId": { "_errors": ["expected string"] } } }
        });
        normalize_unknown_session_error(&mut malformed);
        assert!(!is_unknown_session_error(&malformed));

        let mut other_internal = serde_json::json!({
            "error": { "code": -32603, "message": "Internal error", "data": { "details": "SomethingElse" } }
        });
        normalize_unknown_session_error(&mut other_internal);
        assert!(!is_unknown_session_error(&other_internal));
    }

    #[test]
    fn unknown_session_matcher_recognizes_normalized_sentinel_only() {
        assert!(is_unknown_session_error(&serde_json::json!({
            "error": { "code": -32000, "message": "x", "data": { "kind": "unknown_session" } }
        })));

        // Raw OpenCode shape must be normalized before matching.
        assert!(!is_unknown_session_error(&serde_json::json!({
            "error": { "code": -32603, "message": "Internal error", "data": { "details": "NotFoundError" } }
        })));
        assert!(!is_unknown_session_error(&serde_json::json!({
            "error": { "code": -32602, "message": "Invalid params",
                       "data": { "_errors": [], "sessionId": { "_errors": ["expected string"] } } }
        })));
        // Must NOT match: a -32603 internal error that is NOT a NotFoundError.
        assert!(!is_unknown_session_error(&serde_json::json!({
            "error": { "code": -32603, "message": "Internal error", "data": { "details": "SomethingElse" } }
        })));
        // Must NOT match: NotFoundError under a non--32603 code (different failure).
        assert!(!is_unknown_session_error(&serde_json::json!({
            "error": { "code": -32000, "data": { "details": "NotFoundError" } }
        })));
        // Must NOT match: a successful response or a bare transport error.
        assert!(!is_unknown_session_error(
            &serde_json::json!({ "result": {} })
        ));
        assert!(!is_unknown_session_error(&serde_json::json!({
            "error": { "code": -32603, "message": "Internal error" }
        })));
    }

    #[test]
    fn initialize_protocol_version_is_validated() {
        let result = Map::from_iter([(
            String::from("protocolVersion"),
            Value::Number(i64::from(PROTOCOL_VERSION).into()),
        )]);

        validate_initialize_result(&result, i32::from(PROTOCOL_VERSION))
            .expect("matching protocol version");
    }

    #[test]
    fn bounded_stdout_lines_preserve_partial_then_emit() {
        let mut buffer = String::new();
        let lines = append_stdout_chunk(&mut buffer, br#"{"a":"#, 8).expect("partial chunk");
        assert!(lines.is_empty());
        assert_eq!(buffer, r#"{"a":"#);

        let lines = append_stdout_chunk(&mut buffer, b"1}\n", 8).expect("complete line");
        assert_eq!(lines, vec![r#"{"a":1}"#]);
        assert!(buffer.is_empty());
    }

    #[test]
    fn bounded_stdout_lines_reject_complete_overlong_line() {
        let mut buffer = String::new();
        let error =
            append_stdout_chunk(&mut buffer, b"123456789\n", 8).expect_err("line exceeds cap");
        assert!(error
            .to_string()
            .contains("ACP adapter emitted a line longer than 8 bytes"));
    }

    #[test]
    fn bounded_stdout_lines_reject_unterminated_overlong_line() {
        let mut buffer = String::new();
        let error =
            append_stdout_chunk(&mut buffer, b"123456789", 8).expect_err("line exceeds cap");
        assert!(error
            .to_string()
            .contains("ACP adapter emitted a line longer than 8 bytes"));
    }

    #[test]
    fn session_cancel_notification_has_acp_shape() {
        assert_eq!(
            session_cancel_notification("adapter-session"),
            json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": {
                    "sessionId": "adapter-session",
                },
            })
        );
    }

    #[test]
    fn cancel_method_not_found_detection_accepts_error_data_or_message() {
        assert!(is_cancel_method_not_found(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "error": {
                "code": -32601,
                "message": "method not found",
                "data": { "method": "session/cancel" },
            },
        })));
        assert!(is_cancel_method_not_found(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "error": {
                "code": -32601,
                "message": "unknown method session/cancel",
            },
        })));
        assert!(!is_cancel_method_not_found(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "error": {
                "code": -32000,
                "message": "session/cancel failed",
            },
        })));
    }

    #[test]
    fn cancel_fallback_response_matches_legacy_shape() {
        assert_eq!(
            cancel_notification_fallback_response(Value::Number(4.into())),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "result": {
                    "cancelled": false,
                    "requested": true,
                    "via": "notification-fallback",
                },
            })
        );
    }

    #[test]
    fn request_timeout_uses_acp_method_overrides() {
        assert_eq!(request_timeout("initialize"), Duration::from_secs(10));
        assert_eq!(request_timeout("session/new"), Duration::from_secs(30));
        assert_eq!(request_timeout("session/prompt"), Duration::from_secs(600));
        assert_eq!(
            request_timeout("session/set_mode"),
            Duration::from_secs(120)
        );
    }

    #[test]
    fn model_config_option_detection_accepts_id_or_category() {
        assert!(is_model_config_option(&json!({
            "id": "model",
            "category": "provider",
        })));
        assert!(is_model_config_option(&json!({
            "id": "provider-model",
            "category": "model",
        })));
        assert!(!is_model_config_option(&json!({
            "id": "thought-level",
            "category": "thought_level",
        })));
    }

    #[test]
    fn session_new_session_id_falls_back_to_wrapper_id() {
        assert_eq!(
            session_id_from_session_result(
                &Map::from_iter([(String::from("sessionId"), json!("adapter-session"))]),
                "acp-agent-1",
            ),
            "adapter-session"
        );
        assert_eq!(
            session_id_from_session_result(&Map::new(), "acp-agent-1"),
            "acp-agent-1"
        );
        assert_eq!(
            session_id_from_session_result(
                &Map::from_iter([(String::from("sessionId"), json!(""))]),
                "acp-agent-1",
            ),
            "acp-agent-1"
        );
    }

    #[test]
    fn timeout_error_response_includes_structured_diagnostics() {
        let response = timeout_error_response(
            7,
            "session/prompt",
            Duration::from_secs(120),
            "acp-agent-1",
            "sent session/cancel notification",
            vec![
                String::from("sent request session/prompt id=7"),
                String::from("received notification session/update"),
            ],
        );

        assert_eq!(response["jsonrpc"], json!("2.0"));
        assert_eq!(response["id"], json!(7));
        assert_eq!(response["error"]["code"], json!(-32000));
        assert!(response["error"]["message"]
            .as_str()
            .expect("message")
            .contains("ACP request session/prompt (id=7) timed out after 120000ms"));
        assert_eq!(response["error"]["data"]["kind"], json!("acp_timeout"));
        assert_eq!(response["error"]["data"]["method"], json!("session/prompt"));
        assert_eq!(response["error"]["data"]["id"], json!(7));
        assert_eq!(response["error"]["data"]["timeoutMs"], json!(120000));
        assert_eq!(
            response["error"]["data"]["transportState"],
            json!("sent session/cancel notification")
        );
        assert_eq!(
            response["error"]["data"]["recentActivity"],
            json!([
                "sent request session/prompt id=7",
                "received notification session/update"
            ])
        );
    }

    /// Drive a future that only awaits uncontended in-memory state (e.g. a free
    /// `tokio::sync::Mutex`) to completion without a runtime: such a future is
    /// `Ready` on its first poll. Panics if it parks, which would mean it touched
    /// real async I/O the unit test cannot service. Lets the sync test harness
    /// exercise the real async `Extension::on_dispose` wiring.
    fn poll_uncontended<F: std::future::Future>(future: F) -> F::Output {
        use std::task::{Context, Poll};
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(waker);
        let mut future = std::pin::pin!(future);
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => output,
            Poll::Pending => {
                panic!("future parked; expected uncontended in-memory completion")
            }
        }
    }

    fn test_session_record(session_id: &str, owner_connection_id: &str) -> AcpSessionRecord {
        AcpSessionRecord {
            session_id: session_id.to_string(),
            owner_connection_id: owner_connection_id.to_string(),
            agent_type: String::from("pi"),
            process_id: format!("acp-agent-{session_id}"),
            pid: None,
            modes: None,
            config_options: Vec::new(),
            agent_capabilities: None,
            agent_info: None,
            stdout_buffer: String::new(),
            next_request_id: 3,
            closed: false,
            exit_code: None,
            pending_preamble: None,
            restart: AdapterRestartState {
                runtime: AcpRuntimeKind::JavaScript,
                entrypoint: String::from("/adapter.mjs"),
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: String::from("/"),
                protocol_version: 1,
                client_capabilities: String::from("{}"),
                count: 0,
            },
        }
    }

    #[test]
    fn connection_teardown_evicts_only_that_connections_sessions() {
        // Regression: sessions were removed ONLY by the explicit close_session
        // RPC, so a connection that disconnected without closing its sessions
        // leaked every record (incl. its stdout_buffer) forever. The
        // connection-teardown path must drop exactly that connection's sessions.
        let ext = AcpExtension::new();
        {
            let mut sessions = ext.sessions.try_lock().expect("uncontended sessions lock");
            sessions.insert(String::from("s1"), test_session_record("s1", "conn-a"));
            sessions.insert(String::from("s2"), test_session_record("s2", "conn-a"));
            sessions.insert(String::from("s3"), test_session_record("s3", "conn-b"));
        }

        let reaped = {
            let mut sessions = ext.sessions.try_lock().expect("uncontended sessions lock");
            evict_sessions_for_connection(&mut sessions, "conn-a")
        };

        assert_eq!(reaped.len(), 2, "both conn-a adapter processes reaped");
        let sessions = ext.sessions.try_lock().expect("uncontended sessions lock");
        assert!(!sessions.contains_key("s1"), "conn-a session evicted");
        assert!(!sessions.contains_key("s2"), "conn-a session evicted");
        assert!(
            sessions.contains_key("s3"),
            "other connection's session must survive its peer's teardown"
        );
    }

    #[test]
    fn on_dispose_clears_every_session_record() {
        // H4 (the actually-wired ACP-session leak fix): on extension/sidecar
        // teardown `Extension::on_dispose` must drop EVERY remaining session
        // record so no `stdout_buffer` survives the host process — not just the
        // records for one connection.
        let ext = AcpExtension::new();
        {
            let mut sessions = ext.sessions.try_lock().expect("uncontended sessions lock");
            sessions.insert(String::from("s1"), test_session_record("s1", "conn-a"));
            sessions.insert(String::from("s2"), test_session_record("s2", "conn-b"));
            sessions.insert(String::from("s3"), test_session_record("s3", "conn-c"));
        }

        // Drive the real wired async `on_dispose` impl; it only awaits the
        // uncontended `sessions` mutex, so it completes on the first poll.
        poll_uncontended(ext.on_dispose()).expect("on_dispose succeeds");

        let sessions = ext.sessions.try_lock().expect("uncontended sessions lock");
        assert!(
            sessions.is_empty(),
            "on_dispose must clear the entire sessions map"
        );
    }

    #[test]
    fn capped_stdout_buffer_never_exceeds_limit() {
        let mut buffer = "x".repeat(MAX_SESSION_STDOUT_BUFFER_BYTES + 4096);
        cap_stdout_buffer(&mut buffer);
        assert!(
            buffer.len() <= MAX_SESSION_STDOUT_BUFFER_BYTES,
            "retained stdout_buffer must be bounded"
        );

        // A buffer already within the cap is left untouched.
        let mut small = String::from("partial-line");
        cap_stdout_buffer(&mut small);
        assert_eq!(small, "partial-line");
    }

    #[test]
    fn capped_stdout_buffer_truncates_on_utf8_char_boundary() {
        // All-ASCII inputs never exercise the `is_char_boundary` adjustment loop.
        // A buffer of multi-byte chars forces the naive split point off a char
        // boundary, so the loop must advance it; the result must stay valid UTF-8
        // (no panic / no split char) and keep the most recent trailing bytes.
        const CHAR: char = '€'; // 3 bytes in UTF-8
        let original = CHAR.to_string().repeat(MAX_SESSION_STDOUT_BUFFER_BYTES); // 3 * MAX bytes, far over the cap
        let mut buffer = original.clone();
        cap_stdout_buffer(&mut buffer);

        assert!(
            buffer.len() <= MAX_SESSION_STDOUT_BUFFER_BYTES,
            "capped multi-byte buffer must be bounded"
        );
        // No char was split: a homogeneous 3-byte-char buffer can only have a
        // length that is a multiple of 3 if every retained char is intact.
        assert_eq!(
            buffer.len() % CHAR.len_utf8(),
            0,
            "cap must truncate on a UTF-8 char boundary, not mid-char"
        );
        assert!(
            std::str::from_utf8(buffer.as_bytes()).is_ok(),
            "capped buffer must remain valid UTF-8"
        );
        assert!(
            buffer.chars().all(|c| c == CHAR),
            "every retained char survived intact"
        );
        // The trailing (most recent) bytes are kept, not the head.
        assert!(
            !buffer.is_empty() && original.ends_with(&buffer),
            "cap keeps the trailing partial-line bytes"
        );
    }

    #[test]
    fn adapter_exit_error_is_recognized_for_eviction() {
        // Build the EXACT error string the `ProcessExitedEvent` arm of
        // `session_request` emits (it embeds `ADAPTER_EXITED_ERROR_MARKER`
        // directly), so a change to the producer's wording that drops the marker
        // would break this test instead of silently disabling session eviction.
        let process_id = "acp-agent-1";
        let exit_code = 1;
        let response_id = 3;
        let exited = SidecarError::InvalidState(format!(
            "ACP adapter process {process_id} {ADAPTER_EXITED_ERROR_MARKER} {exit_code} before response id={response_id}",
        ));
        assert!(
            is_adapter_exited_error(&exited),
            "the real adapter-exit error must trigger session eviction"
        );

        // Transient failures must NOT be treated as adapter exit (would evict a
        // session that is still alive).
        let timed_out =
            SidecarError::InvalidState(String::from("timed out waiting for ACP response id=3"));
        assert!(!is_adapter_exited_error(&timed_out));
        let broken_pipe = SidecarError::InvalidState(String::from(
            "failed to write ACP request to adapter stdin: broken pipe",
        ));
        assert!(!is_adapter_exited_error(&broken_pipe));
    }
}
