use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use agent_os_protocol::generated::v1::{
    AcpCallback, AcpCallbackResponse, AcpCloseSessionRequest, AcpCreateSessionRequest,
    AcpErrorResponse, AcpEvent, AcpGetSessionStateRequest, AcpHostRequestCallback,
    AcpPermissionCallback, AcpRequest, AcpResponse, AcpRuntimeKind, AcpSessionClosedResponse,
    AcpSessionCreatedResponse, AcpSessionEvent, AcpSessionRequest, AcpSessionStateResponse,
};
use agent_os_protocol::ACP_EXTENSION_NAMESPACE;
use secure_exec_sidecar::limits::DEFAULT_ACP_MAX_READ_LINE_BYTES;
use secure_exec_sidecar::wire::{
    CloseStdinRequest, EventPayload, ExecuteRequest, GuestFilesystemCallRequest,
    GuestFilesystemOperation, GuestRuntimeKind, KillProcessRequest, StreamChannel,
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
const ACP_CANCEL_METHOD: &str = "session/cancel";
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
const AGENTOS_SYSTEM_PROMPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/core/fixtures/AGENTOS_SYSTEM_PROMPT.md"
));

#[derive(Debug, Default)]
pub struct AcpExtension {
    next_process_id: AtomicUsize,
    sessions: Mutex<BTreeMap<String, AcpSessionRecord>>,
}

#[derive(Debug, Clone)]
struct AcpSessionRecord {
    session_id: String,
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
        let request = decode_request(payload)?;
        let response = match request {
            AcpRequest::AcpCreateSessionRequest(request) => self.create_session(ctx, request).await,
            AcpRequest::AcpGetSessionStateRequest(request) => {
                AcpHandlerOutput::response(self.get_session_state(request).await)
            }
            AcpRequest::AcpCloseSessionRequest(request) => {
                AcpHandlerOutput::response(self.close_session(ctx, request).await)
            }
            AcpRequest::AcpSessionRequest(request) => self.session_request(ctx, request).await,
        };
        let payload = encode_response(response.response.unwrap_or_else(error_response))?;
        ExtensionResponse::with_wire_events(payload, response.events)
    }

    async fn create_session(
        &self,
        mut ctx: ExtensionContext<'_>,
        request: AcpCreateSessionRequest,
    ) -> AcpHandlerOutput {
        let process_id = self.allocate_process_id("acp-agent");
        let mut args = request.args.clone();
        let mut env = hash_to_btree(request.env.clone());
        env.insert(
            String::from("SECURE_EXEC_KEEP_STDIN_OPEN"),
            String::from("1"),
        );
        if let Err(error) = self
            .apply_prompt_injection(&mut ctx, &request, &mut args, &mut env)
            .await
        {
            return AcpHandlerOutput::response(Err(error));
        }

        let started = match ctx
            .spawn_process_wire(ExecuteRequest {
                process_id: process_id.clone(),
                command: None,
                runtime: Some(convert_runtime(request.runtime.clone())),
                entrypoint: Some(request.adapter_entrypoint.clone()),
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

        let bootstrap = self
            .create_session_inner(&mut ctx, &request, &process_id)
            .await;
        if bootstrap.is_err() {
            let _ = ctx
                .kill_process_wire(KillProcessRequest {
                    process_id: process_id.clone(),
                    signal: String::from("SIGTERM"),
                })
                .await;
        }
        let bootstrap = match bootstrap {
            Ok(bootstrap) => bootstrap,
            Err(error) => return AcpHandlerOutput::response(Err(error)),
        };

        let session = AcpSessionRecord {
            session_id: bootstrap.session_id.clone(),
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
        };
        if let Err(error) = ctx
            .bind_process_to_session(&session.session_id, &process_id)
            .await
        {
            return AcpHandlerOutput::response(Err(error));
        }
        self.sessions
            .lock()
            .await
            .insert(session.session_id.clone(), session.clone());

        let mut events = Vec::new();
        for notification in bootstrap.notifications {
            let event = match encode_event(AcpEvent::AcpSessionEvent(AcpSessionEvent {
                session_id: session.session_id.clone(),
                notification,
            })) {
                Ok(event) => event,
                Err(error) => return AcpHandlerOutput::response(Err(error)),
            };
            match ctx.ext_event_wire(event) {
                Ok(event) => events.push(event),
                Err(error) => return AcpHandlerOutput::response(Err(error)),
            }
        }

        AcpHandlerOutput {
            response: Ok(AcpResponse::AcpSessionCreatedResponse(
                session.created_response(),
            )),
            events,
        }
    }

    async fn create_session_inner(
        &self,
        ctx: &mut ExtensionContext<'_>,
        request: &AcpCreateSessionRequest,
        process_id: &str,
    ) -> Result<CreateSessionBootstrap, SidecarError> {
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
            "opencode" => {
                if !env.contains_key("OPENCODE_CONTEXTPATHS") {
                    ctx.guest_filesystem_call_wire(GuestFilesystemCallRequest {
                        operation: GuestFilesystemOperation::WriteFile,
                        path: String::from(OPENCODE_SYSTEM_PROMPT_PATH),
                        destination_path: None,
                        target: None,
                        content: Some(prompt),
                        encoding: None,
                        recursive: false,
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
            }
            _ => {}
        }

        Ok(())
    }

    async fn get_session_state(
        &self,
        request: AcpGetSessionStateRequest,
    ) -> Result<AcpResponse, SidecarError> {
        let sessions = self.sessions.lock().await;
        let session = sessions.get(&request.session_id).ok_or_else(|| {
            SidecarError::InvalidState(format!("unknown ACP session {}", request.session_id))
        })?;
        Ok(AcpResponse::AcpSessionStateResponse(
            session.state_response(),
        ))
    }

    async fn close_session(
        &self,
        mut ctx: ExtensionContext<'_>,
        request: AcpCloseSessionRequest,
    ) -> Result<AcpResponse, SidecarError> {
        let session = self.sessions.lock().await.remove(&request.session_id);
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

        let (process_id, rpc_id, mut stdout_buffer) = {
            let mut sessions = self.sessions.lock().await;
            let Some(session) = sessions.get_mut(&request.session_id) else {
                return AcpHandlerOutput::response(Err(SidecarError::InvalidState(format!(
                    "unknown ACP session {}",
                    request.session_id
                ))));
            };
            let rpc_id = session.next_request_id;
            session.next_request_id += 1;
            (
                session.process_id.clone(),
                rpc_id,
                std::mem::take(&mut session.stdout_buffer),
            )
        };
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
            outbound,
            rpc_id,
            timeout,
            &mut stdout_buffer,
            Some(&request.session_id),
        )
        .await
        {
            Ok(exchange) => exchange,
            Err(error) => return AcpHandlerOutput::response(Err(error)),
        };

        if let Some(session) = self.sessions.lock().await.get_mut(&request.session_id) {
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
                        Ok(event) => exchange.events.push(event),
                        Err(error) => return AcpHandlerOutput::response(Err(error)),
                    }
                }
                Ok(None) => {}
                Err(error) => return AcpHandlerOutput::response(Err(error)),
            }
        }

        AcpHandlerOutput {
            response: Ok(AcpResponse::AcpSessionRpcResponse(
                agent_os_protocol::generated::v1::AcpSessionRpcResponse {
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

    fn allocate_process_id(&self, prefix: &str) -> String {
        let id = self.next_process_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("{prefix}-{id}")
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
                    | AcpRequest::AcpSessionRequest(_) => None,
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

async fn send_json_rpc_request(
    ctx: &mut ExtensionContext<'_>,
    process_id: &str,
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
            .poll_event_wire(remaining.min(Duration::from_millis(250)))
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
                            events.push(ctx.ext_event_wire(encode_event(
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
                            )?)?);
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
                tracing::error!(
                    process_id,
                    stderr = %String::from_utf8_lossy(&output.chunk),
                    "ACP adapter stderr"
                );
            }
            EventPayload::ProcessExitedEvent(exited) if exited.process_id == process_id => {
                return Err(SidecarError::InvalidState(format!(
                    "ACP adapter process {process_id} exited with code {} before response id={response_id}",
                    exited.exit_code
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
        agent_os_protocol::generated::v1::AcpSessionRpcResponse {
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
    use agent_os_protocol::PROTOCOL_VERSION;

    #[test]
    fn acp_extension_uses_agent_os_namespace() {
        assert_eq!(AcpExtension::new().namespace(), ACP_EXTENSION_NAMESPACE);
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
}
