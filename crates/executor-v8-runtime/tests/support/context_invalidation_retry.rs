#[cfg(test)]
pub(crate) fn context_invalidation_retry_controls() {
    let lifecycle = JavascriptExecutionLifecycle::default();
    lifecycle.context_invalidated.store(true, Ordering::Release);
    let attempts = std::cell::Cell::new(0);
    let failed = lifecycle.transfer_teardown(|| {
        attempts.set(attempts.get() + 1);
        Err(std::io::Error::other("injected detach admission failure"))
    });
    assert!(failed.is_err());
    assert!(!*lifecycle.teardown_confirmed.lock().unwrap());
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
}
