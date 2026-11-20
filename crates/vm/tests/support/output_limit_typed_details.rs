#[tokio::test(flavor = "current_thread")]
async fn pending_process_event_limits_preserve_typed_details() {
    let mut manager = test_sidecar();
    let vm_id = create_test_vms(&mut manager, 1).await.remove(0);
    let envelope = || ProcessEventEnvelope {
        connection_id: "vm-handle-test-connection".into(),
        session_id: "vm-handle-test-session".into(),
        vm_id: vm_id.clone(),
        process_id: "limit-fixture".into(),
        child_path: Vec::new(),
        event: ActiveExecutionEvent::Stdout("payload".into()),
    };
    for (name, limit, observed) in [
        ("limits.process.pendingEventCount", 1, 2),
        (
            "limits.process.pendingEventBytes",
            envelope().retained_bytes(),
            envelope().retained_bytes() * 2,
        ),
    ] {
        manager.pending_process_events.clear();
        {
            let mut vm = manager.vms.get_mut(&vm_id).unwrap();
            vm.limits.process.pending_event_count = if name.ends_with("Count") { limit } else { 8 };
            vm.limits.process.pending_event_bytes = if name.ends_with("Bytes") {
                limit
            } else {
                usize::MAX
            };
        }
        manager
            .check_pending_process_event_capacity(&envelope())
            .expect("first event fits");
        manager.pending_process_events.push_back(envelope());
        let error = manager
            .check_pending_process_event_capacity(&envelope())
            .unwrap_err();
        assert_eq!(error.code(), Some("ERR_AGENTOS_RESOURCE_LIMIT"));
        assert_eq!(
            crate::execution::error_code(&error),
            "ERR_AGENTOS_RESOURCE_LIMIT"
        );
        assert!(error.to_string().contains(&format!("raise {name}")));
        let VmError::VmOutputLimit(fault) = error else {
            panic!("expected exact typed VM output aggregate fault")
        };
        assert_eq!(
            fault.ownership,
            crate::wire::OwnershipScope::vm(
                "vm-handle-test-connection",
                "vm-handle-test-session",
                &vm_id
            )
        );
        assert_eq!(
            fault.rejection().unit.as_deref(),
            Some(if name.ends_with("Bytes") {
                "bytes"
            } else {
                "items"
            })
        );
        assert_eq!(
            fault.error.details,
            Some(serde_json::json!({
                "limitName": name, "configPath": name, "limit": limit, "observed": observed,
            }))
        );
    }
}
