#[path = "support/bridge.rs"]
mod bridge_support;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use agentos_native_sidecar::wire::{
    AuthenticateRequest, CloseSessionRequest as CloseWireSessionRequest, ConfigureVmRequest,
    ConnectionOwnership, CreateVmRequest, CronEventKind, EventFrame, EventPayload, ExtEnvelope,
    GuestRuntimeKind, OpenSessionRequest, OwnershipScope, PackageDescriptor, PackagePath,
    RegisterHostCallbacksRequest, RegisteredHostCallbackDefinition, RegisteredHostCallbackExample,
    RequestFrame, RequestPayload, ResponsePayload, ScheduleCronRequest, SessionOwnership,
    SidecarPlacement, SidecarPlacementShared, SidecarRequestPayload, SidecarResponseFrame,
    SidecarResponsePayload, VmOwnership, WakeCronRequest,
};
use agentos_native_sidecar::{NativeSidecar, NativeSidecarConfig};
use agentos_protocol::generated::v1::{
    AcpCallback, AcpCallbackResponse, AcpCloseSessionRequest, AcpCreateSessionRequest, AcpEvent,
    AcpGetSessionStateRequest, AcpListSessionsRequest, AcpPermissionCallbackResponse, AcpRequest,
    AcpResponse, AcpRuntimeKind, AcpSessionRequest,
};
use agentos_protocol::{ACP_EXTENSION_NAMESPACE, PROTOCOL_VERSION as ACP_PROTOCOL_VERSION};
use agentos_vm_config as vm_config;
use bridge_support::RecordingBridge;
use serde_json::Value;

const GUEST_CWD: &str = "/workspace";

#[test]
fn acp_extension_suite() {
    acp_extension_creates_reports_and_closes_session_over_ext();
    acp_terminal_requests_stay_inside_sidecar();
    acp_get_session_state_denies_cross_connection_session_id();
    acp_close_session_is_owner_scoped_and_idempotent();
    acp_session_request_denies_cross_connection_prompt_and_cancel();
    closing_wire_session_preserves_same_connection_sibling_acp_state();
    cron_session_actions_execute_inside_the_native_sidecar();
}

fn closing_wire_session_preserves_same_connection_sibling_acp_state() {
    assert_node_available();
    let mut sidecar = new_sidecar("agentos-acp-wire-session-isolation");
    install_default_acp_callback_handler(&mut sidecar);
    let connection_id = authenticate(&mut sidecar);
    let disposable_session = open_session(&mut sidecar, &connection_id);
    let live_session = open_session(&mut sidecar, &connection_id);
    let cwd = temp_dir("agentos-acp-wire-session-isolation-cwd");
    fs::write(cwd.join("adapter.mjs"), adapter_script()).expect("write adapter script");
    let vm_id = create_vm(&mut sidecar, &connection_id, &live_session, &cwd);

    let created = dispatch_acp(
        &mut sidecar,
        4,
        &connection_id,
        &live_session,
        &vm_id,
        AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
            agent_type: String::from("pi"),
            runtime: Some(AcpRuntimeKind::JavaScript),
            cwd: Some(String::from(GUEST_CWD)),
            args: Some(Vec::new()),
            env: Some(HashMap::new()),
            protocol_version: Some(i32::from(ACP_PROTOCOL_VERSION)),
            client_capabilities: Some(String::from(r#"{"fs":{"readTextFile":true}}"#)),
            mcp_servers: Some(String::from(r#"{"servers":[]}"#)),
            skip_os_instructions: Some(true),
            additional_instructions: None,
        }),
    );
    assert!(matches!(created, AcpResponse::AcpSessionCreatedResponse(_)));

    let closed = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: agentos_native_sidecar::wire::protocol_schema(),
            request_id: 5,
            ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
                connection_id: connection_id.clone(),
            }),
            payload: RequestPayload::CloseSessionRequest(CloseWireSessionRequest {
                session_id: disposable_session,
            }),
        })
        .expect("close sibling wire session");
    assert!(matches!(
        closed.response.payload,
        ResponsePayload::SessionClosedResponse(_)
    ));

    let state = dispatch_acp(
        &mut sidecar,
        6,
        &connection_id,
        &live_session,
        &vm_id,
        AcpRequest::AcpGetSessionStateRequest(AcpGetSessionStateRequest {
            session_id: String::from("adapter-session"),
        }),
    );
    assert!(
        matches!(state, AcpResponse::AcpSessionStateResponse(_)),
        "closing one wire session must preserve a sibling session on the same connection: {state:?}"
    );
    close_owned_session(&mut sidecar, 7, &connection_id, &live_session, &vm_id);
}

fn cron_session_actions_execute_inside_the_native_sidecar() {
    assert_node_available();
    let mut sidecar = new_sidecar("agentos-acp-cron-session");
    let connection_id = authenticate(&mut sidecar);
    let session_id = open_session(&mut sidecar, &connection_id);
    let cwd = temp_dir("agentos-acp-cron-session-cwd");
    fs::write(cwd.join("adapter.mjs"), cron_adapter_script())
        .expect("write cron ACP adapter script");
    let vm_id = create_vm(&mut sidecar, &connection_id, &session_id, &cwd);
    let ownership = OwnershipScope::VmOwnership(VmOwnership {
        connection_id: connection_id.clone(),
        session_id: session_id.clone(),
        vm_id: vm_id.clone(),
    });

    let scheduled = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: agentos_native_sidecar::wire::protocol_schema(),
            request_id: 40,
            ownership: ownership.clone(),
            payload: RequestPayload::ScheduleCronRequest(ScheduleCronRequest {
                id: Some(String::from("agent-turn")),
                schedule: String::from("* * * * * *"),
                action: serde_json::json!({
                    "type": "session",
                    "agentType": "pi",
                    "prompt": "run from cron",
                    "options": {
                        "cwd": GUEST_CWD,
                        "skipOsInstructions": true
                    }
                })
                .to_string(),
                overlap: None,
            }),
        })
        .expect("schedule cron session action");
    let ResponsePayload::CronScheduledResponse(scheduled) = scheduled.response.payload else {
        panic!(
            "unexpected cron schedule response: {:?}",
            scheduled.response.payload
        );
    };
    let deadline = scheduled.alarm.next_alarm_ms.expect("next cron alarm");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix time")
        .as_millis() as u64;
    std::thread::sleep(std::time::Duration::from_millis(
        deadline.saturating_sub(now).saturating_add(5),
    ));

    let wake = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: agentos_native_sidecar::wire::protocol_schema(),
            request_id: 41,
            ownership,
            payload: RequestPayload::WakeCronRequest(WakeCronRequest {
                generation: scheduled.alarm.generation,
            }),
        })
        .expect("wake cron session action");
    let ResponsePayload::CronWakeResponse(wake) = wake.response.payload else {
        panic!("unexpected cron wake response: {:?}", wake.response.payload);
    };
    assert!(
        wake.runs.is_empty(),
        "session actions must not reach the client"
    );
    assert!(wake
        .events
        .iter()
        .any(|event| event.kind == CronEventKind::Complete));
    assert!(!wake
        .events
        .iter()
        .any(|event| event.kind == CronEventKind::Error));
}

fn acp_terminal_requests_stay_inside_sidecar() {
    assert_node_available();
    let mut sidecar = new_sidecar("agentos-acp-extension-terminal");
    sidecar.set_wire_sidecar_request_handler(|frame| {
        let SidecarRequestPayload::ExtEnvelope(envelope) = frame.payload else {
            panic!("unexpected sidecar callback payload");
        };
        let AcpCallback::AcpHostRequestCallback(callback) =
            serde_bare::from_slice(&envelope.payload).expect("decode unknown host callback")
        else {
            panic!("unexpected ACP callback variant");
        };
        let request: Value = serde_json::from_str(&callback.request).expect("host request json");
        assert_eq!(
            request["method"], "host/not-found",
            "native ACP terminal requests must not reach the client callback"
        );
        let response = AcpCallbackResponse::AcpHostRequestCallbackResponse(
            agentos_protocol::generated::v1::AcpHostRequestCallbackResponse { response: None },
        );
        Ok(SidecarResponseFrame {
            schema: frame.schema,
            request_id: frame.request_id,
            ownership: frame.ownership,
            payload: SidecarResponsePayload::ExtEnvelope(ExtEnvelope {
                namespace: envelope.namespace,
                payload: serde_bare::to_vec(&response).expect("encode unknown host response"),
            }),
        })
    });
    let connection_id = authenticate(&mut sidecar);
    let session_id = open_session(&mut sidecar, &connection_id);
    let cwd = temp_dir("agentos-acp-extension-terminal-cwd");
    let adapter = cwd.join("adapter.mjs");
    fs::write(&adapter, terminal_adapter_script()).expect("write terminal adapter script");
    let vm_id = create_vm(&mut sidecar, &connection_id, &session_id, &cwd);

    let created = dispatch_acp(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
            agent_type: String::from("pi"),
            runtime: Some(AcpRuntimeKind::JavaScript),
            cwd: Some(String::from(GUEST_CWD)),
            args: Some(Vec::new()),
            env: Some(HashMap::new()),
            protocol_version: Some(i32::from(ACP_PROTOCOL_VERSION)),
            client_capabilities: Some(String::from(r#"{"terminal":true}"#)),
            mcp_servers: Some(String::from(r#"{"servers":[]}"#)),
            skip_os_instructions: Some(true),
            additional_instructions: None,
        }),
    );
    let AcpResponse::AcpSessionCreatedResponse(created) = created else {
        panic!("unexpected create response: {created:?}");
    };
    let terminal = dispatch_acp(
        &mut sidecar,
        5,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: created.session_id.clone(),
            method: String::from("session/prompt"),
            params: Some(String::from(r#"{"prompt":[]}"#)),
        }),
    );
    let AcpResponse::AcpSessionRpcResponse(terminal) = terminal else {
        panic!("unexpected terminal prompt response: {terminal:?}");
    };
    let terminal: Value =
        serde_json::from_str(&terminal.response).expect("terminal prompt response json");
    assert!(terminal["result"]["terminalOutput"]
        .as_str()
        .unwrap_or_else(|| panic!("terminal output missing from {terminal:?}"))
        .contains("native-terminal"));
    assert_eq!(terminal["result"]["exitCode"], 0);
    assert_eq!(terminal["result"]["truncated"], false);
    assert_eq!(terminal["result"]["unknownMethodCode"], -32601);

    let closed = dispatch_acp(
        &mut sidecar,
        6,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpCloseSessionRequest(AcpCloseSessionRequest {
            session_id: created.session_id,
        }),
    );
    assert!(matches!(closed, AcpResponse::AcpSessionClosedResponse(_)));
}

fn acp_extension_creates_reports_and_closes_session_over_ext() {
    assert_node_available();
    let mut sidecar = new_sidecar("agentos-acp-extension-create");
    sidecar.set_wire_sidecar_request_handler(|frame| match frame.payload {
        SidecarRequestPayload::ExtEnvelope(envelope) => {
            assert_eq!(envelope.namespace, ACP_EXTENSION_NAMESPACE);
            let callback: AcpCallback =
                serde_bare::from_slice(&envelope.payload).expect("decode ACP callback");
            let response = match callback {
                AcpCallback::AcpPermissionCallback(callback) => {
                    assert_eq!(callback.session_id, "adapter-session");
                    assert_eq!(callback.permission_id, "perm-1");
                    assert_eq!(callback.cleanup_after_ms, 125_000);
                    assert!(
                        callback.cleanup_after_ms > 120_000,
                        "client cleanup must occur after the sidecar permission deadline"
                    );
                    AcpCallbackResponse::AcpPermissionCallbackResponse(
                        AcpPermissionCallbackResponse {
                            permission_id: callback.permission_id,
                            reply: Some(String::from("once")),
                        },
                    )
                }
                AcpCallback::AcpHostRequestCallback(callback) => panic!(
                    "native ACP filesystem requests must not reach the client callback: {}",
                    callback.request
                ),
            };
            Ok(SidecarResponseFrame {
                schema: frame.schema,
                request_id: frame.request_id,
                ownership: frame.ownership,
                payload: SidecarResponsePayload::ExtEnvelope(ExtEnvelope {
                    namespace: envelope.namespace,
                    payload: serde_bare::to_vec(&response).expect("encode callback response"),
                }),
            })
        }
        other => panic!("unexpected sidecar callback: {other:?}"),
    });
    let connection_id = authenticate(&mut sidecar);
    let session_id = open_session(&mut sidecar, &connection_id);
    let cwd = temp_dir("agentos-acp-extension-create-cwd");
    let adapter = cwd.join("adapter.mjs");
    fs::write(&adapter, adapter_script()).expect("write adapter script");
    let vm_id = create_vm_with_additional_instructions(
        &mut sidecar,
        &connection_id,
        &session_id,
        &cwd,
        Some("VM-level guidance"),
    );
    register_math_toolkit(&mut sidecar, &connection_id, &session_id, &vm_id);

    let create_request = AcpCreateSessionRequest {
        agent_type: String::from("pi"),
        runtime: Some(AcpRuntimeKind::JavaScript),
        cwd: Some(String::from(GUEST_CWD)),
        args: Some(Vec::new()),
        env: Some(HashMap::new()),
        protocol_version: Some(i32::from(ACP_PROTOCOL_VERSION)),
        client_capabilities: Some(String::from(r#"{"fs":{"readTextFile":true}}"#)),
        mcp_servers: Some(String::from(r#"{"servers":[]}"#)),
        skip_os_instructions: Some(true),
        additional_instructions: Some(String::from("extra guidance")),
    };
    let (created, create_events) = dispatch_acp_with_events(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpCreateSessionRequest(create_request.clone()),
    );

    let AcpResponse::AcpSessionCreatedResponse(created) = created else {
        panic!("unexpected create response: {created:?}");
    };
    assert_eq!(created.session_id, "adapter-session");
    assert_eq!(created.agent_type, "pi");
    assert!(created.process_id.starts_with("acp-agent-"));
    assert!(created.pid.is_some());
    let bootstrap_event = decode_single_acp_session_event(&create_events);
    assert_eq!(
        bootstrap_event["params"]["update"]["sessionUpdate"],
        "current_mode_update"
    );
    assert_eq!(
        bootstrap_event["params"]["update"]["currentModeId"],
        "bootstrap"
    );
    let agent_info: Value = serde_json::from_str(
        created
            .agent_info
            .as_deref()
            .expect("agent info should be present"),
    )
    .expect("agent info json");
    let args = agent_info["args"].as_array().expect("args array");
    assert_eq!(args[0], "--append-system-prompt");
    assert!(args[1]
        .as_str()
        .expect("prompt arg")
        .contains("extra guidance"));
    let prompt = args[1].as_str().expect("prompt arg");
    assert!(prompt.contains("VM-level guidance"));
    assert!(prompt.contains("extra guidance"));
    assert!(prompt.contains("## Available Host Tools"));
    assert!(prompt.contains("`agentos-math add --a <number> --b <number>`"));
    assert!(prompt.contains("Add 1 and 2: `agentos-math add --a 1 --b 2`"));
    assert!(created
        .config_options
        .iter()
        .any(|option| option.contains(r#""id":"model""#)));

    let state = dispatch_acp(
        &mut sidecar,
        5,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpGetSessionStateRequest(AcpGetSessionStateRequest {
            session_id: String::from("adapter-session"),
        }),
    );
    let AcpResponse::AcpSessionStateResponse(state) = state else {
        panic!("unexpected state response: {state:?}");
    };
    assert_eq!(state.session_id, "adapter-session");
    assert_eq!(state.agent_type, "pi");
    assert!(!state.closed);

    let duplicate = dispatch_acp(
        &mut sidecar,
        6,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpCreateSessionRequest(create_request),
    );
    let AcpResponse::AcpErrorResponse(duplicate) = duplicate else {
        panic!("duplicate session id should be rejected: {duplicate:?}");
    };
    assert!(duplicate.message.contains("session id collision"));

    let listed = dispatch_acp(
        &mut sidecar,
        7,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpListSessionsRequest(AcpListSessionsRequest { reserved: false }),
    );
    let AcpResponse::AcpListSessionsResponse(listed) = listed else {
        panic!("unexpected list response: {listed:?}");
    };
    assert_eq!(listed.sessions.len(), 1);
    assert_eq!(listed.sessions[0].session_id, "adapter-session");

    let (prompt, prompt_events) = dispatch_acp_with_events(
        &mut sidecar,
        8,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("adapter-session"),
            method: String::from("session/prompt"),
            params: Some(String::from(
                r#"{"prompt":[{"type":"text","text":"hello"}]}"#,
            )),
        }),
    );
    let AcpResponse::AcpSessionRpcResponse(prompt) = prompt else {
        panic!("unexpected prompt response: {prompt:?}");
    };
    assert_eq!(prompt.session_id, "adapter-session");
    let prompt_response: Value =
        serde_json::from_str(&prompt.response).expect("prompt response json");
    assert_eq!(prompt_response["result"]["echo"], "hello");
    assert_eq!(prompt_response["result"]["sessionId"], "adapter-session");
    assert_eq!(
        prompt_response["result"]["permissionOutcome"]["optionId"],
        "once"
    );
    assert_eq!(prompt.text.as_deref(), Some("agent says hello"));
    let notifications = prompt_events
        .iter()
        .filter_map(|event| {
            let EventPayload::ExtEnvelope(envelope) = &event.payload else {
                return None;
            };
            let AcpEvent::AcpSessionEvent(event) =
                serde_bare::from_slice(&envelope.payload).expect("decode ACP event")
            else {
                panic!("expected an AcpSessionEvent");
            };
            Some(serde_json::from_str::<Value>(&event.notification).expect("notification json"))
        })
        .collect::<Vec<_>>();
    assert_eq!(notifications.len(), 2, "{prompt_events:#?}");
    assert!(notifications
        .iter()
        .any(|event| event["params"]["update"]["currentModeId"] == "ask"));
    assert!(notifications
        .iter()
        .any(|event| { event["params"]["update"]["content"]["text"] == "agent says hello" }));

    let (mode_update, mode_events) = dispatch_acp_with_events(
        &mut sidecar,
        9,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("adapter-session"),
            method: String::from("session/set_mode"),
            params: Some(String::from(r#"{"modeId":"plan"}"#)),
        }),
    );
    let AcpResponse::AcpSessionRpcResponse(mode_update) = mode_update else {
        panic!("unexpected mode response: {mode_update:?}");
    };
    let mode_response: Value =
        serde_json::from_str(&mode_update.response).expect("mode response json");
    assert!(mode_response.get("error").is_none());
    let mode_event = decode_single_acp_session_event(&mode_events);
    assert_eq!(
        mode_event["params"]["update"]["sessionUpdate"],
        "current_mode_update"
    );
    assert_eq!(mode_event["params"]["update"]["currentModeId"], "plan");

    let (config_update, config_events) = dispatch_acp_with_events(
        &mut sidecar,
        10,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("adapter-session"),
            method: String::from("session/set_config_option"),
            params: Some(String::from(r#"{"configId":"model","value":"slow-model"}"#)),
        }),
    );
    let AcpResponse::AcpSessionRpcResponse(config_update) = config_update else {
        panic!("unexpected config response: {config_update:?}");
    };
    let config_response: Value =
        serde_json::from_str(&config_update.response).expect("config response json");
    assert!(config_response.get("error").is_none());
    let config_event = decode_single_acp_session_event(&config_events);
    assert_eq!(
        config_event["params"]["update"]["sessionUpdate"],
        "config_option_update"
    );
    assert_eq!(
        config_event["params"]["update"]["configOptions"][0]["currentValue"],
        "slow-model"
    );

    let updated_state = dispatch_acp(
        &mut sidecar,
        11,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpGetSessionStateRequest(AcpGetSessionStateRequest {
            session_id: String::from("adapter-session"),
        }),
    );
    let AcpResponse::AcpSessionStateResponse(updated_state) = updated_state else {
        panic!("unexpected updated state response: {updated_state:?}");
    };
    let modes: Value = serde_json::from_str(updated_state.modes.as_deref().expect("updated modes"))
        .expect("updated modes json");
    assert_eq!(modes["currentModeId"], "plan");
    let model_option: Value =
        serde_json::from_str(&updated_state.config_options[0]).expect("model option json");
    assert_eq!(model_option["currentValue"], "slow-model");

    let cancel = dispatch_acp(
        &mut sidecar,
        12,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("adapter-session"),
            method: String::from("session/cancel"),
            params: Some(String::from("{}")),
        }),
    );
    let AcpResponse::AcpSessionRpcResponse(cancel) = cancel else {
        panic!("unexpected cancel response: {cancel:?}");
    };
    let cancel_response: Value =
        serde_json::from_str(&cancel.response).expect("cancel response json");
    assert_eq!(cancel_response["result"]["cancelled"], false);
    assert_eq!(cancel_response["result"]["requested"], true);
    assert_eq!(cancel_response["result"]["via"], "notification-fallback");

    let closed = dispatch_acp(
        &mut sidecar,
        13,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpCloseSessionRequest(AcpCloseSessionRequest {
            session_id: String::from("adapter-session"),
        }),
    );
    let AcpResponse::AcpSessionClosedResponse(closed) = closed else {
        panic!("unexpected close response: {closed:?}");
    };
    assert_eq!(closed.session_id, "adapter-session");
}

// ---------------------------------------------------------------------------
// Security tests (adversarial). The peer connection is UNTRUSTED; the test
// asserts the ACP extension DENIES the attack and stays usable.
// ---------------------------------------------------------------------------

/// F-010 (T2 / J.4): a second connection (attacker) must NOT be able to read the
/// ACP session state created by a different connection (victim). `get_session_state`
/// must enforce per-connection ownership; a cross-connection read by a known/guessed
/// `session_id` must fail closed (error response), not leak the victim's session
/// metadata (agent_type, process_id, pid, modes, config_options, agent_info).
///
/// This is the bounded SAFEGUARD assertion and runs by default. It is fast: it
/// performs a single cross-connection read and asserts the deny.
fn acp_get_session_state_denies_cross_connection_session_id() {
    assert_node_available();
    let mut sidecar = new_sidecar("agentos-acp-cross-conn-state");

    // Victim connection creates a real ACP session.
    let victim_conn = authenticate(&mut sidecar);
    let victim_session = open_session(&mut sidecar, &victim_conn);
    let cwd = temp_dir("agentos-acp-cross-conn-state-cwd");
    let adapter = cwd.join("adapter.mjs");
    fs::write(&adapter, adapter_script()).expect("write adapter script");
    let victim_vm = create_vm(&mut sidecar, &victim_conn, &victim_session, &cwd);

    let created = dispatch_acp(
        &mut sidecar,
        4,
        &victim_conn,
        &victim_session,
        &victim_vm,
        AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
            agent_type: String::from("pi"),
            runtime: Some(AcpRuntimeKind::JavaScript),
            cwd: Some(String::from(GUEST_CWD)),
            args: Some(Vec::new()),
            env: Some(HashMap::new()),
            protocol_version: Some(i32::from(ACP_PROTOCOL_VERSION)),
            client_capabilities: Some(String::from(r#"{"fs":{"readTextFile":true}}"#)),
            mcp_servers: Some(String::from(r#"{"servers":[]}"#)),
            skip_os_instructions: Some(true),
            additional_instructions: None,
        }),
    );
    let AcpResponse::AcpSessionCreatedResponse(_) = created else {
        panic!("victim create failed: {created:?}");
    };

    // The owner can still read its own session state.
    let owner_state = dispatch_acp(
        &mut sidecar,
        5,
        &victim_conn,
        &victim_session,
        &victim_vm,
        AcpRequest::AcpGetSessionStateRequest(AcpGetSessionStateRequest {
            session_id: String::from("adapter-session"),
        }),
    );
    assert!(
        matches!(owner_state, AcpResponse::AcpSessionStateResponse(_)),
        "owner connection must still read its own ACP session state, got {owner_state:?}"
    );

    // The owner key includes the VM, not only connection + wire session.
    let sibling_vm_cwd = temp_dir("agentos-acp-sibling-vm-state-cwd");
    let sibling_vm = create_vm(&mut sidecar, &victim_conn, &victim_session, &sibling_vm_cwd);
    let sibling_vm_read = dispatch_acp(
        &mut sidecar,
        6,
        &victim_conn,
        &victim_session,
        &sibling_vm,
        AcpRequest::AcpGetSessionStateRequest(AcpGetSessionStateRequest {
            session_id: String::from("adapter-session"),
        }),
    );
    assert_indistinguishable_deny(
        sibling_vm_read,
        "same-session sibling VM read of another VM's ACP session state",
    );

    // Attacker connection: separate auth + session + vm, then tries to read the
    // victim's ACP session state by its (guessed/known) session id.
    let attacker_conn = authenticate(&mut sidecar);
    assert_ne!(
        attacker_conn, victim_conn,
        "attacker must be a distinct connection"
    );
    let attacker_session = open_session(&mut sidecar, &attacker_conn);
    let attacker_cwd = temp_dir("agentos-acp-cross-conn-attacker-cwd");
    let attacker_vm = create_vm(
        &mut sidecar,
        &attacker_conn,
        &attacker_session,
        &attacker_cwd,
    );

    let leaked = dispatch_acp(
        &mut sidecar,
        7,
        &attacker_conn,
        &attacker_session,
        &attacker_vm,
        AcpRequest::AcpGetSessionStateRequest(AcpGetSessionStateRequest {
            session_id: String::from("adapter-session"),
        }),
    );

    // SECURE expectation: a different connection must be denied, indistinguishably
    // from a missing session (no existence leak).
    assert_indistinguishable_deny(
        leaked,
        "cross-connection read of another connection's ACP session state",
    );

    close_owned_session(&mut sidecar, 8, &victim_conn, &victim_session, &victim_vm);
}

/// AOS-ACP-1 (P1 / J.4 cross-connection ACP close): a second connection
/// (attacker) must NOT be able to CLOSE the ACP session created by a different
/// connection (victim). `close_session` (acp_extension.rs:367-408) resolves the
/// session by `session_id` only and does not check `owner_connection_id`, so an
/// attacker connection that knows/guesses the victim's `session_id` can
/// `close_stdin` + `SIGTERM`/`SIGKILL` the victim's adapter process and dispose
/// its resources — a cross-tenant DoS.
///
/// SECURE expectation: the cross-connection close is an idempotent success that
/// reveals no existence information, while the victim's session stays alive.
///
/// Bounded SAFEGUARD-shaped assertion: a single cross-connection close, fast.
fn acp_close_session_is_owner_scoped_and_idempotent() {
    assert_node_available();
    let mut sidecar = new_sidecar("agentos-acp-cross-conn-close");
    install_default_acp_callback_handler(&mut sidecar);

    // Victim connection creates a real ACP session.
    let victim_conn = authenticate(&mut sidecar);
    let victim_session = open_session(&mut sidecar, &victim_conn);
    let cwd = temp_dir("agentos-acp-cross-conn-close-cwd");
    let adapter = cwd.join("adapter.mjs");
    fs::write(&adapter, adapter_script()).expect("write adapter script");
    let victim_vm = create_vm(&mut sidecar, &victim_conn, &victim_session, &cwd);

    let created = dispatch_acp(
        &mut sidecar,
        4,
        &victim_conn,
        &victim_session,
        &victim_vm,
        AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
            agent_type: String::from("pi"),
            runtime: Some(AcpRuntimeKind::JavaScript),
            cwd: Some(String::from(GUEST_CWD)),
            args: Some(Vec::new()),
            env: Some(HashMap::new()),
            protocol_version: Some(i32::from(ACP_PROTOCOL_VERSION)),
            client_capabilities: Some(String::from(r#"{"fs":{"readTextFile":true}}"#)),
            mcp_servers: Some(String::from(r#"{"servers":[]}"#)),
            skip_os_instructions: Some(true),
            additional_instructions: None,
        }),
    );
    let AcpResponse::AcpSessionCreatedResponse(_) = created else {
        panic!("victim create failed: {created:?}");
    };

    // Attacker connection: separate auth + session + vm, then tries to close the
    // victim's ACP session by its (guessed/known) session id.
    let attacker_conn = authenticate(&mut sidecar);
    assert_ne!(
        attacker_conn, victim_conn,
        "attacker must be a distinct connection"
    );
    let attacker_session = open_session(&mut sidecar, &attacker_conn);
    let attacker_cwd = temp_dir("agentos-acp-cross-conn-close-attacker-cwd");
    let attacker_vm = create_vm(
        &mut sidecar,
        &attacker_conn,
        &attacker_session,
        &attacker_cwd,
    );

    let close_result = dispatch_acp(
        &mut sidecar,
        6,
        &attacker_conn,
        &attacker_session,
        &attacker_vm,
        AcpRequest::AcpCloseSessionRequest(AcpCloseSessionRequest {
            session_id: String::from("adapter-session"),
        }),
    );

    let AcpResponse::AcpSessionClosedResponse(closed) = close_result else {
        panic!("cross-connection close should be an idempotent response: {close_result:?}");
    };
    assert_eq!(closed.session_id, "adapter-session");

    // Listing is scoped to the caller and cannot reveal the victim session.
    let attacker_list = dispatch_acp(
        &mut sidecar,
        7,
        &attacker_conn,
        &attacker_session,
        &attacker_vm,
        AcpRequest::AcpListSessionsRequest(AcpListSessionsRequest { reserved: false }),
    );
    let AcpResponse::AcpListSessionsResponse(attacker_list) = attacker_list else {
        panic!("unexpected attacker list response: {attacker_list:?}");
    };
    assert!(attacker_list.sessions.is_empty());

    // ...and the victim session must remain alive and usable for its owner.
    let owner_state = dispatch_acp(
        &mut sidecar,
        8,
        &victim_conn,
        &victim_session,
        &victim_vm,
        AcpRequest::AcpGetSessionStateRequest(AcpGetSessionStateRequest {
            session_id: String::from("adapter-session"),
        }),
    );
    let AcpResponse::AcpSessionStateResponse(owner_state) = owner_state else {
        panic!(
            "victim ACP session must survive an attacker's cross-connection close, \
             but the owner can no longer read it: {owner_state:?}"
        );
    };
    assert!(
        !owner_state.closed,
        "victim ACP session must not be marked closed after an attacker's \
         cross-connection close attempt"
    );

    // This test harness does not install the host callback transport needed by
    // `session/prompt`; owner-readable state is the stable liveness assertion.

    close_owned_session(&mut sidecar, 9, &victim_conn, &victim_session, &victim_vm);
}

/// AOS-ACP-2 (P1 / J.4 cross-connection ACP drive): a second connection
/// (attacker) must NOT be able to DRIVE the ACP session created by a different
/// connection (victim). `session_request` (acp_extension.rs:410-447) resolves the
/// session by `session_id` only and does not check `owner_connection_id`, so an
/// attacker connection can write to the victim's adapter stdin (prompt / cancel /
/// arbitrary methods), advancing the victim's `next_request_id` and consuming its
/// buffered stdout — cross-tenant session hijack + state corruption.
///
/// SECURE expectation: the cross-connection drive is DENIED (error response),
/// has NO side effect on the victim, and the victim's OWN request still works
/// afterwards. Today the code is expected to drive the victim's adapter (FAIL =
/// vuln present).
///
/// Bounded SAFEGUARD-shaped assertion: a single cross-connection prompt, fast.
fn acp_session_request_denies_cross_connection_prompt_and_cancel() {
    assert_node_available();
    let mut sidecar = new_sidecar("agentos-acp-cross-conn-drive");
    install_default_acp_callback_handler(&mut sidecar);

    // Victim connection creates a real ACP session.
    let victim_conn = authenticate(&mut sidecar);
    let victim_session = open_session(&mut sidecar, &victim_conn);
    let cwd = temp_dir("agentos-acp-cross-conn-drive-cwd");
    let adapter = cwd.join("adapter.mjs");
    fs::write(&adapter, adapter_script()).expect("write adapter script");
    let victim_vm = create_vm(&mut sidecar, &victim_conn, &victim_session, &cwd);

    let created = dispatch_acp(
        &mut sidecar,
        4,
        &victim_conn,
        &victim_session,
        &victim_vm,
        AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
            agent_type: String::from("pi"),
            runtime: Some(AcpRuntimeKind::JavaScript),
            cwd: Some(String::from(GUEST_CWD)),
            args: Some(Vec::new()),
            env: Some(HashMap::new()),
            protocol_version: Some(i32::from(ACP_PROTOCOL_VERSION)),
            client_capabilities: Some(String::from(r#"{"fs":{"readTextFile":true}}"#)),
            mcp_servers: Some(String::from(r#"{"servers":[]}"#)),
            skip_os_instructions: Some(true),
            additional_instructions: None,
        }),
    );
    let AcpResponse::AcpSessionCreatedResponse(_) = created else {
        panic!("victim create failed: {created:?}");
    };

    // Attacker connection: separate auth + session + vm.
    let attacker_conn = authenticate(&mut sidecar);
    assert_ne!(
        attacker_conn, victim_conn,
        "attacker must be a distinct connection"
    );
    let attacker_session = open_session(&mut sidecar, &attacker_conn);
    let attacker_cwd = temp_dir("agentos-acp-cross-conn-drive-attacker-cwd");
    let attacker_vm = create_vm(
        &mut sidecar,
        &attacker_conn,
        &attacker_session,
        &attacker_cwd,
    );

    // Attacker tries to DRIVE the victim's adapter by its session id. The mock
    // adapter expects `session/prompt` to be RPC id 3 (the victim's first drive);
    // if the attacker's prompt is forwarded it consumes that id and corrupts the
    // victim's request stream.
    let attacker_prompt = dispatch_acp(
        &mut sidecar,
        6,
        &attacker_conn,
        &attacker_session,
        &attacker_vm,
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("adapter-session"),
            method: String::from("session/prompt"),
            params: Some(String::from(
                r#"{"prompt":[{"type":"text","text":"attacker"}]}"#,
            )),
        }),
    );
    assert_indistinguishable_deny(
        attacker_prompt,
        "cross-connection PROMPT of another connection's ACP session",
    );

    // Attacker also tries to cancel the victim's session.
    let attacker_cancel = dispatch_acp(
        &mut sidecar,
        7,
        &attacker_conn,
        &attacker_session,
        &attacker_vm,
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("adapter-session"),
            method: String::from("session/cancel"),
            params: Some(String::from("{}")),
        }),
    );
    assert_indistinguishable_deny(
        attacker_cancel,
        "cross-connection CANCEL of another connection's ACP session",
    );

    // The ownership guard precedes method dispatch, so it is method-agnostic: a
    // state-mutating method (session/set_mode) is denied on the same path, before
    // it could mutate the victim's mode/config.
    let attacker_set_mode = dispatch_acp(
        &mut sidecar,
        8,
        &attacker_conn,
        &attacker_session,
        &attacker_vm,
        AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("adapter-session"),
            method: String::from("session/set_mode"),
            params: Some(String::from(r#"{"modeId":"plan"}"#)),
        }),
    );
    assert_indistinguishable_deny(
        attacker_set_mode,
        "cross-connection session/set_mode of another connection's ACP session",
    );

    // No side effect: owner-visible state must remain readable, open, and
    // unchanged by the attacker's denied set_mode attempt.
    let owner_state = dispatch_acp(
        &mut sidecar,
        9,
        &victim_conn,
        &victim_session,
        &victim_vm,
        AcpRequest::AcpGetSessionStateRequest(AcpGetSessionStateRequest {
            session_id: String::from("adapter-session"),
        }),
    );
    let AcpResponse::AcpSessionStateResponse(owner_state) = owner_state else {
        panic!(
            "victim ACP session must remain readable after an attacker's \
             cross-connection drive attempts, got {owner_state:?}"
        );
    };
    assert!(
        !owner_state.closed,
        "victim ACP session must remain open after denied cross-connection drives"
    );
    let modes: Value =
        serde_json::from_str(owner_state.modes.as_deref().expect("modes")).expect("modes json");
    assert_eq!(
        modes["currentModeId"], "default",
        "denied cross-connection set_mode must not mutate victim mode state"
    );

    close_owned_session(&mut sidecar, 10, &victim_conn, &victim_session, &victim_vm);
}

/// Assert an ACP response is a deny that is INDISTINGUISHABLE from a missing
/// session: the shared ACP core's `invalid_state` code and unknown-session
/// message. This locks in the no-existence-leak property
/// — a non-owner must learn nothing (not even that the session exists), so a
/// regression to a distinguishable error (e.g. an `unauthorized` code) fails here.
#[track_caller]
fn assert_indistinguishable_deny(response: AcpResponse, what: &str) {
    let AcpResponse::AcpErrorResponse(error) = response else {
        panic!("{what} must be DENIED with an error response, but it returned: {response:?}");
    };
    assert_eq!(
        error.code, "invalid_state",
        "{what} must fail closed with the same code as a missing session (no \
         'unauthorized' existence oracle); got code {:?} / message {:?}",
        error.code, error.message
    );
    assert!(
        error.message.contains("unknown ACP session"),
        "{what} must read like a missing session (no existence leak); got: {:?}",
        error.message
    );
}

fn install_default_acp_callback_handler(sidecar: &mut NativeSidecar<RecordingBridge>) {
    sidecar.set_wire_sidecar_request_handler(|frame| match frame.payload {
        SidecarRequestPayload::ExtEnvelope(envelope) => {
            assert_eq!(envelope.namespace, ACP_EXTENSION_NAMESPACE);
            let callback: AcpCallback =
                serde_bare::from_slice(&envelope.payload).expect("decode ACP callback");
            let response = match callback {
                AcpCallback::AcpPermissionCallback(callback) => {
                    assert_eq!(callback.cleanup_after_ms, 125_000);
                    assert!(
                        callback.cleanup_after_ms > 120_000,
                        "client cleanup must occur after the sidecar permission deadline"
                    );
                    AcpCallbackResponse::AcpPermissionCallbackResponse(
                        AcpPermissionCallbackResponse {
                            permission_id: callback.permission_id,
                            reply: Some(String::from("once")),
                        },
                    )
                }
                AcpCallback::AcpHostRequestCallback(callback) => panic!(
                    "native ACP filesystem requests must not reach the client callback: {}",
                    callback.request
                ),
            };
            Ok(SidecarResponseFrame {
                schema: frame.schema,
                request_id: frame.request_id,
                ownership: frame.ownership,
                payload: SidecarResponsePayload::ExtEnvelope(ExtEnvelope {
                    namespace: envelope.namespace,
                    payload: serde_bare::to_vec(&response).expect("encode callback response"),
                }),
            })
        }
        other => panic!("unexpected sidecar callback: {other:?}"),
    });
}

fn close_owned_session(
    sidecar: &mut NativeSidecar<RecordingBridge>,
    request_id: i64,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
) {
    let closed = dispatch_acp(
        sidecar,
        request_id,
        connection_id,
        session_id,
        vm_id,
        AcpRequest::AcpCloseSessionRequest(AcpCloseSessionRequest {
            session_id: String::from("adapter-session"),
        }),
    );
    assert!(
        matches!(closed, AcpResponse::AcpSessionClosedResponse(_)),
        "owner cleanup close must succeed, got {closed:?}"
    );
}

fn decode_single_acp_session_event(events: &[EventFrame]) -> Value {
    assert_eq!(events.len(), 1);
    let EventPayload::ExtEnvelope(envelope) = &events[0].payload else {
        panic!("unexpected event: {:?}", events[0]);
    };
    assert_eq!(envelope.namespace, ACP_EXTENSION_NAMESPACE);
    let event: AcpEvent = serde_bare::from_slice(&envelope.payload).expect("decode ACP event");
    let AcpEvent::AcpSessionEvent(event) = event else {
        panic!("expected an AcpSessionEvent, got {event:?}");
    };
    serde_json::from_str(&event.notification).expect("synthetic notification json")
}

fn dispatch_acp(
    sidecar: &mut NativeSidecar<RecordingBridge>,
    request_id: i64,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    request: AcpRequest,
) -> AcpResponse {
    dispatch_acp_with_events(
        sidecar,
        request_id,
        connection_id,
        session_id,
        vm_id,
        request,
    )
    .0
}

fn register_math_toolkit(
    sidecar: &mut NativeSidecar<RecordingBridge>,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
) {
    let result = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: agentos_native_sidecar::wire::protocol_schema(),
            request_id: 3,
            ownership: OwnershipScope::VmOwnership(VmOwnership {
                connection_id: connection_id.to_owned(),
                session_id: session_id.to_owned(),
                vm_id: vm_id.to_owned(),
            }),
            payload: RequestPayload::RegisterHostCallbacksRequest(
                RegisterHostCallbacksRequest {
                    name: String::from("math"),
                    description: String::from("Math utilities"),
                    callbacks: HashMap::from([(
                        String::from("add"),
                        RegisteredHostCallbackDefinition {
                            description: String::from("Add two numbers"),
                            input_schema: String::from(
                                r#"{"type":"object","properties":{"a":{"type":"number"},"b":{"type":"number"}},"required":["a","b"]}"#,
                            ),
                            timeout_ms: None,
                            examples: vec![RegisteredHostCallbackExample {
                                description: String::from("Add 1 and 2"),
                                input: String::from(r#"{"a":1,"b":2}"#),
                            }],
                        },
                    )]),
                },
            ),
        })
        .expect("register host toolkit");
    assert!(matches!(
        result.response.payload,
        ResponsePayload::HostCallbacksRegisteredResponse(_)
    ));
}

fn dispatch_acp_with_events(
    sidecar: &mut NativeSidecar<RecordingBridge>,
    request_id: i64,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    request: AcpRequest,
) -> (AcpResponse, Vec<EventFrame>) {
    let payload = serde_bare::to_vec(&request).expect("encode ACP request");
    let result = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: agentos_native_sidecar::wire::protocol_schema(),
            request_id,
            ownership: OwnershipScope::VmOwnership(VmOwnership {
                connection_id: connection_id.to_owned(),
                session_id: session_id.to_owned(),
                vm_id: vm_id.to_owned(),
            }),
            payload: RequestPayload::ExtEnvelope(ExtEnvelope {
                namespace: String::from(ACP_EXTENSION_NAMESPACE),
                payload,
            }),
        })
        .expect("dispatch ACP extension request");

    match result.response.payload {
        ResponsePayload::ExtEnvelope(envelope) => {
            assert_eq!(envelope.namespace, ACP_EXTENSION_NAMESPACE);
            (
                serde_bare::from_slice(&envelope.payload).expect("decode ACP response"),
                result.events,
            )
        }
        other => panic!("unexpected sidecar response: {other:?}"),
    }
}

fn adapter_script() -> &'static str {
    r#"
import readline from "node:readline";

const lines = readline.createInterface({ input: process.stdin });
let pendingPrompt = null;
let pendingMode = null;

function writeError(id, message) {
  console.log(JSON.stringify({
    jsonrpc: "2.0",
    id,
    error: { code: -32000, message }
  }));
}

for await (const line of lines) {
  if (!line.trim()) continue;
  const message = JSON.parse(line);
  if (!message.method && pendingMode !== null) {
    if (message.result?.content !== "host callback ok") {
      writeError(pendingMode, "unexpected host callback response");
      pendingMode = null;
      continue;
    }
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: pendingMode,
      result: {}
    }));
    pendingMode = null;
  } else if (!message.method && pendingPrompt !== null) {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: pendingPrompt,
      result: {
        echo: "hello",
        sessionId: "adapter-session",
        permissionOutcome: message.result.outcome
      }
    }));
    pendingPrompt = null;
  } else if (message.method === "initialize") {
    if (message.id !== 1) {
      writeError(message.id, `expected initialize id 1, got ${message.id}`);
      continue;
    }
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        protocolVersion: message.params.protocolVersion,
        agentInfo: {
          name: "mock-acp-adapter",
          args: process.argv.slice(2),
        },
        configOptions: []
      }
    }));
  } else if (message.method === "session/new") {
    if (message.id !== 2) {
      writeError(message.id, `expected session/new id 2, got ${message.id}`);
      continue;
    }
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        update: {
          sessionUpdate: "current_mode_update",
          currentModeId: "bootstrap"
        }
      }
    }));
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        sessionId: "adapter-session",
        modes: { currentModeId: "default", availableModes: [] },
        models: {
          currentModelId: "fast-model",
          availableModels: [{ modelId: "fast-model", name: "Fast Model" }]
        }
      }
    }));
  } else if (message.method === "session/prompt") {
    if (message.id !== 3) {
      writeError(message.id, `expected session/prompt id 3, got ${message.id}`);
      continue;
    }
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        update: {
          sessionUpdate: "current_mode_update",
          currentModeId: "ask"
        }
      }
    }));
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { text: "agent says hello" }
        }
      }
    }));
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: 99,
      method: "session/request_permission",
      params: {
        permissionId: "perm-1",
        reason: "Need approval",
        options: [
          { optionId: "once", kind: "allow_once" },
          { optionId: "always", kind: "allow_always" },
          { optionId: "reject", kind: "reject_once" }
        ]
      }
    }));
    pendingPrompt = message.id;
  } else if (message.method === "session/set_mode") {
    if (message.id !== 4) {
      writeError(message.id, `expected session/set_mode id 4, got ${message.id}`);
      continue;
    }
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: 100,
      method: "fs/read_text_file",
      params: {
        sessionId: "adapter-session",
        path: "/tmp/host-callback.txt"
      }
    }));
    pendingMode = message.id;
  } else if (message.method === "session/set_config_option") {
    if (message.id !== 5) {
      writeError(message.id, `expected session/set_config_option id 5, got ${message.id}`);
      continue;
    }
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {}
    }));
  } else {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      error: { code: -32601, message: `unknown method ${message.method}` }
    }));
  }
}
"#
}

fn cron_adapter_script() -> &'static str {
    r#"
import readline from "node:readline";

const lines = readline.createInterface({ input: process.stdin });
for await (const line of lines) {
  if (!line.trim()) continue;
  const message = JSON.parse(line);
  if (message.method === "initialize") {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: { protocolVersion: message.params.protocolVersion, agentInfo: { name: "cron-adapter" } }
    }));
  } else if (message.method === "session/new") {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: { sessionId: "cron-session", modes: { currentModeId: "default", availableModes: [] } }
    }));
  } else if (message.method === "session/prompt") {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: { update: { sessionUpdate: "agent_message_chunk", content: { text: "cron complete" } } }
    }));
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: { stopReason: "end_turn" }
    }));
  } else {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      error: { code: -32601, message: `unknown method ${message.method}` }
    }));
  }
}
"#
}

fn terminal_adapter_script() -> &'static str {
    r#"
import readline from "node:readline";

const lines = readline.createInterface({ input: process.stdin });
let promptRequestId = null;
let terminalId = null;
let exitCode = null;
let terminalOutput = "";
let truncated = false;
let unknownMethodCode = null;

for await (const line of lines) {
  if (!line.trim()) continue;
  const message = JSON.parse(line);
  if (!message.method && message.id === 105) {
    unknownMethodCode = message.error.code;
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: promptRequestId,
      result: {
        terminalOutput,
        exitCode,
        truncated,
        unknownMethodCode
      }
    }));
  } else if (!message.method && message.error && promptRequestId !== null) {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: promptRequestId,
      result: {
        sessionId: "terminal-session",
        agentInfo: { name: "terminal-adapter", terminalError: message.error }
      }
    }));
  } else if (message.method === "initialize") {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: { protocolVersion: message.params.protocolVersion, agentInfo: { name: "terminal-adapter" } }
    }));
  } else if (message.method === "session/new") {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: { sessionId: "terminal-session" }
    }));
  } else if (message.method === "session/prompt") {
    promptRequestId = message.id;
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: 100,
      method: "terminal/create",
      params: { command: "terminal-fixture", cols: 80, rows: 24 }
    }));
  } else if (message.id === 100) {
    terminalId = message.result.terminalId;
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: 104,
      method: "terminal/write",
      params: { terminalId, data: "printf native-terminal; exit 7\n" }
    }));
  } else if (message.id === 104) {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: 101,
      method: "terminal/wait_for_exit",
      params: { terminalId }
    }));
  } else if (message.id === 101) {
    exitCode = message.result.exitCode;
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: 102,
      method: "terminal/output",
      params: { terminalId }
    }));
  } else if (message.id === 102) {
    terminalOutput = message.result.output;
    truncated = message.result.truncated;
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: 103,
      method: "terminal/release",
      params: { terminalId }
    }));
  } else if (message.id === 103) {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: 105,
      method: "host/not-found",
      params: {}
    }));
  } else {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      error: { code: -32601, message: `unknown method ${message.method}` }
    }));
  }
}
"#
}

fn assert_node_available() {
    let output = Command::new("node")
        .arg("--version")
        .output()
        .expect("spawn node --version");
    assert!(output.status.success(), "node must be available");
}

fn new_sidecar(name: &str) -> NativeSidecar<RecordingBridge> {
    NativeSidecar::with_config_and_extensions(
        RecordingBridge::default(),
        NativeSidecarConfig {
            sidecar_id: format!("sidecar-{name}"),
            compile_cache_root: Some(temp_dir(name).join("cache")),
            ..NativeSidecarConfig::default()
        },
        agentos_sidecar_wrapper::extensions(),
    )
    .expect("create native sidecar")
}

fn authenticate(sidecar: &mut NativeSidecar<RecordingBridge>) -> String {
    let result = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: agentos_native_sidecar::wire::protocol_schema(),
            request_id: 1,
            ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
                connection_id: String::from("client"),
            }),
            payload: RequestPayload::AuthenticateRequest(AuthenticateRequest {
                client_name: String::from("acp-extension-test"),
                auth_token: String::new(),
                protocol_version: agentos_native_sidecar::wire::PROTOCOL_VERSION,
                bridge_version: agentos_bridge::bridge_contract().version,
            }),
        })
        .expect("authenticate");
    match result.response.payload {
        ResponsePayload::AuthenticatedResponse(response) => response.connection_id,
        other => panic!("unexpected auth response: {other:?}"),
    }
}

fn open_session(sidecar: &mut NativeSidecar<RecordingBridge>, connection_id: &str) -> String {
    let result = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: agentos_native_sidecar::wire::protocol_schema(),
            request_id: 2,
            ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
                connection_id: connection_id.to_owned(),
            }),
            payload: RequestPayload::OpenSessionRequest(OpenSessionRequest {
                placement: SidecarPlacement::SidecarPlacementShared(SidecarPlacementShared {
                    pool: None,
                }),
            }),
        })
        .expect("open session");
    match result.response.payload {
        ResponsePayload::SessionOpenedResponse(response) => response.session_id,
        other => panic!("unexpected session response: {other:?}"),
    }
}

fn create_vm(
    sidecar: &mut NativeSidecar<RecordingBridge>,
    connection_id: &str,
    session_id: &str,
    cwd: &Path,
) -> String {
    create_vm_with_additional_instructions(sidecar, connection_id, session_id, cwd, None)
}

fn create_vm_with_additional_instructions(
    sidecar: &mut NativeSidecar<RecordingBridge>,
    connection_id: &str,
    session_id: &str,
    cwd: &Path,
    agent_additional_instructions: Option<&str>,
) -> String {
    let result = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: agentos_native_sidecar::wire::protocol_schema(),
            request_id: 3,
            ownership: OwnershipScope::SessionOwnership(SessionOwnership {
                connection_id: connection_id.to_owned(),
                session_id: session_id.to_owned(),
            }),
            payload: RequestPayload::CreateVmRequest(CreateVmRequest {
                runtime: GuestRuntimeKind::JavaScript,
                config: serde_json::to_string(&vm_config::CreateVmConfig {
                    cwd: Some(String::from(GUEST_CWD)),
                    agent_additional_instructions: agent_additional_instructions.map(String::from),
                    permissions: Some(allow_all_permissions()),
                    root_filesystem: Some(vm_config::RootFilesystemConfig {
                        bootstrap_entries: Some(vec![vm_config::RootFilesystemEntry {
                            path: String::from("/tmp/host-callback.txt"),
                            kind: vm_config::RootFilesystemEntryKind::File,
                            mode: Some(0o644),
                            uid: Some(0),
                            gid: Some(0),
                            content: Some(String::from("host callback ok")),
                            encoding: Some(vm_config::RootFilesystemEntryEncoding::Utf8),
                            target: None,
                            executable: false,
                        }]),
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .expect("serialize create VM config"),
            }),
        })
        .expect("create VM");
    let vm_id = match result.response.payload {
        ResponsePayload::VmCreatedResponse(response) => response.vm_id,
        other => panic!("unexpected create VM response: {other:?}"),
    };
    bootstrap_mock_agents(sidecar, connection_id, session_id, &vm_id, cwd);
    vm_id
}

fn bootstrap_mock_agents(
    sidecar: &mut NativeSidecar<RecordingBridge>,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    cwd: &Path,
) {
    let adapter = cwd.join("adapter.mjs");
    if !adapter.exists() {
        return;
    }
    let script = fs::read_to_string(&adapter).expect("read mock adapter");
    let package_dir = cwd.join("packages").join("pi");
    let bin_dir = package_dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("create mock agent bin dir");
    let manifest = serde_json::json!({
        "name": "pi",
        "version": "0.0.0",
        "agent": { "acpEntrypoint": "pi" },
    })
    .to_string();
    fs::write(package_dir.join("agentos-package.json"), manifest)
        .expect("write mock agent manifest");
    fs::write(bin_dir.join("pi"), script).expect("write mock agent command");
    fs::write(
        bin_dir.join("terminal-fixture"),
        r#"#!/usr/bin/env node
process.stdin.resume();
process.stdin.on("data", () => {
  process.stdout.write("native-terminal");
  process.exit(0);
});
"#,
    )
    .expect("write mock terminal command");
    let result = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: agentos_native_sidecar::wire::protocol_schema(),
            request_id: 30,
            ownership: OwnershipScope::VmOwnership(VmOwnership {
                connection_id: connection_id.to_owned(),
                session_id: session_id.to_owned(),
                vm_id: vm_id.to_owned(),
            }),
            payload: RequestPayload::ConfigureVmRequest(ConfigureVmRequest {
                mounts: None,
                permissions: None,
                command_permissions: None,
                loopback_exempt_ports: None,
                packages: Some(vec![PackageDescriptor::PackagePath(PackagePath {
                    path: package_dir.to_string_lossy().into_owned(),
                })]),
                packages_mount_at: Some(String::from("/opt/agentos")),
            }),
        })
        .expect("configure mock ACP package");
    assert!(matches!(
        result.response.payload,
        ResponsePayload::VmConfiguredResponse(_)
    ));
}

fn allow_all_permissions() -> vm_config::PermissionsPolicy {
    vm_config::PermissionsPolicy {
        fs: Some(vm_config::FsPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        network: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        child_process: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        process: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        env: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
        binding: Some(vm_config::PatternPermissionScope::Mode(
            vm_config::PermissionMode::Allow,
        )),
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "agentos-sidecar-{name}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("create temp dir");
    root
}
