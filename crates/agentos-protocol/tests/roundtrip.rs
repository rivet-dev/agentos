use agentos_protocol::generated::v1::{
    AcpCreateSessionRequest, AcpRequest, AcpResponse, AcpRuntimeKind, AcpSessionCreatedResponse,
};

#[test]
fn acp_protocol_round_trips_create_session() {
    let request = AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
        agent_type: String::from("codex"),
        runtime: AcpRuntimeKind::JavaScript,
        adapter_entrypoint: String::from("/root/node_modules/agent/adapter.mjs"),
        cwd: String::from("/home/agentos"),
        args: vec![String::from("--model"), String::from("gpt-5")],
        env: [(
            String::from("SECURE_EXEC_KEEP_STDIN_OPEN"),
            String::from("1"),
        )]
        .into_iter()
        .collect(),
        protocol_version: 1,
        client_capabilities: String::from("{}"),
        mcp_servers: String::from("{}"),
        skip_os_instructions: false,
        additional_instructions: Some(String::from("be concise")),
    });

    let encoded = serde_bare::to_vec(&request).expect("encode acp request");
    let decoded: AcpRequest = serde_bare::from_slice(&encoded).expect("decode acp request");
    assert_eq!(decoded, request);
}

#[test]
fn acp_protocol_round_trips_session_created_response() {
    let response = AcpResponse::AcpSessionCreatedResponse(AcpSessionCreatedResponse {
        session_id: String::from("acp-session-1"),
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
