async fn register_owned_channel_vm(sidecar: &mut VmManager<LocalBridge>) -> String {
    sidecar
        .connections
        .entry("connection".into())
        .or_insert_with(|| crate::state::ConnectionState {
            auth_token: String::new(),
            sessions: std::collections::BTreeSet::from(["session".into()]),
        });
    sidecar
        .sessions
        .entry("session".into())
        .or_insert_with(|| crate::state::SessionState {
            connection_id: "connection".into(),
            placement: crate::protocol::SidecarPlacement::SidecarPlacementShared(
                crate::protocol::SidecarPlacementShared { pool: None },
            ),
            metadata: std::collections::BTreeMap::new(),
            vm_ids: std::collections::BTreeSet::new(),
        });
    let payload = crate::protocol::CreateVmRequest::legacy_test_config(
        crate::protocol::GuestRuntimeKind::JavaScript,
        std::collections::HashMap::new(),
        Default::default(),
        Some(crate::wire::PermissionsPolicy::allow_all()),
    );
    let request = crate::protocol::RequestFrame::new(
        1,
        OwnershipScope::session("connection", "session"),
        crate::protocol::RequestPayload::CreateVm(payload.clone()),
    );
    let dispatch = sidecar
        .create_vm(&request, payload)
        .await
        .expect("owned channel fixture VM");
    let crate::protocol::ResponsePayload::VmCreated(created) = dispatch.response.payload else {
        panic!("expected owned VM")
    };
    created.vm_id
}

// These fixtures stage synthetic public envelopes but own real VM host
// paths. Retire the remaining fixture envelopes explicitly, then use the
// normal session disposal/host-cleanup observer before leaving LocalSet.
async fn close_owned_channel_session(sidecar: &mut VmManager<LocalBridge>) {
    let paths = sidecar
        .vms
        .values()
        .flat_map(|vm| {
            let mut paths = vec![
                vm.runtime_scratch_root.clone(),
                vm.unix_socket_host_dir.clone(),
            ];
            paths.extend(vm.packages_staging_root.iter().cloned());
            paths
        })
        .collect::<Vec<_>>();
    sidecar.pending_process_events.clear();
    drop(sidecar.deferred_process_event_envelope.take());
    // Only this fixture writes the bounded raw channel; no guest executes.
    for _ in 0..4 {
        if sidecar
            .process_event_receiver
            .as_mut()
            .expect("fixture receiver")
            .try_recv()
            .is_err()
        {
            break;
        }
    }
    assert!(
        sidecar
            .process_event_receiver
            .as_ref()
            .expect("fixture receiver")
            .is_empty(),
        "fixture retirement must consume every remaining envelope"
    );
    sidecar.observe_pending_process_event_depth();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        sidecar.close_session("connection", "session"),
    )
    .await
    .expect("bounded normal channel fixture session disposal")
    .expect("normal channel fixture session disposal");
    sidecar
        .finish_host_path_cleanup_before_shutdown(
            tokio::time::Instant::now() + std::time::Duration::from_secs(5),
        )
        .await
        .expect("channel fixture host paths retired");
    assert!(
        sidecar.vms.is_empty(),
        "normal disposal removes VM membership"
    );
    assert!(
        !sidecar.sessions.contains_key("session"),
        "normal disposal removes session membership"
    );
    for path in paths {
        let error = std::fs::symlink_metadata(&path)
            .expect_err("normal channel fixture disposal removes physical host path");
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::NotFound,
            "path={}",
            path.display()
        );
    }
}

#[test]
fn deferred_current_envelope_retries_before_later_channel_envelopes() {
    let config = VmManagerConfig::default();
    let runtime = agentos_driver_tokio::TokioDriver::process(&config.runtime)
        .expect("process-event retry-order test runtime");
    runtime
        .handle()
        .tokio_handle()
        .block_on(tokio::task::LocalSet::new().run_until(async {
            let mut sidecar = VmManager::with_config_extensions_driver_and_executors(
                LocalBridge::default(),
                config,
                Vec::new(),
                runtime.handle(),
                crate::ExecutorRegistry::empty().with(crate::ExecutorKind::NodeV8),
            )
            .expect("process-event retry-order test sidecar");
            let vm_id = register_owned_channel_vm(&mut sidecar).await;
            let envelope = |byte| ProcessEventEnvelope {
                connection_id: String::from("connection"),
                session_id: String::from("session"),
                vm_id: vm_id.clone(),
                child_path: Vec::new(),
                process_id: String::from("process"),
                event: ActiveExecutionEvent::Stdout(vec![byte]),
            };
            sidecar.deferred_process_event_envelope = Some(envelope(1));
            sidecar
                .process_event_sender
                .try_send(envelope(2))
                .expect("queue later envelope");

            assert!(sidecar
                .drain_runtime_process_event_channel_nowait()
                .expect("retry staged and later envelopes"));
            let bytes = std::iter::from_fn(|| sidecar.pending_process_events.take_next(|_| true))
                .map(|envelope| match envelope.event {
                    ActiveExecutionEvent::Stdout(bytes) => bytes[0],
                    other => panic!("expected stdout, received {other:?}"),
                })
                .collect::<Vec<_>>();
            assert_eq!(bytes, vec![1, 2]);
            assert!(sidecar.deferred_process_event_envelope.is_none());
            assert!(sidecar
                .process_event_receiver
                .as_ref()
                .expect("process event receiver")
                .is_empty());
            close_owned_channel_session(&mut sidecar).await;
        }));
}

#[test]
fn temporary_rejection_stays_open_and_rearms_only_after_capacity_release() {
    let config = VmManagerConfig::default();
    let runtime = agentos_driver_tokio::TokioDriver::process(&config.runtime)
        .expect("process-event no-spin test runtime");
    runtime
        .handle()
        .tokio_handle()
        .block_on(tokio::task::LocalSet::new().run_until(async {
            let mut sidecar = VmManager::with_config_extensions_driver_and_executors(
                LocalBridge::default(),
                config,
                Vec::new(),
                runtime.handle(),
                crate::ExecutorRegistry::empty().with(crate::ExecutorKind::NodeV8),
            )
            .expect("process-event no-spin test sidecar");
            let vm_id = register_owned_channel_vm(&mut sidecar).await;
            sidecar.config.runtime.protocol.max_process_events = 2;
            let envelope = |byte| ProcessEventEnvelope {
                connection_id: String::from("connection"),
                session_id: String::from("session"),
                vm_id: vm_id.clone(),
                child_path: Vec::new(),
                process_id: String::from("process"),
                event: ActiveExecutionEvent::Stdout(vec![byte]),
            };
            sidecar.pending_process_events.push_back(envelope(8));
            // Occupy the reserved terminal slot without bypassing the new
            // output-only pool. The staged output still waits for a release.
            sidecar.pending_process_events.push_back(ProcessEventEnvelope {
                event: ActiveExecutionEvent::Exited(9),
                ..envelope(9)
            });
            sidecar.deferred_process_event_envelope = Some(envelope(1));
            sidecar
                .process_event_sender
                .try_send(envelope(2))
                .expect("queue later envelope");

            assert!(!sidecar
                .drain_runtime_process_event_channel_nowait()
                .expect("temporary saturation must not close the protocol or report progress"));
            assert!(sidecar.deferred_process_event_envelope.is_some());
            assert_eq!(sidecar.pending_process_events.len(), 2);
            assert_eq!(
                sidecar
                    .process_event_receiver
                    .as_ref()
                    .expect("process event receiver")
                    .len(),
                1
            );

            let mut notified = Box::pin(sidecar.process_event_notify.notified());
            let mut context = Context::from_waker(Waker::noop());
            assert!(matches!(
                notified.as_mut().poll(&mut context),
                Poll::Pending
            ));

            sidecar.pending_process_events.take_next(|_| true);
            sidecar.observe_pending_process_event_depth();
            sidecar.rearm_deferred_process_event_after_capacity_release();
            assert!(matches!(
                notified.as_mut().poll(&mut context),
                Poll::Ready(())
            ));
            drop(notified);

            sidecar
                .drain_runtime_process_event_channel_nowait()
                .expect("retry staged current envelope");
            let queued = sidecar
                .pending_process_events
                .iter()
                .map(|envelope| match &envelope.event {
                    ActiveExecutionEvent::Stdout(bytes) => bytes[0],
                    ActiveExecutionEvent::Exited(code) => u8::try_from(*code).expect("fixture exit code"),
                    other => panic!("expected stdout or exit, received {other:?}"),
                })
                .collect::<Vec<_>>();
            assert_eq!(queued, vec![9, 1]);
            assert_eq!(
                sidecar
                    .process_event_receiver
                    .as_ref()
                    .expect("process event receiver")
                    .len(),
                1,
                "later envelope remains behind the retried current envelope"
            );
            let mut blocked = Box::pin(sidecar.process_event_notify.notified());
            assert!(
                matches!(blocked.as_mut().poll(&mut context), Poll::Pending),
                "unread channel data must not self-rearm while public capacity is full"
            );
            sidecar.rearm_deferred_process_event_after_capacity_release();
            assert!(
                matches!(blocked.as_mut().poll(&mut context), Poll::Pending),
                "a reorder without free capacity must not consume the release waiter"
            );
            sidecar.pending_process_events.take_next(|_| true);
            sidecar.observe_pending_process_event_depth();
            sidecar.rearm_deferred_process_event_after_capacity_release();
            assert!(matches!(
                blocked.as_mut().poll(&mut context),
                Poll::Ready(())
            ));
            drop(blocked);
            sidecar
                .drain_runtime_process_event_channel_nowait()
                .expect("retry unread channel after release");
            assert!(sidecar
                .process_event_receiver
                .as_ref()
                .expect("receiver")
                .is_empty());
            close_owned_channel_session(&mut sidecar).await;
        }));
}

#[test]
fn partial_claimed_handoff_rearms_public_capacity_retry() {
    let mut config = VmManagerConfig::default();
    config.runtime.protocol.max_process_events = 2;
    let runtime =
        agentos_driver_tokio::TokioDriver::process(&agentos_driver_tokio::DriverConfig::default())
            .expect("driver");
    runtime
        .handle()
        .tokio_handle()
        .block_on(tokio::task::LocalSet::new().run_until(async {
            let mut sidecar = VmManager::with_config_extensions_driver_and_executors(
                LocalBridge::default(),
                config,
                Vec::new(),
                runtime.handle(),
                crate::ExecutorRegistry::empty().with(crate::ExecutorKind::NodeV8),
            )
            .expect("manager");
            let vm_id = register_owned_channel_vm(&mut sidecar).await;
            let target = crate::process_event_broker::ProcessEventTarget::new(
                "connection",
                "session",
                &vm_id,
                "process",
            )
            .expect("target");
            let ownership = OwnershipScope::vm("connection", "session", &vm_id);
            sidecar
                .process_event_broker
                .claim_target(&ownership, target)
                .expect("claim target");
            let envelope = |byte| ProcessEventEnvelope {
                connection_id: "connection".into(),
                session_id: "session".into(),
                vm_id: vm_id.clone(),
                child_path: Vec::new(),
                process_id: "process".into(),
                event: ActiveExecutionEvent::Stdout(vec![byte]),
            };
            sidecar
                .process_event_ingress
                .try_publish(envelope(0))
                .expect("reserve one broker slot");
            sidecar
                .queue_pending_process_event(envelope(1))
                .expect("first public slot");
            sidecar
                .queue_pending_process_event(envelope(2))
                .expect("second public slot");
            sidecar
                .process_event_sender
                .try_send(envelope(3))
                .expect("one unread channel event");
            assert!(!sidecar
                .drain_runtime_process_event_channel_nowait()
                .expect("park full channel"));
            let mut context = Context::from_waker(Waker::noop());
            let mut old_edge = Box::pin(sidecar.process_event_notify.notified());
            let _initial_poll = old_edge.as_mut().poll(&mut context);
            drop(old_edge);
            let released_notify = sidecar.process_event_notify.clone();
            let mut released = Box::pin(released_notify.notified());
            assert_eq!(
                sidecar
                    .route_claimed_pending_process_events()
                    .expect("partial broker handoff"),
                1
            );
            assert_eq!(sidecar.pending_process_events.len(), 1);
            assert!(
                matches!(released.as_mut().poll(&mut context), Poll::Ready(())),
                "a successful handoff before broker pressure must release the global retry waiter"
            );
            drop(released);
            let mut once = Box::pin(sidecar.process_event_notify.notified());
            assert!(
                matches!(once.as_mut().poll(&mut context), Poll::Pending),
                "only one coalesced retry edge"
            );
            drop(once);
            close_owned_channel_session(&mut sidecar).await;
        }));
}

#[test]
fn channel_aggregate_fault_preserves_peer_prefix_unread_continuation_and_owner_errors() {
    let config = VmManagerConfig::default();
    let runtime = agentos_driver_tokio::TokioDriver::process(&config.runtime).unwrap();
    runtime
        .handle()
        .tokio_handle()
        .block_on(tokio::task::LocalSet::new().run_until(async {
            let mut sidecar = VmManager::with_config_extensions_driver_and_executors(
                LocalBridge::default(),
                config,
                Vec::new(),
                runtime.handle(),
                crate::ExecutorRegistry::empty().with(crate::ExecutorKind::NodeV8),
            )
            .unwrap();
            sidecar.config.runtime.protocol.max_process_events = 4;
            let failed = register_owned_channel_vm(&mut sidecar).await;
            let peer = register_owned_channel_vm(&mut sidecar).await;
            sidecar
                .vms
                .get_mut(&failed)
                .unwrap()
                .limits
                .process
                .pending_event_count = 1;
            let envelope = |vm: &str, byte| ProcessEventEnvelope {
                connection_id: "connection".into(),
                session_id: "session".into(),
                vm_id: vm.into(),
                child_path: Vec::new(),
                process_id: "fixture-process".into(),
                event: ActiveExecutionEvent::Stdout(vec![byte]),
            };
            sidecar
                .queue_pending_process_event(envelope(&failed, 0))
                .unwrap();
            sidecar
                .process_event_sender
                .try_send(envelope(&peer, 1))
                .unwrap();
            sidecar
                .process_event_sender
                .try_send(envelope(&failed, 2))
                .unwrap();
            sidecar
                .process_event_sender
                .try_send(envelope(&peer, 3))
                .unwrap();
            let notify = sidecar.process_event_notify.clone();
            let mut context = Context::from_waker(Waker::noop());
            let mut old = Box::pin(notify.notified());
            let _ = old.as_mut().poll(&mut context);
            drop(old);
            let mut continuation = Box::pin(notify.notified());
            assert!(matches!(
                continuation.as_mut().poll(&mut context),
                Poll::Pending
            ));
            let operations = crate::request_operations::OperationTable::from_protocol_config(
                &sidecar.config.runtime.protocol,
            );
            let turn = sidecar
                .pump_process_events_nowait_with_operations(
                    &OwnershipScope::session("connection", "session"),
                    4,
                    &operations,
                )
                .unwrap();
            assert!(turn.emitted_any, "peer prefix must retain public readiness");
            let fault = turn.output_failure.expect("exact channel aggregate fault");
            assert_eq!(fault.error.code, "ERR_AGENTOS_RESOURCE_LIMIT");
            assert_eq!(
                fault.error.details.as_ref().unwrap()["configPath"],
                "limits.process.pendingEventCount"
            );
            assert_eq!(
                fault.ownership,
                crate::wire::OwnershipScope::vm("connection", "session", &failed)
            );
            assert_eq!(sidecar.process_event_receiver.as_ref().unwrap().len(), 1);
            let _drain = operations.close_scope_with_cause(
                fault.ownership.clone(),
                crate::request_operations::OperationCancellationReason::Explicit,
                None,
                Some(fault.clone()),
            );
            let (events, _, _) = sidecar.retire_failed_vm_output(&fault).unwrap();
            assert_eq!(events, 2, "failed pending plus deferred envelopes");
            assert!(
                matches!(continuation.as_mut().poll(&mut context), Poll::Ready(())),
                "unread peer continuation must survive fault return"
            );
            drop(continuation);
            sidecar
                .drain_runtime_process_event_channel_nowait()
                .unwrap();
            let bytes = sidecar
                .pending_process_events
                .iter()
                .map(|event| match &event.event {
                    ActiveExecutionEvent::Stdout(bytes) => bytes[0],
                    _ => panic!("stdout"),
                })
                .collect::<Vec<_>>();
            assert_eq!(bytes, vec![1, 3]);
            let mut idle = Box::pin(notify.notified());
            assert!(
                matches!(idle.as_mut().poll(&mut context), Poll::Pending),
                "drained channel must not idle-rearm"
            );
            drop(idle);
            sidecar.pending_process_events.clear();
            let mut invalid = envelope(&peer, 4);
            invalid.session_id = "wrong-session".into();
            sidecar.process_event_sender.try_send(invalid).unwrap();
            sidecar
                .process_event_sender
                .try_send(envelope(&peer, 5))
                .unwrap();
            let error = sidecar
                .drain_runtime_process_event_channel_nowait()
                .unwrap_err();
            assert!(matches!(error, VmError::InvalidState(_)));
            assert!(error.to_string().contains("ownership mismatch"));
            assert_eq!(
                sidecar.process_event_receiver.as_ref().unwrap().len(),
                1,
                "later peer envelope remains owned"
            );
            close_owned_channel_session(&mut sidecar).await;
        }));
}
