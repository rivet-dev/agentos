mod support;

use agentos_native_sidecar::extension::ExtensionSnapshot;
use agentos_native_sidecar::wire::{
    CloseSessionRequest, CreateVmRequest, GuestRuntimeKind, OpenSessionRequest, RequestPayload,
    ResponsePayload, SidecarPlacement, SidecarPlacementShared,
};
use agentos_native_sidecar::{
    Extension, ExtensionContext, ExtensionFuture, ExtensionResponse, NativeSidecar,
    NativeSidecarConfig, SidecarError,
};
use support::{
    authenticate_wire, create_vm_wire, new_sidecar, open_session_wire, temp_dir, wire_connection,
    wire_request, wire_session, RecordingBridge, TEST_AUTH_TOKEN,
};

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

struct FailingSessionDisposeExtension;

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
        Box::pin(async {
            Err(SidecarError::Bridge(String::from(
                "deterministic session teardown failure",
            )))
        })
    }
}

#[test]
fn failed_close_replays_the_terminal_failure_and_releases_admission() {
    let root = temp_dir("wire-session-close-failure");
    let mut sidecar = NativeSidecar::with_config_and_extensions(
        RecordingBridge::default(),
        NativeSidecarConfig {
            sidecar_id: String::from("wire-session-close-failure"),
            max_sessions_per_connection: 1,
            compile_cache_root: Some(root.join("cache")),
            expected_auth_token: Some(String::from(TEST_AUTH_TOKEN)),
            ..NativeSidecarConfig::default()
        },
        vec![Box::new(FailingSessionDisposeExtension)],
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
    let retry = close(&mut sidecar, 4);
    let failure = |payload: ResponsePayload| match payload {
        ResponsePayload::RejectedResponse(rejected) => {
            assert_eq!(rejected.code, "close_session_failed");
            rejected.message
        }
        other => panic!("expected close_session_failed, got {other:?}"),
    };
    assert_eq!(
        failure(first.response.payload),
        failure(retry.response.payload),
        "a retry must replay the terminal teardown failure"
    );

    let reopened = open_session_wire(&mut sidecar, 5, &owner);
    assert_ne!(reopened, session_id, "failed close releases admission");
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
