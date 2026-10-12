use agentos_executor_v8_runtime::embedded_runtime::{shared_embedded_runtime, EmbeddedV8Runtime};
use agentos_executor_v8_runtime::runtime_protocol::{RuntimeCommand, RuntimeEvent, SessionMessage};
use agentos_executor_v8_runtime::session::RuntimeEventOutputReceiver;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

// Timing-sensitive assertions flake under the CPU contention of a parallel test
// run (see CLAUDE.md > Testing). Gated off by default; the nightly timing lane
// sets AGENTOS_RUN_TIMING_TESTS=1 to enforce them.
fn run_timing_sensitive_tests() -> bool {
    std::env::var_os("AGENTOS_RUN_TIMING_TESTS").is_some()
}

static NEXT_TEST_SESSION_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TEST_VM_GENERATION: AtomicU64 = AtomicU64::new(1);
const TEST_REACTOR_WORK_QUANTUM: usize = 64;
const TEST_PROCESS_VM_EXECUTOR_LIMIT: usize = 64;

fn next_session_id() -> String {
    format!(
        "embedded-runtime-session-{}",
        NEXT_TEST_SESSION_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn process_runtime_context() -> io::Result<agentos_driver_tokio::DriverHandle> {
    let config = agentos_driver_tokio::DriverConfig {
        max_active_vm_executors: TEST_PROCESS_VM_EXECUTOR_LIMIT,
        ..agentos_driver_tokio::DriverConfig::default()
    };
    agentos_driver_tokio::TokioDriver::process(&config)
        .map(agentos_driver_tokio::TokioDriver::handle)
        .map_err(|error| io::Error::other(error.to_string()))
}

fn embedded_runtime(max_concurrency: usize) -> io::Result<EmbeddedV8Runtime> {
    EmbeddedV8Runtime::new(Some(max_concurrency), process_runtime_context()?)
}

fn default_embedded_runtime() -> io::Result<EmbeddedV8Runtime> {
    EmbeddedV8Runtime::new(None, process_runtime_context()?)
}

fn vm_runtime_context(session_id: &str) -> io::Result<agentos_driver_tokio::DriverHandle> {
    use agentos_driver_tokio::accounting::{ResourceClass, ResourceLedger, ResourceLimit};

    let process = process_runtime_context()?;
    let limits = ResourceClass::ALL
        .into_iter()
        .map(|resource| {
            let maximum = if resource == ResourceClass::HandleCommands {
                // Preserve the original regression's exact ordinary-lane bound.
                256
            } else {
                process
                    .resources()
                    .usage(resource)
                    .limit
                    .expect("process test runtime must bound every resource class")
            };
            let config_path = match resource {
                ResourceClass::ReadyHandles => "limits.reactor.maxReadyHandles",
                ResourceClass::Timers => "limits.jsRuntime.maxTimers",
                ResourceClass::HandleCommands => "limits.reactor.maxHandleCommands",
                ResourceClass::BridgeCalls => "limits.reactor.maxBridgeCalls",
                ResourceClass::BridgeRequestBytes => "limits.reactor.maxBridgeRequestBytes",
                ResourceClass::BridgeResponseBytes => "limits.reactor.maxBridgeResponseBytes",
                ResourceClass::AsyncCompletions => "limits.reactor.maxAsyncCompletions",
                _ => "limits.test.completeBoundedVmLedger",
            };
            (resource, ResourceLimit::new(maximum, config_path))
        })
        .collect::<Vec<_>>();
    let resources = Arc::new(ResourceLedger::child(
        format!("test-vm={session_id}"),
        limits,
        Arc::clone(process.resources()),
    ));
    Ok(process.scoped_for_vm(
        resources,
        NEXT_TEST_VM_GENERATION.fetch_add(1, Ordering::Relaxed),
    ))
}

fn register_and_create_session(
    runtime: &Arc<EmbeddedV8Runtime>,
    session_id: &str,
) -> io::Result<RuntimeEventOutputReceiver> {
    let session_runtime = vm_runtime_context(session_id)?;
    let (receiver, _registration) =
        runtime.register_session_with_runtime(session_id, &session_runtime)?;
    runtime.dispatch_create_session_with_runtime(
        RuntimeCommand::CreateSession {
            session_id: session_id.to_owned(),
            heap_limit_mb: None,
            cpu_time_limit_ms: None,
            wall_clock_limit_ms: None,
            limit_external_memory: false,
            warm_hint: None,
        },
        session_runtime,
        TEST_REACTOR_WORK_QUANTUM,
        std::time::Duration::from_secs(30),
    )?;
    Ok(receiver)
}

fn register_and_create_session_with_cpu_time_limit(
    runtime: &Arc<EmbeddedV8Runtime>,
    session_id: &str,
    cpu_time_limit_ms: Option<u32>,
) -> io::Result<RuntimeEventOutputReceiver> {
    let session_runtime = vm_runtime_context(session_id)?;
    let (receiver, _registration) =
        runtime.register_session_with_runtime(session_id, &session_runtime)?;
    runtime.dispatch_create_session_with_runtime(
        RuntimeCommand::CreateSession {
            session_id: session_id.to_owned(),
            heap_limit_mb: None,
            cpu_time_limit_ms,
            wall_clock_limit_ms: None,
            limit_external_memory: false,
            warm_hint: None,
        },
        session_runtime,
        TEST_REACTOR_WORK_QUANTUM,
        std::time::Duration::from_secs(30),
    )?;
    Ok(receiver)
}

fn dispatch_execute(
    runtime: &EmbeddedV8Runtime,
    session_id: &str,
    mode: u8,
    bridge_code: &str,
    user_code: &str,
) -> io::Result<()> {
    runtime.dispatch(RuntimeCommand::SendToSession {
        session_id: session_id.to_owned(),
        message: SessionMessage::Execute {
            mode,
            file_path: String::new(),
            bridge_code: bridge_code.to_owned(),
            post_restore_script: String::new(),
            userland_code: String::new(),
            high_resolution_time: false,
            user_code: user_code.to_owned(),
            wasm_module_bytes: None,
            guest_bridge_policy: None,
        },
    })
}

fn dispatch_execute_after_backpressure(
    runtime: &EmbeddedV8Runtime,
    session_id: &str,
    mode: u8,
    bridge_code: &str,
    user_code: &str,
) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match dispatch_execute(runtime, session_id, mode, bridge_code, user_code) {
            Ok(()) => return Ok(()),
            Err(error)
                if error
                    .to_string()
                    .contains("ERR_AGENTOS_SESSION_COMMAND_LIMIT") =>
            {
                if Instant::now() >= deadline {
                    return Err(error);
                }
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
}

fn wait_for_execution_result(
    receiver: &RuntimeEventOutputReceiver,
    session_id: &str,
) -> RuntimeEvent {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for execution result");
        let event = receiver
            .recv_timeout(remaining)
            .expect("runtime event should arrive before timeout");
        if matches!(
            &event,
            RuntimeEvent::ExecutionResult {
                session_id: event_session_id,
                ..
            } if event_session_id == session_id
        ) {
            return event;
        }
    }
}

fn wait_for_bridge_call(receiver: &RuntimeEventOutputReceiver, session_id: &str) -> RuntimeEvent {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for bridge call");
        let event = receiver
            .recv_timeout(remaining)
            .expect("bridge call should arrive before timeout");
        if matches!(
            &event,
            RuntimeEvent::BridgeCall {
                session_id: event_session_id,
                ..
            } if event_session_id == session_id
        ) {
            return event;
        }
    }
}

fn assert_execution_ok(receiver: &RuntimeEventOutputReceiver, session_id: &str) {
    let event = wait_for_execution_result(receiver, session_id);
    match event {
        RuntimeEvent::ExecutionResult {
            exit_code,
            error,
            exports,
            ..
        } => {
            assert_eq!(exit_code, 0, "expected successful execution result");
            assert!(error.is_none(), "unexpected execution error: {error:?}");
            assert!(
                exports.is_none(),
                "script execution should not export values"
            );
        }
        other => panic!("expected execution result, got {other:?}"),
    }
}

fn wait_until(message: &str, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("{message}");
}

fn assert_create_destroy_reuses_session_ids() -> io::Result<()> {
    let runtime = shared_embedded_runtime(process_runtime_context()?)?;
    let session_id = next_session_id();

    let _receiver = register_and_create_session(&runtime, &session_id)?;
    assert!(
        runtime.session_count() >= 1,
        "embedded runtime should track created sessions"
    );

    let duplicate_error = runtime
        .dispatch(RuntimeCommand::CreateSession {
            session_id: session_id.clone(),
            heap_limit_mb: None,
            cpu_time_limit_ms: None,
            wall_clock_limit_ms: None,
            limit_external_memory: false,
            warm_hint: None,
        })
        .expect_err("duplicate sessions should be rejected");
    assert_eq!(duplicate_error.kind(), io::ErrorKind::Other);

    runtime.session_handle(session_id.clone()).destroy()?;
    assert_eq!(
        runtime.session_count(),
        0,
        "destroying the only test session should return the runtime to zero sessions"
    );

    let _receiver = register_and_create_session(&runtime, &session_id)?;
    runtime.session_handle(session_id).destroy()?;
    assert_eq!(
        runtime.session_count(),
        0,
        "recreated sessions should also tear down cleanly"
    );

    Ok(())
}

fn assert_stale_session_handle_cannot_settle_reused_session_call() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let session_id = next_session_id();

    let _first_receiver = register_and_create_session(&runtime, &session_id)?;
    let stale_handle = runtime.session_handle(session_id.clone());
    stale_handle.destroy()?;
    wait_until(
        "destroyed session generation must release its executor slot before reuse",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );

    let receiver = register_and_create_session(&runtime, &session_id)?;
    let current_handle = runtime.session_handle(session_id.clone());
    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "_loadFileSync.applySyncPromise(void 0, ['/generation-check']);",
    )?;
    let call_id = match wait_for_bridge_call(&receiver, &session_id) {
        RuntimeEvent::BridgeCall { call_id, .. } => call_id,
        other => panic!("expected bridge call, got {other:?}"),
    };

    let error = stale_handle
        .send_bridge_response(call_id, 0, Vec::new())
        .expect_err("stale output generation must not settle a reused session call");
    assert!(
        error
            .to_string()
            .contains("ERR_AGENTOS_BRIDGE_STALE_GENERATION"),
        "unexpected stale-generation error: {error}"
    );
    current_handle.send_bridge_response(call_id, 0, Vec::new())?;
    assert_execution_ok(&receiver, &session_id);
    current_handle.destroy()?;
    Ok(())
}

fn assert_warmed_snapshot_bridge_state() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;
    let bridge_code = "(function() { globalThis.__snapshotMarker = 'warm'; })();";

    runtime.dispatch(RuntimeCommand::WarmSnapshot {
        bridge_code: bridge_code.to_owned(),
        userland_code: String::new(),
    })?;
    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        bridge_code,
        "if (globalThis.__snapshotMarker !== 'warm') { throw new Error(`saw ${globalThis.__snapshotMarker}`); }",
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    Ok(())
}

fn assert_snapshot_rebuild_on_bridge_change() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;
    let bridge_a = "(function() { globalThis.__bridgeSnapshot = 'A'; })();";
    let bridge_b = "(function() { globalThis.__bridgeSnapshot = 'B'; })();";

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        bridge_a,
        "if (globalThis.__bridgeSnapshot !== 'A') { throw new Error(`saw ${globalThis.__bridgeSnapshot}`); }",
    )?;
    assert_execution_ok(&receiver, &session_id);

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        bridge_b,
        "if (globalThis.__bridgeSnapshot !== 'B') { throw new Error(`saw ${globalThis.__bridgeSnapshot}`); }",
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    Ok(())
}

fn assert_execute_rejects_oversized_bridge_code() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;
    let oversized_bridge_code = " ".repeat(16 * 1024 * 1024 + 1);

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        &oversized_bridge_code,
        "globalThis.__should_not_run = true;",
    )?;

    let event = wait_for_execution_result(&receiver, &session_id);
    match event {
        RuntimeEvent::ExecutionResult {
            exit_code,
            error: Some(error),
            ..
        } => {
            assert_eq!(exit_code, 1);
            assert_eq!(error.code, "ERR_V8_BRIDGE_CODE_LIMIT");
            assert!(error
                .message
                .contains("bridge code too large for V8 bridge setup"));
        }
        other => panic!("expected bridge-code limit execution error, got {other:?}"),
    }

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until(
        "expected oversized-bridge session to drain after rejection",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_direct_zero_cpu_time_limit_disables_timeout() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let session_id = next_session_id();
    let receiver = register_and_create_session_with_cpu_time_limit(&runtime, &session_id, Some(0))?;

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "let total = 0; for (let i = 0; i < 100000; i++) { total += i; }",
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until(
        "expected zero-timeout session to drain after successful execution",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_overload_rejects_before_thread_and_recovers_after_release() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let session_a = next_session_id();
    let session_b = next_session_id();
    let receiver_a = register_and_create_session(&runtime, &session_a)?;

    wait_until(
        "expected the first embedded session to occupy the only slot before the second session is created",
        || runtime.active_slot_count() == 1 && runtime.session_count() == 1,
    );

    dispatch_execute(
        runtime.as_ref(),
        &session_a,
        0,
        "",
        "_loadFileSync('/overload-slot-holder');",
    )?;
    let bridge_call = wait_for_bridge_call(&receiver_a, &session_a);
    assert!(
        matches!(
            bridge_call,
            RuntimeEvent::BridgeCall { ref method, .. } if method == "_loadFileSync"
        ),
        "expected the slot-holder execution to reach a synchronous bridge wait"
    );

    let _rejected_receiver = runtime.register_session(&session_b)?;
    let overload = runtime
        .dispatch(RuntimeCommand::CreateSession {
            session_id: session_b.clone(),
            heap_limit_mb: None,
            cpu_time_limit_ms: None,
            wall_clock_limit_ms: None,
            limit_external_memory: false,
            warm_hint: None,
        })
        .expect_err("executor saturation must reject before spawning another VM thread");
    assert!(
        overload
            .to_string()
            .contains("ERR_AGENTOS_VM_EXECUTOR_LIMIT"),
        "unexpected executor overload error: {overload}"
    );
    runtime.unregister_session(&session_b);
    assert_eq!(runtime.active_slot_count(), 1);
    assert_eq!(runtime.session_count(), 1);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_a.clone(),
    })?;
    let terminated = wait_for_execution_result(&receiver_a, &session_a);
    assert!(
        matches!(
            terminated,
            RuntimeEvent::ExecutionResult {
                exit_code: 1,
                ref error,
                ..
            } if error.as_ref().is_some_and(|error| error.message == "Execution terminated")
        ),
        "destroying the in-flight session should terminate its pending execution"
    );

    wait_until(
        "expected the terminated session to release its executor slot",
        || runtime.active_slot_count() == 0 && runtime.session_count() == 0,
    );

    let receiver_b = register_and_create_session(&runtime, &session_b)?;
    dispatch_execute(
        runtime.as_ref(),
        &session_b,
        0,
        "(function() { globalThis.__successorSession = 'released'; })();",
        "if (globalThis.__successorSession !== 'released') { throw new Error(`saw ${globalThis.__successorSession}`); }",
    )?;
    assert_execution_ok(&receiver_b, &session_b);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_b.clone(),
    })?;
    runtime.unregister_session(&session_a);
    runtime.unregister_session(&session_b);
    wait_until(
        "expected all embedded sessions and slots to drain after teardown",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_default_executor_admission_is_uncapped() -> io::Result<()> {
    let runtime = Arc::new(default_embedded_runtime()?);
    let first = next_session_id();
    let second = next_session_id();
    let _first_receiver = register_and_create_session(&runtime, &first)?;
    let _second_receiver = register_and_create_session(&runtime, &second)?;
    assert_eq!(runtime.active_slot_count(), 2);

    for session_id in [&first, &second] {
        runtime.dispatch(RuntimeCommand::DestroySession {
            session_id: session_id.clone(),
        })?;
        runtime.unregister_session(session_id);
    }
    Ok(())
}

fn assert_shared_runtime_handles_share_concurrency_quota() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(3)?);
    let clients = (0..4)
        .map(|_| Arc::clone(&runtime))
        .collect::<Vec<Arc<EmbeddedV8Runtime>>>();
    let session_ids = (0..4).map(|_| next_session_id()).collect::<Vec<_>>();
    let mut receivers = clients
        .iter()
        .zip(session_ids.iter())
        .take(3)
        .map(|(client, session_id)| register_and_create_session(client, session_id))
        .collect::<io::Result<Vec<_>>>()?;

    wait_until(
        "expected the first three embedded sessions to occupy the shared slots before the fourth session is created",
        || runtime.active_slot_count() == 3 && runtime.session_count() == 3,
    );

    for ((client, session_id), receiver) in clients
        .iter()
        .zip(session_ids.iter())
        .take(3)
        .zip(receivers.iter())
    {
        dispatch_execute(
            client.as_ref(),
            session_id,
            0,
            "",
            &format!("_loadFileSync('/shared-slot-holder-{session_id}');"),
        )?;
        let bridge_call = wait_for_bridge_call(receiver, session_id);
        assert!(
            matches!(
                bridge_call,
                RuntimeEvent::BridgeCall { ref method, .. } if method == "_loadFileSync"
            ),
            "expected each shared slot-holder to reach a synchronous bridge wait"
        );
    }
    let _rejected_receiver = clients[3].register_session(&session_ids[3])?;
    let overload = clients[3]
        .dispatch(RuntimeCommand::CreateSession {
            session_id: session_ids[3].clone(),
            heap_limit_mb: None,
            cpu_time_limit_ms: None,
            wall_clock_limit_ms: None,
            limit_external_memory: false,
            warm_hint: None,
        })
        .expect_err("the shared executor limit must reject a fourth VM before thread creation");
    assert!(
        overload
            .to_string()
            .contains("ERR_AGENTOS_VM_EXECUTOR_LIMIT"),
        "unexpected shared executor overload error: {overload}"
    );
    clients[3].unregister_session(&session_ids[3]);
    assert_eq!(runtime.active_slot_count(), 3);
    assert_eq!(runtime.session_count(), 3);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_ids[0].clone(),
    })?;
    let terminated = wait_for_execution_result(&receivers[0], &session_ids[0]);
    assert!(
        matches!(
            terminated,
            RuntimeEvent::ExecutionResult {
                exit_code: 1,
                ref error,
                ..
            } if error.as_ref().is_some_and(|error| error.message == "Execution terminated")
        ),
        "destroying one in-flight session should release a shared executor slot"
    );

    wait_until("expected the shared executor slot to be released", || {
        runtime.active_slot_count() == 2 && runtime.session_count() == 2
    });
    receivers.push(register_and_create_session(&clients[3], &session_ids[3])?);
    dispatch_execute(
        clients[3].as_ref(),
        &session_ids[3],
        0,
        "(function() { globalThis.__sharedQuota = 'released'; })();",
        "if (globalThis.__sharedQuota !== 'released') { throw new Error(`saw ${globalThis.__sharedQuota}`); }",
    )?;
    assert_execution_ok(&receivers[3], &session_ids[3]);

    for session_id in session_ids.iter().skip(1) {
        runtime.dispatch(RuntimeCommand::DestroySession {
            session_id: session_id.clone(),
        })?;
    }
    for session_id in &session_ids {
        runtime.unregister_session(session_id);
    }
    wait_until(
        "expected all shared-runtime sessions and slots to drain after teardown",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_terminate_interrupts_sync_bridge_wait() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "_loadFileSync('/never-responds');",
    )?;

    let bridge_call = wait_for_bridge_call(&receiver, &session_id);
    assert!(
        matches!(
            bridge_call,
            RuntimeEvent::BridgeCall { ref method, .. } if method == "_loadFileSync"
        ),
        "expected the blocked sync bridge call to be visible before termination"
    );

    let session = runtime.session_handle(session_id.clone());
    let mut overload = None;
    for index in 0..=256 {
        match session.send_stream_event(&format!("terminate-flood-{index}"), Vec::new()) {
            Ok(()) => {}
            Err(error) => {
                overload = Some(error);
                break;
            }
        }
    }
    let overload = overload.expect("termination flood must reach typed command backpressure");
    assert!(
        overload
            .to_string()
            .contains("ERR_AGENTOS_SESSION_COMMAND_LIMIT"),
        "unexpected termination-flood overload: {overload}"
    );

    let terminate_started = Instant::now();
    session.terminate()?;
    let terminated = wait_for_execution_result(&receiver, &session_id);

    if run_timing_sensitive_tests() {
        assert!(
            terminate_started.elapsed() < Duration::from_secs(1),
            "terminate() should return promptly while the sync bridge call is blocked"
        );
    }
    assert!(
        matches!(
            terminated,
            RuntimeEvent::ExecutionResult {
                exit_code: 1,
                ref error,
                ..
            } if error.as_ref().is_some_and(|error| error.message == "Execution terminated")
        ),
        "terminate() should interrupt a blocked sync bridge call instead of waiting for a host response"
    );

    dispatch_execute_after_backpressure(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "globalThis.__afterExplicitTerminate = 'ok';",
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until(
        "expected the terminated sync-bridge session to drain cleanly",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_pause_preserves_synchronous_execution_stack() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;
    let handle = runtime.session_handle(session_id.clone());

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "_loadFileSync('/before-pause'); _loadFileSync('/after-resume');",
    )?;

    let first_call = wait_for_bridge_call(&receiver, &session_id);
    let first_call_id = match first_call {
        RuntimeEvent::BridgeCall { call_id, .. } => call_id,
        other => panic!("expected first bridge call, got {other:?}"),
    };
    handle.pause()?;
    handle.send_bridge_response(first_call_id, 0, Vec::new())?;

    let event_while_paused = receiver.recv_timeout(Duration::from_millis(100));
    handle.resume()?;
    assert!(
        event_while_paused.is_err(),
        "paused synchronous execution must not advance to its next host call"
    );

    let second_call = wait_for_bridge_call(&receiver, &session_id);
    let second_call_id = match second_call {
        RuntimeEvent::BridgeCall { call_id, .. } => call_id,
        other => panic!("expected second bridge call, got {other:?}"),
    };
    handle.send_bridge_response(second_call_id, 0, Vec::new())?;
    assert_execution_ok(&receiver, &session_id);

    handle.destroy()?;
    runtime.unregister_session(&session_id);
    wait_until("expected resumed session to drain cleanly", || {
        runtime.session_count() == 0 && runtime.active_slot_count() == 0
    });
    Ok(())
}

fn assert_sync_bridge_response_bypasses_stream_event_flood() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "_loadFileSync.applySyncPromise(void 0, ['/synthetic-stream-flood']);",
    )?;

    let bridge_call = wait_for_bridge_call(&receiver, &session_id);
    let call_id = match bridge_call {
        RuntimeEvent::BridgeCall {
            call_id, method, ..
        } if method == "_loadFileSync" => call_id,
        other => panic!(
            "expected guest JavaScript to block in a real sync bridge call before the flood, got {other:?}"
        ),
    };

    let session = runtime.session_handle(session_id.clone());
    let mut overload = None;
    let mut accepted = 0;
    for index in 0..=256 {
        match session.send_stream_event(&format!("net-socket-{index}"), Vec::new()) {
            Ok(()) => accepted += 1,
            Err(error) => {
                overload = Some(error);
                break;
            }
        }
    }
    assert_eq!(
        accepted, 256,
        "the configured ordinary lane must admit 256 events"
    );
    let overload = overload.expect("ordinary event flood must reach typed backpressure");
    assert!(
        overload
            .to_string()
            .contains("ERR_AGENTOS_SESSION_COMMAND_LIMIT"),
        "unexpected ordinary-lane overload: {overload}"
    );

    session.send_bridge_response(call_id, 0, Vec::new())?;
    assert_execution_ok(&receiver, &session_id);

    dispatch_execute_after_backpressure(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "globalThis.__afterBridgeFlood = 'ok';",
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until(
        "expected bridge-flood regression session to drain cleanly",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_sync_bridge_response_bypasses_full_ordinary_command_lane() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "_loadFileSync.applySyncPromise(void 0, ['/ordinary-lane-full']);",
    )?;
    let call_id = match wait_for_bridge_call(&receiver, &session_id) {
        RuntimeEvent::BridgeCall { call_id, .. } => call_id,
        other => panic!("expected bridge call, got {other:?}"),
    };

    let session = runtime.session_handle(session_id.clone());
    // Fill the entire ordinary 256-entry session command lane with messages
    // that cannot coalesce. Response settlement must not need a slot there.
    let mut overload = None;
    for index in 0..=256 {
        match session.send_stream_event(&format!("ordinary-{index}"), Vec::new()) {
            Ok(()) => {}
            Err(error) => {
                overload = Some(error);
                break;
            }
        }
    }
    let overload = overload.expect("ordinary command lane must become full");
    assert!(
        overload
            .to_string()
            .contains("ERR_AGENTOS_SESSION_COMMAND_LIMIT"),
        "unexpected ordinary-lane overload: {overload}"
    );
    for _ in 0..1_024 {
        session.publish_signal(10, 41)?;
    }
    session.send_bridge_response(call_id, 0, Vec::new())?;
    assert_execution_ok(&receiver, &session_id);

    session.destroy()?;
    wait_until(
        "expected full-command-lane regression session to drain cleanly",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_cpu_terminated_session_can_execute_again() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let session_id = next_session_id();
    let receiver =
        register_and_create_session_with_cpu_time_limit(&runtime, &session_id, Some(25))?;

    dispatch_execute(runtime.as_ref(), &session_id, 0, "", "while (true) {}")?;
    let terminated = wait_for_execution_result(&receiver, &session_id);
    assert!(
        matches!(
            terminated,
            RuntimeEvent::ExecutionResult {
                exit_code: 1,
                ref error,
                ..
            } if error
                .as_ref()
                .is_some_and(|error| error.code == "ERR_SCRIPT_CPU_BUDGET_EXCEEDED")
        ),
        "CPU-budget termination should be attributed before reuse"
    );

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "globalThis.__afterCpuTerminate = 'ok';",
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until(
        "expected CPU-terminated session to drain cleanly after reuse",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_isolate_churn_recreates_embedded_sessions_without_segv() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let bridge_code = "(function() { globalThis.__churnBridgeReady = true; })();";

    for _ in 0..32 {
        let session_id = next_session_id();
        let receiver = register_and_create_session(&runtime, &session_id)?;
        dispatch_execute(
            runtime.as_ref(),
            &session_id,
            0,
            bridge_code,
            "if (globalThis.__churnBridgeReady !== true) { throw new Error('missing bridge'); }",
        )?;
        assert_execution_ok(&receiver, &session_id);
        runtime.dispatch(RuntimeCommand::DestroySession {
            session_id: session_id.clone(),
        })?;
        runtime.unregister_session(&session_id);
        wait_until(
            "quarantined executor must release its slot before its successor starts",
            || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
        );
    }

    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;
    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        bridge_code,
        "globalThis.__afterChurn = 42;",
    )?;
    assert_execution_ok(&receiver, &session_id);
    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until("expected isolate churn sessions to drain", || {
        runtime.session_count() == 0 && runtime.active_slot_count() == 0
    });
    Ok(())
}

fn assert_destroy_quarantines_active_handle_executor() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(1)?);
    let session_id = next_session_id();
    let _receiver = register_and_create_session(&runtime, &session_id)?;
    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "setInterval(() => {}, 1_000);",
    )?;
    thread::sleep(Duration::from_millis(20));

    let started = Instant::now();
    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "active-handle destruction must observe abort before re-entering V8"
    );
    runtime.unregister_session(&session_id);
    wait_until(
        "active-handle destruction must release its quarantined executor",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

#[test]
fn embedded_runtime_session_consolidated_behaviors() -> io::Result<()> {
    // This integration test is its own process entrypoint. Production
    // subsystems may retrieve, but never lazily construct, the process runtime.
    process_runtime_context()?;
    // Keep the embedded-runtime coverage in one test process. V8 teardown across
    // multiple integration tests still trips intermittent SIGSEGVs in this crate.
    assert_create_destroy_reuses_session_ids()?;
    assert_stale_session_handle_cannot_settle_reused_session_call()?;
    assert_warmed_snapshot_bridge_state()?;
    assert_snapshot_rebuild_on_bridge_change()?;
    assert_execute_rejects_oversized_bridge_code()?;
    assert_direct_zero_cpu_time_limit_disables_timeout()?;
    assert_default_executor_admission_is_uncapped()?;
    assert_overload_rejects_before_thread_and_recovers_after_release()?;
    assert_shared_runtime_handles_share_concurrency_quota()?;
    assert_sync_bridge_response_bypasses_stream_event_flood()?;
    assert_sync_bridge_response_bypasses_full_ordinary_command_lane()?;
    assert_terminate_interrupts_sync_bridge_wait()?;
    assert_pause_preserves_synchronous_execution_stack()?;
    assert_cpu_terminated_session_can_execute_again()?;
    assert_isolate_churn_recreates_embedded_sessions_without_segv()?;
    assert_destroy_quarantines_active_handle_executor()?;
    oversized_heap_allocation_reports_trusted_limit_and_preserves_runtime()?;
    Ok(())
}

fn oversized_heap_allocation_reports_trusted_limit_and_preserves_runtime() -> io::Result<()> {
    let runtime = Arc::new(embedded_runtime(2)?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;
    dispatch_execute(
        &runtime,
        &session_id,
        0,
        "",
        "'a'.repeat(256 * 1024 * 1024).toUpperCase().length",
    )?;
    let result = wait_for_execution_result(&receiver, &session_id);
    assert!(
        matches!(
            result,
            RuntimeEvent::ExecutionResult {
                exit_code: 1,
                limit_exceeded: Some(
                    agentos_executor_v8_runtime::ipc_binary::ExecutionLimit::V8Heap
                ),
                ..
            }
        ),
        "expected trusted heap limit result: {result:?}"
    );
    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    let healthy_id = next_session_id();
    let healthy = register_and_create_session(&runtime, &healthy_id)?;
    dispatch_execute(&runtime, &healthy_id, 0, "", "1 + 1")?;
    assert_execution_ok(&healthy, &healthy_id);
    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: healthy_id.clone(),
    })?;
    runtime.unregister_session(&healthy_id);
    Ok(())
}

/// Exercise captured host functions in production session code, in a subprocess
/// so a native lifetime regression cannot corrupt the rest of the test runner.
#[test]
fn retained_bridge_functions_use_current_execution() -> io::Result<()> {
    const CHILD: &str = "AGENTOS_RETAINED_BRIDGE_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "retained_bridge_functions_use_current_execution",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = child.try_wait()? {
                assert!(
                    status.success(),
                    "retained bridge subprocess failed: {status}"
                );
                return Ok(());
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                panic!("retained bridge subprocess exceeded 30 seconds");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    let runtime = Arc::new(embedded_runtime(2)?);
    let mut sessions = Vec::new();
    for _ in 0..2 {
        let id = next_session_id();
        let receiver = register_and_create_session(&runtime, &id)?;
        dispatch_execute(
            &runtime,
            &id,
            2,
            "",
            r#"
            globalThis.savedSync = _loadFileSync;
            globalThis.savedSyncApply = _loadFileSync.applySync;
            globalThis.savedAsync = _kernelPoll;
            globalThis.savedAsyncApply = _kernelPoll.apply;
        "#,
        )?;
        assert_execution_ok(&receiver, &id);
        sessions.push((id, receiver));
    }
    // A retained global may be protected or accessor-backed by guest code.
    // Bridge installation happens only once, preserving captured identities and
    // avoiding invoking a guest setter during the next host operation.
    dispatch_execute(
        &runtime,
        &sessions[0].0,
        2,
        "",
        r#"
        Object.defineProperty(globalThis, '_loadFileSync', {
            value: savedSync, writable: false, configurable: false
        });
        Object.defineProperty(globalThis, '_kernelPoll', {
            get() { return savedAsync; },
            set() { throw Error('bridge global must not be reinstalled'); },
            configurable: false
        });
    "#,
    )?;
    assert_execution_ok(&sessions[0].1, &sessions[0].0);
    let mut call_ids = std::collections::HashSet::new();
    for _ in 0..2 {
        for (id, receiver) in &sessions {
            dispatch_execute(
                &runtime,
                id,
                2,
                "",
                r#"
                savedSync('/retained');
                savedSyncApply(null, ['/retained-alias']);
                savedAsync('retained').then(() => savedAsyncApply(null, ['retained-alias']));
            "#,
            )?;
            for expected_method in [
                "_loadFileSync",
                "_loadFileSync",
                "_kernelPoll",
                "_kernelPoll",
            ] {
                match wait_for_bridge_call(receiver, id) {
                    RuntimeEvent::BridgeCall {
                        call_id,
                        method,
                        session_id,
                        ..
                    } => {
                        assert_eq!(&session_id, id);
                        assert_eq!(method, expected_method);
                        assert!(
                            call_ids.insert(call_id),
                            "call IDs must not repeat across executions"
                        );
                        runtime.session_handle(id.clone()).send_bridge_response(
                            call_id,
                            0,
                            Vec::new(),
                        )?;
                    }
                    event => panic!("expected retained bridge call, got {event:?}"),
                }
            }
            assert_execution_ok(receiver, id);
        }
    }
    // Both retained callback kinds must use the new abort receiver and response
    // lane after termination; a late response from the prior operation must not
    // settle the next operation's resolver.
    let (id, receiver) = &sessions[0];
    let handle = runtime.session_handle(id.clone());
    for blocked_script in ["savedSync('/cancelled')", "savedAsync('cancelled')"] {
        dispatch_execute(&runtime, id, 2, "", blocked_script)?;
        let old_call = match wait_for_bridge_call(receiver, id) {
            RuntimeEvent::BridgeCall { call_id, .. } => call_id,
            event => panic!("expected cancellable retained bridge call, got {event:?}"),
        };
        handle.terminate()?;
        assert!(matches!(
            wait_for_execution_result(receiver, id),
            RuntimeEvent::ExecutionResult { exit_code: 1, .. }
        ));
        dispatch_execute(&runtime, id, 2, "", "savedAsync('after-cancel')")?;
        let current_call = match wait_for_bridge_call(receiver, id) {
            RuntimeEvent::BridgeCall { call_id, .. } => call_id,
            event => panic!("expected recovered retained bridge call, got {event:?}"),
        };
        assert_ne!(old_call, current_call);
        assert!(
            handle
                .send_bridge_response(old_call, 0, Vec::new())
                .is_err(),
            "cancelled operation must reject late responses"
        );
        handle.send_bridge_response(current_call, 0, Vec::new())?;
        assert_execution_ok(receiver, id);
    }
    for (id, _) in sessions {
        runtime.session_handle(id.clone()).destroy()?;
        runtime.unregister_session(&id);
    }
    wait_until("retained sessions must release executor slots", || {
        runtime.active_slot_count() == 0
    });
    Ok(())
}

/// Use the production Execute branch while withholding stdin replies, so EOF
/// scheduling cannot hide an abandoned read across retained operations.
#[test]
fn retained_stdin_read_restarts_on_current_operation_after_pause_and_abort() -> io::Result<()> {
    fn expect_module_ok(receiver: &RuntimeEventOutputReceiver, session_id: &str) {
        let event = wait_for_execution_result(receiver, session_id);
        assert!(
            matches!(
                event,
                RuntimeEvent::ExecutionResult {
                    exit_code: 0,
                    error: None,
                    ..
                }
            ),
            "retained stdin module failed: {event:?}",
        );
    }
    const CHILD: &str = "AGENTOS_RETAINED_STDIN_LIVENESS_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "retained_stdin_read_restarts_on_current_operation_after_pause_and_abort",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = child.try_wait()? {
                assert!(
                    status.success(),
                    "retained stdin subprocess failed: {status}"
                );
                return Ok(());
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                panic!("retained stdin subprocess exceeded 30 seconds");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    let runtime = Arc::new(embedded_runtime(1)?);
    let id = next_session_id();
    let receiver = register_and_create_session(&runtime, &id)?;
    let handle = runtime.session_handle(id.clone());
    // The JS source fixture models the pump's operation-generation contract;
    // the build-tools source tests exercise the actual stdin implementation.
    dispatch_execute(
        &runtime,
        &id,
        3,
        "",
        r#"
        globalThis.stdinEpoch = 0;
        globalThis.stdinRunning = false;
        globalThis.stdinBytes = [3];
        globalThis._retireStdinOperation = () => {
            stdinEpoch++;
            stdinRunning = false;
        };
        globalThis.startStdin = () => {
            if (stdinRunning) throw Error('previous stdin pump remained started');
            stdinRunning = true;
            const epoch = stdinEpoch;
            return _kernelStdinRead(65536, null).then(bytes => {
                if (epoch !== stdinEpoch) throw Error('old continuation executed');
                stdinBytes.push(...bytes);
                stdinRunning = false;
            });
        };
        startStdin();
        "#,
    )?;
    let old_read = match wait_for_bridge_call(&receiver, &id) {
        RuntimeEvent::BridgeCall {
            call_id, method, ..
        } => {
            assert_eq!(method, "_kernelStdinRead");
            call_id
        }
        event => panic!("expected withheld stdin read, got {event:?}"),
    };
    // Complete-on-settle must retire an idle read without waiting for input.
    expect_module_ok(&receiver, &id);
    dispatch_execute_after_backpressure(
        &runtime,
        &id,
        3,
        "",
        "if (stdinEpoch !== 1 || stdinBytes.join() !== '3') throw Error('retirement lost state'); await startStdin(); if (stdinBytes.join() !== '3,7,9') throw Error('lost current input');",
    )?;
    let current_read = match wait_for_bridge_call(&receiver, &id) {
        RuntimeEvent::BridgeCall { call_id, .. } => call_id,
        event => panic!("expected resumed current stdin read, got {event:?}"),
    };
    assert_ne!(old_read, current_read);
    assert!(handle.send_bridge_response(old_read, 2, vec![99]).is_err());
    handle.send_bridge_response(current_read, 2, vec![7, 9])?;
    expect_module_ok(&receiver, &id);

    dispatch_execute_after_backpressure(&runtime, &id, 3, "", "await startStdin()")?;
    let aborted_read = match wait_for_bridge_call(&receiver, &id) {
        RuntimeEvent::BridgeCall { call_id, .. } => call_id,
        event => panic!("expected abortable stdin read, got {event:?}"),
    };
    handle.terminate()?;
    assert!(matches!(
        wait_for_execution_result(&receiver, &id),
        RuntimeEvent::ExecutionResult { exit_code: 1, .. }
    ));
    dispatch_execute_after_backpressure(
        &runtime,
        &id,
        3,
        "",
        "if (stdinEpoch !== 3) throw Error('aborted pump was not retired'); await startStdin(); if (stdinBytes.join() !== '3,7,9,11') throw Error('old input leaked');",
    )?;
    let recovered_read = match wait_for_bridge_call(&receiver, &id) {
        RuntimeEvent::BridgeCall { call_id, .. } => call_id,
        event => panic!("expected new route after abort, got {event:?}"),
    };
    assert_ne!(aborted_read, recovered_read);
    assert!(handle
        .send_bridge_response(aborted_read, 2, vec![99])
        .is_err());
    handle.send_bridge_response(recovered_read, 2, vec![11])?;
    expect_module_ok(&receiver, &id);
    handle.destroy()?;
    runtime.unregister_session(&id);
    wait_until(
        "retained stdin session must release its executor slot",
        || runtime.active_slot_count() == 0,
    );
    Ok(())
}

#[test]
#[cfg(feature = "test-support")]
fn bridge_policy_new_sessions_claim_warm_pool_after_prior_owner_retires() -> io::Result<()> {
    use agentos_executor_v8_runtime::runtime_protocol::{GuestBridgePolicy, WarmSessionHint};
    let runtime = Arc::new(embedded_runtime(1)?);
    let bridge = agentos_executor_v8_runtime::adapter_host::V8RuntimeHost::bridge_code().to_owned();
    for owner in ["bridge-pool-owner-a", "bridge-pool-owner-b"] {
        runtime.pre_warm_workers(bridge.clone(), String::new(), None, 1);
        wait_until("warm worker must be parked before session claim", || {
            !runtime.test_parked_worker_ids().is_empty()
        });
        let parked = runtime.test_parked_worker_ids();
        assert!(
            !parked.is_empty(),
            "control requires an actual parked worker"
        );
        let id = next_session_id();
        let session_runtime = vm_runtime_context(&id)?;
        let (receiver, _registration) =
            runtime.register_session_with_runtime(&id, &session_runtime)?;
        runtime.dispatch_create_session_with_runtime(
            RuntimeCommand::CreateSession {
                session_id: id.clone(),
                heap_limit_mb: None,
                cpu_time_limit_ms: None,
                wall_clock_limit_ms: None,
                limit_external_memory: false,
                warm_hint: Some(WarmSessionHint {
                    bridge_code: bridge.clone(),
                    userland_code: String::new(),
                    heap_limit_mb: None,
                }),
            },
            session_runtime,
            TEST_REACTOR_WORK_QUANTUM,
            Duration::from_secs(30),
        )?;
        let body = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> io::Result<()> {
            let actual = runtime
                .test_session_worker_id(&id)
                .expect("assigned session worker");
            assert!(
                parked.contains(&actual),
                "session must claim a real parked worker"
            );
            let policy = GuestBridgePolicy {
                vm_id: owner.into(),
                limits: Default::default(),
            };
            for _ in 0..2 {
                runtime.dispatch(RuntimeCommand::SendToSession {session_id:id.clone(),message:SessionMessage::Execute {
                    mode:2,file_path:String::new(),bridge_code:bridge.clone(),post_restore_script:String::new(),
                    userland_code:String::new(),high_resolution_time:false,
                    user_code:"if(typeof __agentOSBridgeLimitInstaller!=='undefined')throw Error('installer escaped');".into(),
                    wasm_module_bytes:None,guest_bridge_policy:Some(policy.clone()),
                }})?;
                assert_execution_ok(&receiver, &id);
            }
            let mut foreign = policy.clone();
            foreign.vm_id = "bridge-pool-foreign-owner".into();
            runtime.dispatch(RuntimeCommand::SendToSession {
                session_id: id.clone(),
                message: SessionMessage::Execute {
                    mode: 2,
                    file_path: String::new(),
                    bridge_code: format!("{bridge}\n// changed-source owner rejection control"),
                    post_restore_script: String::new(),
                    userland_code: String::new(),
                    high_resolution_time: false,
                    user_code: "throw Error('foreign guest must not run')".into(),
                    wasm_module_bytes: None,
                    guest_bridge_policy: Some(foreign),
                },
            })?;
            match wait_for_execution_result(&receiver, &id) {
                RuntimeEvent::ExecutionResult {
                    exit_code: 1,
                    error: Some(error),
                    ..
                } => assert_eq!(error.code, "ERR_AGENTOS_INVALID_BRIDGE_POLICY"),
                other => panic!("changed-owner/source Execute unexpectedly accepted: {other:?}"),
            }
            runtime.dispatch(RuntimeCommand::SendToSession {session_id:id.clone(),message:SessionMessage::Execute {
                mode:2,file_path:String::new(),bridge_code:bridge.clone(),post_restore_script:String::new(),
                userland_code:String::new(),high_resolution_time:false,user_code:"if(typeof __agentOSBridgeLimitInstaller!=='undefined')throw Error('installer escaped');".into(),
                wasm_module_bytes:None,guest_bridge_policy:Some(policy),
            }})?;
            assert_execution_ok(&receiver, &id);
            Ok(())
        }));
        let cleanup = runtime.dispatch(RuntimeCommand::DestroySession {
            session_id: id.clone(),
        });
        runtime.unregister_session(&id);
        assert!(
            cleanup.is_ok(),
            "session cleanup failed: {cleanup:?}; body panicked={}",
            body.is_err()
        );
        wait_until(
            "prior logical session must retire before the next owner",
            || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
        );
        assert!(runtime.test_session_worker_id(&id).is_none());
        match body {
            Ok(result) => result?,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
    Ok(())
}
