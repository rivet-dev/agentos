// Included inside transport::tests: production ProtocolEngine, test-only plumbing.
struct OutputLimitEngineInputs {
    engine: ProtocolEngine,
    stdin: Sender<Result<Option<AccountedProtocolFrame>, String>>,
    shutdown: Sender<wire::ControlFrame>,
    output: Arc<ProtocolOutputQueue>,
    // Keep all actual engine inputs open until explicit bounded shutdown.
    _control: Sender<AccountedProtocolFrame>,
    _warnings: Sender<agentos_resource_accounting::queue_tracker::LimitWarning>,
    _write_errors: Sender<String>,
    _services: Sender<ExtensionServiceCommand>,
    _progress: Sender<ExtensionServiceCommand>,
    live_events: tokio::task::JoinHandle<Result<(), String>>,
}

fn output_limit_engine_inputs(
    mut sidecar: VmManager<LocalBridge>,
    config: &VmManagerConfig,
    coordinator: OwnershipCoordinator,
) -> OutputLimitEngineInputs {
    let protocol = config.runtime.protocol.clone();
    let (frame_writer, output) = test_frame_writer_with_inflight(8, 4);
    let callback_transport = Arc::new(FrameSidecarRequestTransport::new(
        frame_writer.clone(),
        FrameSidecarRequestLimits::from_config(config),
    ));
    sidecar.set_sidecar_request_transport(callback_transport.clone());
    let (stdin, stdin_rx) = channel(8);
    let (control, stdin_control_rx) = channel(8);
    let (shutdown, shutdown_rx) = channel(MAX_SHUTDOWN_QUEUE);
    let (event_ready_tx, event_ready_rx) = channel(MAX_EVENT_READY_QUEUE);
    let (warnings, limit_warning_rx) = channel(MAX_LIMIT_WARNING_QUEUE);
    let (write_errors, write_error_rx) = channel(MAX_TRANSPORT_ERROR_QUEUE);
    let (services, extension_service_rx) = channel(8);
    let (progress, progress_service_rx) = channel(8);
    let (extension_service_completion_tx, extension_service_completion_rx) = channel(8);
    let (extension_completion_tx, extension_completion_rx) = channel(8);
    let (request_completion_tx, request_completion_rx) = channel(8);
    let process_event_notify = sidecar.process_event_notify();
    let routed_process_event_notify = Arc::new(Notify::new());
    let extension_services: Arc<dyn ExtensionServices> = Arc::new(
        RoutedExtensionServices::new_with_process_event_broker_and_progress(
            services.clone(),
            progress.clone(),
            routed_process_event_notify.clone(),
            sidecar.process_event_broker(),
        ),
    );
    sidecar.set_extension_services(extension_services.clone());
    let (live_transport, mut live_rx) = FrameEventTransport::new(
        WireFrameCodec::new(4096),
        &protocol,
        agentos_driver_tokio::metrics::DriverMetrics::new(),
    );
    sidecar.set_event_transport(Arc::new(live_transport));
    let live_writer = frame_writer.clone();
    let errors = write_errors.clone();
    let live_events = tokio::task::spawn_local(async move {
        while let Some(pending) = live_rx.recv().await {
            let published = live_writer
                .publish(
                    ProtocolOutputClass::Ordinary,
                    ProtocolFrame::EventFrame(pending.event),
                )
                .await;
            drop(pending._reservation);
            if let Err(error) = published {
                errors
                    .try_send(error.to_string())
                    .map_err(|send| send.to_string())?;
                return Err(error.to_string());
            }
        }
        Ok(())
    });
    let request_operations = OperationTable::from_protocol_config(&protocol);
    let progress_requests = request_operations.progress_requests();
    OutputLimitEngineInputs {
        engine: ProtocolEngine {
            protocol,
            sidecar,
            extension_services,
            ownership_coordinator: coordinator,
            request_operations,
            progress_requests,
            callback_transport,
            frame_writer,
            stdin_eof_state: Arc::new(StdinEofState::default()),
            stdin_rx,
            stdin_control_rx,
            shutdown_rx,
            extension_service_rx,
            progress_service_rx,
            extension_service_completion_tx,
            extension_service_completion_rx,
            extension_completion_tx,
            extension_completion_rx,
            request_completion_tx,
            request_completion_rx,
            limit_warning_rx,
            event_ready_tx,
            event_ready_rx,
            process_event_notify,
            routed_process_event_notify,
            write_error_rx,
        },
        stdin,
        shutdown,
        output,
        _control: control,
        _warnings: warnings,
        _write_errors: write_errors,
        _services: services,
        _progress: progress,
        live_events,
    }
}

async fn output_limit_vm(
    sidecar: &mut VmManager<LocalBridge>,
    coordinator: &OwnershipCoordinator,
    connection: &str,
    session: &str,
    request_id: RequestId,
) -> String {
    let mut create = agentos_vm::protocol::CreateVmRequest::legacy_test_config(
        agentos_vm::protocol::GuestRuntimeKind::JavaScript,
        Default::default(),
        Default::default(),
        Some(agentos_vm::protocol::PermissionsPolicy::allow_all()),
    );
    // Keep VM bridge/fetch limits coherent with this fixture's 4096-byte codec.
    let mut vm_config: agentos_vm_config::CreateVmConfig =
        serde_json::from_str(&create.config).expect("finite VM config");
    let limits = vm_config.limits.get_or_insert_with(Default::default);
    limits
        .http
        .get_or_insert_with(Default::default)
        .max_fetch_response_bytes = Some(4096);
    let reactor = limits.reactor.get_or_insert_with(Default::default);
    reactor.max_bridge_request_bytes = Some(4096);
    reactor.max_bridge_response_bytes = Some(4096);
    create.config = serde_json::to_string(&vm_config).expect("encode finite VM config");
    let request = agentos_vm::protocol::RequestFrame::new(
        request_id,
        agentos_vm::protocol::OwnershipScope::session(connection, session),
        agentos_vm::protocol::RequestPayload::CreateVm(create.clone()),
    );
    let prepared = sidecar
        .prepare_create_vm(&request, create)
        .expect("prepare finite VM");
    let completed = prepared.execute().await.expect("create finite VM");
    let created = sidecar
        .complete_create_vm(completed)
        .expect("publish finite VM");
    let agentos_vm::protocol::ResponsePayload::VmCreated(created) = created.response.payload else {
        panic!("expected VM creation")
    };
    coordinator
        .connection(connection)
        .expect("coordinator connection")
        .session(session)
        .expect("coordinator session")
        .open_vm(created.vm_id.clone())
        .expect("coordinator VM");
    created.vm_id
}

fn per_vm_output_limit_keeps_real_engine_peers(pressure: Option<bool>) {
    let mut config = VmManagerConfig::default();
    let runtime = agentos_driver_tokio::TokioDriver::process(&config.runtime)
        .expect("unchanged singleton driver topology");
    // Match the production sidecar: the shared driver owns timers and I/O;
    // the local set owns thread-affine protocol work.
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .finish();
    let _protocol_logs = tracing::subscriber::set_default(subscriber);
    runtime.block_on(tokio::task::LocalSet::new().run_until(async {
        config.runtime.protocol.max_process_events = 4;
        config.runtime.protocol.shutdown_grace_ms = 500;
        config.max_frame_bytes = 4096;
        // Match test_frame_writer_with_inflight: each terminal reserve is one encoded frame.
        config.runtime.protocol.terminal_fallback_bytes = 4096 + 4;
        let mut sidecar = VmManager::with_config_extensions_and_runtime(
            LocalBridge::default(), config.clone(), Vec::new(), runtime.handle(),
        ).expect("finite manager");
        let coordinator = OwnershipCoordinator::from_runtime_config(&config.runtime);
        for (connection, session) in [("a-connection", "a-session"), ("z-connection", "z-session")] {
            agentos_vm::test_support::register_session(&mut sidecar, connection, session);
            coordinator.register_connection(connection).expect("register connection")
                .open_session(session).expect("register session");
        }
        let affected = output_limit_vm(&mut sidecar, &coordinator, "a-connection", "a-session", 1).await;
        let same = output_limit_vm(&mut sidecar, &coordinator, "a-connection", "a-session", 2).await;
        let independent = output_limit_vm(&mut sidecar, &coordinator, "z-connection", "z-session", 3).await;
        let accounting = pressure.map(|by_bytes| {
            agentos_vm::test_support::configure_empty_vm_public_output_pressure(
                &mut sidecar, &affected, by_bytes,
            )
        });
        eprintln!("STAB218_ACCOUNTING: pressure={pressure:?} {accounting:?}");
        let first = agentos_vm::test_support::insert_process(&mut sidecar, &affected, "root-a", false);
        let second = agentos_vm::test_support::insert_process(&mut sidecar, &affected, "root-b", false);
        let same_producer = agentos_vm::test_support::insert_process(&mut sidecar, &same, "same-peer", false);
        let other_producer = agentos_vm::test_support::insert_process(&mut sidecar, &independent, "other-peer", false);
        if let Some(by_bytes) = pressure {
            first.stdout(b"a");
            if by_bytes {
                // Only the first envelope is public before the second producer starts.
                sidecar.pump_process_events_nowait(&agentos_vm::protocol::OwnershipScope::vm(
                    "a-connection", "a-session", &affected), 1).expect("first envelope fits");
                second.stdout(b"bb");
            } else {
                second.stdout(b"b");
            }
        }
        eprintln!("STAB218_STAGE: producers ready");
        let inputs = output_limit_engine_inputs(sidecar, &config, coordinator);
        let OutputLimitEngineInputs { engine, stdin, shutdown, output, _control,
            _warnings, _write_errors, _services, _progress, mut live_events } = inputs;
        let ingress = test_protocol_budget(8, 4096, "finite output-limit ingress");
        let peers = [("a-connection", "a-session", same.as_str(), "same-peer", b'p'),
            ("z-connection", "z-session", independent.as_str(), "other-peer", b'q')];
        let (frames_tx, mut frames_rx) = channel::<ProtocolFrame>(32);
        let reader_output = output.clone();
        // One finite physical-writer stand-in drains the existing output queue.
        // It is always released by close() and joined below, including failure.
        let mut reader = tokio::task::spawn_blocking(move || -> Result<(), String> {
            while let Some(encoded) = reader_output.recv_combined() {
                let frame = WireFrameCodec::new(4096).decode(&encoded.bytes)
                    .map_err(|error| error.to_string())?;
                frames_tx.try_send(frame).map_err(|error| error.to_string())?;
            }
            Ok(())
        });
        let mut engine = tokio::task::spawn_local(async move {
            eprintln!("STAB218_STAGE: protocol engine entered");
            run_protocol_engine(engine).await.map_err(|error| error.to_string())
        });
        let mut engine_result = None;
        let body = tokio::time::timeout(Duration::from_secs(10), async {
            for (index, (connection, session, vm, _, _)) in peers.iter().enumerate() {
                stdin.try_send(Ok(Some(test_accounted_frame(ProtocolFrame::RequestFrame(
                    request_frame(701 + index as RequestId, vm_ownership(connection, session, vm),
                        RequestPayload::GetProcessSnapshotRequest)), &ingress))))
                    .map_err(|error| error.to_string())?;
            }
            same_producer.stdout(b"p"); same_producer.exit(0);
            other_producer.stdout(b"q"); other_producer.exit(0);
            let mut responses = BTreeSet::new();
            let mut outputs = BTreeSet::new();
            let mut exits = BTreeSet::new();
            let mut frames_seen = 0;
            for late in [false, true] {
                if late {
                    for (index, (connection, session, vm, _, _)) in peers.iter().enumerate() {
                        stdin.try_send(Ok(Some(test_accounted_frame(ProtocolFrame::RequestFrame(
                            request_frame(703 + index as RequestId, vm_ownership(connection, session, vm),
                                RequestPayload::GetProcessSnapshotRequest)), &ingress))))
                            .map_err(|error| error.to_string())?;
                    }
                }
                while responses.len() < if late { 4 } else { 2 }
                    || (!late && (outputs.len() < 2 || exits.len() < 2)) {
                    tokio::select! {
                        result = &mut engine => {
                            let message = format!("STAB218_ENGINE_RESULT: {result:?}");
                            engine_result = Some(result);
                            return Err(message);
                        }
                        frame = frames_rx.recv() => {
                            let frame = frame.ok_or_else(|| "finite output reader closed".to_string())?;
                            frames_seen += 1;
                            if frames_seen > 32 { return Err("finite output frame bound exceeded".into()); }
                            match frame {
                                ProtocolFrame::ResponseFrame(response) if (701..=704).contains(&response.request_id) => {
                                    let index = ((response.request_id - 701) % 2) as usize;
                                    let (connection, session, vm, _, _) = peers[index];
                                    if response.ownership != vm_ownership(connection, session, vm)
                                        || !matches!(&response.payload, ResponsePayload::ProcessSnapshotResponse(_))
                                        || !responses.insert(response.request_id) {
                                        return Err(format!("peer response changed/rejected/duplicated: {response:?}"));
                                    }
                                }
                                ProtocolFrame::EventFrame(event) => {
                                    for (connection, session, vm, process, byte) in &peers {
                                        if event.ownership != vm_ownership(connection, session, vm) { continue; }
                                        match &event.payload {
                                            wire::EventPayload::ProcessOutputEvent(value) if value.process_id == *process => {
                                                if value.channel != wire::StreamChannel::Stdout || value.chunk != [*byte]
                                                    || !outputs.insert(*byte) {
                                                    return Err(format!("peer output changed/duplicated: {value:?}"));
                                                }
                                            }
                                            wire::EventPayload::ProcessExitedEvent(value) if value.process_id == *process => {
                                                if value.exit_code != 0 || value.exit_signal.is_some() || !exits.insert(*byte) {
                                                    return Err(format!("peer exit changed/duplicated: {value:?}"));
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            if engine.is_finished() || !output.is_open() {
                return Err("real engine/transport ended after scoped output limit".into());
            }
            Ok::<(), String>(())
        }).await.map_err(|error| format!("finite peer body deadline: {error}")).and_then(|result| result);
        let mut cleanup = Vec::new();
        if engine_result.is_none() {
            match shutdown.try_send(wire::ControlFrame { schema: wire::protocol_schema(),
                payload: wire::ControlPayload::ShutdownControl(wire::ShutdownControl {
                    reason: "finite output-limit test cleanup".into(),
                }) }) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    // A fatal engine drain closes ingress before its task returns.
                    // Retain and join that original engine outcome below.
                    eprintln!("STAB218_CLEANUP: shutdown ingress already closed; joining engine");
                }
                Err(error) => cleanup.push(format!("shutdown: {error}")),
            }
            match tokio::time::timeout(Duration::from_secs(3), &mut engine).await {
                Ok(result) => engine_result = Some(result),
                Err(error) => {
                    cleanup.push(format!("engine cleanup deadline/forced abort: {error}"));
                    engine.abort();
                    match tokio::time::timeout(Duration::from_secs(1), &mut engine).await {
                        Ok(result) => engine_result = Some(result),
                        Err(error) => cleanup.push(format!("engine abort join: {error}")),
                    }
                }
            }
        }
        match tokio::time::timeout(Duration::from_secs(2), &mut live_events).await {
            Ok(Ok(Ok(()))) => {}
            Ok(other) => cleanup.push(format!("live-event drainer: {other:?}")),
            Err(error) => {
                cleanup.push(format!("live-event drainer deadline/forced abort: {error}"));
                live_events.abort();
                match tokio::time::timeout(Duration::from_secs(1), &mut live_events).await {
                    Ok(Err(error)) if error.is_cancelled() => {}
                    other => cleanup.push(format!("live-event abort join: {other:?}")),
                }
            }
        }
        output.close();
        match tokio::time::timeout(Duration::from_secs(2), &mut reader).await {
            Ok(Ok(Ok(()))) => {}
            other => cleanup.push(format!("output reader: {other:?}")),
        }
        let engine_ok = matches!(&engine_result, Some(Ok(Ok(()))));
        assert!(body.is_ok() && engine_ok && cleanup.is_empty(),
            "per-VM output limit must preserve real protocol-engine peers; body={body:?}; engine={engine_result:?}; cleanup={cleanup:?}");
    }));
}

#[test]
fn per_vm_public_output_count_limit_preserves_protocol_engine_peers() {
    per_vm_output_limit_keeps_real_engine_peers(Some(false));
}

#[test]
fn per_vm_public_output_byte_limit_preserves_protocol_engine_peers() {
    per_vm_output_limit_keeps_real_engine_peers(Some(true));
}

#[test]
fn protocol_engine_fixture_preserves_healthy_peer_requests_and_output() {
    per_vm_output_limit_keeps_real_engine_peers(None);
}

#[tokio::test(flavor = "current_thread")]
async fn output_fault_replaces_precomputed_terminal_at_claim_and_drops_only_its_event_tail() {
    tokio::task::LocalSet::new().run_until(async {
            let config = agentos_driver_tokio::DriverConfig::default();
            let coordinator = OwnershipCoordinator::from_runtime_config(&config);
            let connection = coordinator.register_connection("connection-a").unwrap();
            let session = connection.open_session("session-a").unwrap();
            let _a = session.open_vm("vm-a").unwrap();
            let _b = session.open_vm("vm-b").unwrap();
            let operations = OperationTable::from_protocol_config(&config.protocol);
            let (writer, output) = test_frame_writer_with_inflight(8, 4);
            let mut tasks = JoinSet::new();
            let mut sessions = BTreeSet::new(); let mut connections = BTreeSet::new();
            let affected = OwnershipScope::vm("connection-a", "session-a", "vm-a");
            for (id, owner) in [(1, affected.clone()), (2, OwnershipScope::vm("connection-a", "session-a", "vm-b"))] {
                let operation = operations.admit(RequestOperationKey::new("connection-a", id), RequestOperationMetadata::new(owner.clone(), "precomputed", VmConcurrencyClass::OwnershipOnly), 1).unwrap();
                let reservation = writer.try_reserve_terminal(4100).unwrap();
                let ProtocolFrame::EventFrame(mut tail) = queue_test_event(id) else { unreachable!() };
                tail.ownership = owner.clone();
                let dispatch = WireDispatchResult {
                    response: response_frame(id, owner, ResponsePayload::VmConfiguredResponse(wire::VmConfiguredResponse {
                        applied_mounts: 0, applied_software: 0, projected_commands: Vec::new(),
                    })),
                    events: vec![tail],
                };
                schedule_dispatch_output(dispatch, ExtensionRequestClass::Ordinary, Some(operation), None, reservation, false, &writer, &mut tasks, &mut sessions, &mut connections).unwrap();
            }
            // No await after spawning: both precomputed successes are still
            // unclaimed when this same authority closes the affected scope.
            let fault = VmOutputFailure {
                ownership: affected, generation: 7,
                error: agentos_executor_contract::backend::HostServiceError::new("ERR_AGENTOS_RESOURCE_LIMIT", "original byte fault")
                    .with_details(serde_json::json!({"limitName":"limits.process.pendingEventBytes","limit":3,"observed":4,"configPath":"limits.process.pendingEventBytes"})),
            };
            let (disposal, first) = coordinator.begin_vm_output_failure(&fault, &operations).unwrap();
            assert!(first);
            tokio::time::timeout(Duration::from_secs(1), async {
                while let Some(result) = tasks.join_next().await { result.unwrap().unwrap(); }
            }).await.unwrap();
            let mut responses = BTreeMap::new();
            for _ in 0..2 {
                let encoded = tokio::time::timeout(Duration::from_secs(1), output.recv_control()).await.unwrap().unwrap();
                let ProtocolFrame::ResponseFrame(response) = decode_test_output(encoded) else { panic!("terminal response expected") };
                responses.insert(response.request_id, response.payload);
            }
            let Some(ResponsePayload::RejectedResponse(rejection)) = responses.get(&1) else { panic!("affected terminal must use original typed fault") };
            assert_eq!(rejection.code, "ERR_AGENTOS_RESOURCE_LIMIT");
            assert_eq!(rejection.message, "original byte fault");
            assert_eq!(rejection.unit.as_deref(), Some("bytes"));
            assert_eq!(rejection.configured_limit, Some(3)); assert_eq!(rejection.requested, Some(4));
            assert!(matches!(responses.get(&2), Some(ResponsePayload::VmConfiguredResponse(_))));
            let peer_tail = {
                let mut state = output.state.lock().unwrap();
                assert!(state.open);
                assert_eq!(state.ordinary.len(), 1);
                state.ordinary.pop_front().expect("peer event tail")
            };
            let ProtocolFrame::EventFrame(peer_tail) = decode_test_output(peer_tail) else {
                panic!("peer event tail must be an event");
            };
            assert_eq!(peer_tail.ownership, OwnershipScope::vm("connection-a", "session-a", "vm-b"));
            let wire::EventPayload::StructuredEvent(peer_event) = peer_tail.payload else {
                panic!("peer event tail must retain its structured payload");
            };
            assert_eq!(peer_event.name, "queue-test");
            assert_eq!(peer_event.detail.get("request_id").map(String::as_str), Some("2"));
            assert!(output.state.lock().unwrap().ordinary.is_empty());
            output.close();
            assert!(output.recv_ordinary().is_none());
            assert_eq!(output.state.lock().unwrap().control_len(), 0);
            assert_eq!(writer.terminal_budget.usage(), (0, 0));
            let disposal = disposal.unwrap();
            tokio::time::timeout(Duration::from_secs(1), disposal.wait_drained()).await.unwrap();
            disposal.complete().unwrap();
            output.close();
        }).await;
}

#[test]
fn forced_fault_publication_preserves_original_details_and_generic_peer_shutdown() {
    let owner = OwnershipScope::vm("connection-a", "session-a", "vm-a");
    let fault = VmOutputFailure { ownership: owner.clone(), generation: 7,
            error: agentos_executor_contract::backend::HostServiceError::new("ERR_AGENTOS_RESOURCE_LIMIT", "original byte cause").with_details(serde_json::json!({"limitName":"limits.process.pendingEventBytes","limit":3,"observed":4,"configPath":"limits.process.pendingEventBytes"})), };
    for progress in [false, true] {
        let mut outcome = ForcedRequestOutcome {
            key: RequestOperationKey::new("connection-a", 71),
            ownership: owner.clone(),
            output_failure: Some(fault.clone()),
        };
        let ProtocolFrame::ResponseFrame(response) =
            forced_shutdown_response(&outcome, OperationCancellationReason::Shutdown, progress)
        else {
            panic!("response")
        };
        assert_eq!(response.request_id, 71);
        assert_eq!(response.ownership, owner);
        let ResponsePayload::RejectedResponse(rejection) = response.payload else {
            panic!("typed rejection")
        };
        assert_eq!(rejection.code, "ERR_AGENTOS_RESOURCE_LIMIT");
        assert_eq!(rejection.message, "original byte cause");
        assert_eq!(rejection.unit.as_deref(), Some("bytes"));
        assert_eq!(
            rejection.limit_name.as_deref(),
            Some("limits.process.pendingEventBytes")
        );
        assert_eq!(rejection.configured_limit, Some(3));
        assert_eq!(rejection.requested, Some(4));
        assert_eq!(
            rejection.configuration_path.as_deref(),
            Some("limits.process.pendingEventBytes")
        );
        assert_eq!(rejection.vm_id.as_deref(), Some("vm-a"));
        assert_eq!(rejection.session_generation, Some(7));
        assert_eq!(rejection.errno.as_deref(), Some("ENOBUFS"));
        outcome.output_failure = None;
        let ProtocolFrame::ResponseFrame(response) =
            forced_shutdown_response(&outcome, OperationCancellationReason::Shutdown, progress)
        else {
            panic!("response")
        };
        assert_eq!(response.request_id, 71);
        assert_eq!(response.ownership, owner);
        let ResponsePayload::RejectedResponse(rejection) = response.payload else {
            panic!("generic shutdown")
        };
        assert_eq!(
            rejection.code,
            if progress {
                "ERR_AGENTOS_PROGRESS_SHUTDOWN"
            } else {
                "ERR_AGENTOS_REQUEST_SHUTDOWN"
            }
        );
        assert_eq!(rejection.errno.as_deref(), Some("ESHUTDOWN"));
    }
}

#[test]
fn protocol_extension_self_disposal_completes_without_draining_its_own_request() {
    struct SelfDisposalExtension;
    impl Extension for SelfDisposalExtension {
        fn namespace(&self) -> &str {
            "dev.rivet.agentos.test.self-disposal"
        }
        fn handle_request<'a>(
            &'a self,
            mut ctx: agentos_vm::ExtensionContext,
            _payload: Vec<u8>,
        ) -> agentos_vm::ExtensionFuture<'a, agentos_vm::ExtensionResponse> {
            Box::pin(async move {
                ctx.bind_vm_to_session("owned-self-disposal").await?;
                let events = ctx
                    .dispose_session_resources_wire("owned-self-disposal")
                    .await?;
                agentos_vm::ExtensionResponse::with_wire_events(b"vm-disposed".to_vec(), events)
            })
        }
    }
    let mut config = VmManagerConfig::default();
    let runtime = agentos_driver_tokio::TokioDriver::process(&config.runtime)
        .expect("singleton fixture driver");
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .finish();
    let _protocol_logs = tracing::subscriber::set_default(subscriber);
    runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            config.max_frame_bytes = 4096;
            config.runtime.protocol.terminal_fallback_bytes = 4100;
            config.runtime.protocol.shutdown_grace_ms = 500;
            let mut sidecar = VmManager::with_config_extensions_and_runtime(
                LocalBridge::default(), config.clone(), vec![Box::new(SelfDisposalExtension)], runtime.handle(),
            ).expect("extension fixture manager");
            let coordinator = OwnershipCoordinator::from_runtime_config(&config.runtime);
            agentos_vm::test_support::register_session(&mut sidecar, "self-connection", "self-session");
            coordinator.register_connection("self-connection").unwrap().open_session("self-session").unwrap();
            let vm_id = output_limit_vm(&mut sidecar, &coordinator, "self-connection", "self-session", 1).await;
            let (scratch, staging, unix) = agentos_vm::test_support::host_path_cleanup_paths(&sidecar, &vm_id);
            let mut paths = vec![scratch, unix]; paths.extend(staging);
            let membership = coordinator.clone();
            let OutputLimitEngineInputs { engine, stdin, shutdown, output, _control, _warnings,
                _write_errors, _services, _progress, mut live_events } = output_limit_engine_inputs(sidecar, &config, coordinator);
            let (frames_tx, mut frames_rx) = channel::<ProtocolFrame>(8);
            let reader_output = output.clone();
            let mut reader = tokio::task::spawn_blocking(move || -> Result<(), String> {
                while let Some(encoded) = reader_output.recv_combined() {
                    let frame = WireFrameCodec::new(4096).decode(&encoded.bytes).map_err(|error| error.to_string())?;
                    frames_tx.try_send(frame).map_err(|error| error.to_string())?;
                }
                Ok(())
            });
            let mut engine = tokio::task::spawn_local(async move { run_protocol_engine(engine).await.map_err(|error| error.to_string()) });
            let ingress = test_protocol_budget(4, 4096, "self-disposal ingress");
            let body = tokio::time::timeout(Duration::from_secs(3), async {
                stdin.try_send(Ok(Some(test_accounted_frame(ProtocolFrame::RequestFrame(request_frame(
                    701, vm_ownership("self-connection", "self-session", &vm_id), RequestPayload::ExtEnvelope(ExtEnvelope {
                        namespace: "dev.rivet.agentos.test.self-disposal".into(), payload: Vec::new(),
                    }),
                )), &ingress)))).map_err(|error| error.to_string())?;
                let mut received = false;
                for _ in 0..8 {
                    let frame = frames_rx.recv().await.ok_or("self-disposal output closed")?;
                    if let ProtocolFrame::ResponseFrame(response) = frame {
                        if response.request_id == 701 {
                            match response.payload {
                                wire::ResponsePayload::ExtEnvelope(value) if value.payload == b"vm-disposed" => { received = true; break; }
                                other => return Err(format!("self-disposal terminal changed: {other:?}")),
                            }
                        }
                    }
                }
                if !received { return Err("missing self-disposal terminal".into()); }
                if membership.connection("self-connection").unwrap().session("self-session").unwrap().vm(&vm_id).is_ok() {
                    return Err("self-disposal retained VM membership".into());
                }
                for path in &paths {
                    match std::fs::symlink_metadata(path) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        other => return Err(format!("self-disposal physical path retained: {} {other:?}", path.display())),
                    }
                }
                Ok::<(), String>(())
            }).await.map_err(|error| format!("self-disposal deadline: {error}")).and_then(|value| value);
            let mut cleanup = Vec::new();
            if let Err(error) = shutdown.try_send(wire::ControlFrame { schema: wire::protocol_schema(),
                payload: wire::ControlPayload::ShutdownControl(wire::ShutdownControl { reason: "self-disposal fixture cleanup".into() }),
            }) { cleanup.push(format!("shutdown admission: {error}")); }
            let engine_result = match tokio::time::timeout(Duration::from_secs(3), &mut engine).await {
                Ok(result) => Some(result),
                Err(error) => {
                    cleanup.push(format!("engine retirement deadline/forced abort: {error}")); engine.abort();
                    match tokio::time::timeout(Duration::from_secs(1), &mut engine).await {
                        Ok(result) => Some(result),
                        Err(error) => { cleanup.push(format!("engine abort join: {error}")); None }
                    }
                }
            };
            match tokio::time::timeout(Duration::from_secs(2), &mut live_events).await {
                Ok(Ok(Ok(()))) => {}
                Ok(other) => cleanup.push(format!("live-event retirement: {other:?}")),
                Err(error) => {
                    cleanup.push(format!("live-event deadline/forced abort: {error}")); live_events.abort();
                    match tokio::time::timeout(Duration::from_secs(1), &mut live_events).await {
                        Ok(Err(error)) if error.is_cancelled() => {}
                        other => cleanup.push(format!("live-event abort join: {other:?}")),
                    }
                }
            }
            output.close();
            match tokio::time::timeout(Duration::from_secs(2), &mut reader).await {
                Ok(Ok(Ok(()))) => {}
                other => cleanup.push(format!("output reader retirement: {other:?}")),
            }
            assert!(body.is_ok() && matches!(engine_result, Some(Ok(Ok(())))) && cleanup.is_empty(),
                "extension self-disposal must not await its own request; body={body:?}; engine={engine_result:?}; cleanup={cleanup:?}");
        }));
}
