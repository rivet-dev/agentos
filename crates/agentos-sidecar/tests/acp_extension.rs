#[path = "support/bridge.rs"]
mod bridge_support;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use agentos_protocol::generated::v1::{
    AcpCallback, AcpCallbackResponse, AcpCloseSessionRequest, AcpCreateSessionRequest, AcpEvent,
    AcpGetSessionStateRequest, AcpHostRequestCallbackResponse, AcpPermissionCallbackResponse,
    AcpRequest, AcpResponse, AcpRuntimeKind, AcpSessionRequest,
};
use agentos_protocol::{ACP_EXTENSION_NAMESPACE, PROTOCOL_VERSION as ACP_PROTOCOL_VERSION};
use bridge_support::RecordingBridge;
use secure_exec_sidecar::wire::{
    AuthenticateRequest, ConnectionOwnership, CreateVmRequest, EventFrame, EventPayload,
    ExtEnvelope, GuestRuntimeKind, OpenSessionRequest, OwnershipScope, RequestFrame,
    RequestPayload, ResponsePayload, SessionOwnership, SidecarPlacement, SidecarPlacementShared,
    SidecarRequestPayload, SidecarResponseFrame, SidecarResponsePayload, VmOwnership,
};
use secure_exec_sidecar::{NativeSidecar, NativeSidecarConfig};
use secure_exec_vm_config as vm_config;
use serde_json::Value;

#[test]
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
                    AcpCallbackResponse::AcpPermissionCallbackResponse(
                        AcpPermissionCallbackResponse {
                            permission_id: callback.permission_id,
                            reply: String::from("once"),
                        },
                    )
                }
                AcpCallback::AcpHostRequestCallback(callback) => {
                    assert_eq!(callback.session_id, "adapter-session");
                    let request: Value =
                        serde_json::from_str(&callback.request).expect("host callback request");
                    assert_eq!(request["id"], 100);
                    assert_eq!(request["method"], "fs/read_text_file");
                    AcpCallbackResponse::AcpHostRequestCallbackResponse(
                        AcpHostRequestCallbackResponse {
                            response: Some(String::from(
                                r#"{"jsonrpc":"2.0","id":100,"result":{"content":"host callback ok"}}"#,
                            )),
                        },
                    )
                }
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
    let vm_id = create_vm(&mut sidecar, &connection_id, &session_id, &cwd);

    let (created, create_events) = dispatch_acp_with_events(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
            agent_type: String::from("pi"),
            runtime: AcpRuntimeKind::JavaScript,
            adapter_entrypoint: adapter.to_string_lossy().into_owned(),
            cwd: cwd.to_string_lossy().into_owned(),
            args: Vec::new(),
            env: HashMap::new(),
            protocol_version: i32::from(ACP_PROTOCOL_VERSION),
            client_capabilities: String::from(r#"{"fs":{"readTextFile":true}}"#),
            mcp_servers: String::from(r#"{"servers":[]}"#),
            skip_os_instructions: true,
            additional_instructions: Some(String::from("extra guidance")),
        }),
    );

    let AcpResponse::AcpSessionCreatedResponse(created) = created else {
        panic!("unexpected create response: {created:?}");
    };
    assert_eq!(created.session_id, "adapter-session");
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

    let (prompt, prompt_events) = dispatch_acp_with_events(
        &mut sidecar,
        6,
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
    assert_eq!(prompt_events.len(), 1);
    let EventPayload::ExtEnvelope(envelope) = &prompt_events[0].payload else {
        panic!("unexpected prompt event: {:?}", prompt_events[0]);
    };
    assert_eq!(envelope.namespace, ACP_EXTENSION_NAMESPACE);
    let event: AcpEvent = serde_bare::from_slice(&envelope.payload).expect("decode ACP event");
    let AcpEvent::AcpSessionEvent(event) = event else {
        panic!("expected an AcpSessionEvent, got {event:?}");
    };
    assert_eq!(event.session_id, "adapter-session");
    let notification: Value = serde_json::from_str(&event.notification).expect("notification json");
    assert_eq!(notification["method"], "session/update");
    assert_eq!(notification["params"]["update"]["currentModeId"], "ask");

    let (mode_update, mode_events) = dispatch_acp_with_events(
        &mut sidecar,
        7,
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
        8,
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
        9,
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
        10,
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
        11,
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
#[test]
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
            runtime: AcpRuntimeKind::JavaScript,
            adapter_entrypoint: adapter.to_string_lossy().into_owned(),
            cwd: cwd.to_string_lossy().into_owned(),
            args: Vec::new(),
            env: HashMap::new(),
            protocol_version: i32::from(ACP_PROTOCOL_VERSION),
            client_capabilities: String::from(r#"{"fs":{"readTextFile":true}}"#),
            mcp_servers: String::from(r#"{"servers":[]}"#),
            skip_os_instructions: true,
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
        6,
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

    close_owned_session(&mut sidecar, 7, &victim_conn, &victim_session, &victim_vm);
}

/// AOS-ACP-1 (P1 / J.4 cross-connection ACP close): a second connection
/// (attacker) must NOT be able to CLOSE the ACP session created by a different
/// connection (victim). `close_session` (acp_extension.rs:367-408) resolves the
/// session by `session_id` only and does not check `owner_connection_id`, so an
/// attacker connection that knows/guesses the victim's `session_id` can
/// `close_stdin` + `SIGTERM`/`SIGKILL` the victim's adapter process and dispose
/// its resources — a cross-tenant DoS.
///
/// SECURE expectation: the cross-connection close is DENIED (error response) AND
/// the victim's session stays alive (the owner can still read its own state and
/// still drive a prompt afterwards). Today the code is expected to return
/// `AcpSessionClosedResponse` and tear the victim down (FAIL = vuln present).
///
/// Bounded SAFEGUARD-shaped assertion: a single cross-connection close, fast.
#[test]
fn acp_close_session_denies_cross_connection_session_id() {
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
            runtime: AcpRuntimeKind::JavaScript,
            adapter_entrypoint: adapter.to_string_lossy().into_owned(),
            cwd: cwd.to_string_lossy().into_owned(),
            args: Vec::new(),
            env: HashMap::new(),
            protocol_version: i32::from(ACP_PROTOCOL_VERSION),
            client_capabilities: String::from(r#"{"fs":{"readTextFile":true}}"#),
            mcp_servers: String::from(r#"{"servers":[]}"#),
            skip_os_instructions: true,
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

    // SECURE expectation: a different connection must be denied, indistinguishably
    // from a missing session (no existence leak).
    assert_indistinguishable_deny(
        close_result,
        "cross-connection CLOSE of another connection's ACP session",
    );

    // ...and the victim session must remain alive and usable for its owner.
    let owner_state = dispatch_acp(
        &mut sidecar,
        7,
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
#[test]
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
            runtime: AcpRuntimeKind::JavaScript,
            adapter_entrypoint: adapter.to_string_lossy().into_owned(),
            cwd: cwd.to_string_lossy().into_owned(),
            args: Vec::new(),
            env: HashMap::new(),
            protocol_version: i32::from(ACP_PROTOCOL_VERSION),
            client_capabilities: String::from(r#"{"fs":{"readTextFile":true}}"#),
            mcp_servers: String::from(r#"{"servers":[]}"#),
            skip_os_instructions: true,
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
/// session: same `invalid_state` code and the same "unknown ACP session" message
/// a non-existent session produces. This locks in the no-existence-leak property
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
                    AcpCallbackResponse::AcpPermissionCallbackResponse(
                        AcpPermissionCallbackResponse {
                            permission_id: callback.permission_id,
                            reply: String::from("once"),
                        },
                    )
                }
                AcpCallback::AcpHostRequestCallback(callback) => {
                    let request: Value =
                        serde_json::from_str(&callback.request).expect("host callback request");
                    assert_eq!(request["method"], "fs/read_text_file");
                    AcpCallbackResponse::AcpHostRequestCallbackResponse(
                        AcpHostRequestCallbackResponse {
                            response: Some(String::from(
                                r#"{"jsonrpc":"2.0","id":100,"result":{"content":"host callback ok"}}"#,
                            )),
                        },
                    )
                }
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
            schema: secure_exec_sidecar::wire::protocol_schema(),
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
            schema: secure_exec_sidecar::wire::protocol_schema(),
            request_id: 1,
            ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
                connection_id: String::from("client"),
            }),
            payload: RequestPayload::AuthenticateRequest(AuthenticateRequest {
                client_name: String::from("acp-extension-test"),
                auth_token: String::new(),
                protocol_version: secure_exec_sidecar::wire::PROTOCOL_VERSION,
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
            schema: secure_exec_sidecar::wire::protocol_schema(),
            request_id: 2,
            ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
                connection_id: connection_id.to_owned(),
            }),
            payload: RequestPayload::OpenSessionRequest(OpenSessionRequest {
                placement: SidecarPlacement::SidecarPlacementShared(SidecarPlacementShared {
                    pool: None,
                }),
                metadata: HashMap::new(),
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
    let result = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: secure_exec_sidecar::wire::protocol_schema(),
            request_id: 3,
            ownership: OwnershipScope::SessionOwnership(SessionOwnership {
                connection_id: connection_id.to_owned(),
                session_id: session_id.to_owned(),
            }),
            payload: RequestPayload::CreateVmRequest(CreateVmRequest {
                runtime: GuestRuntimeKind::JavaScript,
                config: serde_json::to_string(&vm_config::CreateVmConfig {
                    cwd: Some(cwd.to_string_lossy().into_owned()),
                    permissions: Some(allow_all_permissions()),
                    ..Default::default()
                })
                .expect("serialize create VM config"),
            }),
        })
        .expect("create VM");
    match result.response.payload {
        ResponsePayload::VmCreatedResponse(response) => response.vm_id,
        other => panic!("unexpected create VM response: {other:?}"),
    }
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
