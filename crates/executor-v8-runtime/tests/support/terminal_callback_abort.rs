// Invoked from the single V8 suite; no extra V8 test/platform lifetime.
#[cfg(test)]
pub(crate) fn terminal_callback_abort_controls() {
    owned_queue_abort_fence_controls();
    native_process_exit_abort_controls();
    use std::time::Duration;
    let mut isolate = crate::isolate::create_isolate(None);
    // Compile before requesting termination; execution, not the asynchronous
    // request flag, proves interruption at V8's actual interrupt check.
    v8::scope!(scope, &mut *isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let source = v8::String::new(
        scope,
        "(()=>{let sum=0;for(let i=0;i<1000;i++)sum+=i;return sum})()",
    )
    .unwrap();
    let script = v8::Script::compile(scope, source, None).unwrap();
    let abort = new_execution_abort();
    let (_active, _rx) = ActiveExecutionAbort::arm(&abort);
    // External publication first: the owned cancellation must leave its stop set.
    signal_execution_abort(&abort, ExecutionAbortReason::Terminated);
    scope.terminate_execution();
    assert!(!cancel_owned_callback_stop(scope, &abort));
    {
        v8::tc_scope!(tc, scope);
        assert!(
            script.run(tc).is_none(),
            "external termination interrupts finite work"
        );
        assert!(
            tc.has_terminated(),
            "failure is termination, not a guest exception"
        );
    }
    assert!(matches!(
        execution_abort_reason(&abort),
        Some(ExecutionAbortReason::Terminated)
    ));
    scope.cancel_terminate_execution(); // Test cleanup of the external stop.
    let value = script
        .run(scope)
        .expect("cancelled termination permits clean execution");
    assert_eq!(value.int32_value(scope), Some(499500));

    let abort = new_execution_abort();
    let (_active, _rx) = ActiveExecutionAbort::arm(&abort);
    scope.terminate_execution(); // The callback-owned stop.
    let external_abort = abort.clone();
    let handle = scope.thread_safe_handle();
    let (start_tx, start_rx) = crossbeam_channel::bounded(1);
    let (done_tx, done_rx) = crossbeam_channel::bounded(1);
    let external = std::thread::spawn(move || {
        start_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("owned cancellation barrier");
        signal_execution_abort(&external_abort, ExecutionAbortReason::Terminated);
        handle.terminate_execution();
        done_tx.send(()).expect("external termination completion");
    });
    assert!(cancel_owned_callback_stop(scope, &abort));
    let value = script
        .run(scope)
        .expect("cancelled termination permits clean execution");
    assert_eq!(value.int32_value(scope), Some(499500));
    start_tx.send(()).unwrap();
    done_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("external stop reasserted after cancellation");
    external.join().unwrap();
    {
        v8::tc_scope!(tc, scope);
        assert!(
            script.run(tc).is_none(),
            "external termination interrupts finite work"
        );
        assert!(
            tc.has_terminated(),
            "failure is termination, not a guest exception"
        );
    }
    assert!(matches!(
        execution_abort_reason(&abort),
        Some(ExecutionAbortReason::Terminated)
    ));
    scope.cancel_terminate_execution(); // Test cleanup after proving precedence.
    let value = script
        .run(scope)
        .expect("cancelled termination permits clean execution");
    assert_eq!(value.int32_value(scope), Some(499500));
}

// A committed native exit does not cancel a distinct guard/teardown owner.
fn native_process_exit_abort_controls() {
    for reason in [
        ExecutionAbortReason::Terminated,
        ExecutionAbortReason::CpuBudgetExceeded,
        ExecutionAbortReason::WallClockTimedOut,
    ] {
        let mut isolate = crate::isolate::create_isolate(None);
        let context = crate::isolate::create_context(&mut isolate);
        let _call = crate::terminal_callback::begin_call(&mut isolate, 1);
        v8::scope!(scope, &mut *isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        crate::terminal_callback::install(scope).expect("terminal hooks installed");
        let abort = new_execution_abort();
        let (_active, _rx) = ActiveExecutionAbort::arm(&abort);
        let exit = v8::String::new(scope, "__agentOsRequestProcessExit(9)").unwrap();
        let exit = v8::Script::compile(scope, exit, None).unwrap();
        // Match the established external-abort control: finite work enters a
        // JavaScript function/loop rather than assuming constant evaluation polls.
        let finite = v8::String::new(
            scope,
            "(()=>{let sum=0;for(let i=0;i<1000;i++)sum+=i;return sum})()",
        )
        .unwrap();
        let finite = v8::Script::compile(scope, finite, None).unwrap();
        assert!(exit.run(scope).is_none(), "native exit interrupts its call");
        assert!(crate::terminal_callback::is_process_exit(scope));
        assert_eq!(crate::terminal_callback::result(scope).unwrap().0, 9);
        // Production teardown and watchdogs publish the durable reason first,
        // then request the independent V8 stop. Publication alone is not a
        // termination request; the prior top-level exit has already unwound.
        signal_execution_abort(&abort, reason);
        scope.terminate_execution();
        assert!(!cancel_owned_callback_stop(scope, &abort));
        assert_eq!(execution_abort_reason(&abort), Some(reason));
        {
            v8::tc_scope!(tc, scope);
            assert!(finite.run(tc).is_none());
            assert!(tc.has_terminated(), "external stop remains noncatchable");
        }
        // The session's unchanged final result selection gives this external
        // reason priority; clearing here is only fixture teardown.
        scope.cancel_terminate_execution();
        let value = finite.run(scope).unwrap();
        assert_eq!(value.int32_value(scope), Some(499500));
    }
}

#[cfg(test)]
fn owned_queue_abort_fence_controls() {
    let mut owner =
        crate::owned_microtasks::SessionIsolate::new(crate::isolate::create_isolate(None));
    let isolate = &mut *owner;
    let context = crate::owned_microtasks::create_root(isolate).unwrap();
    let queue = {
        v8::scope!(scope, &mut *isolate);
        v8::Local::new(scope, &context)
            .get_microtask_queue()
            .expect("attached test context") as *const v8::MicrotaskQueue
    };
    let abort = new_execution_abort();
    let (_active, _rx) = ActiveExecutionAbort::arm(&abort);
    signal_execution_abort(&abort, ExecutionAbortReason::Terminated);
    isolate.terminate_execution();
    assert!(!cancel_owned_callback_stop(isolate, &abort));
    let after = {
        v8::scope!(scope, &mut *isolate);
        v8::Local::new(scope, &context)
            .get_microtask_queue()
            .expect("attached test context") as *const v8::MicrotaskQueue
    };
    assert!(
        std::ptr::eq(queue, after),
        "external abort must not rotate the owned call queue"
    );
    assert!(matches!(
        execution_abort_reason(&abort),
        Some(ExecutionAbortReason::Terminated)
    ));
    isolate.cancel_terminate_execution(); // Test-only external-stop cleanup.
}
