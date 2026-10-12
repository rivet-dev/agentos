// Invoked from the single V8 suite; no extra V8 test/platform lifetime.
#[cfg(test)]
pub(crate) fn terminal_callback_abort_controls() {
    use std::time::Duration;
    let mut isolate = crate::isolate::create_isolate(None);
    // Compile before requesting termination; execution, not the asynchronous
    // request flag, proves interruption at V8's actual interrupt check.
    let scope = &mut v8::HandleScope::new(&mut isolate);
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
        let tc = &mut v8::TryCatch::new(scope);
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
        let tc = &mut v8::TryCatch::new(scope);
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
