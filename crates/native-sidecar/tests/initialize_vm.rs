mod support;

use agentos_native_sidecar::wire::{
    CreateVmRequest, DisposeReason, DisposeVmRequest, GuestRuntimeKind, InitializeVmRequest,
    RegisterHostCallbacksRequest, RegisteredHostCallbackDefinition, RequestPayload,
    ResponsePayload,
};
use std::collections::HashMap;
use support::{
    authenticate_wire, new_sidecar, open_session_wire, wire_request, wire_session, wire_vm,
};

fn toolkit(name: &str) -> RegisterHostCallbacksRequest {
    RegisterHostCallbacksRequest {
        name: name.to_owned(),
        description: String::from("test tools"),
        callbacks: HashMap::from([(
            String::from("echo"),
            RegisteredHostCallbackDefinition {
                description: String::from("echo input"),
                input_schema: String::from(r#"{"type":"object"}"#),
                timeout_ms: None,
                examples: Vec::new(),
            },
        )]),
    }
}

fn initialize_payload(host_callbacks: Vec<RegisterHostCallbacksRequest>) -> InitializeVmRequest {
    let create = CreateVmRequest::json_config(
        GuestRuntimeKind::JavaScript,
        agentos_vm_config::CreateVmConfig::default(),
    );
    InitializeVmRequest {
        runtime: create.runtime,
        config: create.config,
        mounts: None,
        packages: None,
        packages_mount_at: None,
        host_callbacks: Some(host_callbacks),
    }
}

#[test]
fn initialize_vm_is_atomic_and_rolls_back_partial_state() {
    let mut sidecar = new_sidecar("initialize-vm");
    let connection_id = authenticate_wire(&mut sidecar, "client");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let duplicate = toolkit("duplicate");

    let failed = sidecar
        .dispatch_wire_blocking(wire_request(
            3,
            wire_session(&connection_id, &session_id),
            RequestPayload::InitializeVmRequest(initialize_payload(vec![
                duplicate.clone(),
                duplicate,
            ])),
        ))
        .expect("dispatch failed initialization");
    let ResponsePayload::RejectedResponse(rejected) = failed.response.payload else {
        panic!("unexpected failed initialization response");
    };
    assert_eq!(rejected.code, "conflict");
    assert!(rejected.message.contains("already registered"));

    let disposed = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, "vm-1"),
            RequestPayload::DisposeVmRequest(DisposeVmRequest {
                reason: DisposeReason::Requested,
            }),
        ))
        .expect("dispatch dispose of rolled-back VM");
    assert!(matches!(
        disposed.response.payload,
        ResponsePayload::RejectedResponse(_)
    ));

    let initialized = sidecar
        .dispatch_wire_blocking(wire_request(
            5,
            wire_session(&connection_id, &session_id),
            RequestPayload::InitializeVmRequest(initialize_payload(vec![toolkit("tools")])),
        ))
        .expect("dispatch successful initialization");
    let ResponsePayload::VmInitializedResponse(initialized) = initialized.response.payload else {
        panic!("unexpected successful initialization response");
    };
    assert_eq!(initialized.vm_id, "vm-2");
    assert_eq!(initialized.applied_mounts, 0);
    assert_eq!(initialized.host_callbacks.len(), 1);
    assert_eq!(initialized.host_callbacks[0].registration, "tools");
}
