//! Regression guard for adapter-stderr ("Adapter stderr silently discarded").
//!
//! Original bug: the ACP adapter child process' stderr (and its premature exit)
//! were silently discarded, so an adapter that crashed before answering a
//! JSON-RPC request would hang or fail opaquely instead of surfacing a
//! diagnostic to the caller.
//!
//! In the current source the adapter runs inside the VM and the shared exchange
//! loop `send_json_rpc_request()` in
//! `crates/agentos-sidecar/src/acp_extension.rs` now (a) forwards agent
//! stderr as an Agent OS ACP extension event and (b) observes the adapter
//! `ProcessExitedEvent` and returns
//! `SidecarError::InvalidState("ACP adapter process {id} exited with code {} ...")`.
//!
//! This test drives the exit-code-observation half of the fix through the
//! public Ext RPC surface: it spins up an adapter that, on `session/prompt`,
//! writes a diagnostic line to stderr and then `process.exit(1)` WITHOUT sending
//! a response. The dispatch must come back as `ResponsePayload::Rejected` whose
//! message mentions "exited with code 1", proving the crash is surfaced rather
//! than silently hidden. If the bug regressed (stderr/exit dropped), the loop
//! would instead hang until timeout / return a different error and this test
//! would fail.

#[path = "support/bridge.rs"]
mod bridge_support;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use agentos_native_sidecar::wire::{
    AuthenticateRequest, ConfigureVmRequest, ConnectionOwnership, CreateVmRequest, ExtEnvelope,
    GuestRuntimeKind, OpenSessionRequest, OwnershipScope, PackageDescriptor, PackagePath,
    RequestFrame, RequestPayload, ResponsePayload, SessionOwnership, SidecarPlacement,
    SidecarPlacementShared, VmOwnership,
};
use agentos_native_sidecar::{NativeSidecar, NativeSidecarConfig};
use agentos_protocol::generated::v1::{
    AcpCreateSessionRequest, AcpErrorResponse, AcpRequest, AcpResponse, AcpRuntimeKind,
    AcpSessionRequest,
};
use agentos_protocol::{ACP_EXTENSION_NAMESPACE, PROTOCOL_VERSION as ACP_PROTOCOL_VERSION};
use agentos_vm_config as vm_config;
use bridge_support::RecordingBridge;

#[test]
fn adapter_stderr_and_exit_surface_to_caller() {
    assert_node_available();
    let mut sidecar = new_sidecar("adapter-stderr");
    // No sidecar request handler is installed: this adapter crashes before it
    // would ever issue a host callback, so none is needed.

    let connection_id = authenticate(&mut sidecar);
    let session_id = open_session(&mut sidecar, &connection_id);
    let cwd = temp_dir("adapter-stderr-cwd");
    let adapter = cwd.join("crashing-adapter.mjs");
    fs::write(&adapter, crashing_adapter_script()).expect("write adapter script");
    let vm_id = create_vm(&mut sidecar, &connection_id, &session_id, &cwd);

    // Bootstrap the session normally (initialize + session/new succeed).
    let created = dispatch_acp(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
            agent_type: String::from("pi"),
            runtime: Some(AcpRuntimeKind::JavaScript),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            args: Some(Vec::new()),
            env: Some(HashMap::new()),
            protocol_version: Some(i32::from(ACP_PROTOCOL_VERSION)),
            client_capabilities: Some(String::from(r#"{"fs":{}}"#)),
            mcp_servers: Some(String::from(r#"{"servers":[]}"#)),
            skip_os_instructions: Some(true),
            additional_instructions: None,
        }),
    );

    let AcpResponse::AcpSessionCreatedResponse(created) = created else {
        panic!("unexpected create response: {created:?}");
    };
    assert_eq!(created.session_id, "adapter-session");

    // Now send the prompt: the adapter writes to stderr and exits(1) without a
    // JSON-RPC response. The exchange loop must observe the exit and surface it
    // to the caller. The dispatch handler converts the resulting
    // `SidecarError::InvalidState` into an `AcpErrorResponse` that carries the
    // exit-code diagnostic, rather than hanging until timeout or silently
    // dropping the failure.
    let response = dispatch_acp(
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

    let AcpResponse::AcpErrorResponse(AcpErrorResponse { code, message }) = response else {
        panic!("expected an AcpErrorResponse surfacing the adapter crash, got: {response:?}");
    };
    assert_eq!(
        code, "invalid_state",
        "adapter crash should surface as an invalid_state error, got code {code:?}"
    );
    assert!(
        message.contains("exited with code 1"),
        "expected adapter-exit diagnostic to surface to caller, got: {message}"
    );
}

/// Adapter that handshakes correctly, then on `session/prompt` writes a
/// diagnostic to stderr and exits without responding.
fn crashing_adapter_script() -> &'static str {
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
        agentInfo: { name: "crashing-acp-adapter", args: process.argv.slice(2) },
        configOptions: []
      }
    }));
  } else if (message.method === "session/new") {
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
    // Emit a diagnostic on stderr (the stream Agent OS forwards as agent stderr),
    // then crash WITHOUT sending a JSON-RPC response so the caller must rely on
    // exit-code observation to avoid hanging.
    process.stderr.write("fatal: adapter blew up handling session/prompt\n");
    process.exit(1);
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

fn dispatch_acp(
    sidecar: &mut NativeSidecar<RecordingBridge>,
    request_id: i64,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    request: AcpRequest,
) -> AcpResponse {
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
            serde_bare::from_slice(&envelope.payload).expect("decode ACP response")
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
            schema: agentos_native_sidecar::wire::protocol_schema(),
            request_id: 1,
            ownership: OwnershipScope::ConnectionOwnership(ConnectionOwnership {
                connection_id: String::from("client"),
            }),
            payload: RequestPayload::AuthenticateRequest(AuthenticateRequest {
                client_name: String::from("acp-extension-adapter-stderr"),
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
                    cwd: Some(cwd.to_string_lossy().into_owned()),
                    permissions: Some(allow_all_permissions()),
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
    configure_mock_agent_package(sidecar, connection_id, session_id, &vm_id, cwd);
    vm_id
}

fn configure_mock_agent_package(
    sidecar: &mut NativeSidecar<RecordingBridge>,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    cwd: &Path,
) {
    let adapter = cwd.join("crashing-adapter.mjs");
    if !adapter.exists() {
        return;
    }
    let script = fs::read_to_string(&adapter).expect("read crashing adapter");
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
        .expect("configure crashing ACP package");
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
