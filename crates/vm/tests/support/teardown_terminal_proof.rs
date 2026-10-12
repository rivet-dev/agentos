// Included directly in execution::process::pending_event_reservation_tests.
// No guest execution or host-path fixture: use the real kernel runtime control
// and HostFunction backend's immediate ShutdownOutcome::Exited proof.
#[test]
fn teardown_observes_backend_exit_proof_behind_full_queue_without_releasing_output() {
    use agentos_vm_kernel::process_runtime::ProcessExit;
    use libc::SIGKILL;

    for mode in 0..3 {
        let mut config = KernelVmConfig::new("vm-teardown-terminal-proof");
        config.permissions = Permissions::allow_all();
        let mut kernel = SidecarKernel::new(MountTable::new(MemoryFileSystem::new()), config);
        kernel
            .register_driver(CommandDriver::new(EXECUTION_DRIVER_NAME, [WASM_COMMAND]))
            .unwrap();
        let handle = kernel
            .spawn_process(
                WASM_COMMAND,
                Vec::new(),
                SpawnOptions {
                    requester_driver: Some(EXECUTION_DRIVER_NAME.into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let pid = handle.pid();
        let budget = VmPendingByteBudget::new(
            4096,
            queue_tracker::TrackedLimit::PendingExecutionEventBytes,
        );
        let mut limits = crate::limits::VmLimits::default();
        limits.process.pending_event_count = 1;
        limits.process.pending_event_bytes = 4096;
        let mut process = ActiveProcess::new(
            pid,
            handle,
            test_runtime_context(),
            limits,
            1,
            GuestRuntimeKind::WebAssembly,
            ActiveExecution::HostFunction(HostFunctionExecution::default()),
        )
        .with_vm_pending_byte_budgets(
            VmPendingByteBudget::new(4096, queue_tracker::TrackedLimit::PendingKernelStdinBytes),
            Arc::clone(&budget),
        );
        assert_eq!(process.pending_execution_event_count_limit, 1);
        let queued = if mode == 2 {
            kernel
                .kill_process(EXECUTION_DRIVER_NAME, pid, SIGTERM)
                .unwrap();
            process.apply_runtime_controls().unwrap();
            assert_eq!(
                process.pending_runtime_exit,
                Some(ProcessExit::Signaled {
                    signal: SIGTERM,
                    core_dumped: false
                })
            );
            // Stage genuine earlier backend proof in the ordinary retained
            // queue to prove its existing precedence over the later slot.
            process.take_pending_runtime_exit_event().unwrap()
        } else {
            ActiveExecutionEvent::Stdout(vec![0x5a])
        };
        let queued_bytes = queued.retained_bytes();
        process.queue_pending_execution_event(queued).unwrap();
        assert_eq!(process.pending_execution_events.len(), 1);
        assert_eq!(budget.used(), queued_bytes);
        if mode != 0 {
            let signal = if mode == 2 { SIGKILL } else { SIGTERM };
            kernel
                .kill_process(EXECUTION_DRIVER_NAME, pid, signal)
                .unwrap();
            process.apply_runtime_controls().unwrap();
            assert_eq!(
                process.pending_runtime_exit,
                Some(ProcessExit::Signaled {
                    signal,
                    core_dumped: false
                })
            );
        }
        let observed = process.take_teardown_exit_event().unwrap();
        let observed_signal = process.exit_signal;
        let pending_after_first = process.pending_runtime_exit;
        let retained_count = process.pending_execution_events.len();
        let retained_bytes = process.pending_execution_event_bytes;
        let reserved_bytes = budget.used();
        let second = process.take_teardown_exit_event().unwrap();
        let pending_after_second = process.pending_runtime_exit;
        let no_duplicate = process.take_teardown_exit_event().unwrap().is_none();
        let retained_output = if mode != 2 {
            process.pop_pending_execution_event()
        } else {
            None
        };
        let released_bytes = budget.used();

        // Retire the actual fixture kernel process from its real backend
        // signal proof after recording the observations, even on old behavior.
        if mode == 0 {
            kernel
                .kill_process(EXECUTION_DRIVER_NAME, pid, SIGTERM)
                .unwrap();
            process.apply_runtime_controls().unwrap();
        }
        let signal = if mode == 2 { SIGKILL } else { SIGTERM };
        process.kernel_handle.finish_signaled(signal, false);
        drop(process);
        kernel.waitpid(pid).unwrap();
        assert!(kernel.list_processes().is_empty());
        assert_eq!(budget.used(), 0);

        if mode == 0 {
            assert!(
                observed.is_none() && second.is_none() && no_duplicate,
                "full stdout without terminal proof must stay pending"
            );
            assert!(pending_after_first.is_none() && pending_after_second.is_none());
        } else if mode == 1 {
            assert!(
                matches!(observed, Some(ActiveExecutionEvent::Exited(code)) if code == 128 + SIGTERM),
                "already-recorded backend exit must bypass a full nonterminal queue"
            );
            assert_eq!(observed_signal, Some(SIGTERM));
            assert!(pending_after_first.is_none() && pending_after_second.is_none());
            assert!(
                second.is_none() && no_duplicate,
                "backend exit proof is consumed once"
            );
        } else {
            assert!(
                matches!(observed, Some(ActiveExecutionEvent::Exited(code)) if code == 128 + SIGTERM),
                "earlier queued genuine terminal keeps precedence"
            );
            assert_eq!(observed_signal, Some(SIGTERM));
            assert_eq!(
                pending_after_first,
                Some(ProcessExit::Signaled {
                    signal: SIGKILL,
                    core_dumped: false
                })
            );
            assert!(
                matches!(second, Some(ActiveExecutionEvent::Exited(code)) if code == 128 + SIGKILL)
            );
            assert!(pending_after_second.is_none() && no_duplicate);
        }
        if mode != 2 {
            assert_eq!(retained_count, 1);
            assert_eq!(retained_bytes, queued_bytes);
            assert_eq!(
                reserved_bytes, queued_bytes,
                "terminal observation must not release retained stdout reservation"
            );
            assert!(
                matches!(retained_output, Some(ActiveExecutionEvent::Stdout(bytes)) if bytes == vec![0x5a]),
                "retained output contents/order must remain intact"
            );
        } else {
            assert_eq!((retained_count, retained_bytes, reserved_bytes), (0, 0, 0));
        }
        assert_eq!(
            released_bytes, 0,
            "normal queue consumption releases the original reservation once"
        );
    }
}

#[cfg(all(feature = "node-v8", feature = "test-support"))]
#[test]
fn standalone_async_exit_proof_preserves_full_queue_output() {
    use agentos_executor_contract::GuestRuntimeConfig;
    use agentos_executor_node_v8::{
        CreateJavascriptContextRequest, JavascriptExecutionEngine, JavascriptExecutionError,
        JavascriptExecutionEvent, JavascriptExecutionLimits, StartJavascriptExecutionRequest,
    };
    use nix::libc::SIGKILL;
    use std::time::Duration;

    #[derive(Debug)]
    struct Observation {
        cap: usize,
        probe_code: Option<i32>,
        count: usize,
        bytes: usize,
        reserved: usize,
        expected_bytes: usize,
        no_fixed_slot: bool,
    }
    let runtime = test_runtime_context();
    let handle = runtime.tokio_handle().clone();
    let mut observations = Vec::new();
    for cap in [2usize, 1usize] {
        let temporary = tempfile::tempdir().unwrap();
        std::fs::write(
            temporary.path().join("entry.cjs"),
            b"process.stdout.write('a'); process.exitCode = 7;\n",
        )
        .unwrap();
        let vm_id = format!("standalone-terminal-{cap}");
        let mut engine = JavascriptExecutionEngine::new(runtime.clone());
        engine.set_import_cache_base_dir(&vm_id, temporary.path().join("import-cache"));
        let context = engine.create_context(CreateJavascriptContextRequest {
            vm_id: vm_id.clone(),
            bootstrap_module: None,
            compile_cache_root: None,
        });
        let mut execution = engine
            .prepare_execution(StartJavascriptExecutionRequest {
                vm_id: vm_id.clone(),
                context_id: context.context_id,
                argv: vec!["./entry.cjs".into()],
                argv0: None,
                env: Default::default(),
                cwd: temporary.path().to_path_buf(),
                limits: JavascriptExecutionLimits {
                    wall_clock_limit_ms: Some(5_000),
                    ..Default::default()
                },
                // Deliberately the supported standalone adapter contract, not VM
                // descriptor I/O. CJS entry loading is serviced by the real helper.
                guest_runtime: GuestRuntimeConfig {
                    kernel_stdio: false,
                    ..Default::default()
                },
                inline_code: None,
                wasm_module_bytes: None,
            })
            .expect("prepare actual standalone V8 execution");
        assert!(execution.is_prepared_for_start());
        let roots = engine
            .import_cache_cleanup_guards(&vm_id)
            .into_iter()
            .map(|guard| guard.prepare_retirement().root_path().to_path_buf())
            .collect::<Vec<_>>();
        let (posted, mut enqueued) = tokio::sync::oneshot::channel();
        execution.observe_terminal_enqueue_for_test(posted).unwrap();
        let (duplicate, _duplicate_receiver) = tokio::sync::oneshot::channel();
        let duplicate_rejected = execution
            .observe_terminal_enqueue_for_test(duplicate)
            .is_err();
        let mut config = KernelVmConfig::new(&vm_id);
        config.permissions = Permissions::allow_all();
        let mut kernel = SidecarKernel::new(MountTable::new(MemoryFileSystem::new()), config);
        kernel
            .register_driver(CommandDriver::new(
                EXECUTION_DRIVER_NAME,
                [JAVASCRIPT_COMMAND],
            ))
            .unwrap();
        let kernel_handle = kernel
            .spawn_process(
                JAVASCRIPT_COMMAND,
                Vec::new(),
                SpawnOptions {
                    requester_driver: Some(EXECUTION_DRIVER_NAME.into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let pid = kernel_handle.pid();
        let budget = VmPendingByteBudget::new(
            4096,
            queue_tracker::TrackedLimit::PendingExecutionEventBytes,
        );
        let mut limits = crate::limits::VmLimits::default();
        limits.process.pending_event_count = cap;
        limits.process.pending_event_bytes = 4096;
        // Construct/attach the real kernel control before starting guest code.
        let mut process = ActiveProcess::new(
            pid,
            kernel_handle,
            runtime.clone(),
            limits,
            cap,
            GuestRuntimeKind::JavaScript,
            ActiveExecution::Javascript(execution),
        )
        .with_vm_pending_byte_budgets(
            VmPendingByteBudget::new(4096, queue_tracker::TrackedLimit::PendingKernelStdinBytes),
            Arc::clone(&budget),
        );
        let no_native_child = process.execution.native_process_id().is_none();
        let mut real_exit = None;
        let mut cleanup = Vec::new();
        let mut late_rejected = false;
        let body: Result<Observation, String> = handle.block_on(async {
            process
                .execution
                .start_prepared()
                .map_err(|error| error.to_string())?;
            let ActiveExecution::Javascript(js) = &mut process.execution else {
                unreachable!()
            };
            let (late, _late_receiver) = tokio::sync::oneshot::channel();
            late_rejected = js.observe_terminal_enqueue_for_test(late).is_err();
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            // Drive only genuine standalone module bootstrap until its first
            // real stdout, before mapping an RPC into a common HostCall.
            let stdout = loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    return Err("standalone startup deadline".into());
                }
                let event = js
                    .poll_event(remaining)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "standalone startup event deadline".to_owned())?;
                match event {
                    JavascriptExecutionEvent::SyncRpcRequest(request) => {
                        if !js
                            .try_service_standalone_module_sync_rpc(&request)
                            .map_err(|error| error.to_string())?
                        {
                            return Err(format!(
                                "unexpected standalone startup RPC {}",
                                request.method
                            ));
                        }
                    }
                    JavascriptExecutionEvent::Stdout(bytes) => break bytes,
                    JavascriptExecutionEvent::Exited(code) => {
                        real_exit = Some(code);
                        return Err(format!("real exit {code} arrived before standalone stdout"));
                    }
                    other => return Err(format!("unexpected standalone startup event {other:?}")),
                }
            };
            let output_event = ActiveExecutionEvent::Stdout(stdout);
            let expected_bytes = output_event.retained_bytes();
            process
                .queue_pending_polled_execution_event(PolledExecutionEvent::unreserved(
                    output_event,
                ))
                .map_err(|error| error.to_string())?;
            // Stop adapter consumption here. The only remaining guest action is
            // local exitCode assignment; no host service is needed to finish.
            let code = tokio::time::timeout_at(deadline, &mut enqueued)
                .await
                .map_err(|_| "real terminal enqueue acknowledgement deadline".to_owned())?
                .map_err(|_| "real terminal enqueue observer closed".to_owned())?;
            if code != 7 {
                return Err(format!("real terminal enqueue had unexpected code {code}"));
            }
            let no_fixed_slot = process.pending_runtime_exit.is_none();
            let probe_code = match process
                .take_teardown_exit_event()
                .map_err(|error| error.to_string())?
            {
                Some(ActiveExecutionEvent::Exited(code)) => {
                    real_exit = Some(code);
                    Some(code)
                }
                None => None,
                Some(other) => return Err(format!("unexpected terminal probe {other:?}")),
            };
            Ok(Observation {
                cap,
                probe_code,
                count: process.pending_execution_events.len(),
                bytes: process.pending_execution_event_bytes,
                reserved: budget.used(),
                expected_bytes,
                no_fixed_slot: no_fixed_slot && process.pending_runtime_exit.is_none(),
            })
        });

        // Both positive and expected-RED outcomes consume the original output
        // normally, then obtain the actual exit if the full-queue probe missed it.
        let output = process.pop_pending_execution_event();
        let released = budget.used();
        if real_exit.is_none() {
            match process.take_teardown_exit_event() {
                Ok(Some(ActiveExecutionEvent::Exited(code))) => real_exit = Some(code),
                other => cleanup.push(format!(
                    "real terminal after normal output consumption: {other:?}"
                )),
            }
        }
        let mut duplicate_exit = false;
        if real_exit.is_some() {
            let ActiveExecution::Javascript(js) = &mut process.execution else {
                unreachable!()
            };
            match js.try_poll_event() {
                Ok(None) | Err(JavascriptExecutionError::EventChannelClosed) => {}
                Ok(Some(JavascriptExecutionEvent::Exited(_))) => duplicate_exit = true,
                other => cleanup.push(format!("unexpected event after actual exit: {other:?}")),
            }
        }
        let unforced_exit = real_exit;
        let mut retired = false;
        if let Some(code) = real_exit {
            // This is a direct kernel fixture with no VmManager tracking. Its
            // only normal finish value comes from the real consumed V8 event.
            process.kernel_handle.finish(code);
            retired = true;
        } else {
            cleanup.push("setup/terminal failure required explicit fixture cancellation".into());
            match kernel.kill_process(EXECUTION_DRIVER_NAME, pid, SIGKILL) {
                Ok(()) => match process.apply_runtime_controls() {
                    Ok(()) => match process.take_pending_runtime_exit_event() {
                        Some(ActiveExecutionEvent::Exited(_))
                            if process.exit_signal == Some(SIGKILL) =>
                        {
                            process.kernel_handle.finish_signaled(SIGKILL, false);
                            retired = true;
                        }
                        other => cleanup.push(format!("fixture signal proof missing: {other:?}")),
                    },
                    Err(error) => cleanup.push(format!("fixture cancel control: {error}")),
                },
                Err(error) => cleanup.push(format!("fixture kernel cancellation: {error}")),
            }
        }
        drop(process); // Normal execution Drop transfers its real session teardown.
        let mut reaped = None;
        if retired {
            match kernel.waitpid(pid) {
                Ok(result) => reaped = Some((result.pid, result.status)),
                Err(error) => cleanup.push(format!("kernel reap: {error}")),
            }
        }
        engine.dispose_vm(&vm_id); // Release actual context/catalog/cache owners.
        let contexts = engine.context_count_for_test();
        drop(engine);
        assert!(
            cleanup.is_empty(),
            "standalone witness cleanup invalid: {cleanup:?}; body={body:?}"
        );
        assert!(kernel.list_processes().is_empty());
        assert_eq!(reaped, Some((pid, 7)));
        assert_eq!(contexts, 0);
        assert!(no_native_child, "the witness must use actual in-process V8");
        assert!(roots.len() == 1 && roots.iter().all(|root| !root.exists()));
        assert!(
            duplicate_rejected && late_rejected,
            "observer must be once-only and pre-start"
        );
        assert!(
            matches!(output, Some(ActiveExecutionEvent::Stdout(bytes)) if bytes.as_slice() == b"a")
        );
        assert_eq!((released, budget.used()), (0, 0));
        assert_eq!(unforced_exit, Some(7));
        assert!(!duplicate_exit);
        let observation =
            body.expect("standalone startup/ack setup must succeed before behavioral RED");
        assert_eq!(
            (observation.count, observation.bytes, observation.reserved),
            (1, observation.expected_bytes, observation.expected_bytes)
        );
        assert!(
            observation.no_fixed_slot,
            "the witness must use real adapter/FIFO proof only"
        );
        observations.push(observation);
    }
    assert_eq!(observations[0].cap, 2);
    assert_eq!(
        observations[0].probe_code,
        Some(7),
        "non-full real-adapter control must pass"
    );
    assert_eq!(observations[1].cap, 1);
    assert_eq!(observations[1].probe_code, Some(7),
        "real enqueued adapter exit must remain observable behind full local stdout without releasing output");
}
