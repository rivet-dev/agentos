#[test]
fn vm_output_failure_claim_selects_original_cause_and_preserves_peer_and_claimed_terminal() {
    for setting in [
        "limits.process.pendingEventCount",
        "limits.process.pendingEventBytes",
    ] {
        let registry = registry(4, 1024);
        let scope = OwnershipScope::vm("a", "s", "v");
        let fault = crate::state::VmOutputFailure {
                ownership: scope.clone(), generation: 7,
                error: crate::executor::backend::HostServiceError::new("ERR_AGENTOS_RESOURCE_LIMIT", "original output budget")
                    .with_details(serde_json::json!({"limitName":setting,"limit":3,"observed":4,"configPath":setting})),
            };
        let admit = |id, owner| {
            registry
                .admit(
                    RequestOperationKey::new("a", id),
                    RequestOperationMetadata::new(owner, "test", VmConcurrencyClass::OwnershipOnly),
                    1,
                )
                .unwrap()
        };
        let affected = admit(1, scope.clone());
        let published = admit(2, scope.clone());
        let peer = admit(3, OwnershipScope::vm("a", "s", "peer"));
        let progress = registry
            .progress_requests()
            .admit_owned(RequestOperationKey::new("a", 4), scope.clone(), 1)
            .unwrap();
        assert!(published.try_mark_terminal());
        let drain = registry.close_scope_with_cause(
            scope.clone(),
            OperationCancellationReason::Explicit,
            None,
            Some(fault.clone()),
        );
        assert_eq!(
            affected.claim_terminal(),
            Some(TerminalClaim::OutputFailure(fault.clone()))
        );
        assert_eq!(affected.claim_terminal(), None);
        assert_eq!(published.claim_terminal(), None);
        assert_eq!(peer.claim_terminal(), Some(TerminalClaim::Original));
        assert_eq!(
            progress.claim_acknowledgement(),
            Some(TerminalClaim::OutputFailure(fault.clone()))
        );
        let rejection = fault.rejection();
        assert_eq!(rejection.code, "ERR_AGENTOS_RESOURCE_LIMIT");
        assert_eq!(rejection.message, "original output budget");
        assert_eq!(rejection.configured_limit, Some(3));
        assert_eq!(rejection.requested, Some(4));
        assert_eq!(rejection.configuration_path.as_deref(), Some(setting));
        assert_eq!(rejection.session_generation, Some(7));
        assert_eq!(
            rejection.unit.as_deref(),
            Some(if setting.ends_with("Bytes") {
                "bytes"
            } else {
                "items"
            })
        );
        affected.release();
        published.release();
        peer.release();
        progress.release();
        assert_eq!(drain.retire_closed_scope(), Ok(()));
        assert_eq!(registry.output_failure(&scope), None);
    }
}

#[test]
fn forced_outcomes_keep_vm_fault_for_unclaimed_and_publishing_without_duplicating_retained() {
    for (claim, retain) in [(false, false), (true, false), (true, true)] {
        let registry = registry(4, 1024);
        let scope = OwnershipScope::vm("a", "s", "v");
        let peer = OwnershipScope::vm("a", "s", "peer");
        let fault = crate::state::VmOutputFailure {
                ownership: scope.clone(), generation: 7,
                error: crate::executor::backend::HostServiceError::new("ERR_AGENTOS_RESOURCE_LIMIT", "original byte cause")
                    .with_details(serde_json::json!({"limitName":"limits.process.pendingEventBytes","limit":3,"observed":4,"configPath":"limits.process.pendingEventBytes"})),
            };
        let operation = registry
            .admit(
                RequestOperationKey::new("a", 1),
                RequestOperationMetadata::new(
                    scope.clone(),
                    "affected",
                    VmConcurrencyClass::OwnershipOnly,
                ),
                1,
            )
            .unwrap();
        let progress = registry
            .progress_requests()
            .admit_owned(RequestOperationKey::new("a", 2), scope.clone(), 1)
            .unwrap();
        let peer_operation = registry
            .admit(
                RequestOperationKey::new("a", 3),
                RequestOperationMetadata::new(
                    peer.clone(),
                    "peer",
                    VmConcurrencyClass::OwnershipOnly,
                ),
                1,
            )
            .unwrap();
        let peer_progress = registry
            .progress_requests()
            .admit_owned(RequestOperationKey::new("a", 4), peer.clone(), 1)
            .unwrap();
        if claim {
            assert!(operation.try_mark_terminal());
            assert!(progress.try_acknowledge());
        }
        if retain {
            assert!(operation.mark_terminal_retained());
            assert!(progress.mark_acknowledgement_retained());
        }
        let _drain = registry.close_scope_with_cause(
            scope.clone(),
            OperationCancellationReason::Explicit,
            None,
            Some(fault.clone()),
        );
        let ordinary = registry.force_terminalize(OperationCancellationReason::Shutdown);
        let acknowledgements = registry
            .progress_requests()
            .force_acknowledge(OperationCancellationReason::Shutdown);
        for (outcomes, affected_id, peer_id) in [(&ordinary, 1, 3), (&acknowledgements, 2, 4)] {
            assert_eq!(outcomes.len(), if retain { 1 } else { 2 });
            let affected = outcomes.iter().find(|o| o.key.request_id == affected_id);
            if retain {
                assert!(affected.is_none());
            } else {
                let affected = affected.unwrap();
                assert_eq!(affected.key.connection_id, "a");
                assert_eq!(affected.ownership, scope);
                assert_eq!(affected.output_failure, Some(fault.clone()));
            }
            let peer_outcome = outcomes
                .iter()
                .find(|o| o.key.request_id == peer_id)
                .unwrap();
            assert_eq!(peer_outcome.ownership, peer);
            assert!(peer_outcome.output_failure.is_none());
        }
        assert!(registry
            .force_terminalize(OperationCancellationReason::Shutdown)
            .is_empty());
        assert!(registry
            .progress_requests()
            .force_acknowledge(OperationCancellationReason::Shutdown)
            .is_empty());
        assert_eq!(operation.claim_terminal(), None);
        assert_eq!(progress.claim_acknowledgement(), None);
        operation.release();
        progress.release();
        peer_operation.release();
        peer_progress.release();
    }
    // An excluded real disposer can lose its closure before global drain.
    let registry = registry(1, 16);
    let owner = OwnershipScope::vm("a", "s", "v");
    let key = RequestOperationKey::new("a", 99);
    let operation = registry
        .admit(
            key.clone(),
            RequestOperationMetadata::new(
                owner.clone(),
                "dispose",
                VmConcurrencyClass::OwnershipOnly,
            ),
            1,
        )
        .unwrap();
    let fault = crate::state::VmOutputFailure { ownership: owner.clone(), generation: 7,
            error: crate::executor::backend::HostServiceError::new("ERR_AGENTOS_RESOURCE_LIMIT", "retained disposer cause").with_details(serde_json::json!({"limitName":"limits.process.pendingEventCount","limit":1,"observed":2,"configPath":"limits.process.pendingEventCount"})), };
    let drain = registry.close_scope_with_cause(
        owner.clone(),
        OperationCancellationReason::Explicit,
        Some(&key),
        Some(fault.clone()),
    );
    drain.retire_closed_scope().unwrap();
    assert!(registry.output_failure(&owner).is_none());
    let outcomes = registry.force_terminalize(OperationCancellationReason::Shutdown);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].key, key);
    assert_eq!(outcomes[0].output_failure, Some(fault));
    assert!(!operation.try_mark_terminal());
    operation.release();
}
