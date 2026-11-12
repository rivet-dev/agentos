mod support;

use agentos_vm::wire::{
    GuestFilesystemCallRequest, GuestFilesystemOperation, GuestRuntimeKind, RequestPayload,
    ResponsePayload,
};
use std::collections::HashMap;
use support::{
    authenticate_wire, create_vm_wire_with_metadata, dispose_vm_and_close_session_wire,
    open_session_wire, wire_request, wire_vm,
};

fn call(
    operation: GuestFilesystemOperation,
    content: Option<&str>,
    len: Option<u64>,
) -> GuestFilesystemCallRequest {
    GuestFilesystemCallRequest {
        operation,
        path: "/tmp/quota-transport.txt".into(),
        destination_path: None,
        target: None,
        content: content.map(str::to_owned),
        encoding: None,
        recursive: false,
        max_depth: None,
        mode: None,
        uid: None,
        gid: None,
        atime_ms: None,
        mtime_ms: None,
        len,
        offset: None,
    }
}

#[test]
fn trusted_filesystem_quota_rejection_preserves_u64_and_file() {
    let mut manager = support::new_sidecar("filesystem-quota-transport");
    let connection = authenticate_wire(&mut manager, "quota-connection");
    let session = open_session_wire(&mut manager, 2, &connection);
    let cwd = support::temp_dir("filesystem-quota-transport-cwd");
    let limit = u64::MAX - 1;
    let (vm, _) = create_vm_wire_with_metadata(
        &mut manager,
        3,
        &connection,
        &session,
        GuestRuntimeKind::JavaScript,
        &cwd,
        HashMap::from([("resource.max_filesystem_bytes".into(), limit.to_string())]),
    );
    let write = manager
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection, &session, &vm),
            RequestPayload::GuestFilesystemCallRequest(call(
                GuestFilesystemOperation::WriteFile,
                Some("seed"),
                None,
            )),
        ))
        .expect("seed real filesystem RPC");
    let rejected = manager
        .dispatch_wire_blocking(wire_request(
            5,
            wire_vm(&connection, &session, &vm),
            RequestPayload::GuestFilesystemCallRequest(call(
                GuestFilesystemOperation::Truncate,
                None,
                Some(u64::MAX),
            )),
        ))
        .expect("quota request returns a response");
    let read = manager
        .dispatch_wire_blocking(wire_request(
            6,
            wire_vm(&connection, &session, &vm),
            RequestPayload::GuestFilesystemCallRequest(call(
                GuestFilesystemOperation::ReadFile,
                None,
                None,
            )),
        ))
        .expect("read after rejected truncate");
    // Release actual VM/session/connection before the meaningful baseline assertion.
    dispose_vm_and_close_session_wire(&mut manager, &connection, &session, &vm);
    std::fs::remove_dir_all(cwd).expect("remove private fixture cwd");
    assert!(matches!(
        write.response.payload,
        ResponsePayload::GuestFilesystemResultResponse(_)
    ));
    match read.response.payload {
        ResponsePayload::GuestFilesystemResultResponse(value) => {
            assert_eq!(value.content.as_deref(), Some("seed"))
        }
        other => panic!("rejected quota changed readable file: {other:?}"),
    }
    let ResponsePayload::RejectedResponse(value) = rejected.response.payload else {
        panic!("quota did not reject")
    };
    assert_eq!(
        value.code, "ERR_AGENTOS_RESOURCE_LIMIT",
        "trusted quota RPC lost its typed limit metadata: {value:?}"
    );
    assert_eq!(value.errno.as_deref(), Some("ENOSPC"));
    assert_eq!(
        value.limit_name.as_deref(),
        Some("limits.resources.maxFilesystemBytes")
    );
    assert_eq!(value.configuration_path, value.limit_name);
    assert_eq!(value.configured_limit, Some(limit));
    assert_eq!(value.requested, Some(u64::MAX));
    assert_eq!(value.current_usage, None);
    assert_eq!(value.unit.as_deref(), Some("bytes"));
    assert_eq!(value.retryable, Some(false));
    let bytes = serde_bare::to_vec(&value).expect("existing rejection schema encodes fullu64");
    let decoded: agentos_vm::wire::RejectedResponse =
        serde_bare::from_slice(&bytes).expect("decode same rejection schema");
    assert_eq!(decoded.configured_limit, Some(u64::MAX - 1));
    assert_eq!(decoded.requested, Some(u64::MAX));
}

#[test]
fn trusted_filesystem_write_quota_returns_named_limit_without_overwrite() {
    let mut manager = support::new_sidecar("filesystem-write-quota-transport");
    let connection = authenticate_wire(&mut manager, "quota-write-connection");
    let session = open_session_wire(&mut manager, 2, &connection);
    let cwd = support::temp_dir("filesystem-write-quota-transport-cwd");
    let limit = 1024_u64 * 1024;
    let (vm, _) = create_vm_wire_with_metadata(
        &mut manager,
        3,
        &connection,
        &session,
        GuestRuntimeKind::JavaScript,
        &cwd,
        HashMap::from([("resource.max_filesystem_bytes".into(), limit.to_string())]),
    );
    let seed = manager
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection, &session, &vm),
            RequestPayload::GuestFilesystemCallRequest(call(
                GuestFilesystemOperation::WriteFile,
                Some("seed"),
                None,
            )),
        ))
        .expect("seed actual file");
    let content = "x".repeat(1536 * 1024);
    let rejected = manager
        .dispatch_wire_blocking(wire_request(
            5,
            wire_vm(&connection, &session, &vm),
            RequestPayload::GuestFilesystemCallRequest(call(
                GuestFilesystemOperation::WriteFile,
                Some(&content),
                None,
            )),
        ))
        .expect("write quota response");
    let read = manager
        .dispatch_wire_blocking(wire_request(
            6,
            wire_vm(&connection, &session, &vm),
            RequestPayload::GuestFilesystemCallRequest(call(
                GuestFilesystemOperation::ReadFile,
                None,
                None,
            )),
        ))
        .expect("read preserved file");
    dispose_vm_and_close_session_wire(&mut manager, &connection, &session, &vm);
    std::fs::remove_dir_all(cwd).expect("remove private fixture cwd");
    assert!(matches!(
        seed.response.payload,
        ResponsePayload::GuestFilesystemResultResponse(_)
    ));
    match read.response.payload {
        ResponsePayload::GuestFilesystemResultResponse(value) => {
            assert_eq!(value.content.as_deref(), Some("seed"))
        }
        other => panic!("write quota changed file: {other:?}"),
    }
    let ResponsePayload::RejectedResponse(value) = rejected.response.payload else {
        panic!("write quota did not reject")
    };
    assert_eq!(
        value.code, "ERR_AGENTOS_RESOURCE_LIMIT",
        "trusted write quota RPC lost its typed limit metadata: {value:?}"
    );
    assert_eq!(value.errno.as_deref(), Some("ENOSPC"));
    assert_eq!(
        value.limit_name.as_deref(),
        Some("limits.resources.maxFilesystemBytes")
    );
    assert_eq!(value.configuration_path, value.limit_name);
    assert_eq!(value.configured_limit, Some(limit));
    assert!(value
        .requested
        .is_some_and(|projected| projected >= 1536 * 1024));
    assert_eq!(value.current_usage, None);
    assert_eq!(value.unit.as_deref(), Some("bytes"));
}
