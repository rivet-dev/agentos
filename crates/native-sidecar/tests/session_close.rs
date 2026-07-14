mod support;

use agentos_native_sidecar::extension::ExtensionSnapshot;
use agentos_native_sidecar::wire::{
    CloseSessionRequest, CreateVmRequest, DisposeReason, DisposeVmRequest, EventPayload,
    GuestRuntimeKind, OpenSessionRequest, RequestPayload, ResponsePayload, SidecarPlacement,
    SidecarPlacementShared, VmLifecycleState,
};
use agentos_native_sidecar::{
    Extension, ExtensionContext, ExtensionFuture, ExtensionResponse, NativeSidecar,
    NativeSidecarConfig, SidecarError,
};
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use support::{
    authenticate_wire, create_vm_wire, execute_wire, new_sidecar, open_session_wire, temp_dir,
    wire_connection, wire_request, wire_session, wire_vm, RecordingBridge, TEST_AUTH_TOKEN,
};

#[test]
fn connection_loss_forces_reclamation_after_cleanup_event_limit() {
    let root = temp_dir("connection-loss-cleanup-limit");
    let mut sidecar = NativeSidecar::with_config(
        RecordingBridge::default(),
        NativeSidecarConfig {
            sidecar_id: String::from("connection-loss-cleanup-limit"),
            compile_cache_root: Some(root.join("cache")),
            max_extension_session_cleanup_events: 1,
            ..NativeSidecarConfig::default()
        },
    )
    .expect("create bounded sidecar");
    let connection_id = authenticate_wire(&mut sidecar, "owner");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = root.join("vm");
    fs::create_dir_all(&cwd).expect("create VM cwd");
    let entrypoint = cwd.join("ignore-term.mjs");
    fs::write(
        &entrypoint,
        "process.on('SIGTERM', () => {}); setInterval(() => process.stdout.write('x'), 1);\n",
    )
    .expect("write active process fixture");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
    );
    execute_wire(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        "process-1",
        GuestRuntimeKind::JavaScript,
        "/workspace/ignore-term.mjs",
        Vec::new(),
    );

    let error = sidecar
        .remove_connection_blocking(&connection_id)
        .expect_err("forced disconnect still reports the bounded cleanup failure");
    assert!(error.to_string().contains("limit_exceeded"));
    let debug = format!("{sidecar:?}");
    for empty_count in [
        "connection_count: 0",
        "session_count: 0",
        "vm_count: 0",
        "vm_disposal_progress_count: 0",
        "extension_process_output_buffer_count: 0",
    ] {
        assert!(
            debug.contains(empty_count),
            "missing {empty_count} in {debug}"
        );
    }
}

#[test]
fn dispose_vm_failure_returns_committed_lifecycle_events_with_typed_rejection() {
    let mut sidecar = new_sidecar("wire-dispose-vm-failure-events");
    let connection_id = authenticate_wire(&mut sidecar, "owner");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("wire-dispose-vm-failure-events-cwd");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
    );
    sidecar
        .with_bridge_mut(|bridge| {
            bridge.push_lifecycle_event_error("injected lifecycle teardown failure");
            bridge.push_filesystem_flush_error("injected filesystem flush failure");
        })
        .expect("inject lifecycle failure");

    let result = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            RequestPayload::DisposeVmRequest(DisposeVmRequest {
                reason: DisposeReason::Requested,
            }),
        ))
        .expect("dispose failure is represented by a protocol response");
    let ResponsePayload::RejectedResponse(rejected) = result.response.payload else {
        panic!("expected cleanup rejection");
    };
    assert_eq!(rejected.code, "cleanup_failed");
    assert!(rejected.message.contains("VM vm-1 lifecycle emission"));
    assert!(rejected
        .message
        .contains("VM vm-1 filesystem snapshot flush"));
    assert!(
        rejected
            .message
            .find("VM vm-1 lifecycle emission")
            .expect("lifecycle error identity")
            < rejected
                .message
                .find("VM vm-1 filesystem snapshot flush")
                .expect("flush error identity"),
        "independent teardown failures preserve deterministic phase order"
    );
    assert!(result.events.iter().any(|event| {
        matches!(&event.payload, EventPayload::VmLifecycleEvent(event) if event.state == VmLifecycleState::Disposing)
    }));
    assert!(result.events.iter().any(|event| {
        matches!(&event.payload, EventPayload::VmLifecycleEvent(event) if event.state == VmLifecycleState::Disposed)
    }));
}

#[test]
fn close_session_disposes_owned_vms_is_idempotent_and_rejects_cross_owner() {
    let mut sidecar = new_sidecar("wire-close-session");
    let owner = authenticate_wire(&mut sidecar, "owner");
    let session_id = open_session_wire(&mut sidecar, 2, &owner);
    let cwd = temp_dir("wire-close-session-cwd");
    let (_vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &owner,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
    );

    let other = authenticate_wire(&mut sidecar, "other");
    let cross_owner = sidecar
        .dispatch_wire_blocking(wire_request(
            5,
            wire_connection(&other),
            RequestPayload::CloseSessionRequest(CloseSessionRequest {
                session_id: session_id.clone(),
            }),
        ))
        .expect("cross-owner close returns a protocol response");
    assert!(matches!(
        cross_owner.response.payload,
        ResponsePayload::RejectedResponse(ref rejected)
            if rejected.code == "ownership_mismatch"
    ));

    let wrong_scope = sidecar
        .dispatch_wire_blocking(wire_request(
            51,
            wire_session(&owner, &session_id),
            RequestPayload::CloseSessionRequest(CloseSessionRequest {
                session_id: session_id.clone(),
            }),
        ))
        .expect("wrong-scope close returns a protocol response");
    assert!(matches!(
        wrong_scope.response.payload,
        ResponsePayload::RejectedResponse(ref rejected)
            if rejected.code == "invalid_ownership"
    ));

    let unauthenticated = sidecar
        .dispatch_wire_blocking(wire_request(
            52,
            wire_connection("missing-connection"),
            RequestPayload::CloseSessionRequest(CloseSessionRequest {
                session_id: session_id.clone(),
            }),
        ))
        .expect("unauthenticated close returns a protocol response");
    assert!(matches!(
        unauthenticated.response.payload,
        ResponsePayload::RejectedResponse(ref rejected)
            if rejected.code == "unauthenticated"
    ));

    let closed = sidecar
        .dispatch_wire_blocking(wire_request(
            6,
            wire_connection(&owner),
            RequestPayload::CloseSessionRequest(CloseSessionRequest {
                session_id: session_id.clone(),
            }),
        ))
        .expect("owner closes session");
    assert!(matches!(
        closed.response.payload,
        ResponsePayload::SessionClosedResponse(ref response)
            if response.session_id == session_id
    ));
    assert!(
        !closed.events.is_empty(),
        "closing a session with an owned VM must expose disposal lifecycle events"
    );

    let create_after_close = sidecar
        .dispatch_wire_blocking(wire_request(
            7,
            wire_session(&owner, &session_id),
            RequestPayload::CreateVmRequest(CreateVmRequest::json_config(
                GuestRuntimeKind::JavaScript,
                agentos_vm_config::CreateVmConfig::default(),
            )),
        ))
        .expect("closed-session create returns rejection");
    assert!(matches!(
        create_after_close.response.payload,
        ResponsePayload::RejectedResponse(_)
    ));

    let retry = sidecar
        .dispatch_wire_blocking(wire_request(
            8,
            wire_connection(&owner),
            RequestPayload::CloseSessionRequest(CloseSessionRequest {
                session_id: session_id.clone(),
            }),
        ))
        .expect("same-owner close retry is acknowledged");
    assert!(matches!(
        retry.response.payload,
        ResponsePayload::SessionClosedResponse(ref response)
            if response.session_id == session_id
    ));
    assert!(retry.events.is_empty());
}

struct FailingSessionDisposeExtension {
    attempts: Arc<AtomicUsize>,
}

impl Extension for FailingSessionDisposeExtension {
    fn namespace(&self) -> &str {
        "dev.agentos.test.close-failure"
    }

    fn handle_request<'a>(
        &'a self,
        _ctx: ExtensionContext<'a>,
        _payload: Vec<u8>,
    ) -> ExtensionFuture<'a, ExtensionResponse> {
        Box::pin(async { Ok(ExtensionResponse::new(Vec::new())) })
    }

    fn on_session_disposed<'a>(&'a self, _ctx: ExtensionSnapshot) -> ExtensionFuture<'a, ()> {
        let attempts = self.attempts.clone();
        Box::pin(async move {
            if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(SidecarError::Bridge(String::from(
                    "transient session teardown failure",
                )))
            } else {
                Ok(())
            }
        })
    }
}

#[test]
fn failed_close_retries_cleanup_before_recording_terminal_success() {
    let root = temp_dir("wire-session-close-failure");
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut sidecar = NativeSidecar::with_config_and_extensions(
        RecordingBridge::default(),
        NativeSidecarConfig {
            sidecar_id: String::from("wire-session-close-failure"),
            max_sessions_per_connection: 1,
            compile_cache_root: Some(root.join("cache")),
            expected_auth_token: Some(String::from(TEST_AUTH_TOKEN)),
            ..NativeSidecarConfig::default()
        },
        vec![Box::new(FailingSessionDisposeExtension {
            attempts: attempts.clone(),
        })],
    )
    .expect("create sidecar with failing teardown extension");
    let owner = authenticate_wire(&mut sidecar, "owner");
    let session_id = open_session_wire(&mut sidecar, 2, &owner);

    let close = |sidecar: &mut NativeSidecar<RecordingBridge>, request_id| {
        sidecar
            .dispatch_wire_blocking(wire_request(
                request_id,
                wire_connection(&owner),
                RequestPayload::CloseSessionRequest(CloseSessionRequest {
                    session_id: session_id.clone(),
                }),
            ))
            .expect("failed close returns a typed protocol rejection")
    };
    let first = close(&mut sidecar, 3);
    let failure = |payload: ResponsePayload| match payload {
        ResponsePayload::RejectedResponse(rejected) => {
            assert_eq!(rejected.code, "close_session_failed");
            rejected.message
        }
        other => panic!("expected close_session_failed, got {other:?}"),
    };
    assert!(failure(first.response.payload).contains("transient session teardown failure"));
    assert_eq!(attempts.load(Ordering::SeqCst), 1);

    let blocked_reopen = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_connection(&owner),
            RequestPayload::OpenSessionRequest(OpenSessionRequest {
                placement: SidecarPlacement::SidecarPlacementShared(SidecarPlacementShared {
                    pool: None,
                }),
            }),
        ))
        .expect("failed close retains the bounded session slot");
    assert!(matches!(
        blocked_reopen.response.payload,
        ResponsePayload::RejectedResponse(_)
    ));

    let retry = close(&mut sidecar, 5);
    assert!(matches!(
        retry.response.payload,
        ResponsePayload::SessionClosedResponse(_)
    ));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    let replay = close(&mut sidecar, 6);
    assert!(matches!(
        replay.response.payload,
        ResponsePayload::SessionClosedResponse(_)
    ));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);

    let reopened = open_session_wire(&mut sidecar, 7, &owner);
    assert_ne!(reopened, session_id, "successful close releases admission");
}

#[test]
fn open_session_enforces_configured_per_connection_bound() {
    let root = temp_dir("wire-session-limit");
    let mut sidecar = NativeSidecar::with_config(
        RecordingBridge::default(),
        NativeSidecarConfig {
            sidecar_id: String::from("wire-session-limit"),
            max_sessions_per_connection: 1,
            compile_cache_root: Some(root.join("cache")),
            expected_auth_token: Some(String::from(TEST_AUTH_TOKEN)),
            ..NativeSidecarConfig::default()
        },
    )
    .expect("create session-limited sidecar");
    let connection_id = authenticate_wire(&mut sidecar, "limited");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);

    let rejected = sidecar
        .dispatch_wire_blocking(wire_request(
            3,
            wire_connection(&connection_id),
            RequestPayload::OpenSessionRequest(OpenSessionRequest {
                placement: SidecarPlacement::SidecarPlacementShared(SidecarPlacementShared {
                    pool: None,
                }),
            }),
        ))
        .expect("session limit returns typed rejection");
    let ResponsePayload::RejectedResponse(rejected) = rejected.response.payload else {
        panic!("expected session limit rejection");
    };
    assert_eq!(rejected.code, "session_limit_exceeded");
    assert!(rejected.message.contains("max_sessions_per_connection"));
    assert!(rejected
        .message
        .contains("AGENTOS_MAX_SESSIONS_PER_CONNECTION"));

    sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_connection(&connection_id),
            RequestPayload::CloseSessionRequest(CloseSessionRequest { session_id }),
        ))
        .expect("close releases the bounded session slot");
    let _reopened = open_session_wire(&mut sidecar, 5, &connection_id);
}
