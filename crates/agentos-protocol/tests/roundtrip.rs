use agentos_protocol::generated::v1::{
    AcpCallback, AcpCallbackResponse, AcpCreateSessionRequest, AcpPermissionCallback,
    AcpPermissionCallbackResponse, AcpRequest, AcpResponse, AcpRuntimeKind,
    AcpSessionCreatedResponse, AcpSessionResumedResponse,
};

#[test]
fn acp_protocol_round_trips_permission_callback_and_response() {
    let callback = AcpCallback::AcpPermissionCallback(AcpPermissionCallback {
        session_id: String::from("session-1"),
        permission_id: String::from("permission-1"),
        params: String::from(r#"{"reason":"approve"}"#),
        cleanup_after_ms: 125_000,
    });
    let encoded = serde_bare::to_vec(&callback).expect("encode permission callback");
    let decoded: AcpCallback =
        serde_bare::from_slice(&encoded).expect("decode permission callback");
    assert_eq!(decoded, callback);

    let response =
        AcpCallbackResponse::AcpPermissionCallbackResponse(AcpPermissionCallbackResponse {
            permission_id: String::from("permission-1"),
            reply: Some(String::from("once")),
        });
    let encoded = serde_bare::to_vec(&response).expect("encode permission callback response");
    let decoded: AcpCallbackResponse =
        serde_bare::from_slice(&encoded).expect("decode permission callback response");
    assert_eq!(decoded, response);
}
use agentos_protocol::{
    read_only_config_message, select_config_by_category, AcpPromptTextAccumulator,
    ResolvedAcpCreateSessionRequest, DEFAULT_ACP_CLIENT_CAPABILITIES, DEFAULT_ACP_CWD,
    DEFAULT_ACP_MCP_SERVERS, DEFAULT_ACP_PROTOCOL_VERSION,
};

#[test]
fn acp_protocol_round_trips_create_session() {
    let request = AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
        agent_type: String::from("codex"),
        runtime: Some(AcpRuntimeKind::JavaScript),
        cwd: Some(String::from("/home/agentos")),
        args: Some(vec![String::from("--model"), String::from("gpt-5")]),
        env: Some(
            [(String::from("AGENTOS_KEEP_STDIN_OPEN"), String::from("1"))]
                .into_iter()
                .collect(),
        ),
        protocol_version: Some(1),
        client_capabilities: Some(String::from("{}")),
        mcp_servers: Some(String::from("{}")),
        skip_os_instructions: Some(false),
        additional_instructions: Some(String::from("be concise")),
    });

    let encoded = serde_bare::to_vec(&request).expect("encode acp request");
    let decoded: AcpRequest = serde_bare::from_slice(&encoded).expect("decode acp request");
    assert_eq!(decoded, request);
}

#[test]
fn config_category_selection_is_sidecar_owned() {
    let options = vec![String::from(
        r#"{"id":"model-picker","category":"model","readOnly":true}"#,
    )];
    let selected = select_config_by_category(&options, "model").expect("select model");
    assert_eq!(selected.config_id, "model-picker");
    assert!(selected.read_only);

    let missing = select_config_by_category(&options, "thought_level").expect("select fallback");
    assert_eq!(missing.config_id, "thought_level");
    assert!(!missing.read_only);
    assert!(
        read_only_config_message("opencode", "model").contains("configured before createSession()")
    );
}

#[test]
fn prompt_text_accumulation_is_sidecar_shared() {
    let mut capture = AcpPromptTextAccumulator::default();
    capture
        .push_notification(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "text": "hello" }
                }
            }
        }))
        .expect("capture chunk");
    capture
        .push_notification(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": { "update": { "sessionUpdate": "current_mode_update" } }
        }))
        .expect("ignore non-chunk");
    assert_eq!(capture.into_text(), "hello");
}

#[test]
fn omitted_session_fields_resolve_from_shared_sidecar_defaults() {
    let resolved = ResolvedAcpCreateSessionRequest::from(AcpCreateSessionRequest {
        agent_type: String::from("pi"),
        runtime: None,
        cwd: None,
        args: None,
        env: None,
        protocol_version: None,
        client_capabilities: None,
        mcp_servers: None,
        skip_os_instructions: None,
        additional_instructions: None,
    });

    assert_eq!(resolved.runtime, AcpRuntimeKind::JavaScript);
    assert_eq!(resolved.cwd, DEFAULT_ACP_CWD);
    assert!(resolved.args.is_empty());
    assert!(resolved.env.is_empty());
    assert_eq!(resolved.protocol_version, DEFAULT_ACP_PROTOCOL_VERSION);
    assert_eq!(
        resolved.client_capabilities,
        DEFAULT_ACP_CLIENT_CAPABILITIES
    );
    assert_eq!(resolved.mcp_servers, DEFAULT_ACP_MCP_SERVERS);
    assert!(!resolved.skip_os_instructions);
}

#[test]
fn acp_protocol_round_trips_session_created_response() {
    let response = AcpResponse::AcpSessionCreatedResponse(AcpSessionCreatedResponse {
        session_id: String::from("acp-session-1"),
        agent_type: String::from("codex"),
        process_id: String::from("acp-agent-1"),
        pid: Some(42),
        modes: Some(String::from(r#"{"currentModeId":"default"}"#)),
        config_options: vec![String::from(r#"{"id":"model","values":["gpt-5"]}"#)],
        agent_capabilities: Some(String::from("{}")),
        agent_info: None,
    });

    let encoded = serde_bare::to_vec(&response).expect("encode acp response");
    let decoded: AcpResponse = serde_bare::from_slice(&encoded).expect("decode acp response");
    assert_eq!(decoded, response);
}

#[test]
fn acp_protocol_round_trips_session_resumed_route_identity() {
    let response = AcpResponse::AcpSessionResumedResponse(AcpSessionResumedResponse {
        session_id: String::from("acp-session-2"),
        mode: String::from("fallback"),
        agent_type: String::from("pi"),
        process_id: String::from("acp-agent-2"),
        pid: Some(84),
    });

    let encoded = serde_bare::to_vec(&response).expect("encode resumed response");
    let decoded: AcpResponse = serde_bare::from_slice(&encoded).expect("decode resumed response");
    assert_eq!(decoded, response);
}
