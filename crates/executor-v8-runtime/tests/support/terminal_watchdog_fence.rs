// Real guard cancel and the same firing helper used by wall/CPU runtime tasks.
#[cfg(test)]
pub(crate) fn terminal_watchdog_fence_controls() {
    let gate = Arc::new(Mutex::new(false));
    let fired = Arc::new(AtomicBool::new(false));
    let mut guard = TimeoutGuard {
        fired: fired.clone(),
        fire_cancel_gate: gate.clone(),
        task: None,
    };
    guard.cancel();
    fire_watchdog(&gate, &fired, || {
        panic!("cancel won: no later firing effects")
    });
    assert!(!guard.timed_out());

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
    let handle = scope.thread_safe_handle();
    let gate = Arc::new(Mutex::new(false));
    let fired = Arc::new(AtomicBool::new(false));
    let guard = TimeoutGuard {
        fired: fired.clone(),
        fire_cancel_gate: gate.clone(),
        task: None,
    };
    let (entered_tx, entered_rx) = crossbeam_channel::bounded(1);
    let (release_tx, release_rx) = crossbeam_channel::bounded(1);
    let effect_completed = Arc::new(AtomicBool::new(false));
    let firing_completed = effect_completed.clone();
    let firing = std::thread::spawn(move || {
        fire_watchdog(&gate, &fired, || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            handle.terminate_execution();
            firing_completed.store(true, Ordering::SeqCst);
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let (started_tx, started_rx) = crossbeam_channel::bounded(1);
    let cancelling = std::thread::spawn(move || {
        let mut guard = guard;
        started_tx.send(()).unwrap();
        guard.cancel();
        assert!(guard.timed_out());
        assert!(
            effect_completed.load(Ordering::SeqCst),
            "fire won: cancellation returns after firing effect"
        );
    });
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    release_tx.send(()).unwrap();
    firing.join().unwrap();
    cancelling.join().unwrap();
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
    scope.cancel_terminate_execution();
    let value = script
        .run(scope)
        .expect("cancelled termination permits clean execution");
    assert_eq!(value.int32_value(scope), Some(499500));
}
