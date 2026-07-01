//! Coverage for adapter-crash observability + bounded auto-restart.
//!
//! When the ACP adapter process exits without `close_session` the extension
//! must (a) log the exit, (b) emit an `AcpAgentExitedEvent` describing the
//! sidecar's auto-restart outcome, and (c) attempt the restart itself:
//! respawn the adapter with the retained launch parameters and natively
//! re-attach the session via `initialize` + `session/load`. Only a successful
//! restart keeps the session record; otherwise it is evicted exactly like the
//! pre-restart behavior.
//!
//! One top-level test drives both outcomes through the public Ext RPC surface
//! (multiple libtest cases per V8-backed binary still trip teardown crashes):
//!
//! 1. A `loadSession`-capable adapter crashes on its first `session/prompt`
//!    (exit 7). The prompt dispatch fails with the exit diagnostic + restart
//!    notice, the dispatch events carry `AcpAgentExitedEvent { restart:
//!    "restarted", exit_code: Some(7) }`, and a retried prompt against the
//!    SAME session id succeeds against the respawned adapter.
//! 2. An adapter without any native resume capability crashes the same way.
//!    The event reports `restart: "unsupported"` and the session record is
//!    evicted (a follow-up prompt fails with unknown-session).
//!
//! This is bounded ("still test the expensive safeguards"): every crash is an
//! immediate `process.exit`, and the restart handshake ends the exchange, so
//! nothing waits on a watchdog.

#[path = "support/bridge.rs"]
mod bridge_support;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use agentos_protocol::generated::v1::{
    AcpAgentExitedEvent, AcpCreateSessionRequest, AcpErrorResponse, AcpEvent, AcpRequest,
    AcpResponse, AcpRuntimeKind, AcpSessionRequest,
};
use agentos_protocol::{ACP_EXTENSION_NAMESPACE, PROTOCOL_VERSION as ACP_PROTOCOL_VERSION};
use bridge_support::RecordingBridge;
use secure_exec_sidecar::wire::{
    AuthenticateRequest, ConnectionOwnership, CreateVmRequest, EventFrame, EventPayload,
    ExtEnvelope, GuestRuntimeKind, OpenSessionRequest, OwnershipScope, RequestFrame,
    RequestPayload, ResponsePayload, SessionOwnership, SidecarPlacement, SidecarPlacementShared,
    VmOwnership,
};
use secure_exec_sidecar::{NativeSidecar, NativeSidecarConfig};
use secure_exec_vm_config as vm_config;

#[test]
fn adapter_crash_restarts_or_evicts_and_emits_exit_event() {
    assert_node_available();
    let mut sidecar = new_sidecar("adapter-restart");

    let connection_id = authenticate(&mut sidecar);
    let session_id = open_session(&mut sidecar, &connection_id);
    let cwd = temp_dir("adapter-restart-cwd");
    let restartable = cwd.join("restartable-adapter.mjs");
    fs::write(&restartable, restartable_adapter_script()).expect("write restartable adapter");
    let unsupported = cwd.join("unsupported-adapter.mjs");
    fs::write(&unsupported, unsupported_adapter_script()).expect("write unsupported adapter");
    let vm_id = create_vm(&mut sidecar, &connection_id, &session_id, &cwd);

    // ------------------------------------------------------------------
    // Scenario 1: loadSession-capable adapter → crash is auto-restarted.
    // ------------------------------------------------------------------
    let created = dispatch_acp(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        create_session_request(&restartable, &cwd),
    )
    .0;
    let AcpResponse::AcpSessionCreatedResponse(created) = created else {
        panic!("unexpected create response: {created:?}");
    };
    assert_eq!(created.session_id, "restartable-session");

    // First prompt: the adapter exits(7) without responding. The dispatch must
    // surface the exit AND report that the session was auto-restarted.
    let (response, events) = dispatch_acp(
        &mut sidecar,
        5,
        &connection_id,
        &session_id,
        &vm_id,
        prompt_request("restartable-session", "hello"),
    );
    let AcpResponse::AcpErrorResponse(AcpErrorResponse { code, message }) = response else {
        panic!("expected the crashed prompt to fail, got: {response:?}");
    };
    assert_eq!(code, "invalid_state");
    assert!(
        message.contains("exited with code 7"),
        "expected exit diagnostic in: {message}"
    );
    assert!(
        message.contains("auto-restarted") && message.contains("retry the request"),
        "expected restart notice in: {message}"
    );
    let exit_event = decode_single_agent_exited_event(&events);
    assert_eq!(exit_event.session_id, "restartable-session");
    assert_eq!(exit_event.agent_type, "pi");
    assert_eq!(exit_event.exit_code, Some(7));
    assert_eq!(exit_event.restart, "restarted");
    assert_eq!(exit_event.restart_count, 1);
    assert_eq!(exit_event.max_restarts, 3);

    // Retried prompt: the respawned adapter (which answered `session/load` for
    // the same session id) must serve it.
    let (response, _) = dispatch_acp(
        &mut sidecar,
        6,
        &connection_id,
        &session_id,
        &vm_id,
        prompt_request("restartable-session", "hello again"),
    );
    let AcpResponse::AcpSessionRpcResponse(prompt) = response else {
        panic!("expected the retried prompt to succeed, got: {response:?}");
    };
    assert_eq!(prompt.session_id, "restartable-session");
    let prompt_response: serde_json::Value =
        serde_json::from_str(&prompt.response).expect("prompt response json");
    assert_eq!(prompt_response["result"]["echo"], "hello again");

    // ------------------------------------------------------------------
    // Scenario 2: no native resume capability → crash evicts the session.
    // ------------------------------------------------------------------
    let created = dispatch_acp(
        &mut sidecar,
        7,
        &connection_id,
        &session_id,
        &vm_id,
        create_session_request(&unsupported, &cwd),
    )
    .0;
    let AcpResponse::AcpSessionCreatedResponse(created) = created else {
        panic!("unexpected create response: {created:?}");
    };
    assert_eq!(created.session_id, "unsupported-session");

    let (response, events) = dispatch_acp(
        &mut sidecar,
        8,
        &connection_id,
        &session_id,
        &vm_id,
        prompt_request("unsupported-session", "hello"),
    );
    let AcpResponse::AcpErrorResponse(AcpErrorResponse { code, message }) = response else {
        panic!("expected the crashed prompt to fail, got: {response:?}");
    };
    assert_eq!(code, "invalid_state");
    assert!(
        message.contains("exited with code 3"),
        "expected exit diagnostic in: {message}"
    );
    assert!(
        message.contains("auto-restart unsupported") && message.contains("session evicted"),
        "expected unsupported-restart notice in: {message}"
    );
    let exit_event = decode_single_agent_exited_event(&events);
    assert_eq!(exit_event.session_id, "unsupported-session");
    assert_eq!(exit_event.exit_code, Some(3));
    assert_eq!(exit_event.restart, "unsupported");
    assert_eq!(exit_event.restart_count, 1);

    // The record is gone: a follow-up prompt fails with unknown-session.
    let (response, _) = dispatch_acp(
        &mut sidecar,
        9,
        &connection_id,
        &session_id,
        &vm_id,
        prompt_request("unsupported-session", "anyone home?"),
    );
    let AcpResponse::AcpErrorResponse(AcpErrorResponse { message, .. }) = response else {
        panic!("expected the post-eviction prompt to fail, got: {response:?}");
    };
    assert!(
        message.contains("unknown ACP session"),
        "expected unknown-session error after eviction, got: {message}"
    );

    // ------------------------------------------------------------------
    // Scenario 3: idle crash (between turns) is detected LAZILY on the
    // next request, still emits the exit event, and still auto-restarts.
    // ------------------------------------------------------------------
    let idle = cwd.join("idle-crash-adapter.mjs");
    fs::write(&idle, idle_crash_adapter_script()).expect("write idle-crash adapter");
    let created = dispatch_acp(
        &mut sidecar,
        10,
        &connection_id,
        &session_id,
        &vm_id,
        create_session_request(&idle, &cwd),
    )
    .0;
    let AcpResponse::AcpSessionCreatedResponse(created) = created else {
        panic!("unexpected create response: {created:?}");
    };
    assert_eq!(created.session_id, "idle-session");

    // First prompt succeeds normally; the adapter exits AFTER responding, so
    // the crash happens while the session is idle and nothing observes it yet.
    let (response, _) = dispatch_acp(
        &mut sidecar,
        11,
        &connection_id,
        &session_id,
        &vm_id,
        prompt_request("idle-session", "first"),
    );
    let AcpResponse::AcpSessionRpcResponse(prompt) = response else {
        panic!("expected the first prompt to succeed, got: {response:?}");
    };
    let prompt_response: serde_json::Value =
        serde_json::from_str(&prompt.response).expect("prompt response json");
    assert_eq!(prompt_response["result"]["echo"], "first");

    // Give the adapter process time to actually die while idle.
    std::thread::sleep(std::time::Duration::from_millis(1500));

    // The next prompt is the lazy detection point: it must fail with the
    // adapter-gone diagnostic, emit the exit event, and auto-restart.
    let (response, events) = dispatch_acp(
        &mut sidecar,
        12,
        &connection_id,
        &session_id,
        &vm_id,
        prompt_request("idle-session", "second"),
    );
    let AcpResponse::AcpErrorResponse(AcpErrorResponse { code, message }) = response else {
        panic!("expected the post-idle-crash prompt to fail, got: {response:?}");
    };
    assert_eq!(code, "invalid_state");
    // Diagnostic: shows which lazy-observation path fired on this run
    // (failed stdin write "has no active process" vs queued exit event).
    eprintln!("idle-crash lazy detection error: {message}");
    assert!(
        message.contains("auto-restarted") && message.contains("retry the request"),
        "expected restart notice in: {message}"
    );
    let exit_event = decode_single_agent_exited_event(&events);
    assert_eq!(exit_event.session_id, "idle-session");
    assert_eq!(exit_event.restart, "restarted");
    assert_eq!(exit_event.restart_count, 1);
    // Which observation path fires depends on whether the reap completed
    // before the request's stdin write: a failed write ("has no active
    // process") observes NO exit code, while the in-pump ProcessExitedEvent
    // observes the real code (0). Both are the lazy-detection contract.
    assert!(
        exit_event.exit_code.is_none() || exit_event.exit_code == Some(0),
        "unexpected exit code for idle crash: {:?}",
        exit_event.exit_code
    );

    // The restarted adapter serves the same session id on the next turn.
    let (response, _) = dispatch_acp(
        &mut sidecar,
        13,
        &connection_id,
        &session_id,
        &vm_id,
        prompt_request("idle-session", "third"),
    );
    let AcpResponse::AcpSessionRpcResponse(prompt) = response else {
        panic!("expected the post-restart prompt to succeed, got: {response:?}");
    };
    assert_eq!(prompt.session_id, "idle-session");
    let prompt_response: serde_json::Value =
        serde_json::from_str(&prompt.response).expect("prompt response json");
    assert_eq!(prompt_response["result"]["echo"], "third");
}

/// Adapter that advertises `loadSession` and exits(0) AFTER successfully
/// answering its first post-`session/new` prompt — an idle crash between
/// turns. After a `session/load` re-attach it answers prompts normally.
/// The response is flushed before exiting so the crashing turn itself
/// succeeds and the exit is only observable lazily, on the next request.
fn idle_crash_adapter_script() -> &'static str {
    r#"
import readline from "node:readline";

const lines = readline.createInterface({ input: process.stdin });
let loaded = false;

for await (const line of lines) {
  if (!line.trim()) continue;
  const message = JSON.parse(line);
  if (message.method === "initialize") {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        protocolVersion: message.params.protocolVersion,
        agentCapabilities: { loadSession: true },
        agentInfo: { name: "idle-crash-acp-adapter" },
        configOptions: []
      }
    }));
  } else if (message.method === "session/new") {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        sessionId: "idle-session",
        modes: { currentModeId: "default", availableModes: [] }
      }
    }));
  } else if (message.method === "session/load") {
    loaded = true;
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        modes: { currentModeId: "default", availableModes: [] }
      }
    }));
  } else if (message.method === "session/prompt") {
    const response = JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        stopReason: "end_turn",
        echo: message.params.prompt?.[0]?.text ?? null
      }
    });
    if (!loaded) {
      // Flush the response, then die while the session is idle.
      process.stdout.write(response + "\n", () => process.exit(0));
    } else {
      console.log(response);
    }
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

/// Adapter that advertises `loadSession`, crashes (exit 7) on the first
/// `session/prompt` after `session/new`, but answers prompts normally once the
/// session was re-attached via `session/load`. Process-local state only, so
/// the exact same script serves both the original and the respawned launch.
fn restartable_adapter_script() -> &'static str {
    r#"
import readline from "node:readline";

const lines = readline.createInterface({ input: process.stdin });
let loaded = false;

for await (const line of lines) {
  if (!line.trim()) continue;
  const message = JSON.parse(line);
  if (message.method === "initialize") {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        protocolVersion: message.params.protocolVersion,
        agentCapabilities: { loadSession: true },
        agentInfo: { name: "restartable-acp-adapter" },
        configOptions: []
      }
    }));
  } else if (message.method === "session/new") {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        sessionId: "restartable-session",
        modes: { currentModeId: "default", availableModes: [] }
      }
    }));
  } else if (message.method === "session/load") {
    loaded = true;
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        modes: { currentModeId: "default", availableModes: [] }
      }
    }));
  } else if (message.method === "session/prompt") {
    if (!loaded) {
      process.stderr.write("fatal: adapter crashed handling session/prompt\n");
      process.exit(7);
    }
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        stopReason: "end_turn",
        echo: message.params.prompt?.[0]?.text ?? null
      }
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

/// Adapter with NO native resume capability that crashes (exit 3) on
/// `session/prompt`: the auto-restart must classify it `unsupported` and evict.
fn unsupported_adapter_script() -> &'static str {
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
      result: {
        protocolVersion: message.params.protocolVersion,
        agentInfo: { name: "unsupported-acp-adapter" },
        configOptions: []
      }
    }));
  } else if (message.method === "session/new") {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        sessionId: "unsupported-session",
        modes: { currentModeId: "default", availableModes: [] }
      }
    }));
  } else if (message.method === "session/prompt") {
    process.stderr.write("fatal: adapter crashed handling session/prompt\n");
    process.exit(3);
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

fn create_session_request(adapter: &Path, cwd: &Path) -> AcpRequest {
    AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
        agent_type: String::from("pi"),
        runtime: AcpRuntimeKind::JavaScript,
        adapter_entrypoint: adapter.to_string_lossy().into_owned(),
        cwd: cwd.to_string_lossy().into_owned(),
        args: Vec::new(),
        env: HashMap::new(),
        protocol_version: i32::from(ACP_PROTOCOL_VERSION),
        client_capabilities: String::from(r#"{"fs":{}}"#),
        mcp_servers: String::from(r#"{"servers":[]}"#),
        skip_os_instructions: true,
        additional_instructions: None,
    })
}

fn prompt_request(session_id: &str, text: &str) -> AcpRequest {
    AcpRequest::AcpSessionRequest(AcpSessionRequest {
        session_id: String::from(session_id),
        method: String::from("session/prompt"),
        params: Some(format!(
            r#"{{"prompt":[{{"type":"text","text":"{text}"}}]}}"#
        )),
    })
}

/// Decode the single `AcpAgentExitedEvent` in a dispatch's event batch (which
/// may also carry `AcpAgentStderrEvent`s from the crashing adapter).
fn decode_single_agent_exited_event(events: &[EventFrame]) -> AcpAgentExitedEvent {
    let mut exited = Vec::new();
    for frame in events {
        let EventPayload::ExtEnvelope(envelope) = &frame.payload else {
            continue;
        };
        if envelope.namespace != ACP_EXTENSION_NAMESPACE {
            continue;
        }
        let event: AcpEvent =
            serde_bare::from_slice(&envelope.payload).expect("decode ACP event");
        if let AcpEvent::AcpAgentExitedEvent(event) = event {
            exited.push(event);
        }
    }
    assert_eq!(
        exited.len(),
        1,
        "expected exactly one AcpAgentExitedEvent, got {exited:?}"
    );
    exited.remove(0)
}

fn dispatch_acp(
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
        ResponsePayload::RejectedResponse(rejected) => panic!(
            "ACP dispatch was rejected at the wire layer instead of returning an \
             ACP error response: code={} message={}",
            rejected.code, rejected.message
        ),
        other => panic!("unexpected sidecar response: {other:?}"),
    }
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
                client_name: String::from("acp-extension-adapter-restart"),
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
