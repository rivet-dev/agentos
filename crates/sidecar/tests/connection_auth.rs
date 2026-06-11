mod support;

use agent_os_sidecar::protocol::{
    AuthenticateRequest, CreateVmRequest, GuestRuntimeKind, OpenSessionRequest, OwnershipScope,
    RequestPayload, ResponsePayload, SidecarPlacement,
};
use support::{
    TEST_AUTH_TOKEN, authenticate, authenticate_with_token, new_sidecar,
    new_sidecar_with_auth_token, open_session, request, temp_dir,
};

#[test]
fn authenticate_ignores_client_connection_hints_and_preserves_existing_owners() {
    let mut sidecar = new_sidecar("connection-auth");

    let connection_a = authenticate(&mut sidecar, "client-a");
    let session_a = open_session(&mut sidecar, 2, &connection_a);

    let auth_b = authenticate_with_token(&mut sidecar, 3, &connection_a, TEST_AUTH_TOKEN);
    let connection_b = match auth_b.response.payload {
        ResponsePayload::Authenticated(response) => {
            assert_eq!(
                auth_b.response.ownership,
                OwnershipScope::connection(&response.connection_id)
            );
            assert_ne!(response.connection_id, connection_a);
            response.connection_id
        }
        other => panic!("unexpected second auth response: {other:?}"),
    };

    let cwd = temp_dir("connection-auth-cwd");
    let create_vm = sidecar
        .dispatch_blocking(request(
            4,
            OwnershipScope::session(&connection_b, &session_a),
            RequestPayload::CreateVm(CreateVmRequest {
                runtime: GuestRuntimeKind::JavaScript,
                metadata: std::collections::BTreeMap::from([(
                    String::from("cwd"),
                    cwd.to_string_lossy().into_owned(),
                )]),
                root_filesystem: Default::default(),
                permissions: None,
            }),
        ))
        .expect("dispatch cross-connection create_vm");

    match create_vm.response.payload {
        ResponsePayload::Rejected(response) => {
            assert_eq!(response.code, "invalid_state");
            assert!(response.message.contains("not owned"));
        }
        other => panic!("unexpected create_vm response: {other:?}"),
    }
}

#[test]
fn authenticate_rejects_invalid_auth_tokens() {
    let mut sidecar = new_sidecar_with_auth_token("connection-auth-invalid", "expected-token");

    let rejected_connection = "client-a";
    let result = authenticate_with_token(&mut sidecar, 1, rejected_connection, "wrong-token");

    match result.response.payload {
        ResponsePayload::Rejected(response) => {
            assert_eq!(response.code, "unauthorized");
            assert!(response.message.contains("invalid auth token"));
        }
        other => panic!("unexpected invalid auth response: {other:?}"),
    }

    assert_rejected_auth_does_not_open_connection(&mut sidecar, 2, rejected_connection);
    assert_rejected_auth_does_not_open_connection(&mut sidecar, 3, "conn-1");
}

#[test]
fn authenticate_rejects_bridge_contract_version_mismatch() {
    let mut sidecar = new_sidecar("connection-auth-bridge-version");
    let rejected_connection = "client-a";

    let result = sidecar
        .dispatch_blocking(request(
            1,
            OwnershipScope::connection(rejected_connection),
            RequestPayload::Authenticate(AuthenticateRequest {
                client_name: String::from("bridge-version-test"),
                auth_token: String::from(TEST_AUTH_TOKEN),
                bridge_version: agent_os_bridge::bridge_contract().version + 1,
            }),
        ))
        .expect("dispatch mismatched authenticate");

    match result.response.payload {
        ResponsePayload::Rejected(response) => {
            assert_eq!(response.code, "bridge_version_mismatch");
            assert!(response.message.contains("expected"));
            assert!(response.message.contains("got"));
        }
        other => panic!("unexpected bridge version auth response: {other:?}"),
    }

    assert_rejected_auth_does_not_open_connection(&mut sidecar, 2, rejected_connection);
    assert_rejected_auth_does_not_open_connection(&mut sidecar, 3, "conn-1");
}

fn assert_rejected_auth_does_not_open_connection(
    sidecar: &mut agent_os_sidecar::NativeSidecar<support::RecordingBridge>,
    request_id: i64,
    connection_id: &str,
) {
    let result = sidecar
        .dispatch_blocking(request(
            request_id,
            OwnershipScope::connection(connection_id),
            RequestPayload::OpenSession(OpenSessionRequest {
                placement: SidecarPlacement::Shared { pool: None },
                metadata: Default::default(),
            }),
        ))
        .expect("dispatch open session after rejected authenticate");

    match result.response.payload {
        ResponsePayload::Rejected(response) => {
            assert_eq!(response.code, "invalid_state");
            assert!(response.message.contains("has not authenticated"));
        }
        other => panic!("unexpected post-rejection session response: {other:?}"),
    }
}
