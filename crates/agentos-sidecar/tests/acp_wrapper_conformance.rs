//! End-to-end ACP conformance at the native and browser wrapper boundaries.
//!
//! The lower-level core suite compares the blocking and resumable state machines.
//! This suite deliberately goes one layer higher: requests travel through the real
//! native extension or the real browser wire dispatcher and browser extension.

#[path = "../../bridge/tests/support.rs"]
mod browser_bridge_support;
#[path = "support/bridge.rs"]
mod native_bridge_support;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use agentos_native_sidecar::wire::{
    protocol_schema, AuthenticateRequest, ConfigureVmRequest, ConnectionOwnership, CreateVmRequest,
    DisposeReason, DisposeVmRequest, EventFrame, EventPayload, ExtEnvelope, GuestRuntimeKind,
    InitializeVmRequest, OpenSessionRequest, OwnershipScope, PackageDescriptor, PackageInline,
    PackagePath, ProtocolFrame, RegisterHostCallbacksRequest, RegisteredHostCallbackDefinition,
    RequestFrame, RequestPayload, ResponsePayload, SessionOwnership, SidecarPlacement,
    SidecarPlacementShared, VmOwnership, WireFrameCodec, PROTOCOL_VERSION,
};
use agentos_native_sidecar::{
    EventSinkTransport, NativeSidecar, NativeSidecarConfig, SidecarError,
};
use agentos_native_sidecar_browser::{
    wire_dispatch::BrowserWireDispatcher, BrowserWorkerBridge, BrowserWorkerHandle,
    BrowserWorkerHandleRequest, BrowserWorkerSpawnRequest,
};
use agentos_protocol::generated::v1::{
    AcpAbortPendingRequest, AcpCloseSessionRequest, AcpCreateSessionRequest,
    AcpDeliverAgentOutputRequest, AcpEvent, AcpGetSessionStateRequest, AcpListAgentsRequest,
    AcpListSessionsRequest, AcpPendingAbortReason, AcpRequest, AcpResponse,
    AcpResumeSessionRequest, AcpRuntimeKind, AcpSessionRequest, AcpSetSessionConfigRequest,
};
use agentos_protocol::{ACP_EXTENSION_NAMESPACE, PROTOCOL_VERSION as ACP_PROTOCOL_VERSION};
use browser_bridge_support::RecordingBridge as BrowserBridge;
use native_bridge_support::RecordingBridge as NativeBridge;
use serde_json::{json, Value};

const GUEST_CWD: &str = "/workspace";

const ECHO_AGENT_SOURCE: &str =
    include_str!("../../../packages/browser/tests/fixtures/acp-echo-agent.mjs");
static NATIVE_TEST_LOCK: Mutex<()> = Mutex::new(());

impl BrowserWorkerBridge for BrowserBridge {
    fn create_worker(
        &mut self,
        request: BrowserWorkerSpawnRequest,
    ) -> Result<BrowserWorkerHandle, Self::Error> {
        self.browser_worker_spawns.push(BTreeMap::from([
            (
                String::from("wasm_permission_tier"),
                request
                    .wasm_permission_tier
                    .map(|tier| format!("{tier:?}"))
                    .unwrap_or_default(),
            ),
            (
                String::from("argv"),
                serde_json::to_string(&request.process_config.argv)
                    .expect("serialize browser worker argv"),
            ),
        ]));
        Ok(BrowserWorkerHandle {
            worker_id: format!("wrapper-conformance-worker-{}", request.context_id),
            runtime: request.runtime,
        })
    }

    fn terminate_worker(
        &mut self,
        _request: BrowserWorkerHandleRequest,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[test]
fn browser_wrapper_rejects_inbound_host_requests_during_create() {
    let root = temp_dir("acp-wrapper-inbound-request");
    let request = create_fixture_request(&root);
    let response = run_browser_create(&root, request);
    assert_eq!(
        semantic_created_response(&response)["sessionId"],
        json!("echo-session-1"),
    );
}

#[test]
fn browser_does_not_advertise_host_tools_without_callback_transport() {
    let codec = WireFrameCodec::default();
    let root = temp_dir("acp-browser-no-host-tool-advertising");
    let mut dispatcher = BrowserWireDispatcher::new(BrowserBridge::default());
    dispatcher
        .sidecar_mut()
        .register_extension(Box::new(agentos_sidecar_browser::BrowserAcpExtension::new()))
        .expect("register real browser ACP extension");
    let (vm_id, ownership) = create_browser_vm(&codec, &mut dispatcher, &root);
    project_browser_agent_package(&mut dispatcher, &vm_id, "pi");
    let registered = dispatch_browser(
        &codec,
        &mut dispatcher,
        RequestFrame {
            schema: protocol_schema(),
            request_id: 4,
            ownership: ownership.clone(),
            payload: RequestPayload::RegisterHostCallbacksRequest(RegisterHostCallbacksRequest {
                name: String::from("browser-unroutable-tool"),
                description: String::from("UNROUTABLE_BROWSER_TOOL_MARKER"),
                callbacks: HashMap::from([(
                    String::from("call"),
                    RegisteredHostCallbackDefinition {
                        description: String::from("UNROUTABLE_BROWSER_TOOL_MARKER"),
                        input_schema: String::from(r#"{"type":"object"}"#),
                        timeout_ms: None,
                        examples: Vec::new(),
                    },
                )]),
            }),
        },
    );
    assert!(matches!(
        registered.payload,
        ResponsePayload::HostCallbacksRegisteredResponse(_)
    ));
    let mut request = create_fixture_request(&root);
    let AcpRequest::AcpCreateSessionRequest(create) = &mut request else {
        unreachable!();
    };
    create.agent_type = String::from("pi");
    create.skip_os_instructions = Some(false);
    let response = dispatch_browser_acp(&codec, &mut dispatcher, ownership, 5, request);
    assert!(matches!(response, AcpResponse::AcpPendingResponse(_)));

    let spawn = &dispatcher.sidecar_mut().bridge().browser_worker_spawns[0];
    let argv = spawn.get("argv").expect("recorded browser worker argv");
    assert!(argv.contains("--append-system-prompt"));
    assert!(!argv.contains("UNROUTABLE_BROWSER_TOOL_MARKER"));
    assert!(!argv.contains("browser-unroutable-tool"));
}

#[test]
fn browser_initialize_vm_projects_real_packed_agent_then_lists_and_creates_it() {
    let codec = WireFrameCodec::default();
    let root = temp_dir("acp-browser-initialize-packed-agent");
    let mut dispatcher = BrowserWireDispatcher::new(BrowserBridge::default());
    dispatcher
        .sidecar_mut()
        .register_extension(Box::new(agentos_sidecar_browser::BrowserAcpExtension::new()))
        .expect("register real browser ACP extension");

    let authenticated = dispatch_browser(
        &codec,
        &mut dispatcher,
        RequestFrame {
            schema: protocol_schema(),
            request_id: 1,
            ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
                connection_id: String::from("packed-bootstrap"),
            }),
            payload: RequestPayload::AuthenticateRequest(AuthenticateRequest {
                client_name: String::from("packed-browser"),
                auth_token: String::new(),
                protocol_version: PROTOCOL_VERSION,
                bridge_version: agentos_bridge::bridge_contract().version,
            }),
        },
    );
    let ResponsePayload::AuthenticatedResponse(authenticated) = authenticated.payload else {
        panic!("unexpected authentication response");
    };
    let opened = dispatch_browser(
        &codec,
        &mut dispatcher,
        RequestFrame {
            schema: protocol_schema(),
            request_id: 2,
            ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
                connection_id: authenticated.connection_id.clone(),
            }),
            payload: RequestPayload::OpenSessionRequest(OpenSessionRequest {
                placement: SidecarPlacement::SidecarPlacementShared(SidecarPlacementShared {
                    pool: None,
                }),
            }),
        },
    );
    let ResponsePayload::SessionOpenedResponse(opened) = opened.payload else {
        panic!("unexpected session-open response");
    };
    let create = CreateVmRequest::json_config(
        GuestRuntimeKind::JavaScript,
        agentos_vm_config::CreateVmConfig {
            cwd: Some(String::from(GUEST_CWD)),
            ..Default::default()
        },
    );
    let initialized = dispatch_browser(
        &codec,
        &mut dispatcher,
        RequestFrame {
            schema: protocol_schema(),
            request_id: 3,
            ownership: OwnershipScope::SessionOwnership(SessionOwnership {
                connection_id: authenticated.connection_id.clone(),
                session_id: opened.session_id.clone(),
            }),
            payload: RequestPayload::InitializeVmRequest(InitializeVmRequest {
                runtime: create.runtime,
                config: create.config,
                mounts: None,
                packages: Some(vec![PackageDescriptor::PackageInline(PackageInline {
                    content: packed_browser_agent_package("echo"),
                })]),
                packages_mount_at: Some(String::from("/opt/agentos")),
                host_callbacks: None,
            }),
        },
    );
    let ResponsePayload::VmInitializedResponse(initialized) = initialized.payload else {
        panic!(
            "unexpected initialize-VM response: {:?}",
            initialized.payload
        );
    };
    assert_eq!(initialized.agents.len(), 1);
    assert_eq!(initialized.agents[0].id, "echo");
    assert!(initialized
        .projected_commands
        .iter()
        .any(|command| command.guest_path == "/opt/agentos/bin/echo-agent"));
    assert_eq!(
        dispatcher
            .sidecar_mut()
            .read_file(&initialized.vm_id, "/opt/agentos/bin/echo-agent")
            .expect("read projected command"),
        ECHO_AGENT_SOURCE.as_bytes()
    );
    let manifest_error = dispatcher
        .sidecar_mut()
        .read_file(
            &initialized.vm_id,
            "/opt/agentos/pkgs/echo/current/agentos-package.json",
        )
        .expect_err("toolchain manifest must be stripped from the packed projection");
    assert!(manifest_error.to_string().contains("ENOENT"));

    let ownership = OwnershipScope::VmOwnership(VmOwnership {
        connection_id: authenticated.connection_id,
        session_id: opened.session_id,
        vm_id: initialized.vm_id,
    });
    let listed = dispatch_browser_acp(
        &codec,
        &mut dispatcher,
        ownership.clone(),
        4,
        AcpRequest::AcpListAgentsRequest(AcpListAgentsRequest { reserved: false }),
    );
    assert!(matches!(
        listed,
        AcpResponse::AcpListAgentsResponse(ref agents)
            if agents.agents.len() == 1 && agents.agents[0].id == "echo"
    ));

    let mut response = dispatch_browser_acp(
        &codec,
        &mut dispatcher,
        ownership.clone(),
        5,
        create_fixture_request(&root),
    );
    let mut write_cursor = 0;
    let mut request_id = 6;
    while let AcpResponse::AcpPendingResponse(pending) = response {
        let outbound: Value = {
            let bridge = dispatcher.sidecar_mut().bridge();
            let write = bridge
                .stdin_writes
                .get(write_cursor)
                .expect("pending ACP bootstrap produced a write");
            serde_json::from_slice(&write.chunk).expect("decode adapter request")
        };
        write_cursor += 1;
        let mut chunk = Vec::new();
        for message in adapter_output_for(&outbound, AdapterFixtureBehavior::default()) {
            chunk.extend(serde_json::to_vec(&message).expect("encode adapter output"));
            chunk.push(b'\n');
        }
        response = dispatch_browser_acp(
            &codec,
            &mut dispatcher,
            ownership.clone(),
            request_id,
            deliver_output_request(&pending.process_id, &chunk),
        );
        request_id += 1;
    }
    assert_eq!(
        semantic_created_response(&response)["sessionId"],
        json!("echo-session-1")
    );
}

#[test]
fn native_and_browser_wrapper_conformance() {
    native_and_browser_wrappers_match_full_session_lifecycle();
    native_and_browser_wrappers_match_native_and_fallback_resume_paths();
    native_and_browser_wrappers_scope_identical_adapter_session_ids_by_exact_owner();
    native_wrapper_retries_committed_event_after_one_shot_sink_failure();
}

fn native_and_browser_wrappers_match_full_session_lifecycle() {
    let _native_guard = lock_native_tests();
    assert_node_available();
    let root = temp_dir("acp-wrapper-lifecycle");
    let package_dir = prepare_echo_package(&root);
    let mut native =
        NativeLifecycleHarness::new_with_packages_mount_at(&root, &package_dir, "/srv/agentos");
    let mut browser = BrowserLifecycleHarness::new(&root);

    assert_wrapper_step_eq(
        "list projected agents",
        native.request(list_agents_request()),
        browser.request(list_agents_request()),
    );

    let native_create = native.request(create_fixture_request(&root));
    let browser_create = browser.request(create_fixture_request(&root));
    assert_wrapper_step_eq("create", native_create, browser_create.clone());
    assert_eq!(browser_create.writes.len(), 2);
    assert_eq!(browser_create.writes[0]["method"], json!("initialize"));
    assert_eq!(browser_create.writes[1]["method"], json!("session/new"));
    let native_prompt = native.request(prompt_request());
    let browser_prompt = browser.request(prompt_request());
    assert_wrapper_step_eq("prompt", native_prompt.clone(), browser_prompt.clone());
    assert_eq!(native_prompt.response["text"], json!("echo: hello"));
    assert_eq!(native_prompt.events.len(), 1);
    assert_eq!(
        native_prompt.events[0]["notification"]["method"],
        json!("session/update"),
    );
    assert_eq!(
        native_prompt.events[0]["notification"]["params"]["update"]["content"]["text"],
        json!("echo: hello"),
    );
    assert_eq!(browser_prompt.writes.len(), 1);
    assert_eq!(browser_prompt.writes[0]["method"], json!("session/prompt"));

    let native_config = native.request(writable_config_request());
    let browser_config = browser.request(writable_config_request());
    assert_wrapper_step_eq(
        "writable config",
        native_config.clone(),
        browser_config.clone(),
    );
    assert_eq!(native_config.events.len(), 1);
    assert_eq!(
        native_config.events[0]["notification"]["params"]["update"]["sessionUpdate"],
        json!("config_option_update"),
    );
    assert_eq!(
        native_config.events[0]["notification"]["params"]["update"]["configOptions"][0]
            ["currentValue"],
        json!("detailed"),
    );
    assert_eq!(browser_config.writes.len(), 1);
    assert_eq!(
        browser_config.writes[0]["method"],
        json!("session/set_config_option"),
    );
    let native_state = native.request(state_request());
    let browser_state = browser.request(state_request());
    assert_wrapper_step_eq("state after config", native_state.clone(), browser_state);
    assert_eq!(
        native_state.response["configOptions"][0]["currentValue"],
        json!("detailed"),
        "writable config must update authoritative wrapper state",
    );
    let native_read_only = native.request(read_only_config_request());
    let browser_read_only = browser.request(read_only_config_request());
    assert_wrapper_step_eq(
        "read-only config",
        native_read_only.clone(),
        browser_read_only.clone(),
    );
    assert_eq!(
        native_read_only.response["response"]["error"]["code"],
        -32601
    );
    assert!(native_read_only.events.is_empty());
    assert!(native_read_only.writes.is_empty());
    assert!(browser_read_only.writes.is_empty());
    let native_cancel = native.request(cancel_request());
    let browser_cancel = browser.request(cancel_request());
    assert_wrapper_step_eq(
        "cancel fallback",
        native_cancel.clone(),
        browser_cancel.clone(),
    );
    assert_eq!(
        native_cancel.response["response"]["result"]["via"],
        json!("notification-fallback"),
        "method-not-found cancellation must use the notification fallback",
    );
    assert_eq!(browser_cancel.writes.len(), 2);
    assert_eq!(browser_cancel.writes[1]["method"], json!("session/cancel"));
    assert!(browser_cancel.writes[1].get("id").is_none());
    assert_wrapper_step_eq(
        "list before close",
        native.request(list_request()),
        browser.request(list_request()),
    );

    let native_sibling = native.sibling_ownership(&root);
    let browser_sibling = browser.sibling_ownership(&root);
    for (label, request) in [
        ("state", state_request()),
        ("prompt", prompt_request()),
        ("config", writable_config_request()),
        ("cancel", cancel_request()),
    ] {
        let native_step = native.request_from(native_sibling.clone(), request.clone());
        let browser_step = browser.request_from(browser_sibling.clone(), request);
        assert_wrapper_step_eq(
            &format!("cross-owner {label}"),
            native_step.clone(),
            browser_step.clone(),
        );
        assert_eq!(
            native_step.response["code"],
            json!("session_not_found"),
            "cross-owner {label} must fail closed without revealing the session",
        );
        assert!(native_step.events.is_empty());
        assert!(browser_step.events.is_empty());
        assert!(
            native_step.writes.is_empty(),
            "rejected {label} wrote stdin"
        );
        assert!(
            browser_step.writes.is_empty(),
            "rejected {label} wrote stdin"
        );
    }
    assert_wrapper_step_eq(
        "cross-owner close is an idempotent no-op",
        native.request_from(native_sibling, close_request()),
        browser.request_from(browser_sibling, close_request()),
    );
    assert_wrapper_step_eq(
        "owner state survives cross-owner close",
        native.request(state_request()),
        browser.request(state_request()),
    );

    assert_wrapper_step_eq(
        "close",
        native.request(close_request()),
        browser.request(close_request()),
    );
    assert_wrapper_step_eq(
        "idempotent close",
        native.request(close_request()),
        browser.request(close_request()),
    );
    assert_wrapper_step_eq(
        "state absence after close",
        native.request(state_request()),
        browser.request(state_request()),
    );
    let native_list = native.request(list_request());
    let browser_list = browser.request(list_request());
    assert_wrapper_step_eq("list after close", native_list.clone(), browser_list);
    assert_eq!(native_list.response["sessions"], json!([]));
    assert_eq!(browser.resource_counts(), (0, 0, 0, 0, 0, 0, 0));
    let native_disposed = native.dispose_vm(native.ownership.clone());
    assert!(matches!(
        native_disposed,
        ResponsePayload::VmDisposedResponse(_)
    ));
    let disposed = browser.dispose_vm(browser.ownership.clone());
    assert!(matches!(disposed, ResponsePayload::VmDisposedResponse(_)));
    assert_eq!(browser.resource_counts(), (0, 0, 0, 0, 0, 0, 0));
}

#[derive(Default)]
struct FailOnceRecordingEventSink {
    attempts: AtomicUsize,
    events: Mutex<Vec<EventFrame>>,
}

impl EventSinkTransport for FailOnceRecordingEventSink {
    fn emit_event(&self, event: EventFrame) -> Result<(), SidecarError> {
        if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(SidecarError::Io(String::from(
                "injected one-shot ACP event sink failure",
            )));
        }
        self.events.lock().expect("record event").push(event);
        Ok(())
    }
}

fn native_wrapper_retries_committed_event_after_one_shot_sink_failure() {
    let _native_guard = lock_native_tests();
    assert_node_available();
    let root = temp_dir("acp-wrapper-event-acknowledgement");
    let package_dir = prepare_echo_package(&root);
    let mut native = NativeLifecycleHarness::new(&root, &package_dir);
    let owner_a = native.ownership.clone();
    let owner_b = native.sibling_ownership(&root);
    assert_eq!(
        native
            .request_from(owner_a.clone(), create_fixture_request(&root))
            .response["type"],
        json!("created")
    );
    assert_eq!(
        native
            .request_from(owner_b.clone(), create_fixture_request(&root))
            .response["type"],
        json!("created")
    );

    let sink = Arc::new(FailOnceRecordingEventSink::default());
    native.sidecar.set_event_transport(sink.clone());
    let prompt = native.request_from(owner_a.clone(), prompt_request());
    assert_eq!(prompt.response["type"], json!("rpc"));
    assert_eq!(prompt.response["text"], json!("echo: hello"));
    assert!(prompt.events.is_empty());
    assert_eq!(sink.attempts.load(Ordering::SeqCst), 1);
    assert!(sink.events.lock().expect("inspect events").is_empty());

    let sibling_state = native.request_from(owner_b, state_request());
    assert_eq!(sibling_state.response["type"], json!("state"));
    assert_eq!(
        sink.attempts.load(Ordering::SeqCst),
        1,
        "owner B must neither receive nor acknowledge owner A's retained event"
    );
    assert!(sink.events.lock().expect("inspect events").is_empty());

    let state = native.request_from(owner_a.clone(), state_request());
    assert_eq!(state.response["type"], json!("state"));
    assert_eq!(sink.attempts.load(Ordering::SeqCst), 2);
    let delivered = sink.events.lock().expect("inspect retried event");
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].ownership, owner_a);
    let event = semantic_event_payload(&delivered[0].payload).expect("semantic ACP event");
    assert_eq!(event["type"], json!("session"));
    assert_eq!(event["notification"]["method"], json!("session/update"));
    drop(delivered);

    native.request_from(owner_a, state_request());
    assert_eq!(
        sink.attempts.load(Ordering::SeqCst),
        2,
        "acknowledged events must not be delivered twice"
    );
}

fn native_and_browser_wrappers_match_native_and_fallback_resume_paths() {
    let _native_guard = lock_native_tests();
    assert_node_available();
    for (label, env, expected_mode, expect_error) in [
        (
            "native load",
            vec![("ECHO_LOAD_SESSION", "1")],
            Some("native"),
            false,
        ),
        ("capability fallback", vec![], Some("fallback"), false),
        (
            "unknown native session fallback",
            vec![("ECHO_LOAD_SESSION", "1"), ("ECHO_UNKNOWN_SESSION", "1")],
            Some("fallback"),
            false,
        ),
        (
            "native load failure",
            vec![("ECHO_LOAD_SESSION", "1"), ("ECHO_LOAD_FAILURE", "1")],
            None,
            true,
        ),
    ] {
        let root = temp_dir(&format!("acp-wrapper-resume-{}", label.replace(' ', "-")));
        let package_dir = prepare_echo_package(&root);
        let mut native = NativeLifecycleHarness::new(&root, &package_dir);
        let mut browser = BrowserLifecycleHarness::new(&root);
        let native_step = native.request(resume_request("durable-session", &env));
        let browser_step = browser.request(resume_request("durable-session", &env));

        if expect_error {
            assert_eq!(
                native_step.response["type"],
                json!("error"),
                "native {label}"
            );
            assert_eq!(
                browser_step.response["type"],
                json!("error"),
                "browser {label}"
            );
            assert_eq!(native_step.response["code"], browser_step.response["code"]);
            assert!(native_step.response["message"]
                .as_str()
                .is_some_and(|message| message.contains("session/load failure")));
            assert!(browser_step.response["message"]
                .as_str()
                .is_some_and(|message| message.contains("session/load failure")));
        } else {
            assert_eq!(
                native_step.response["type"],
                json!("resumed"),
                "native {label}"
            );
            assert_eq!(
                browser_step.response["type"],
                json!("resumed"),
                "browser {label}"
            );
            assert_eq!(native_step.response["mode"], json!(expected_mode.unwrap()));
            assert_eq!(browser_step.response["mode"], json!(expected_mode.unwrap()));
            assert_eq!(
                native_step.response["sessionId"],
                browser_step.response["sessionId"]
            );
        }
        assert_eq!(
            native_step.events, browser_step.events,
            "events for {label}"
        );
        let native_disposed = native.dispose_vm(native.ownership.clone());
        assert!(
            matches!(native_disposed, ResponsePayload::VmDisposedResponse(_)),
            "unexpected native VM disposal response: {native_disposed:?}"
        );
        let browser_disposed = browser.dispose_vm(browser.ownership.clone());
        assert!(matches!(
            browser_disposed,
            ResponsePayload::VmDisposedResponse(_)
        ));
    }
}

#[test]
fn browser_wrapper_pending_interactions_are_owner_scoped_and_abort_releases_execution() {
    let root = temp_dir("acp-wrapper-pending-ownership");
    let mut browser = BrowserLifecycleHarness::new(&root);
    let owner = browser.ownership.clone();
    let sibling = browser.sibling_ownership(&root);
    let process_id = browser.begin_pending_create(owner.clone(), &root);
    let killed_before = browser.killed_execution_count();

    let cross_owner_delivery = browser.dispatch_as(
        sibling.clone(),
        deliver_output_request(
            &process_id,
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}
"#,
        ),
    );
    assert_acp_error(
        &cross_owner_delivery,
        "invalid_state",
        &format!("no pending ACP interaction for {process_id}"),
    );
    let cross_owner_abort = browser.dispatch_as(
        sibling,
        abort_pending_request(&process_id, AcpPendingAbortReason::AgentExited),
    );
    assert_acp_error(
        &cross_owner_abort,
        "invalid_state",
        &format!("no pending ACP interaction for {process_id}"),
    );
    assert_eq!(browser.killed_execution_count(), killed_before);

    let owner_delivery = browser.dispatch_as(
        owner.clone(),
        deliver_output_request(
            &process_id,
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}
"#,
        ),
    );
    assert!(matches!(owner_delivery, AcpResponse::AcpPendingResponse(_)));
    assert_eq!(browser.last_adapter_write()["method"], json!("session/new"));

    let owner_abort = browser.dispatch_as(
        owner.clone(),
        abort_pending_request(&process_id, AcpPendingAbortReason::AgentExited),
    );
    assert_acp_error(
        &owner_abort,
        "agent_exited",
        &format!("agent exited before completing the ACP interaction ({process_id})"),
    );
    assert_eq!(browser.killed_execution_count(), killed_before + 1);

    let after_abort = browser.dispatch_as(
        owner,
        deliver_output_request(
            &process_id,
            br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"never-created"}}
"#,
        ),
    );
    assert_acp_error(
        &after_abort,
        "invalid_state",
        &format!("no pending ACP interaction for {process_id}"),
    );
}

#[test]
fn browser_wrapper_vm_disposal_cleans_only_that_owners_pending_interaction() {
    let root = temp_dir("acp-wrapper-pending-disposal");
    let mut browser = BrowserLifecycleHarness::new(&root);
    let owner_a = browser.ownership.clone();
    let owner_b = browser.sibling_ownership(&root);
    let process_a = browser.begin_pending_create(owner_a.clone(), &root);
    let process_b = browser.begin_pending_create(owner_b.clone(), &root);
    let killed_before = browser.killed_execution_count();
    assert_eq!(browser.resource_counts(), (0, 2, 2, 0, 0, 1, 1));

    let disposed = browser.dispose_vm(owner_a.clone());
    assert!(matches!(disposed, ResponsePayload::VmDisposedResponse(_)));
    assert_eq!(
        browser.killed_execution_count(),
        killed_before + 1,
        "disposing VM A must abort exactly VM A's pending ACP execution",
    );
    assert_eq!(browser.resource_counts(), (0, 1, 1, 0, 0, 0, 0));
    let disposed_again = browser.dispose_vm(owner_a);
    assert!(matches!(
        disposed_again,
        ResponsePayload::RejectedResponse(_)
    ));
    assert_eq!(browser.resource_counts(), (0, 1, 1, 0, 0, 0, 0));

    let owner_b_delivery = browser.dispatch_as(
        owner_b.clone(),
        deliver_output_request(
            &process_b,
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}
"#,
        ),
    );
    assert!(matches!(
        owner_b_delivery,
        AcpResponse::AcpPendingResponse(_)
    ));
    let owner_b_abort = browser.dispatch_as(
        owner_b,
        abort_pending_request(&process_b, AcpPendingAbortReason::InteractionTimeout),
    );
    assert_acp_error(
        &owner_b_abort,
        "agent_interaction_timeout",
        &format!("agent interaction timed out ({process_b})"),
    );
    assert_eq!(
        browser.killed_execution_count(),
        killed_before + 2,
        "VM B must remain independently routable and releasable",
    );
    assert_eq!(browser.resource_counts(), (0, 0, 0, 0, 0, 0, 0));

    assert_ne!(process_a, process_b);
}

fn native_and_browser_wrappers_scope_identical_adapter_session_ids_by_exact_owner() {
    let _native_guard = lock_native_tests();
    assert_node_available();
    let root = temp_dir("acp-wrapper-same-session-id");
    let package_dir = prepare_echo_package(&root);
    let mut native = NativeLifecycleHarness::new(&root, &package_dir);
    let mut browser = BrowserLifecycleHarness::new(&root);
    let native_owner_a = native.ownership.clone();
    let native_owner_b = native.sibling_ownership(&root);
    let browser_owner_a = browser.ownership.clone();
    let browser_owner_b = browser.sibling_ownership(&root);

    for (label, native_owner, browser_owner) in [
        ("owner A", native_owner_a.clone(), browser_owner_a.clone()),
        ("owner B", native_owner_b.clone(), browser_owner_b.clone()),
    ] {
        let native_create =
            native.request_from(native_owner.clone(), create_fixture_request(&root));
        let browser_create =
            browser.request_from(browser_owner.clone(), create_fixture_request(&root));
        assert_ne!(
            native_create.response["type"],
            json!("error"),
            "native {label} create failed: {}",
            native_create.response
        );
        assert_eq!(
            native_create.response["response"]["sessionId"],
            json!("echo-session-1"),
            "native {label}"
        );
        assert_eq!(
            browser_create.response["response"]["sessionId"],
            json!("echo-session-1"),
            "browser {label}"
        );

        let native_prompt = native.request_from(native_owner.clone(), prompt_request());
        let browser_prompt = browser.request_from(browser_owner.clone(), prompt_request());
        assert_eq!(
            native_prompt.response["text"],
            json!("echo: hello"),
            "native {label}"
        );
        assert_eq!(
            browser_prompt.response["text"],
            json!("echo: hello"),
            "browser {label}"
        );

        let expected = json!([{ "sessionId": "echo-session-1", "agentType": "echo" }]);
        assert_eq!(
            native.request_from(native_owner, list_request()).response["sessions"],
            expected,
            "native {label}"
        );
        assert_eq!(
            browser.request_from(browser_owner, list_request()).response["sessions"],
            expected,
            "browser {label}"
        );
    }

    assert_eq!(
        native
            .request_from(native_owner_a.clone(), close_request())
            .response["type"],
        json!("closed")
    );
    assert_eq!(
        browser
            .request_from(browser_owner_a.clone(), close_request())
            .response["type"],
        json!("closed")
    );
    assert_eq!(
        native.request_from(native_owner_a, list_request()).response["sessions"],
        json!([])
    );
    assert_eq!(
        browser
            .request_from(browser_owner_a, list_request())
            .response["sessions"],
        json!([])
    );

    for (label, native_step, browser_step) in [
        (
            "state",
            native.request_from(native_owner_b.clone(), state_request()),
            browser.request_from(browser_owner_b.clone(), state_request()),
        ),
        (
            "prompt",
            native.request_from(native_owner_b, prompt_request()),
            browser.request_from(browser_owner_b, prompt_request()),
        ),
    ] {
        assert_ne!(
            native_step.response["type"],
            json!("error"),
            "native {label}"
        );
        assert_ne!(
            browser_step.response["type"],
            json!("error"),
            "browser {label}"
        );
    }
}

#[test]
fn browser_wrapper_restarts_rebinds_retries_and_exhausts_in_core() {
    let root = temp_dir("acp-wrapper-browser-restart");
    let mut browser = BrowserLifecycleHarness::new(&root);
    assert_eq!(
        browser.request(create_fixture_request(&root)).response["type"],
        json!("created")
    );

    for restart_count in 1..=3 {
        let owner = browser.ownership.clone();
        let prompt = browser.dispatch_as(owner.clone(), prompt_request());
        let AcpResponse::AcpPendingResponse(prompt_pending) = prompt else {
            panic!("prompt must be pending before crash, got {prompt:?}");
        };
        assert_eq!(
            browser.next_adapter_request()["method"],
            json!("session/prompt")
        );
        let restart = browser.dispatch_as(
            owner.clone(),
            abort_pending_request_with_exit_code(
                &prompt_pending.process_id,
                AcpPendingAbortReason::AgentExited,
                Some(137),
            ),
        );
        let AcpResponse::AcpPendingResponse(restart_pending) = restart else {
            panic!("restart {restart_count} must be pending, got {restart:?}");
        };
        assert_eq!(restart_pending.timeout_phase, "restart.initialize");
        assert_ne!(restart_pending.process_id, prompt_pending.process_id);
        assert_eq!(
            browser.next_adapter_request()["method"],
            json!("initialize")
        );

        let initialized = browser.dispatch_as(
            owner.clone(),
            deliver_output_request(
                &restart_pending.process_id,
                br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}}
"#,
            ),
        );
        assert!(matches!(initialized, AcpResponse::AcpPendingResponse(_)));
        assert_eq!(
            browser.next_adapter_request()["method"],
            json!("session/load")
        );
        let completed = browser.dispatch_as(
            owner,
            deliver_output_request(
                &restart_pending.process_id,
                br#"{"jsonrpc":"2.0","id":2,"result":{}}
"#,
            ),
        );
        let AcpResponse::AcpErrorResponse(error) = completed else {
            panic!("restart completion must ask the caller to retry");
        };
        assert_eq!(error.code, "invalid_state");
        assert!(error.message.contains("auto-restarted"));
        assert!(error.message.contains("retry the request"));
        let events = drain_browser_acp_events(&browser.codec, &mut browser.dispatcher);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["restart"], json!("restarted"));
        assert_eq!(events[0]["exitCode"], json!(137));

        let retry = browser.request(prompt_request());
        assert_eq!(retry.response["type"], json!("rpc"));
        assert_eq!(retry.response["text"], json!("echo: hello"));
    }

    let owner = browser.ownership.clone();
    let prompt = browser.dispatch_as(owner.clone(), prompt_request());
    let AcpResponse::AcpPendingResponse(prompt_pending) = prompt else {
        panic!("prompt must be pending before exhausted crash");
    };
    assert_eq!(
        browser.next_adapter_request()["method"],
        json!("session/prompt")
    );
    let exhausted = browser.dispatch_as(
        owner,
        abort_pending_request(
            &prompt_pending.process_id,
            AcpPendingAbortReason::AgentExited,
        ),
    );
    let AcpResponse::AcpErrorResponse(error) = exhausted else {
        panic!("exhausted restart must terminate immediately");
    };
    assert_eq!(error.code, "invalid_state");
    assert!(error.message.contains("restart budget exhausted"));
    assert!(error.message.contains("session evicted"));
    let events = drain_browser_acp_events(&browser.codec, &mut browser.dispatcher);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["restart"], json!("exhausted"));
    assert_eq!(
        browser.request(list_request()).response["sessions"],
        json!([])
    );
}

#[test]
fn browser_wrapper_restart_rejects_adapter_without_native_resume() {
    let root = temp_dir("acp-wrapper-browser-restart-unsupported");
    let mut browser = BrowserLifecycleHarness::new(&root);
    browser.request(create_fixture_request(&root));
    let owner = browser.ownership.clone();
    let AcpResponse::AcpPendingResponse(prompt_pending) =
        browser.dispatch_as(owner.clone(), prompt_request())
    else {
        panic!("prompt must be pending before crash");
    };
    assert_eq!(
        browser.next_adapter_request()["method"],
        json!("session/prompt")
    );
    let AcpResponse::AcpPendingResponse(restart_pending) = browser.dispatch_as(
        owner.clone(),
        abort_pending_request(
            &prompt_pending.process_id,
            AcpPendingAbortReason::AgentExited,
        ),
    ) else {
        panic!("restart must begin pending");
    };
    assert_eq!(
        browser.next_adapter_request()["method"],
        json!("initialize")
    );
    let unsupported = browser.dispatch_as(
        owner,
        deliver_output_request(
            &restart_pending.process_id,
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{}}}
"#,
        ),
    );
    let AcpResponse::AcpErrorResponse(error) = unsupported else {
        panic!("unsupported restart must be a typed terminal response");
    };
    assert_eq!(error.code, "invalid_state");
    assert!(error.message.contains("auto-restart unsupported"));
    let events = drain_browser_acp_events(&browser.codec, &mut browser.dispatcher);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["restart"], json!("unsupported"));
}

#[test]
fn browser_wrapper_restart_failures_are_terminal_and_resource_clean() {
    for failure in ["malformed", "replacement_exit"] {
        let root = temp_dir(&format!("acp-wrapper-browser-restart-{failure}"));
        let mut browser = BrowserLifecycleHarness::new(&root);
        browser.request(create_fixture_request(&root));
        let owner = browser.ownership.clone();
        let AcpResponse::AcpPendingResponse(prompt_pending) =
            browser.dispatch_as(owner.clone(), prompt_request())
        else {
            panic!("prompt must be pending before {failure}");
        };
        assert_eq!(
            browser.next_adapter_request()["method"],
            json!("session/prompt")
        );
        let AcpResponse::AcpPendingResponse(restart_pending) = browser.dispatch_as(
            owner.clone(),
            abort_pending_request_with_exit_code(
                &prompt_pending.process_id,
                AcpPendingAbortReason::AgentExited,
                Some(9),
            ),
        ) else {
            panic!("replacement must begin pending");
        };
        assert_eq!(
            browser.next_adapter_request()["method"],
            json!("initialize")
        );

        let response = if failure == "malformed" {
            browser.dispatch_as(
                owner,
                deliver_output_request(&restart_pending.process_id, b"not-json\n"),
            )
        } else {
            browser.dispatch_as(
                owner,
                abort_pending_request_with_exit_code(
                    &restart_pending.process_id,
                    AcpPendingAbortReason::AgentExited,
                    Some(42),
                ),
            )
        };
        let AcpResponse::AcpErrorResponse(error) = response else {
            panic!("{failure} restart must return a terminal ACP error");
        };
        assert_eq!(error.code, "invalid_state");
        assert!(error.message.contains("auto-restart failed"));
        let events = drain_browser_acp_events(&browser.codec, &mut browser.dispatcher);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["restart"], json!("failed"));
        assert_eq!(browser.resource_counts(), (0, 0, 0, 0, 0, 0, 0));
    }
}

#[test]
fn browser_wrapper_initial_pending_failures_clear_every_resource_route() {
    for failure in ["malformed", "exit", "timeout"] {
        let root = temp_dir(&format!("acp-wrapper-browser-initial-{failure}"));
        let mut browser = BrowserLifecycleHarness::new(&root);
        let owner = browser.ownership.clone();
        let process_id = browser.begin_pending_create(owner.clone(), &root);
        let response = match failure {
            "malformed" => {
                browser.dispatch_as(owner, deliver_output_request(&process_id, b"not-json\n"))
            }
            "exit" => browser.dispatch_as(
                owner,
                abort_pending_request_with_exit_code(
                    &process_id,
                    AcpPendingAbortReason::AgentExited,
                    Some(17),
                ),
            ),
            "timeout" => browser.dispatch_as(
                owner,
                abort_pending_request(&process_id, AcpPendingAbortReason::InteractionTimeout),
            ),
            _ => unreachable!(),
        };
        assert!(matches!(response, AcpResponse::AcpErrorResponse(_)));
        assert_eq!(browser.resource_counts(), (0, 0, 0, 0, 0, 0, 0));
    }
}

#[test]
fn browser_wrapper_rejects_malformed_create_json_before_spawning() {
    for field in ["clientCapabilities", "mcpServers"] {
        let root = temp_dir(&format!("acp-wrapper-browser-invalid-{field}"));
        let mut browser = BrowserLifecycleHarness::new(&root);
        let mut request = create_fixture_request(&root);
        let AcpRequest::AcpCreateSessionRequest(create) = &mut request else {
            unreachable!()
        };
        match field {
            "clientCapabilities" => create.client_capabilities = Some(String::from("{")),
            "mcpServers" => create.mcp_servers = Some(String::from("[")),
            _ => unreachable!(),
        }

        let owner = browser.ownership.clone();
        let response = browser.dispatch_as(owner, request);
        let AcpResponse::AcpErrorResponse(error) = response else {
            panic!("malformed {field} must be rejected before spawn")
        };
        assert_eq!(error.code, "invalid_state");
        assert!(error.message.contains(field));
        assert_eq!(browser.resource_counts(), (0, 0, 0, 0, 0, 0, 0));
    }
}

#[test]
fn browser_wrapper_releases_context_when_adapter_start_fails() {
    let root = temp_dir("acp-wrapper-browser-start-failure-context");
    let mut browser = BrowserLifecycleHarness::new(&root);
    browser
        .dispatcher
        .sidecar_mut()
        .bridge_mut()
        .push_execution_start_error("injected adapter start failure");

    let owner = browser.ownership.clone();
    let response = browser.dispatch_as(owner, create_fixture_request(&root));
    let AcpResponse::AcpErrorResponse(error) = response else {
        panic!("adapter start failure must be terminal")
    };
    assert_eq!(error.code, "execution");
    assert!(error.message.contains("injected adapter start failure"));
    assert_eq!(browser.resource_counts(), (0, 0, 0, 0, 0, 0, 0));
}

#[test]
fn browser_wrapper_repeated_create_close_churn_returns_to_zero_resources() {
    let root = temp_dir("acp-wrapper-browser-create-close-churn");
    let mut browser = BrowserLifecycleHarness::new(&root);
    for iteration in 0..16 {
        assert_eq!(
            browser.request(create_fixture_request(&root)).response["type"],
            json!("created"),
            "create {iteration}"
        );
        assert_eq!(
            browser.request(close_request()).response["type"],
            json!("closed"),
            "close {iteration}"
        );
        let counts = browser.resource_counts();
        assert_eq!(
            counts,
            (0, 0, 0, 0, 0, 0, 0),
            "ACP core, route, worker, or context leak after iteration {iteration}"
        );
    }
    let disposed = browser.dispose_vm(browser.ownership.clone());
    assert!(matches!(disposed, ResponsePayload::VmDisposedResponse(_)));
    assert_eq!(browser.resource_counts(), (0, 0, 0, 0, 0, 0, 0));
}

#[derive(Debug, Clone, PartialEq)]
struct WrapperStep {
    response: Value,
    events: Vec<Value>,
    writes: Vec<Value>,
}

fn assert_wrapper_step_eq(label: &str, native: WrapperStep, browser: WrapperStep) {
    assert_eq!(
        native.response, browser.response,
        "native/browser response mismatch for {label}",
    );
    assert_eq!(
        native.events, browser.events,
        "native/browser event mismatch for {label}",
    );
}

fn prompt_request() -> AcpRequest {
    AcpRequest::AcpSessionRequest(AcpSessionRequest {
        session_id: String::from("echo-session-1"),
        method: String::from("session/prompt"),
        params: Some(json!({ "prompt": [{ "type": "text", "text": "hello" }] }).to_string()),
    })
}

fn writable_config_request() -> AcpRequest {
    AcpRequest::AcpSetSessionConfigRequest(AcpSetSessionConfigRequest {
        session_id: String::from("echo-session-1"),
        category: String::from("tone"),
        value: String::from("detailed"),
    })
}

fn read_only_config_request() -> AcpRequest {
    AcpRequest::AcpSetSessionConfigRequest(AcpSetSessionConfigRequest {
        session_id: String::from("echo-session-1"),
        category: String::from("model"),
        value: String::from("other"),
    })
}

fn cancel_request() -> AcpRequest {
    AcpRequest::AcpSessionRequest(AcpSessionRequest {
        session_id: String::from("echo-session-1"),
        method: String::from("session/cancel"),
        params: Some(String::from("{}")),
    })
}

fn state_request() -> AcpRequest {
    AcpRequest::AcpGetSessionStateRequest(AcpGetSessionStateRequest {
        session_id: String::from("echo-session-1"),
    })
}

fn list_request() -> AcpRequest {
    AcpRequest::AcpListSessionsRequest(AcpListSessionsRequest { reserved: false })
}

fn close_request() -> AcpRequest {
    AcpRequest::AcpCloseSessionRequest(AcpCloseSessionRequest {
        session_id: String::from("echo-session-1"),
    })
}

fn list_agents_request() -> AcpRequest {
    AcpRequest::AcpListAgentsRequest(AcpListAgentsRequest { reserved: false })
}

fn resume_request(session_id: &str, env: &[(&str, &str)]) -> AcpRequest {
    AcpRequest::AcpResumeSessionRequest(AcpResumeSessionRequest {
        session_id: session_id.to_string(),
        agent_type: String::from("echo"),
        transcript_path: Some(String::from("/history/echo-session.jsonl")),
        cwd: Some(String::from("/workspace")),
        env: Some(
            env.iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
        ),
    })
}

fn deliver_output_request(process_id: &str, chunk: &[u8]) -> AcpRequest {
    AcpRequest::AcpDeliverAgentOutputRequest(AcpDeliverAgentOutputRequest {
        process_id: process_id.to_string(),
        chunk: chunk.to_vec(),
    })
}

fn abort_pending_request(process_id: &str, reason: AcpPendingAbortReason) -> AcpRequest {
    abort_pending_request_with_exit_code(process_id, reason, None)
}

fn abort_pending_request_with_exit_code(
    process_id: &str,
    reason: AcpPendingAbortReason,
    exit_code: Option<i32>,
) -> AcpRequest {
    AcpRequest::AcpAbortPendingRequest(AcpAbortPendingRequest {
        process_id: process_id.to_string(),
        reason,
        exit_code,
    })
}

fn assert_acp_error(response: &AcpResponse, code: &str, message: &str) {
    let AcpResponse::AcpErrorResponse(error) = response else {
        panic!("expected ACP error {code}, got {response:?}");
    };
    assert_eq!(error.code, code);
    assert_eq!(error.message, message);
}

struct NativeLifecycleHarness {
    sidecar: NativeSidecar<NativeBridge>,
    ownership: OwnershipScope,
    package_dir: PathBuf,
    next_request_id: i64,
    write_cursor: usize,
}

impl NativeLifecycleHarness {
    fn new(root: &Path, package_dir: &Path) -> Self {
        Self::new_with_packages_mount_at(root, package_dir, "/opt/agentos")
    }

    fn new_with_packages_mount_at(
        root: &Path,
        package_dir: &Path,
        packages_mount_at: &str,
    ) -> Self {
        let mut sidecar = NativeSidecar::with_config_and_extensions(
            NativeBridge::default(),
            NativeSidecarConfig {
                sidecar_id: String::from("wrapper-lifecycle-native"),
                compile_cache_root: Some(root.join("compile-cache-lifecycle")),
                ..NativeSidecarConfig::default()
            },
            agentos_sidecar_wrapper::extensions(),
        )
        .expect("create native lifecycle sidecar");
        let connection_id = authenticate_native(&mut sidecar);
        let session_id = open_native_session(&mut sidecar, &connection_id);
        let vm_id = create_native_vm(&mut sidecar, &connection_id, &session_id, root);
        let ownership = OwnershipScope::VmOwnership(VmOwnership {
            connection_id: connection_id.clone(),
            session_id: session_id.clone(),
            vm_id: vm_id.clone(),
        });
        let configured = sidecar
            .dispatch_wire_blocking(RequestFrame {
                schema: protocol_schema(),
                request_id: 4,
                ownership: ownership.clone(),
                payload: RequestPayload::ConfigureVmRequest(ConfigureVmRequest {
                    mounts: None,
                    permissions: None,
                    command_permissions: None,
                    loopback_exempt_ports: None,
                    packages: Some(vec![PackageDescriptor::PackagePath(PackagePath {
                        path: package_dir.to_string_lossy().into_owned(),
                    })]),
                    packages_mount_at: Some(packages_mount_at.to_owned()),
                }),
            })
            .expect("configure native lifecycle VM");
        let ResponsePayload::VmConfiguredResponse(configured) = configured.response.payload else {
            panic!("unexpected native lifecycle configure response")
        };
        assert_eq!(configured.agents.len(), 1);
        assert_eq!(
            configured.agents[0].adapter_entrypoint,
            format!("{packages_mount_at}/bin/echo-agent")
        );
        Self {
            sidecar,
            ownership,
            package_dir: package_dir.to_path_buf(),
            next_request_id: 5,
            write_cursor: 0,
        }
    }

    fn request(&mut self, request: AcpRequest) -> WrapperStep {
        self.request_from(self.ownership.clone(), request)
    }

    fn sibling_ownership(&mut self, root: &Path) -> OwnershipScope {
        let connection_id =
            authenticate_native_at(&mut self.sidecar, 100, "wrapper-sibling-native");
        let session_id = open_native_session_at(&mut self.sidecar, &connection_id, 101);
        let vm_id = create_native_vm_at(&mut self.sidecar, &connection_id, &session_id, root, 102);
        let ownership = OwnershipScope::VmOwnership(VmOwnership {
            connection_id,
            session_id,
            vm_id,
        });
        let configured = self
            .sidecar
            .dispatch_wire_blocking(RequestFrame {
                schema: protocol_schema(),
                request_id: 103,
                ownership: ownership.clone(),
                payload: RequestPayload::ConfigureVmRequest(ConfigureVmRequest {
                    mounts: None,
                    permissions: None,
                    command_permissions: None,
                    loopback_exempt_ports: None,
                    packages: Some(vec![PackageDescriptor::PackagePath(PackagePath {
                        path: self.package_dir.to_string_lossy().into_owned(),
                    })]),
                    packages_mount_at: Some(String::from("/opt/agentos")),
                }),
            })
            .expect("configure native sibling lifecycle VM");
        assert!(matches!(
            configured.response.payload,
            ResponsePayload::VmConfiguredResponse(_)
        ));
        ownership
    }

    fn request_from(&mut self, ownership: OwnershipScope, request: AcpRequest) -> WrapperStep {
        let result = self
            .sidecar
            .dispatch_wire_blocking(RequestFrame {
                schema: protocol_schema(),
                request_id: self.next_request_id,
                ownership,
                payload: RequestPayload::ExtEnvelope(ExtEnvelope {
                    namespace: String::from(ACP_EXTENSION_NAMESPACE),
                    payload: serde_bare::to_vec(&request).expect("encode native lifecycle request"),
                }),
            })
            .expect("dispatch native lifecycle request");
        self.next_request_id += 1;
        let response = semantic_response(&decode_acp_response(result.response.payload));
        let events = semantic_events(result.events.iter());
        let (writes, next_cursor) = self
            .sidecar
            .with_bridge_mut(|bridge| {
                (
                    decode_writes(&bridge.stdin_writes[self.write_cursor..]),
                    bridge.stdin_writes.len(),
                )
            })
            .expect("inspect native lifecycle bridge");
        self.write_cursor = next_cursor;
        WrapperStep {
            response,
            events,
            writes,
        }
    }

    fn dispose_vm(&mut self, ownership: OwnershipScope) -> ResponsePayload {
        let response = self
            .sidecar
            .dispatch_wire_blocking(RequestFrame {
                schema: protocol_schema(),
                request_id: self.next_request_id,
                ownership,
                payload: RequestPayload::DisposeVmRequest(DisposeVmRequest {
                    reason: DisposeReason::Requested,
                }),
            })
            .expect("dispose native lifecycle VM");
        self.next_request_id += 1;
        response.response.payload
    }
}

struct BrowserLifecycleHarness {
    codec: WireFrameCodec,
    dispatcher: BrowserWireDispatcher<BrowserBridge>,
    ownership: OwnershipScope,
    vm_id: String,
    acp_diagnostics: agentos_sidecar_browser::BrowserAcpDiagnostics,
    next_request_id: i64,
    adapter_write_cursor: usize,
}

impl BrowserLifecycleHarness {
    fn new(root: &Path) -> Self {
        let codec = WireFrameCodec::default();
        let mut dispatcher = BrowserWireDispatcher::new(BrowserBridge::default());
        let extension = agentos_sidecar_browser::BrowserAcpExtension::new();
        let acp_diagnostics = extension.diagnostics();
        dispatcher
            .sidecar_mut()
            .register_extension(Box::new(extension))
            .expect("register real browser lifecycle ACP extension");
        let (vm_id, ownership) = create_browser_vm(&codec, &mut dispatcher, root);
        project_browser_agent_package(&mut dispatcher, &vm_id, "echo");
        Self {
            codec,
            dispatcher,
            ownership,
            vm_id,
            acp_diagnostics,
            next_request_id: 4,
            adapter_write_cursor: 0,
        }
    }

    fn request(&mut self, request: AcpRequest) -> WrapperStep {
        self.request_from(self.ownership.clone(), request)
    }

    fn sibling_ownership(&mut self, root: &Path) -> OwnershipScope {
        let (vm_id, ownership) = create_browser_vm_at(
            &self.codec,
            &mut self.dispatcher,
            root,
            100,
            "wrapper-sibling-browser",
        );
        project_browser_agent_package(&mut self.dispatcher, &vm_id, "echo");
        ownership
    }

    fn begin_pending_create(&mut self, ownership: OwnershipScope, root: &Path) -> String {
        let response = self.dispatch_as(ownership, create_fixture_request(root));
        let AcpResponse::AcpPendingResponse(pending) = response else {
            panic!("browser create must begin pending, got {response:?}");
        };
        pending.process_id
    }

    fn killed_execution_count(&mut self) -> usize {
        self.dispatcher
            .sidecar_mut()
            .bridge()
            .killed_executions
            .len()
    }

    fn resource_counts(&mut self) -> (usize, usize, usize, usize, usize, usize, usize) {
        let acp = self
            .acp_diagnostics
            .resource_counts()
            .expect("inspect browser ACP resources");
        (
            acp.sessions,
            acp.pending_interactions,
            acp.process_routes,
            self.dispatcher.execution_count(),
            self.dispatcher.process_execution_route_count(),
            self.dispatcher
                .sidecar_mut()
                .active_worker_count(&self.vm_id),
            self.dispatcher.sidecar_mut().context_count(&self.vm_id),
        )
    }

    fn last_adapter_write(&mut self) -> Value {
        let bridge = self.dispatcher.sidecar_mut().bridge();
        let write = bridge.stdin_writes.last().expect("adapter stdin write");
        serde_json::from_slice(&write.chunk).expect("decode adapter stdin JSON")
    }

    fn dispose_vm(&mut self, ownership: OwnershipScope) -> ResponsePayload {
        let response = dispatch_browser(
            &self.codec,
            &mut self.dispatcher,
            RequestFrame {
                schema: protocol_schema(),
                request_id: self.next_request_id,
                ownership,
                payload: RequestPayload::DisposeVmRequest(DisposeVmRequest {
                    reason: DisposeReason::Requested,
                }),
            },
        );
        self.next_request_id += 1;
        response.payload
    }

    fn request_from(&mut self, ownership: OwnershipScope, request: AcpRequest) -> WrapperStep {
        let fixture_behavior = AdapterFixtureBehavior::from_request(&request);
        let closing = matches!(request, AcpRequest::AcpCloseSessionRequest(_));
        let step_write_start = self.dispatcher.sidecar_mut().bridge().stdin_writes.len();
        let mut response = self.dispatch_as(ownership.clone(), request);
        while let AcpResponse::AcpPendingResponse(pending) = response {
            if closing {
                response = self.dispatch_as(
                    ownership.clone(),
                    abort_pending_request_with_exit_code(
                        &pending.process_id,
                        AcpPendingAbortReason::AgentExited,
                        Some(0),
                    ),
                );
                continue;
            }
            let outbound = self.next_adapter_request();
            let output = adapter_output_for(&outbound, fixture_behavior);
            assert!(
                !output.is_empty(),
                "fixture adapter produced no response for {outbound}"
            );
            let mut chunk = Vec::new();
            for message in output {
                chunk.extend(serde_json::to_vec(&message).expect("encode fixture adapter output"));
                chunk.push(b'\n');
            }
            response = self.dispatch_as(
                ownership.clone(),
                AcpRequest::AcpDeliverAgentOutputRequest(AcpDeliverAgentOutputRequest {
                    process_id: pending.process_id,
                    chunk,
                }),
            );
        }
        let writes = {
            let bridge = self.dispatcher.sidecar_mut().bridge();
            decode_writes(&bridge.stdin_writes[step_write_start..])
        };
        WrapperStep {
            response: semantic_response(&response),
            events: drain_browser_acp_events(&self.codec, &mut self.dispatcher),
            writes,
        }
    }

    fn dispatch_as(&mut self, ownership: OwnershipScope, request: AcpRequest) -> AcpResponse {
        let response = dispatch_browser_acp(
            &self.codec,
            &mut self.dispatcher,
            ownership,
            self.next_request_id,
            request,
        );
        self.next_request_id += 1;
        response
    }

    fn next_adapter_request(&mut self) -> Value {
        loop {
            let value: Value = {
                let bridge = self.dispatcher.sidecar_mut().bridge();
                let write = bridge
                    .stdin_writes
                    .get(self.adapter_write_cursor)
                    .unwrap_or_else(|| panic!("pending browser request produced no adapter write"));
                serde_json::from_slice(&write.chunk).expect("decode browser adapter write")
            };
            self.adapter_write_cursor += 1;
            if value.get("method").is_some() && value.get("id").is_some() {
                return value;
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
struct AdapterFixtureBehavior {
    load_session: bool,
    unknown_session: bool,
    load_failure: bool,
}

impl AdapterFixtureBehavior {
    fn from_request(request: &AcpRequest) -> Self {
        let AcpRequest::AcpResumeSessionRequest(request) = request else {
            return Self::default();
        };
        let env = request.env.as_ref();
        Self {
            load_session: env
                .and_then(|env| env.get("ECHO_LOAD_SESSION"))
                .is_some_and(|value| value == "1"),
            unknown_session: env
                .and_then(|env| env.get("ECHO_UNKNOWN_SESSION"))
                .is_some_and(|value| value == "1"),
            load_failure: env
                .and_then(|env| env.get("ECHO_LOAD_FAILURE"))
                .is_some_and(|value| value == "1"),
        }
    }
}

fn adapter_output_for(request: &Value, behavior: AdapterFixtureBehavior) -> Vec<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    match request.get("method").and_then(Value::as_str) {
        Some("initialize") => vec![json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": initialize_result_with_capabilities(behavior.load_session),
        })],
        Some("session/new") => vec![json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "sessionId": "echo-session-1" },
        })],
        Some("session/load") if behavior.load_failure => vec![json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32000, "message": "injected session/load failure" },
        })],
        Some("session/load") if behavior.unknown_session => vec![json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32603,
                "message": "unknown session",
                "data": { "details": "NotFoundError" },
            },
        })],
        Some("session/load") => vec![json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {},
        })],
        Some("session/prompt") => vec![
            json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": params.get("sessionId").cloned().unwrap_or(Value::Null),
                    "update": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": { "type": "text", "text": "echo: hello" },
                    },
                },
            }),
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "stopReason": "end_turn" },
            }),
        ],
        Some("session/set_config_option") => vec![json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {},
        })],
        Some("session/cancel") => vec![json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "unknown session/cancel" },
        })],
        method => panic!("unexpected fixture adapter method {method:?}"),
    }
}

fn decode_writes(writes: &[agentos_bridge::WriteExecutionStdinRequest]) -> Vec<Value> {
    writes
        .iter()
        .map(|write| serde_json::from_slice(&write.chunk).expect("decode adapter stdin JSON"))
        .collect()
}

fn semantic_events<'a>(events: impl IntoIterator<Item = &'a EventFrame>) -> Vec<Value> {
    events
        .into_iter()
        .filter_map(|event| semantic_event_payload(&event.payload))
        .collect()
}

fn semantic_event_payload(payload: &EventPayload) -> Option<Value> {
    let EventPayload::ExtEnvelope(envelope) = payload else {
        return None;
    };
    if envelope.namespace != ACP_EXTENSION_NAMESPACE {
        return None;
    }
    let event: AcpEvent = serde_bare::from_slice(&envelope.payload).expect("decode ACP event");
    // V8 can emit this native-runtime diagnostic after the adapter's terminal
    // event. Production must keep surfacing it, but it is not adapter behavior
    // and has no browser equivalent; adapter stderr has its own parity test.
    if matches!(
        &event,
        AcpEvent::AcpAgentStderrEvent(event)
            if event.chunk.starts_with(b"[ERR_LATE_STREAM_EVENT]")
    ) {
        return None;
    }
    Some(match event {
        AcpEvent::AcpSessionEvent(event) => json!({
            "type": "session",
            "sessionId": event.session_id,
            "notification": parse_json(&event.notification),
        }),
        AcpEvent::AcpAgentStderrEvent(event) => json!({
            "type": "stderr",
            "sessionId": event.session_id,
            "agentType": event.agent_type,
            "chunk": event.chunk,
        }),
        AcpEvent::AcpAgentExitedEvent(event) => json!({
            "type": "exit",
            "sessionId": event.session_id,
            "agentType": event.agent_type,
            "exitCode": event.exit_code,
            "restart": event.restart,
        }),
    })
}

fn drain_browser_acp_events(
    codec: &WireFrameCodec,
    dispatcher: &mut BrowserWireDispatcher<BrowserBridge>,
) -> Vec<Value> {
    let mut events = Vec::new();
    while let Some(bytes) = dispatcher
        .poll_event_bytes()
        .expect("poll browser lifecycle events")
    {
        let ProtocolFrame::EventFrame(event) = codec
            .decode_message(&bytes)
            .expect("decode browser lifecycle event frame")
        else {
            panic!("expected browser event frame");
        };
        if let Some(event) = semantic_event_payload(&event.payload) {
            events.push(event);
        }
    }
    events
}

fn semantic_response(response: &AcpResponse) -> Value {
    match response {
        AcpResponse::AcpSessionCreatedResponse(_) => json!({
            "type": "created",
            "response": semantic_created_response(response),
        }),
        AcpResponse::AcpSessionRpcResponse(response) => json!({
            "type": "rpc",
            "sessionId": response.session_id,
            "response": parse_json(&response.response),
            "text": response.text,
        }),
        AcpResponse::AcpSessionStateResponse(response) => json!({
            "type": "state",
            "sessionId": response.session_id,
            "agentType": response.agent_type,
            "closed": response.closed,
            "exitCode": response.exit_code,
            "modes": parse_optional_json(&response.modes),
            "configOptions": response.config_options.iter().map(|value| parse_json(value)).collect::<Vec<_>>(),
            "agentCapabilities": parse_optional_json(&response.agent_capabilities),
            "agentInfo": parse_optional_json(&response.agent_info),
        }),
        AcpResponse::AcpListSessionsResponse(response) => json!({
            "type": "list",
            "sessions": response.sessions.iter().map(|session| json!({
                "sessionId": session.session_id,
                "agentType": session.agent_type,
            })).collect::<Vec<_>>(),
        }),
        AcpResponse::AcpSessionClosedResponse(response) => json!({
            "type": "closed",
            "sessionId": response.session_id,
        }),
        AcpResponse::AcpSessionResumedResponse(response) => json!({
            "type": "resumed",
            "sessionId": response.session_id,
            "mode": response.mode,
            "agentType": response.agent_type,
        }),
        AcpResponse::AcpErrorResponse(response) => json!({
            "type": "error",
            "code": response.code,
            "message": response.message,
        }),
        AcpResponse::AcpPendingResponse(response) => {
            panic!("ordinary wrapper scenario leaked pending response {response:?}")
        }
        AcpResponse::AcpAgentStderrDeliveredResponse(response) => json!({
            "type": "stderr-delivered",
            "processId": response.process_id,
        }),
        AcpResponse::AcpListAgentsResponse(response) => json!({
            "type": "agents",
            "agents": response.agents.iter().map(|agent| &agent.id).collect::<Vec<_>>(),
        }),
    }
}

fn create_fixture_request(_root: &Path) -> AcpRequest {
    AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
        agent_type: String::from("echo"),
        runtime: Some(AcpRuntimeKind::JavaScript),
        cwd: Some(String::from(GUEST_CWD)),
        args: Some(Vec::new()),
        env: Some(HashMap::new()),
        protocol_version: Some(i32::from(ACP_PROTOCOL_VERSION)),
        client_capabilities: Some(String::from("{}")),
        mcp_servers: Some(String::from("[]")),
        skip_os_instructions: Some(true),
        additional_instructions: None,
    })
}

fn bootstrap_output() -> [String; 2] {
    [
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": initialize_result(),
        })
        .to_string(),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": { "sessionId": "echo-session-1" },
        })
        .to_string(),
    ]
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": i32::from(ACP_PROTOCOL_VERSION),
        "agentInfo": { "name": "echo", "version": "0.0.0" },
        "agentCapabilities": {},
        "modes": {
            "currentModeId": "mode-a",
            "availableModes": [{ "id": "mode-a", "name": "Mode A" }],
        },
        "configOptions": [
            {
                "id": "tone",
                "category": "tone",
                "currentValue": "brief",
                "allowedValues": [
                    { "id": "brief", "label": "Brief" },
                    { "id": "detailed", "label": "Detailed" },
                ],
            },
            {
                "id": "model",
                "category": "model",
                "currentValue": "fixed",
                "readOnly": true,
            },
        ],
    })
}

fn initialize_result_with_capabilities(load_session: bool) -> Value {
    let mut result = initialize_result();
    if load_session {
        result["agentCapabilities"] = json!({ "loadSession": true });
    }
    result
}

fn semantic_created_response(response: &AcpResponse) -> Value {
    let AcpResponse::AcpSessionCreatedResponse(created) = response else {
        panic!("expected ACP session-created response, got {response:?}");
    };
    json!({
        "sessionId": created.session_id,
        "agentType": created.agent_type,
        "modes": parse_optional_json(&created.modes),
        "configOptions": created.config_options.iter().map(|value| parse_json(value)).collect::<Vec<_>>(),
        "agentCapabilities": parse_optional_json(&created.agent_capabilities),
        "agentInfo": parse_optional_json(&created.agent_info),
    })
}

fn parse_optional_json(value: &Option<String>) -> Value {
    value.as_deref().map(parse_json).unwrap_or(Value::Null)
}

fn parse_json(value: &str) -> Value {
    serde_json::from_str(value)
        .unwrap_or_else(|error| panic!("invalid ACP fixture JSON {value:?}: {error}"))
}

fn authenticate_native(sidecar: &mut NativeSidecar<NativeBridge>) -> String {
    authenticate_native_at(sidecar, 1, "wrapper-conformance-native")
}

fn authenticate_native_at(
    sidecar: &mut NativeSidecar<NativeBridge>,
    request_id: i64,
    client_name: &str,
) -> String {
    let result = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: protocol_schema(),
            request_id,
            ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
                connection_id: format!("{client_name}-bootstrap"),
            }),
            payload: RequestPayload::AuthenticateRequest(AuthenticateRequest {
                client_name: client_name.to_string(),
                auth_token: String::new(),
                protocol_version: PROTOCOL_VERSION,
                bridge_version: agentos_bridge::bridge_contract().version,
            }),
        })
        .expect("authenticate native wrapper");
    let ResponsePayload::AuthenticatedResponse(response) = result.response.payload else {
        panic!("unexpected native authentication response");
    };
    response.connection_id
}

fn open_native_session(sidecar: &mut NativeSidecar<NativeBridge>, connection_id: &str) -> String {
    open_native_session_at(sidecar, connection_id, 2)
}

fn open_native_session_at(
    sidecar: &mut NativeSidecar<NativeBridge>,
    connection_id: &str,
    request_id: i64,
) -> String {
    let result = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: protocol_schema(),
            request_id,
            ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
                connection_id: connection_id.to_owned(),
            }),
            payload: RequestPayload::OpenSessionRequest(OpenSessionRequest {
                placement: SidecarPlacement::SidecarPlacementShared(SidecarPlacementShared {
                    pool: None,
                }),
            }),
        })
        .expect("open native session");
    let ResponsePayload::SessionOpenedResponse(response) = result.response.payload else {
        panic!("unexpected native open-session response");
    };
    response.session_id
}

fn create_native_vm(
    sidecar: &mut NativeSidecar<NativeBridge>,
    connection_id: &str,
    session_id: &str,
    root: &Path,
) -> String {
    create_native_vm_at(sidecar, connection_id, session_id, root, 3)
}

fn create_native_vm_at(
    sidecar: &mut NativeSidecar<NativeBridge>,
    connection_id: &str,
    session_id: &str,
    _root: &Path,
    request_id: i64,
) -> String {
    let result = sidecar
        .dispatch_wire_blocking(RequestFrame {
            schema: protocol_schema(),
            request_id,
            ownership: OwnershipScope::SessionOwnership(SessionOwnership {
                connection_id: connection_id.to_owned(),
                session_id: session_id.to_owned(),
            }),
            payload: RequestPayload::CreateVmRequest(CreateVmRequest::json_config(
                GuestRuntimeKind::JavaScript,
                agentos_vm_config::CreateVmConfig {
                    cwd: Some(String::from(GUEST_CWD)),
                    ..Default::default()
                },
            )),
        })
        .expect("create native VM");
    let ResponsePayload::VmCreatedResponse(response) = result.response.payload else {
        panic!("unexpected native create-VM response");
    };
    response.vm_id
}

fn project_browser_agent_package(
    dispatcher: &mut BrowserWireDispatcher<BrowserBridge>,
    vm_id: &str,
    agent_id: &str,
) {
    let package = packed_browser_agent_package(agent_id);
    let projected = dispatcher
        .sidecar_mut()
        .project_aospkg_bytes(vm_id, package)
        .expect("project real browser agent package");
    let agent = projected.agent.expect("package projects one ACP agent");
    assert_eq!(agent.id, agent_id);
    assert_eq!(agent.adapter_entrypoint, "/opt/agentos/bin/echo-agent");
}

fn packed_browser_agent_package(agent_id: &str) -> Vec<u8> {
    let manifest = serde_json::to_vec_pretty(&json!({
        "name": agent_id,
        "version": "0.0.1",
        "agent": { "acpEntrypoint": "echo-agent" }
    }))
    .expect("encode browser agent package manifest");
    let mut source = tar::Builder::new(Vec::new());
    append_browser_package_entry(&mut source, "agentos-package.json", &manifest, 0o644);
    append_browser_package_entry(
        &mut source,
        "bin/echo-agent",
        ECHO_AGENT_SOURCE.as_bytes(),
        0o755,
    );
    let source = source
        .into_inner()
        .expect("finish browser agent source tar");
    vfs::package_format::pack::pack_aospkg_from_tar_bytes(&source)
        .expect("pack browser agent .aospkg")
        .0
}

fn append_browser_package_entry(
    builder: &mut tar::Builder<Vec<u8>>,
    path: &str,
    contents: &[u8],
    mode: u32,
) {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(mode);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_size(contents.len() as u64);
    header.set_cksum();
    builder
        .append_data(&mut header, path, Cursor::new(contents))
        .expect("append browser agent package entry");
}

fn run_browser_create(root: &Path, request: AcpRequest) -> AcpResponse {
    let codec = WireFrameCodec::default();
    let mut dispatcher = BrowserWireDispatcher::new(BrowserBridge::default());
    dispatcher
        .sidecar_mut()
        .register_extension(Box::new(agentos_sidecar_browser::BrowserAcpExtension::new()))
        .expect("register real browser ACP extension");
    let (vm_id, ownership) = create_browser_vm(&codec, &mut dispatcher, root);
    project_browser_agent_package(&mut dispatcher, &vm_id, "echo");

    let mut response = dispatch_browser_acp(&codec, &mut dispatcher, ownership.clone(), 4, request);
    let AcpResponse::AcpPendingResponse(pending) = &response else {
        panic!("browser create must begin pending: {response:?}");
    };
    response = dispatch_browser_acp(
        &codec,
        &mut dispatcher,
        ownership.clone(),
        5,
        AcpRequest::AcpDeliverAgentOutputRequest(AcpDeliverAgentOutputRequest {
            process_id: pending.process_id.clone(),
            chunk: br#"{"jsonrpc":"2.0","id":"host-1","method":"host/read","params":{}}
"#
            .to_vec(),
        }),
    );
    assert!(
        matches!(response, AcpResponse::AcpPendingResponse(_)),
        "inbound host request must not complete or escape the browser handshake"
    );
    for (index, line) in bootstrap_output().into_iter().enumerate() {
        let AcpResponse::AcpPendingResponse(pending) = response else {
            panic!("browser ACP bootstrap completed before fixture line {index}: {response:?}");
        };
        let mut chunk = line.into_bytes();
        chunk.push(b'\n');
        response = dispatch_browser_acp(
            &codec,
            &mut dispatcher,
            ownership.clone(),
            6 + index as i64,
            AcpRequest::AcpDeliverAgentOutputRequest(AcpDeliverAgentOutputRequest {
                process_id: pending.process_id,
                chunk,
            }),
        );
    }

    assert!(
        !matches!(response, AcpResponse::AcpPendingResponse(_)),
        "browser fixture must complete the create handshake"
    );
    let bridge = dispatcher.sidecar_mut().bridge();
    assert_eq!(bridge.browser_worker_spawns.len(), 1);
    assert_eq!(bridge.stdin_writes.len(), 3);
    let unsupported: Value = serde_json::from_slice(&bridge.stdin_writes[1].chunk)
        .expect("browser unsupported inbound-request response");
    assert_eq!(unsupported["id"], "host-1");
    assert_eq!(unsupported["error"]["code"], -32601);
    assert_eq!(unsupported["error"]["data"]["method"], "host/read");
    while let Some(event) = dispatcher
        .poll_event_bytes()
        .expect("poll browser event queue")
    {
        let ProtocolFrame::EventFrame(event) = codec
            .decode_message(&event)
            .expect("decode browser event frame")
        else {
            panic!("expected browser event frame");
        };
        if let agentos_native_sidecar::wire::EventPayload::ExtEnvelope(envelope) = event.payload {
            if envelope.namespace == ACP_EXTENSION_NAMESPACE {
                let event: AcpEvent =
                    serde_bare::from_slice(&envelope.payload).expect("decode browser ACP event");
                assert!(
                    !matches!(event, AcpEvent::AcpSessionEvent(_)),
                    "inbound host requests must never become AcpSessionEvents"
                );
            }
        }
    }
    response
}

fn create_browser_vm(
    codec: &WireFrameCodec,
    dispatcher: &mut BrowserWireDispatcher<BrowserBridge>,
    root: &Path,
) -> (String, OwnershipScope) {
    create_browser_vm_at(codec, dispatcher, root, 1, "wrapper-conformance-browser")
}

fn create_browser_vm_at(
    codec: &WireFrameCodec,
    dispatcher: &mut BrowserWireDispatcher<BrowserBridge>,
    _root: &Path,
    request_id: i64,
    client_name: &str,
) -> (String, OwnershipScope) {
    let authenticated = dispatch_browser(
        codec,
        dispatcher,
        RequestFrame {
            schema: protocol_schema(),
            request_id,
            ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
                connection_id: format!("{client_name}-bootstrap"),
            }),
            payload: RequestPayload::AuthenticateRequest(AuthenticateRequest {
                client_name: client_name.to_string(),
                auth_token: String::new(),
                protocol_version: PROTOCOL_VERSION,
                bridge_version: agentos_bridge::bridge_contract().version,
            }),
        },
    );
    let ResponsePayload::AuthenticatedResponse(authenticated) = authenticated.payload else {
        panic!("unexpected browser authentication response");
    };
    let opened = dispatch_browser(
        codec,
        dispatcher,
        RequestFrame {
            schema: protocol_schema(),
            request_id: request_id + 1,
            ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
                connection_id: authenticated.connection_id.clone(),
            }),
            payload: RequestPayload::OpenSessionRequest(OpenSessionRequest {
                placement: SidecarPlacement::SidecarPlacementShared(SidecarPlacementShared {
                    pool: None,
                }),
            }),
        },
    );
    let ResponsePayload::SessionOpenedResponse(opened) = opened.payload else {
        panic!("unexpected browser open-session response");
    };
    let created = dispatch_browser(
        codec,
        dispatcher,
        RequestFrame {
            schema: protocol_schema(),
            request_id: request_id + 2,
            ownership: OwnershipScope::SessionOwnership(SessionOwnership {
                connection_id: authenticated.connection_id.clone(),
                session_id: opened.session_id.clone(),
            }),
            payload: RequestPayload::CreateVmRequest(CreateVmRequest::json_config(
                GuestRuntimeKind::JavaScript,
                agentos_vm_config::CreateVmConfig {
                    cwd: Some(String::from(GUEST_CWD)),
                    ..Default::default()
                },
            )),
        },
    );
    let ResponsePayload::VmCreatedResponse(created) = created.payload else {
        panic!("unexpected browser create-VM response");
    };
    let ownership = OwnershipScope::VmOwnership(VmOwnership {
        connection_id: authenticated.connection_id,
        session_id: opened.session_id,
        vm_id: created.vm_id.clone(),
    });
    (created.vm_id, ownership)
}

fn dispatch_browser_acp(
    codec: &WireFrameCodec,
    dispatcher: &mut BrowserWireDispatcher<BrowserBridge>,
    ownership: OwnershipScope,
    request_id: i64,
    request: AcpRequest,
) -> AcpResponse {
    let response = dispatch_browser(
        codec,
        dispatcher,
        RequestFrame {
            schema: protocol_schema(),
            request_id,
            ownership,
            payload: RequestPayload::ExtEnvelope(ExtEnvelope {
                namespace: String::from(ACP_EXTENSION_NAMESPACE),
                payload: serde_bare::to_vec(&request).expect("encode browser ACP request"),
            }),
        },
    );
    decode_acp_response(response.payload)
}

fn decode_acp_response(payload: ResponsePayload) -> AcpResponse {
    let ResponsePayload::ExtEnvelope(envelope) = payload else {
        panic!("expected ACP extension response, got {payload:?}");
    };
    assert_eq!(envelope.namespace, ACP_EXTENSION_NAMESPACE);
    serde_bare::from_slice(&envelope.payload).expect("decode ACP response")
}

fn dispatch_browser(
    codec: &WireFrameCodec,
    dispatcher: &mut BrowserWireDispatcher<BrowserBridge>,
    request: RequestFrame,
) -> agentos_native_sidecar::wire::ResponseFrame {
    let request = codec
        .encode_message(&ProtocolFrame::RequestFrame(request))
        .expect("encode browser wire request");
    let response = dispatcher
        .handle_request_bytes(&request)
        .expect("dispatch browser wire request");
    let ProtocolFrame::ResponseFrame(response) = codec
        .decode_message(&response)
        .expect("decode browser wire response")
    else {
        panic!("expected browser response frame");
    };
    response
}

fn prepare_echo_package(root: &Path) -> PathBuf {
    let package_dir = root.join("packages").join("echo");
    let bin_dir = package_dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("create echo package directory");
    fs::write(
        package_dir.join("agentos-package.json"),
        json!({
            "name": "echo",
            "version": "0.0.0",
            "agent": { "acpEntrypoint": "echo-agent" },
        })
        .to_string(),
    )
    .expect("write transition package manifest");
    fs::write(bin_dir.join("echo-agent"), ECHO_AGENT_SOURCE).expect("write echo agent fixture");
    package_dir
}

fn assert_node_available() {
    let output = Command::new("node")
        .arg("--version")
        .output()
        .expect("spawn node --version");
    assert!(output.status.success(), "node must be available");
}

fn lock_native_tests() -> MutexGuard<'static, ()> {
    NATIVE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "agentos-{name}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos()
    ));
    fs::create_dir_all(&path).expect("create conformance temp directory");
    path
}
