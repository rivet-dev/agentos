#[tokio::test(flavor = "current_thread")]
async fn host_path_cleanup_keeps_router_live_and_blocks_success_until_removed() {
    for (abort_request, drop_detached, connection_shutdown) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        tokio::task::LocalSet::new()
            .run_until(async {
                let connection_id = "dispose-route-connection";
                let session_id = "dispose-route-session";
                let config = VmManagerConfig::default();
                let runtime = agentos_driver_tokio::TokioDriver::process(&config.runtime)
                    .expect("test runtime");
                let mut sidecar = VmManager::with_config_extensions_and_runtime(
                    LocalBridge::default(),
                    config.clone(),
                    Vec::new(),
                    runtime.handle(),
                )
                .expect("test sidecar");
                agentos_vm::test_support::register_session(&mut sidecar, connection_id, session_id);
                let create_payload = agentos_vm::protocol::CreateVmRequest::legacy_test_config(
                    agentos_vm::protocol::GuestRuntimeKind::JavaScript,
                    Default::default(),
                    Default::default(),
                    None,
                );
                let create_request = agentos_vm::protocol::RequestFrame::new(
                    1,
                    agentos_vm::protocol::OwnershipScope::session(connection_id, session_id),
                    agentos_vm::protocol::RequestPayload::CreateVm(create_payload.clone()),
                );
                let prepared_create = sidecar
                    .prepare_create_vm(&create_request, create_payload)
                    .expect("prepare test VM");
                let completed_create = prepared_create.execute().await.expect("build test VM");
                let created = sidecar
                    .complete_create_vm(completed_create)
                    .expect("publish test VM");
                let agentos_vm::protocol::ResponsePayload::VmCreated(created) =
                    created.response.payload
                else {
                    panic!("expected VM-created response")
                };
                let vm_id = created.vm_id;

                let ownership_coordinator =
                    OwnershipCoordinator::from_runtime_config(&config.runtime);
                let coordinator_session = ownership_coordinator
                    .register_connection(connection_id)
                    .and_then(|connection| connection.open_session(session_id))
                    .expect("test coordinator session");
                coordinator_session
                    .open_vm(vm_id.clone())
                    .expect("test coordinator VM");

                let operations = OperationTable::from_protocol_config(&config.runtime.protocol);
                let progress_requests =
                    ProgressOperationView::from_protocol_config(&config.runtime.protocol);
                let (writer, output) = test_frame_writer_with_inflight(8, 2);
                let ingress_budget = test_protocol_budget(4, 4096, "dispose route ingress");
                let (service_tx, _service_rx) = channel(4);
                let services: Arc<dyn ExtensionServices> = Arc::new(RoutedExtensionServices::new(
                    service_tx,
                    sidecar.process_event_notify(),
                ));
                let (extension_completion_tx, _extension_completion_rx) = channel(2);
                let (request_completion_tx, mut request_completion_rx) = channel(4);
                let mut extension_tasks = JoinSet::new();
                let mut request_tasks = JoinSet::new();
                let mut output_tasks = JoinSet::new();
                let mut active_sessions = BTreeSet::new();
                let mut active_connections = BTreeSet::new();

                let (scratch, staging, unix) =
                    agentos_vm::test_support::host_path_cleanup_paths(&sidecar, &vm_id);
                assert!(scratch.is_dir() && unix.is_dir());
                assert!(staging.is_none(), "fixture has no package staging owner");
                std::fs::write(scratch.join("owned-file"), b"cleanup fixture").unwrap();
                if drop_detached {
                    let scope = vm_ownership(connection_id, session_id, &vm_id);
                    let request = request_frame(
                        71,
                        scope.clone(),
                        RequestPayload::DisposeVmRequest(wire::DisposeVmRequest {
                            reason: wire::DisposeReason::Requested,
                        }),
                    );
                    let request = wire::request_frame_to_compat(request).unwrap();
                    let payload = agentos_vm::protocol::DisposeVmRequest {
                        reason: agentos_vm::protocol::DisposeReason::Requested,
                    };
                    let abandoned = sidecar
                        .prepare_dispose_vm(&request, payload.clone())
                        .unwrap();
                    drop(abandoned);
                    sidecar
                        .finish_host_path_cleanup_before_shutdown(
                            tokio::time::Instant::now() + Duration::from_secs(1),
                        )
                        .await
                        .expect("abandoned pre-detach plan owns no cleanup");
                    assert!(agentos_vm::test_support::contains_vm(&sidecar, &vm_id));

                    let plan = sidecar.prepare_dispose_vm(&request, payload).unwrap();
                    let _closing = ownership_coordinator
                        .begin_vm_disposal(&scope, OperationCancellationReason::Explicit)
                        .unwrap();
                    let detached = sidecar.detach_vm_for_disposal(plan).unwrap();
                    assert!(!agentos_vm::test_support::contains_vm(&sidecar, &vm_id));
                    drop(detached);
                    let error = sidecar
                        .finish_host_path_cleanup_before_shutdown(tokio::time::Instant::now())
                        .await
                        .expect_err("dropping a detached payload cannot confirm cleanup");
                    assert!(error
                        .to_string()
                        .contains("ERR_AGENTOS_VM_PATH_CLEANUP_UNCONFIRMED"));
                    assert!(
                        scratch.is_dir() && unix.is_dir(),
                        "unconfirmed paths were not removed"
                    );
                    // The test owns these deliberately abandoned directories.
                    std::fs::remove_dir_all(&scratch).unwrap();
                    std::fs::remove_dir_all(&unix).unwrap();
                    output.close();
                    return;
                }
                if connection_shutdown {
                    let request = request_frame(
                        71,
                        vm_ownership(connection_id, session_id, &vm_id),
                        RequestPayload::DisposeVmRequest(wire::DisposeVmRequest {
                            reason: wire::DisposeReason::Requested,
                        }),
                    );
                    let request = wire::request_frame_to_compat(request).unwrap();
                    let payload = agentos_vm::protocol::DisposeVmRequest {
                        reason: agentos_vm::protocol::DisposeReason::Requested,
                    };
                    let pending_plan = sidecar.prepare_dispose_vm(&request, payload).unwrap();
                    active_connections.insert(connection_id.to_owned());
                    active_sessions.insert(SessionScope {
                        connection_id: connection_id.to_owned(),
                        session_id: session_id.to_owned(),
                    });
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
                    let error = cleanup_connections(
                        &mut sidecar,
                        &ownership_coordinator,
                        &active_connections,
                        &mut active_sessions,
                        deadline,
                    )
                    .await
                    .expect_err("an existing cleanup reservation prevents detachment");
                    assert!(error
                        .to_string()
                        .contains("already has a prepared disposal"));
                    assert!(sidecar.has_connection(connection_id));
                    assert!(agentos_vm::test_support::contains_vm(&sidecar, &vm_id));
                    assert_eq!(active_sessions.len(), 1);
                    assert_eq!(
                        ownership_coordinator
                            .connection(connection_id)
                            .unwrap()
                            .snapshot()
                            .child_count,
                        1
                    );
                    assert!(scratch.is_dir() && unix.is_dir());

                    drop(pending_plan);
                    sidecar
                        .finish_host_path_cleanup_before_shutdown(deadline)
                        .await
                        .unwrap();
                    let (started, worker_started) = tokio::sync::oneshot::channel();
                    let (release, gate) = std::sync::mpsc::channel();
                    agentos_vm::test_support::set_host_path_cleanup_gate(
                        vm_id.clone(),
                        move || {
                            started.send(()).unwrap();
                            gate.recv_timeout(Duration::from_secs(2)).unwrap();
                        },
                    );
                    let cleanup = cleanup_connections(
                        &mut sidecar,
                        &ownership_coordinator,
                        &active_connections,
                        &mut active_sessions,
                        deadline,
                    );
                    let release_worker = async {
                        tokio::time::timeout(Duration::from_secs(1), worker_started)
                            .await
                            .unwrap()
                            .unwrap();
                        assert!(scratch.is_dir() && unix.is_dir());
                        release.send(()).unwrap();
                    };
                    let (result, ()) = tokio::join!(cleanup, release_worker);
                    result.unwrap();
                    assert!(!scratch.exists() && !unix.exists());
                    assert!(!sidecar.has_connection(connection_id));
                    assert!(ownership_coordinator.connection(connection_id).is_err());
                    assert!(active_sessions.is_empty());
                    sidecar
                        .finish_host_path_cleanup_before_shutdown(deadline)
                        .await
                        .unwrap();
                    output.close();
                    return;
                }
                let (started, worker_started) = tokio::sync::oneshot::channel();
                let (release, gate) = std::sync::mpsc::channel();
                agentos_vm::test_support::set_host_path_cleanup_gate(vm_id.clone(), move || {
                    started.send(()).expect("publish cleanup worker entry");
                    gate.recv_timeout(Duration::from_secs(5))
                        .expect("release cleanup worker");
                });

                let mut request_queue =
                    RequestQueue::new(&agentos_driver_tokio::DriverProtocolConfig::default());
                macro_rules! route {
                    ($request:expr) => {
                        route_protocol_frame(
                            test_accounted_frame(
                                ProtocolFrame::RequestFrame($request),
                                &ingress_budget,
                            ),
                            &mut sidecar,
                            &services,
                            &operations,
                            &progress_requests,
                            &ownership_coordinator,
                            &extension_completion_tx,
                            &request_completion_tx,
                            &mut extension_tasks,
                            &mut request_tasks,
                            &mut output_tasks,
                            &writer,
                            1,
                            &mut active_sessions,
                            &mut active_connections,
                            &mut request_queue,
                        )
                        .unwrap()
                    };
                }
                macro_rules! complete_next {
                    () => {{
                        let completion = tokio::time::timeout(
                            Duration::from_secs(2),
                            request_completion_rx.recv(),
                        )
                        .await
                        .expect("completion deadline")
                        .expect("completion");
                        finish_request(
                            completion,
                            &mut sidecar,
                            &ownership_coordinator,
                            &request_completion_tx,
                            &mut request_tasks,
                            &writer,
                            &mut output_tasks,
                            &mut active_sessions,
                            &mut active_connections,
                        )
                        .unwrap()
                    }};
                }
                macro_rules! terminal {
                    () => {{
                        let encoded =
                            tokio::time::timeout(Duration::from_secs(2), output.recv_control())
                                .await
                                .expect("terminal deadline")
                                .expect("terminal frame");
                        let ProtocolFrame::ResponseFrame(response) = decode_test_output(encoded)
                        else {
                            panic!("expected terminal response");
                        };
                        response
                    }};
                }

                route!(request_frame(
                    71,
                    vm_ownership(connection_id, session_id, &vm_id),
                    RequestPayload::DisposeVmRequest(wire::DisposeVmRequest {
                        reason: wire::DisposeReason::Requested,
                    }),
                ));
                complete_next!(); // The real drained request detaches and starts teardown.
                tokio::time::timeout(Duration::from_secs(2), worker_started)
                    .await
                    .expect("cleanup worker start deadline")
                    .expect("cleanup worker started");
                assert!(request_completion_rx.try_recv().is_err());
                assert!(scratch.is_dir() && unix.is_dir());
                assert!(
                    runtime.handle().blocking().reserved_bytes()
                        >= scratch.as_os_str().len() + unix.as_os_str().len(),
                    "the active cleanup owns blocking byte reservations",
                );

                route!(request_frame(
                    72,
                    connection_ownership("unrelated-host"),
                    RequestPayload::AuthenticateRequest(AuthenticateRequest {
                        client_name: "unrelated-host".into(),
                        auth_token: "fixture-token".into(),
                        protocol_version: wire::PROTOCOL_VERSION,
                        bridge_version: agentos_vm_host_interface::bridge_contract().version,
                    }),
                ));
                complete_next!();
                let response = terminal!();
                assert_eq!(response.request_id, 72);
                assert!(matches!(
                    response.payload,
                    ResponsePayload::AuthenticatedResponse(_)
                ));
                assert!(
                    request_completion_rx.try_recv().is_err(),
                    "disposal must still wait for paths"
                );

                let expired = sidecar
                    .finish_host_path_cleanup_before_shutdown(tokio::time::Instant::now())
                    .await
                    .expect_err("expired shutdown cannot report success while cleanup owns paths");
                assert!(expired
                    .to_string()
                    .contains("ERR_AGENTOS_VM_PATH_CLEANUP_UNCONFIRMED"));
                assert!(scratch.is_dir() && unix.is_dir());
                if abort_request {
                    request_tasks.abort_all();
                    while let Some(result) = request_tasks.join_next().await {
                        if let Err(error) = result {
                            assert!(error.is_cancelled());
                        }
                    }
                    release
                        .send(())
                        .expect("allow cleanup after outer request cancellation");
                    sidecar
                        .finish_host_path_cleanup_before_shutdown(
                            tokio::time::Instant::now() + Duration::from_secs(2),
                        )
                        .await
                        .expect("shutdown still observes the process-owned cleanup");
                    assert!(!scratch.exists() && !unix.exists());
                    assert!(request_completion_rx.try_recv().is_err());
                    assert_eq!(operations.snapshot().in_flight_requests, 0);
                    output.close();
                    return;
                }
                release.send(()).expect("allow cleanup to complete");
                complete_next!();
                assert!(
                    !scratch.exists() && !unix.exists(),
                    "removal precedes terminal response"
                );
                sidecar
                    .finish_host_path_cleanup_before_shutdown(tokio::time::Instant::now())
                    .await
                    .expect("completed cleanup permits graceful exit");
                let response = terminal!();
                assert_eq!(response.request_id, 71);
                assert!(matches!(
                    response.payload,
                    ResponsePayload::VmDisposedResponse(_)
                ));
                while let Some(result) = output_tasks.join_next().await {
                    result.unwrap().unwrap();
                }
                assert_eq!(operations.snapshot().in_flight_requests, 0);
                output.close();
            })
            .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn ordinary_dispose_drain_deadline_preserves_counted_closing() {
    tokio::task::LocalSet::new().run_until(async {
        let connection_id = "bounded-drain-connection";
        let session_id = "bounded-drain-session";
        let config = VmManagerConfig::default();
        let runtime = agentos_driver_tokio::TokioDriver::process(&config.runtime).unwrap();
        let mut sidecar = VmManager::with_config_extensions_and_runtime(
            LocalBridge::default(), config.clone(), Vec::new(), runtime.handle(),
        ).unwrap();
        agentos_vm::test_support::register_session(&mut sidecar, connection_id, session_id);
        let mut payload = agentos_vm::protocol::CreateVmRequest::legacy_test_config(
            agentos_vm::protocol::GuestRuntimeKind::WebAssembly,
            Default::default(), Default::default(), None,
        );
        let mut create_config: serde_json::Value = serde_json::from_str(&payload.config).unwrap();
        create_config["limits"] = serde_json::json!({"reactor":{"shutdownDeadlineMs":20}});
        payload.config = serde_json::to_string(&create_config).unwrap();
        let create = agentos_vm::protocol::RequestFrame::new(
            1, agentos_vm::protocol::OwnershipScope::session(connection_id, session_id),
            agentos_vm::protocol::RequestPayload::CreateVm(payload.clone()),
        );
        let prepared = sidecar.prepare_create_vm(&create, payload).unwrap();
        let completed = prepared.execute().await.unwrap();
        let created = sidecar.complete_create_vm(completed).unwrap();
        let agentos_vm::protocol::ResponsePayload::VmCreated(created) = created.response.payload else {
            panic!("expected real VM creation");
        };
        let vm_id = created.vm_id;
        let coordinator = OwnershipCoordinator::from_runtime_config(&config.runtime);
        let session = coordinator.register_connection(connection_id).unwrap()
            .open_session(session_id).unwrap();
        let vm_owner = session.open_vm(vm_id.clone()).unwrap();
        let ownership = vm_ownership(connection_id, session_id, &vm_id);
        let operations = OperationTable::from_protocol_config(&config.runtime.protocol);
        let held_request = request_frame(70, ownership.clone(), RequestPayload::GetProcessSnapshotRequest);
        let held = operations.admit(
            RequestOperationKey::new(connection_id, 70),
            request_operation_metadata(&held_request, &BTreeMap::new()), 1,
        ).unwrap();
        let cancellation = held.cancellation();
        // A real counted owner deliberately retains its permit after cooperative
        // cancellation, until this fixture releases it. No guest/thread is stuck.
        let held_permit = coordinator.admit(held.metadata(), cancellation.clone()).await.unwrap();
        let request = request_frame(71, ownership.clone(),
            RequestPayload::DisposeVmRequest(wire::DisposeVmRequest { reason: wire::DisposeReason::Requested }));
        let compat = wire::request_frame_to_compat(request.clone()).unwrap();
        let plan = sidecar.prepare_dispose_vm(&compat,
            agentos_vm::protocol::DisposeVmRequest { reason: agentos_vm::protocol::DisposeReason::Requested }).unwrap();
        let operation = operations.admit(
            RequestOperationKey::new(connection_id, 71),
            request_operation_metadata(&request, &BTreeMap::new()), 1,
        ).unwrap();
        let disposal = coordinator.begin_vm_disposal_with_operations(
            &ownership, OperationCancellationReason::Explicit, &operations, operation.key(),
        ).unwrap();
        let (scratch, staging, unix) = agentos_vm::test_support::host_path_cleanup_paths(&sidecar, &vm_id);
        assert!(scratch.is_dir() && unix.is_dir() && staging.is_none());
        let (writer, output) = test_frame_writer_with_inflight(8, 2);
        let (completion_tx, mut completion_rx) = channel(4);
        let mut request_tasks = JoinSet::new();
        let mut output_tasks = JoinSet::new();
        let mut active_sessions = BTreeSet::new();
        let mut active_connections = BTreeSet::new();
        schedule_prepared_dispose_vm(plan, disposal, request, operation,
            writer.try_reserve_terminal(1).unwrap(), &completion_tx, &mut request_tasks);

        // A bounded observation of the actual scheduler, not a new disposal
        // implementation. Keep all owner assertions after explicit release.
        let early = tokio::time::timeout(Duration::from_millis(250), completion_rx.recv()).await;
        let returned_while_held = matches!(&early, Ok(Some(_)));
        let typed_deadline = matches!(&early,
            Ok(Some(DetachedRequestCompletion::Request(DetachedRealRequestCompletion {
                result: DetachedRequestResult::Generic(Err(VmError::VmTeardownDeadline { .. })), ..
            }))));
        let phase = vm_owner.snapshot().phase;
        let counted = session.snapshot().child_count;
        let active_operations = vm_owner.snapshot().active_operations;
        let cancelled = cancellation.reason();
        let membership_retained = agentos_vm::test_support::contains_vm(&sidecar, &vm_id);
        let paths_retained = scratch.is_dir() && unix.is_dir();
        let admission_closed = coordinator.with_vm_admission(&ownership, || ()).is_err();
        // Release the exact held table row and coordinator permit before any
        // assertion, on both outcomes. The same scheduled task remains owned.
        held.release();
        drop(held_permit);
        let completion = match early {
            Ok(Some(completion)) => completion,
            Ok(None) => panic!("scheduler completion channel closed"),
            Err(_) => tokio::time::timeout(Duration::from_secs(2), completion_rx.recv()).await
                .expect("drain completes after owner release").expect("drain completion"),
        };
        let starts_teardown = matches!(&completion,
            DetachedRequestCompletion::Request(DetachedRealRequestCompletion {
                result: DetachedRequestResult::DisposeDrained { .. }, ..
            }));
        finish_request(completion, &mut sidecar, &coordinator, &completion_tx,
            &mut request_tasks, &writer, &mut output_tasks, &mut active_sessions,
            &mut active_connections).unwrap();
        if starts_teardown {
            let completion = tokio::time::timeout(Duration::from_secs(2), completion_rx.recv()).await
                .expect("existing detached teardown completes").expect("teardown completion");
            assert!(matches!(&completion,
                DetachedRequestCompletion::Request(DetachedRealRequestCompletion {
                    result: DetachedRequestResult::DisposeExecuted { .. }, ..
                })));
            finish_request(completion, &mut sidecar, &coordinator, &completion_tx,
                &mut request_tasks, &writer, &mut output_tasks, &mut active_sessions,
                &mut active_connections).unwrap();
        }
        let response = tokio::time::timeout(Duration::from_secs(2), output.recv_control()).await
            .expect("original request terminal output").expect("terminal frame");
        let ProtocolFrame::ResponseFrame(response) = decode_test_output(response) else {
            panic!("expected actual terminal response");
        };
        assert_eq!(response.request_id, 71);
        while let Some(result) = request_tasks.join_next().await { result.unwrap(); }
        while let Some(result) = output_tasks.join_next().await { result.unwrap().unwrap(); }
        // Deadline rejection may drop only an unactivated plan; wait for that
        // owned abandoned reservation before using normal parent cleanup. This
        // is not disposal.complete() or physical success for the timed-out VM.
        sidecar.finish_host_path_cleanup_before_shutdown(
            tokio::time::Instant::now() + Duration::from_secs(2)).await.unwrap();
        if sidecar.has_connection(connection_id) {
            active_connections.insert(connection_id.to_owned());
            active_sessions.insert(SessionScope {
                connection_id: connection_id.to_owned(), session_id: session_id.to_owned(),
            });
            cleanup_connections(&mut sidecar, &coordinator, &active_connections,
                &mut active_sessions, tokio::time::Instant::now() + Duration::from_secs(2)).await.unwrap();
        }
        sidecar.finish_host_path_cleanup_before_shutdown(
            tokio::time::Instant::now() + Duration::from_secs(2)).await.unwrap();
        assert!(!scratch.exists() && !unix.exists());
        assert!(!sidecar.has_connection(connection_id));
        assert!(coordinator.connection(connection_id).is_err());
        assert_eq!(operations.snapshot().in_flight_requests, 0);
        assert!(completion_rx.try_recv().is_err());
        output.close();
        assert_eq!(cancelled, Some(OperationCancellationReason::Explicit));
        assert_eq!(phase, agentos_vm::ownership_coordinator::CoordinatorPhase::Closing);
        assert_eq!(counted, 1);
        assert_eq!(active_operations, 1);
        assert!(membership_retained && paths_retained && admission_closed,
            "an undrained VM must remain live and counted with admission closed");
        assert!(returned_while_held && typed_deadline,
            "ordinary DisposeVm must reject at its local drain deadline while preserving counted Closing; returned={returned_while_held} typed={typed_deadline}");
    }).await;
}
