#[tokio::test(flavor = "current_thread")]
async fn output_failure_fences_only_one_vm_and_preserves_counted_closing_on_drain_timeout() {
    let (coordinator, _, session, _, _) = configured();
    let operations = scope_test_registry();
    let owner = OwnershipScope::vm("connection-a", "session-a", "vm-a");
    let operation = operations
        .admit(
            RequestOperationKey::new("connection-a", 1),
            vm_metadata("connection-a", "session-a", "vm-a"),
            1,
        )
        .unwrap();
    let permit = coordinator
        .admit(operation.metadata(), operation.cancellation())
        .await
        .unwrap();
    let fault = crate::state::VmOutputFailure {
            ownership: owner.clone(), generation: 1,
            error: crate::executor::backend::HostServiceError::new("ERR_AGENTOS_RESOURCE_LIMIT", "original count fault")
                .with_details(serde_json::json!({"limitName":"limits.process.pendingEventCount","limit":1,"observed":2,"configPath":"limits.process.pendingEventCount"})),
        };
    let (disposal, first) = coordinator
        .begin_vm_output_failure(&fault, &operations)
        .unwrap();
    assert!(first);
    let disposal = disposal.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(1), disposal.wait_drained())
            .await
            .is_err()
    );
    assert_eq!(session.snapshot().child_count, 2);
    assert_eq!(
        session.vm("vm-a").unwrap().snapshot().phase,
        CoordinatorPhase::Closing
    );
    assert!(coordinator.with_vm_admission(&owner, || ()).is_err());
    assert!(coordinator
        .with_vm_admission(
            &OwnershipScope::vm("connection-a", "session-a", "vm-b"),
            || ()
        )
        .is_ok());
    let (duplicate, first) = coordinator
        .begin_vm_output_failure(&fault, &operations)
        .unwrap();
    assert!(duplicate.is_none());
    assert!(!first);
    assert_eq!(
        operation.claim_terminal(),
        Some(crate::request_operations::TerminalClaim::OutputFailure(
            fault
        ))
    );
    operation.release();
    drop(permit);
    tokio::time::timeout(std::time::Duration::from_secs(1), disposal.wait_drained())
        .await
        .unwrap();
    disposal.complete().unwrap();
    assert!(coordinator.registered_vm_generation(&owner).is_none());
    assert!(operations.output_failure(&owner).is_none());
    assert_eq!(session.snapshot().child_count, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn output_failure_attaches_to_the_existing_real_disposer_without_second_drain() {
    let (coordinator, _, _, _, _) = configured();
    let operations = scope_test_registry();
    let owner = OwnershipScope::vm("connection-a", "session-a", "vm-a");
    let key = RequestOperationKey::new("connection-a", 99);
    let disposer = operations
        .admit(
            key.clone(),
            vm_metadata("connection-a", "session-a", "vm-a"),
            1,
        )
        .unwrap();
    let disposal = coordinator
        .begin_vm_disposal_with_operations(
            &owner,
            OperationCancellationReason::Explicit,
            &operations,
            &key,
        )
        .unwrap();
    let fault = crate::state::VmOutputFailure {
            ownership: owner.clone(), generation: 1,
            error: crate::executor::backend::HostServiceError::new("ERR_AGENTOS_RESOURCE_LIMIT", "original byte fault")
                .with_details(serde_json::json!({"limitName":"limits.process.pendingEventBytes","limit":3,"observed":4,"configPath":"limits.process.pendingEventBytes"})),
        };
    let (new_disposal, first) = coordinator
        .begin_vm_output_failure(&fault, &operations)
        .unwrap();
    assert!(new_disposal.is_none());
    assert!(first);
    let (new_disposal, first) = coordinator
        .begin_vm_output_failure(&fault, &operations)
        .unwrap();
    assert!(new_disposal.is_none());
    assert!(!first);
    // Match finish_request DisposeExecuted: complete the VM disposal and
    // remove its fence BEFORE the excluded real request claims terminal.
    tokio::time::timeout(std::time::Duration::from_secs(1), disposal.wait_drained())
        .await
        .unwrap();
    disposal.complete().unwrap();
    assert!(operations.output_failure(&owner).is_none());
    assert_eq!(
        disposer.claim_terminal(),
        Some(crate::request_operations::TerminalClaim::OutputFailure(
            fault
        ))
    );
    assert_eq!(disposer.claim_terminal(), None);
    disposer.release();
}

#[tokio::test(flavor = "current_thread")]
async fn output_failure_attaches_to_request_free_cleanup_without_self_drain_and_preserves_late_terminal(
) {
    let (coordinator, _, session, _, _) = configured();
    let operations = scope_test_registry();
    coordinator.bind_operation_table(&operations).unwrap();
    let owner = OwnershipScope::vm("connection-a", "session-a", "vm-a");
    let initiating = operations
        .admit(
            RequestOperationKey::new("connection-a", 1),
            RequestOperationMetadata::new(
                owner.clone(),
                "extension self-disposal",
                VmConcurrencyClass::OwnershipOnly,
            ),
            1,
        )
        .unwrap();
    // This is the real extension request class: membership is checked but
    // no VM gate permit is retained while the extension awaits disposal.
    let initiating_permit = coordinator
        .admit(initiating.metadata(), initiating.cancellation())
        .await
        .unwrap();
    assert!(initiating_permit.vm_gate.is_none());
    assert!(initiating_permit.vm_lifecycle.is_none());
    let active = operations
        .admit(
            RequestOperationKey::new("connection-a", 2),
            vm_metadata("connection-a", "session-a", "vm-a"),
            1,
        )
        .unwrap();
    let permit = coordinator
        .admit(active.metadata(), active.cancellation())
        .await
        .unwrap();
    let disposal = coordinator
        .begin_vm_disposal(&owner, OperationCancellationReason::Explicit)
        .unwrap();
    let fault = crate::state::VmOutputFailure { ownership: owner.clone(), generation: 1,
            error: crate::executor::backend::HostServiceError::new("ERR_AGENTOS_RESOURCE_LIMIT", "original request-free cleanup race").with_details(serde_json::json!({"limitName":"limits.process.pendingEventCount","limit":1,"observed":2,"configPath":"limits.process.pendingEventCount"})), };
    let (second, first) = coordinator
        .begin_vm_output_failure(&fault, &operations)
        .unwrap();
    assert!(second.is_none());
    assert!(first);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(1), disposal.wait_drained())
            .await
            .is_err()
    );
    assert_eq!(session.snapshot().child_count, 2);
    active.release();
    drop(permit);
    tokio::time::timeout(std::time::Duration::from_secs(1), disposal.wait_drained())
        .await
        .unwrap();
    // Completion precedes the initiating request's terminal publication,
    // exactly as the extension service and protocol completion route do.
    disposal.complete().unwrap();
    assert!(operations.output_failure(&owner).is_none());
    assert_eq!(
        initiating.claim_terminal(),
        Some(crate::request_operations::TerminalClaim::OutputFailure(
            fault
        ))
    );
    initiating.release();
    drop(initiating_permit);
    assert!(coordinator
        .with_vm_admission(
            &OwnershipScope::vm("connection-a", "session-a", "vm-b"),
            || ()
        )
        .is_ok());
}

#[test]
fn completed_plain_disposal_cannot_attach_fault_to_absent_or_replaced_generation() {
    let (coordinator, _, session, old_owner, _) = configured();
    let operations = scope_test_registry();
    coordinator.bind_operation_table(&operations).unwrap();
    let owner = OwnershipScope::vm("connection-a", "session-a", "vm-a");
    let disposal = coordinator
        .begin_vm_disposal(&owner, OperationCancellationReason::Explicit)
        .unwrap();
    disposal.complete().unwrap();
    let fault = crate::state::VmOutputFailure {
        ownership: owner.clone(),
        generation: 1,
        error: crate::executor::backend::HostServiceError::new(
            "ERR_AGENTOS_RESOURCE_LIMIT",
            "stale disposal fault",
        ),
    };
    assert!(coordinator
        .begin_vm_output_failure_for_owner(&old_owner, &fault, &operations)
        .is_err());
    assert!(operations.output_failure(&owner).is_none());
    let successor = session.open_vm("vm-a").unwrap();
    assert_ne!(old_owner.generation, successor.generation);
    assert!(coordinator
        .begin_vm_output_failure_for_owner(&old_owner, &fault, &operations)
        .is_err());
    assert!(operations.output_failure(&owner).is_none());
    assert!(coordinator.with_vm_admission(&owner, || ()).is_ok());
    coordinator
        .begin_vm_disposal(&owner, OperationCancellationReason::Explicit)
        .unwrap()
        .complete()
        .unwrap();
}
