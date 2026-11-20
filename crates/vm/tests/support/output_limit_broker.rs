#[tokio::test(flavor = "current_thread")]
async fn output_fault_releases_old_lease_and_ingress_after_fence_retirement_without_touching_peer()
{
    for drop_before_complete in [true, false] {
        let config = DriverConfig::default();
        let coordinator =
            crate::ownership_coordinator::OwnershipCoordinator::from_runtime_config(&config);
        let connection = coordinator.register_connection("connection-a").unwrap();
        let session = connection.open_session("session-a").unwrap();
        let _vm_a = session.open_vm("vm-a").unwrap();
        let _vm_b = session.open_vm("vm-b").unwrap();
        let operations =
            crate::request_operations::OperationTable::from_protocol_config(&config.protocol);
        let (broker, ingress, mut driver) = ProcessEventBroker::with_limits(limits(8, 4)).unwrap();
        broker
            .bind_output_authority(
                coordinator.clone(),
                operations.clone(),
                Arc::new(Notify::new()),
            )
            .unwrap();
        let a = target("vm-a", "p-a");
        let b = target("vm-b", "p-b");
        let owner = OwnershipScope::vm("connection-a", "session-a", "vm-a");
        let waiter_a = broker
            .register_waiter(&owner, a.clone(), OperationCancellation::new())
            .unwrap();
        let waiter_b = broker
            .register_waiter(
                &OwnershipScope::vm("connection-a", "session-a", "vm-b"),
                b.clone(),
                OperationCancellation::new(),
            )
            .unwrap();
        ingress.try_publish(event(&a, 1)).unwrap();
        broker.route(driver.receiver.try_recv().unwrap()).unwrap();
        let held = waiter_a.next_lease().await.unwrap();
        ingress.try_publish(event(&a, 2)).unwrap();
        ingress.try_publish(event(&b, 3)).unwrap();
        let fault = crate::state::VmOutputFailure {
                ownership: owner.clone(), generation: 1,
                error: crate::executor::backend::HostServiceError::new("ERR_AGENTOS_RESOURCE_LIMIT", "fixture count fault")
                    .with_details(serde_json::json!({"limitName":"limits.process.pendingEventCount","limit":1,"observed":2,"configPath":"limits.process.pendingEventCount"})),
            };
        let (disposal, first) = coordinator
            .begin_vm_output_failure(&fault, &operations)
            .unwrap();
        assert!(first);
        let disposal = disposal.unwrap();
        broker
            .dispose_vm(
                "connection-a",
                "session-a",
                "vm-a",
                OperationCancellationReason::Explicit,
            )
            .unwrap();
        let mut held = Some(held);
        if drop_before_complete {
            drop(held.take());
        }
        tokio::time::timeout(Duration::from_secs(1), disposal.wait_drained())
            .await
            .unwrap();
        disposal.complete().unwrap();
        assert!(operations.output_failure(&owner).is_none());
        let _successor = session.open_vm("vm-a").unwrap();
        operations.reopen_scope(&owner);
        drop(held.take());
        broker.route(driver.receiver.try_recv().unwrap()).unwrap(); // old A ingress
        broker.route(driver.receiver.try_recv().unwrap()).unwrap(); // live B ingress
        let snapshot = broker.snapshot().unwrap();
        assert_eq!(snapshot.disposed_events, 2);
        assert_eq!(snapshot.pending_events, 1);
        assert_eq!(snapshot.event_budget_in_use, 1);
        assert_eq!(
            stdout(waiter_b.next_lease().await.unwrap().commit().unwrap()),
            vec![3]
        );
        let new_waiter = broker
            .register_waiter(&owner, a.clone(), OperationCancellation::new())
            .unwrap();
        ingress.try_publish(event(&a, 4)).unwrap();
        broker.route(driver.receiver.try_recv().unwrap()).unwrap();
        assert_eq!(
            stdout(new_waiter.next_lease().await.unwrap().commit().unwrap()),
            vec![4]
        );
        assert_eq!(broker.snapshot().unwrap().event_budget_in_use, 0);
    }
}
