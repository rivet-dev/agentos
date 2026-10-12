#[cfg(test)]
pub(crate) fn context_invalidation_retry_controls() {
    let lifecycle = JavascriptExecutionLifecycle::default();
    lifecycle.context_invalidated.store(true, Ordering::Release);
    assert_eq!(
        lifecycle.with_live_readiness(|| 1).unwrap(),
        Some(1),
        "invalidation is not confirmed teardown"
    );
    let attempts = std::cell::Cell::new(0);
    let failed = lifecycle.transfer_teardown(|| {
        attempts.set(attempts.get() + 1);
        Err(std::io::Error::other("injected detach admission failure"))
    });
    assert!(failed.is_err());
    assert!(!*lifecycle.teardown_confirmed.lock().unwrap());
    assert_eq!(
        lifecycle.with_live_readiness(|| 2).unwrap(),
        Some(2),
        "failed detach retains readiness ownership"
    );
    assert!(lifecycle.context_invalidated.load(Ordering::Acquire));
    lifecycle
        .transfer_teardown(|| {
            attempts.set(attempts.get() + 1);
            Ok(())
        })
        .unwrap();
    lifecycle
        .transfer_teardown(|| panic!("confirmed quarantine transfer is not dispatched twice"))
        .unwrap();
    assert_eq!(attempts.get(), 2);
    assert_eq!(
        lifecycle
            .with_live_readiness(|| panic!("retired readiness cannot be re-advertised"))
            .unwrap(),
        None::<()>
    );

    let poisoned = JavascriptExecutionLifecycle::default();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = poisoned.teardown_confirmed.lock().unwrap();
        panic!("inject teardown ownership poison");
    }))
    .is_err());
    let error = poisoned
        .with_live_readiness::<()>(|| panic!("unknown ownership must not advertise readiness"))
        .expect_err("poison must remain an explicit failure, not successful retirement");
    assert!(error
        .to_string()
        .contains("teardown ownership lock poisoned"));
}
