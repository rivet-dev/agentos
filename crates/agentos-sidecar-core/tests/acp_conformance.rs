//! Behavioral conformance between the ACP core's blocking (native strategy) and resumable
//! (browser strategy) public dispatch APIs.
//!
//! Process ids and pids are transport details and are normalized. Session responses, adapter
//! JSON-RPC, prompt text, forwarded request shapes, and authoritative session state are compared.

use std::collections::{HashMap, VecDeque};

use agentos_protocol::generated::v1::{
    AcpCreateSessionRequest, AcpDeliverAgentOutputRequest, AcpEvent, AcpGetSessionStateRequest,
    AcpListAgentsRequest, AcpRequest, AcpResponse, AcpResumeSessionRequest, AcpRuntimeKind,
    AcpSessionRequest, AcpSetSessionConfigRequest,
};
use agentos_sidecar_core::host::{
    AgentOutput, ProjectedAgentLaunch, SpawnAgentRequest, SpawnedAgent,
};
use agentos_sidecar_core::{AcpCore, AcpCoreError, AcpHost};
use serde_json::{json, Value};

const OWNER: &str = "conformance-owner";
#[derive(Clone, Copy, Debug)]
enum Strategy {
    Blocking,
    Resumable,
}

#[derive(Default)]
struct ScriptedHost {
    output: VecDeque<AgentOutput>,
    writes: Vec<Value>,
    spawns: Vec<SpawnAgentRequest>,
    bindings: Vec<(String, String)>,
    killed: Vec<(String, String)>,
    host_tool_reference: String,
    projected_agents: Vec<ProjectedAgentLaunch>,
    projected_agent_error: Option<AcpCoreError>,
}

impl ScriptedHost {
    fn enqueue(&mut self, chunks: &[&str]) {
        self.output.extend(chunks.iter().map(|chunk| {
            let mut bytes = chunk.as_bytes().to_vec();
            if !bytes.ends_with(b"\n") {
                bytes.push(b'\n');
            }
            AgentOutput::Stdout(bytes)
        }));
    }
}

impl AcpHost for ScriptedHost {
    fn resolve_projected_agent(
        &mut self,
        id: &str,
    ) -> Result<Option<ProjectedAgentLaunch>, AcpCoreError> {
        if let Some(error) = self.projected_agent_error.clone() {
            return Err(error);
        }
        Ok(self
            .projected_agents
            .iter()
            .find(|agent| agent.id == id)
            .cloned())
    }

    fn list_projected_agents(&mut self) -> Result<Vec<ProjectedAgentLaunch>, AcpCoreError> {
        if let Some(error) = self.projected_agent_error.clone() {
            return Err(error);
        }
        Ok(self.projected_agents.clone())
    }

    fn registered_host_tool_reference(&mut self) -> Result<String, AcpCoreError> {
        Ok(self.host_tool_reference.clone())
    }

    fn spawn_agent(&mut self, request: SpawnAgentRequest) -> Result<SpawnedAgent, AcpCoreError> {
        let process_id = request.process_id.clone();
        self.spawns.push(request);
        Ok(SpawnedAgent {
            process_id,
            pid: Some(42),
        })
    }

    fn bind_session(&mut self, session_id: &str, process_id: &str) -> Result<(), AcpCoreError> {
        self.bindings
            .push((session_id.to_string(), process_id.to_string()));
        Ok(())
    }

    fn write_stdin(&mut self, _process_id: &str, chunk: &[u8]) -> Result<(), AcpCoreError> {
        let text = std::str::from_utf8(chunk)
            .map_err(|error| AcpCoreError::InvalidState(format!("fixture stdin UTF-8: {error}")))?;
        let message = serde_json::from_str(text.trim()).map_err(|error| {
            AcpCoreError::InvalidState(format!("fixture stdin JSON {text:?}: {error}"))
        })?;
        self.writes.push(message);
        Ok(())
    }

    fn close_stdin(&mut self, _process_id: &str) -> Result<(), AcpCoreError> {
        Ok(())
    }

    fn poll_output(&mut self, _process_id: &str) -> Result<Option<AgentOutput>, AcpCoreError> {
        Ok(self.output.pop_front())
    }

    fn kill_agent(&mut self, process_id: &str, signal: &str) -> Result<(), AcpCoreError> {
        self.killed
            .push((process_id.to_string(), signal.to_string()));
        Ok(())
    }

    fn wait_for_exit(
        &mut self,
        _process_id: &str,
        _timeout_ms: u64,
    ) -> Result<Option<i32>, AcpCoreError> {
        Ok(Some(0))
    }

    fn write_file(&mut self, _path: &str, _contents: &[u8]) -> Result<(), AcpCoreError> {
        Ok(())
    }

    fn read_file(&mut self, _path: &str) -> Result<Vec<u8>, AcpCoreError> {
        Ok(Vec::new())
    }

    fn now_ms(&self) -> u64 {
        0
    }
}

struct Runner {
    strategy: Strategy,
    core: AcpCore,
    host: ScriptedHost,
    last_events: Vec<Value>,
}

impl Runner {
    fn new(strategy: Strategy) -> Self {
        Self {
            strategy,
            core: AcpCore::new(),
            host: ScriptedHost {
                // Deliberately unsorted: list_agents must impose deterministic id
                // order instead of exposing host map/container order.
                projected_agents: vec![fixture_agent("pi"), fixture_agent("echo")],
                ..ScriptedHost::default()
            },
            last_events: Vec::new(),
        }
    }

    fn request(&mut self, request: AcpRequest, output: &[&str]) -> AcpResponse {
        let uses_resumable_exchange = matches!(
            request,
            AcpRequest::AcpCreateSessionRequest(_)
                | AcpRequest::AcpResumeSessionRequest(_)
                | AcpRequest::AcpSessionRequest(_)
                | AcpRequest::AcpSetSessionConfigRequest(_)
        );

        let response = match self.strategy {
            Strategy::Blocking => {
                self.host.enqueue(output);
                self.core
                    .dispatch(&mut self.host, OWNER, request)
                    .expect("blocking ACP dispatch")
            }
            Strategy::Resumable if uses_resumable_exchange => {
                let mut response = self
                    .core
                    .dispatch_resumable(&mut self.host, OWNER, request)
                    .expect("begin resumable ACP dispatch");
                for chunk in output {
                    let AcpResponse::AcpPendingResponse(pending) = response else {
                        panic!("resumable exchange completed before scripted output: {response:?}");
                    };
                    let mut bytes = chunk.as_bytes().to_vec();
                    if !bytes.ends_with(b"\n") {
                        bytes.push(b'\n');
                    }
                    response = self
                        .core
                        .dispatch_resumable(
                            &mut self.host,
                            OWNER,
                            AcpRequest::AcpDeliverAgentOutputRequest(
                                AcpDeliverAgentOutputRequest {
                                    process_id: pending.process_id,
                                    chunk: bytes,
                                },
                            ),
                        )
                        .expect("deliver resumable ACP output");
                }
                assert!(
                    !matches!(response, AcpResponse::AcpPendingResponse(_)),
                    "script must finish the resumable exchange"
                );
                response
            }
            Strategy::Resumable => {
                self.host.enqueue(output);
                self.core
                    .dispatch_resumable(&mut self.host, OWNER, request)
                    .expect("non-resumable browser ACP dispatch")
            }
        };
        self.last_events = self
            .core
            .take_events(OWNER)
            .into_iter()
            .map(semantic_event)
            .collect();
        response
    }
}

fn fixture_agent(id: &str) -> ProjectedAgentLaunch {
    ProjectedAgentLaunch {
        id: id.to_string(),
        adapter_entrypoint: String::from("/opt/agentos/bin/echo-agent"),
        env: [(String::from("MANIFEST_DEFAULT"), String::from("yes"))]
            .into_iter()
            .collect(),
        launch_args: vec![String::from("--fixture")],
    }
}

struct Pair {
    blocking: Runner,
    resumable: Runner,
}

impl Pair {
    fn new() -> Self {
        Self {
            blocking: Runner::new(Strategy::Blocking),
            resumable: Runner::new(Strategy::Resumable),
        }
    }

    fn request(&mut self, request: AcpRequest, output: &[&str]) -> AcpResponse {
        let blocking = self.blocking.request(request.clone(), output);
        let resumable = self.resumable.request(request, output);
        assert_eq!(semantic_response(&blocking), semantic_response(&resumable));
        assert_eq!(self.blocking.last_events, self.resumable.last_events);
        blocking
    }

    fn state(&mut self, session_id: &str) -> Value {
        let response = self.request(
            AcpRequest::AcpGetSessionStateRequest(AcpGetSessionStateRequest {
                session_id: session_id.to_string(),
            }),
            &[],
        );
        semantic_response(&response)
    }
}

fn parse_optional_json(value: &Option<String>) -> Value {
    value
        .as_deref()
        .map(|value| serde_json::from_str(value).expect("valid fixture JSON"))
        .unwrap_or(Value::Null)
}

fn parse_config_options(values: &[String]) -> Vec<Value> {
    values
        .iter()
        .map(|value| serde_json::from_str(value).expect("valid config option JSON"))
        .collect()
}

/// Normalize only process identity. Everything behavioral remains exact.
fn semantic_response(response: &AcpResponse) -> Value {
    match response {
        AcpResponse::AcpSessionCreatedResponse(created) => json!({
            "kind": "created",
            "sessionId": created.session_id,
            "agentType": created.agent_type,
            "modes": parse_optional_json(&created.modes),
            "configOptions": parse_config_options(&created.config_options),
            "agentCapabilities": parse_optional_json(&created.agent_capabilities),
            "agentInfo": parse_optional_json(&created.agent_info),
        }),
        AcpResponse::AcpSessionRpcResponse(response) => json!({
            "kind": "rpc",
            "sessionId": response.session_id,
            "response": serde_json::from_str::<Value>(&response.response)
                .expect("valid adapter response JSON"),
            "text": response.text,
        }),
        AcpResponse::AcpSessionStateResponse(state) => json!({
            "kind": "state",
            "sessionId": state.session_id,
            "agentType": state.agent_type,
            "closed": state.closed,
            "exitCode": state.exit_code,
            "modes": parse_optional_json(&state.modes),
            "configOptions": parse_config_options(&state.config_options),
            "agentCapabilities": parse_optional_json(&state.agent_capabilities),
            "agentInfo": parse_optional_json(&state.agent_info),
        }),
        AcpResponse::AcpSessionResumedResponse(resumed) => json!({
            "kind": "resumed",
            "sessionId": resumed.session_id,
            "mode": resumed.mode,
            "agentType": resumed.agent_type,
        }),
        AcpResponse::AcpListAgentsResponse(response) => json!({
            "kind": "agents",
            "agents": response.agents.iter().map(|agent| json!({
                "id": agent.id,
                "installed": agent.installed,
                "adapterEntrypoint": agent.adapter_entrypoint,
            })).collect::<Vec<_>>(),
        }),
        other => json!({ "kind": "other", "debug": format!("{other:?}") }),
    }
}

fn semantic_event(event: AcpEvent) -> Value {
    match event {
        AcpEvent::AcpSessionEvent(event) => json!({
            "kind": "session",
            "sessionId": event.session_id,
            "notification": serde_json::from_str::<Value>(&event.notification)
                .expect("valid session notification JSON"),
        }),
        other => json!({ "kind": "other", "debug": format!("{other:?}") }),
    }
}

fn create_request() -> AcpRequest {
    AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
        agent_type: String::from("echo"),
        runtime: Some(AcpRuntimeKind::JavaScript),
        cwd: Some(String::from("/workspace")),
        args: Some(Vec::new()),
        env: Some(HashMap::new()),
        protocol_version: Some(1),
        client_capabilities: Some(String::from("{}")),
        mcp_servers: Some(String::from("[]")),
        skip_os_instructions: None,
        additional_instructions: None,
    })
}

fn bootstrap_output(session_id: &str) -> Vec<String> {
    vec![
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": { "loadSession": true },
                "agentInfo": { "name": "echo" }
            }
        })
        .to_string(),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "sessionId": session_id,
                "modes": {
                    "currentModeId": "default",
                    "availableModes": [
                        { "id": "default", "name": "Default" },
                        { "id": "plan", "name": "Plan" }
                    ]
                },
                "configOptions": [
                    {
                        "id": "thought_level",
                        "category": "thought_level",
                        "currentValue": "low"
                    },
                    {
                        "id": "model-picker",
                        "category": "model",
                        "readOnly": true
                    }
                ]
            }
        })
        .to_string(),
    ]
}

fn refs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

#[test]
fn create_bootstrap_and_prompt_notification_text_are_strategy_identical() {
    let mut pair = Pair::new();
    let bootstrap = bootstrap_output("session-1");
    let created = pair.request(create_request(), &refs(&bootstrap));
    let AcpResponse::AcpSessionCreatedResponse(created) = created else {
        panic!("expected created response");
    };
    assert_eq!(created.session_id, "session-1");
    assert_eq!(created.agent_type, "echo");

    let prompt_output = [
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"hello from notification"}}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}"#,
    ];
    let prompted = pair.request(
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("session-1"),
            method: String::from("session/prompt"),
            params: Some(String::from(
                r#"{"prompt":[{"type":"text","text":"hello"}]}"#,
            )),
        }),
        &prompt_output,
    );
    let AcpResponse::AcpSessionRpcResponse(prompted) = prompted else {
        panic!("expected prompt response");
    };
    assert_eq!(prompted.text.as_deref(), Some("hello from notification"));
    let response: Value = serde_json::from_str(&prompted.response).expect("prompt response JSON");
    assert_eq!(response["result"]["stopReason"], "end_turn");
    assert_eq!(pair.blocking.last_events.len(), 1);
    assert_eq!(
        pair.blocking.last_events[0]["notification"]["params"]["update"]["content"]["text"],
        "hello from notification"
    );
}

#[test]
fn create_launch_prompt_is_strategy_identical_and_sidecar_owned() {
    let mut pair = Pair::new();
    pair.blocking.host.host_tool_reference = String::from("host tools");
    pair.resumable.host.host_tool_reference = String::from("host tools");
    let mut request = create_request();
    let AcpRequest::AcpCreateSessionRequest(create) = &mut request else {
        unreachable!();
    };
    create.agent_type = String::from("pi");
    create.additional_instructions = Some(String::from("caller guidance"));
    let bootstrap = bootstrap_output("session-prompt-plan");
    pair.request(request, &refs(&bootstrap));

    let blocking = pair.blocking.host.spawns.last().expect("blocking spawn");
    let resumable = pair.resumable.host.spawns.last().expect("resumable spawn");
    assert_eq!(
        blocking.entrypoint.as_deref(),
        Some("/opt/agentos/bin/echo-agent")
    );
    assert_eq!(blocking.args, resumable.args);
    assert_eq!(blocking.env, resumable.env);
    assert_eq!(
        blocking.env.get("MANIFEST_DEFAULT").map(String::as_str),
        Some("yes")
    );
    assert_eq!(blocking.args.first().map(String::as_str), Some("--fixture"));
    let prompt_flag = blocking
        .args
        .iter()
        .position(|argument| argument == "--append-system-prompt")
        .expect("sidecar prompt flag");
    let prompt = &blocking.args[prompt_flag + 1];
    assert!(prompt.contains("# agentOS"));
    assert!(prompt.contains("caller guidance"));
    assert!(prompt.contains("host tools"));
}

#[test]
fn list_agents_is_sorted_and_strategy_identical() {
    let mut pair = Pair::new();
    let response = pair.request(
        AcpRequest::AcpListAgentsRequest(AcpListAgentsRequest { reserved: false }),
        &[],
    );
    let AcpResponse::AcpListAgentsResponse(response) = response else {
        panic!("expected list agents response");
    };
    assert_eq!(
        response
            .agents
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<Vec<_>>(),
        vec!["echo", "pi"]
    );
    assert!(response.agents.iter().all(|agent| agent.installed));
}

#[test]
fn projected_agent_catalog_errors_propagate_without_becoming_unknown_agent() {
    for strategy in [Strategy::Blocking, Strategy::Resumable] {
        let mut runner = Runner::new(strategy);
        runner.host.projected_agent_error = Some(AcpCoreError::Execution(String::from(
            "projected catalog unavailable",
        )));
        let error = runner
            .core
            .dispatch(
                &mut runner.host,
                OWNER,
                AcpRequest::AcpListAgentsRequest(AcpListAgentsRequest { reserved: false }),
            )
            .expect_err("catalog failure must propagate");
        assert_eq!(
            error,
            AcpCoreError::Execution(String::from("projected catalog unavailable"))
        );

        let create_error = match strategy {
            Strategy::Blocking => runner
                .core
                .dispatch(&mut runner.host, OWNER, create_request()),
            Strategy::Resumable => {
                runner
                    .core
                    .dispatch_resumable(&mut runner.host, OWNER, create_request())
            }
        }
        .expect_err("projected-agent resolve failure must propagate");
        assert_eq!(
            create_error,
            AcpCoreError::Execution(String::from("projected catalog unavailable"))
        );
    }
}

#[test]
fn writable_and_read_only_config_update_authoritative_state_in_both_strategies() {
    let mut pair = Pair::new();
    let bootstrap = bootstrap_output("session-config");
    pair.request(create_request(), &refs(&bootstrap));

    let read_only = pair.request(
        AcpRequest::AcpSetSessionConfigRequest(AcpSetSessionConfigRequest {
            session_id: String::from("session-config"),
            category: String::from("model"),
            value: String::from("model-2"),
        }),
        &[],
    );
    let AcpResponse::AcpSessionRpcResponse(read_only) = read_only else {
        panic!("read-only config must return an immediate RPC response");
    };
    let read_only: Value = serde_json::from_str(&read_only.response).expect("read-only JSON");
    assert_eq!(read_only["error"]["code"], -32601);

    pair.request(
        AcpRequest::AcpSetSessionConfigRequest(AcpSetSessionConfigRequest {
            session_id: String::from("session-config"),
            category: String::from("thought_level"),
            value: String::from("high"),
        }),
        &[r#"{"jsonrpc":"2.0","id":3,"result":{}}"#],
    );
    assert_eq!(pair.blocking.last_events.len(), 1);
    assert_eq!(
        pair.blocking.last_events[0]["notification"]["params"]["update"]["configOptions"][0]
            ["currentValue"],
        "high"
    );
    let state = pair.state("session-config");
    assert_eq!(state["configOptions"][0]["currentValue"], "high");
    // `state()` drains its own empty event batch, so inspect the synthetic event
    // captured immediately after the successful set request through the traces.
    pair.request(
        AcpRequest::AcpSetSessionConfigRequest(AcpSetSessionConfigRequest {
            session_id: String::from("session-config"),
            category: String::from("thought_level"),
            value: String::from("low"),
        }),
        &[
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"config_option_update","configOptions":[{"id":"thought_level","category":"thought_level","currentValue":"low"}]}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"result":{}}"#,
        ],
    );
    assert_eq!(pair.blocking.last_events.len(), 1);
    assert_eq!(
        pair.blocking.last_events[0]["notification"]["params"]["update"]["sessionUpdate"],
        "config_option_update"
    );
}

#[test]
fn mode_success_updates_authoritative_state_in_both_strategies() {
    let mut pair = Pair::new();
    let bootstrap = bootstrap_output("session-mode");
    pair.request(create_request(), &refs(&bootstrap));

    pair.request(
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("session-mode"),
            method: String::from("session/set_mode"),
            params: Some(String::from(r#"{"modeId":"plan"}"#)),
        }),
        &[r#"{"jsonrpc":"2.0","id":3,"result":{}}"#],
    );
    assert_eq!(pair.blocking.last_events.len(), 1);
    assert_eq!(
        pair.blocking.last_events[0]["notification"]["params"]["update"]["currentModeId"],
        "plan"
    );
    let state = pair.state("session-mode");
    assert_eq!(state["modes"]["currentModeId"], "plan");
    pair.request(
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("session-mode"),
            method: String::from("session/set_mode"),
            params: Some(String::from(r#"{"modeId":"default"}"#)),
        }),
        &[
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"current_mode_update","currentModeId":"default"}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"result":{}}"#,
        ],
    );
    assert_eq!(pair.blocking.last_events.len(), 1);
    assert_eq!(
        pair.blocking.last_events[0]["notification"]["params"]["update"]["currentModeId"],
        "default"
    );
}

#[test]
fn unsupported_cancel_uses_the_same_notification_fallback() {
    let mut pair = Pair::new();
    let bootstrap = bootstrap_output("session-cancel");
    pair.request(create_request(), &refs(&bootstrap));

    let response = pair.request(
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("session-cancel"),
            method: String::from("session/cancel"),
            params: Some(String::from("{}")),
        }),
        &[r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"unknown session/cancel"}}"#],
    );
    let AcpResponse::AcpSessionRpcResponse(response) = response else {
        panic!("expected cancel response");
    };
    let response: Value = serde_json::from_str(&response.response).expect("cancel response JSON");
    assert_eq!(response["result"]["via"], "notification-fallback");
    for runner in [&pair.blocking, &pair.resumable] {
        let notification = runner.host.writes.last().expect("cancel notification");
        assert_eq!(notification["method"], "session/cancel");
        assert_eq!(notification["params"]["sessionId"], "session-cancel");
        assert!(notification.get("id").is_none());
    }
}

fn resume_request(session_id: &str, transcript_path: Option<&str>) -> AcpRequest {
    AcpRequest::AcpResumeSessionRequest(AcpResumeSessionRequest {
        session_id: session_id.to_string(),
        agent_type: String::from("echo"),
        transcript_path: transcript_path.map(str::to_string),
        cwd: Some(String::from("/workspace")),
        env: Some(HashMap::new()),
    })
}

#[test]
fn native_and_fallback_resume_plus_one_shot_preamble_are_strategy_identical() {
    let mut native = Pair::new();
    let native_output = [
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"available_commands_update","availableCommands":[]}}}"#,
        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"result":{}}"#,
    ];
    let resumed = native.request(resume_request("durable-1", None), &native_output);
    let AcpResponse::AcpSessionResumedResponse(resumed) = resumed else {
        panic!("expected native resume response");
    };
    assert_eq!(resumed.mode, "native");
    assert_eq!(resumed.session_id, "durable-1");
    assert_eq!(native.blocking.last_events.len(), 1);
    assert_eq!(native.blocking.last_events[0]["sessionId"], "durable-1");

    let mut fallback = Pair::new();
    let fallback_output = [
        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fresh-1"}}"#,
    ];
    let resumed = fallback.request(
        resume_request("durable-missing", Some("/transcripts/prior.jsonl")),
        &fallback_output,
    );
    let AcpResponse::AcpSessionResumedResponse(resumed) = resumed else {
        panic!("expected fallback resume response");
    };
    assert_eq!(resumed.mode, "fallback");
    assert_eq!(resumed.session_id, "fresh-1");

    let prompt_output = [r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}"#];
    fallback.request(
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("fresh-1"),
            method: String::from("session/prompt"),
            params: Some(String::from(
                r#"{"prompt":[{"type":"text","text":"continue"}]}"#,
            )),
        }),
        &prompt_output,
    );

    for runner in [&fallback.blocking, &fallback.resumable] {
        let prompt = runner.host.writes.last().expect("forwarded prompt");
        assert_eq!(prompt["method"], "session/prompt");
        assert!(prompt["params"]["prompt"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("/transcripts/prior.jsonl")));
        assert_eq!(prompt["params"]["prompt"][1]["text"], "continue");
    }
}
