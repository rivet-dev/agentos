#[cfg(feature = "test-support")]
#[test]
fn internal_session_disposal_bounds_host_cleanup_without_abandoning_worker() {
    const CONNECTION: &str = "internal-cleanup-connection";
    const SESSION: &str = "internal-cleanup-session";
    // The ordinary manager constructor registers no executors. This real
    // WebAssembly-configured VM never launches guest work or snapshot prep.
    let mut manager = VmManager::new(LocalBridge::default()).unwrap();
    crate::test_support::register_session(&mut manager, CONNECTION, SESSION);
    let payload = CreateVmRequest::legacy_test_config(
        GuestRuntimeKind::WebAssembly,
        Default::default(),
        Default::default(),
        None,
    );
    let request = RequestFrame::new(
        601,
        OwnershipScope::session(CONNECTION, SESSION),
        RequestPayload::CreateVm(payload.clone()),
    );
    let prepared = manager.prepare_create_vm(&request, payload).unwrap();
    let completed = block_on(prepared.execute()).unwrap();
    let vm_id = completed.vm_id.clone();
    manager.complete_create_vm(completed).unwrap();
    let (generation, scratch, unix) = {
        let mut vm = manager.vms.get_mut(&vm_id).unwrap();
        assert!(vm.active_processes.is_empty());
        assert!(vm.executions.is_empty());
        assert!(vm.kernel.list_processes().is_empty());
        vm.limits.reactor.shutdown_deadline_ms = 20;
        (
            vm.generation,
            vm.runtime_scratch_root.clone(),
            vm.unix_socket_host_dir.clone(),
        )
    };
    let (started_tx, mut started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let gate_timed_out = Arc::new(AtomicBool::new(false));
    let gate_timeout = Arc::clone(&gate_timed_out);
    crate::test_support::set_host_path_cleanup_gate(vm_id.clone(), move || {
        if started_tx.send(()).is_err() {
            eprintln!("test internal cleanup gate observer disappeared");
        }
        match release_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(()) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("test internal cleanup gate released by observation unwind");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                gate_timeout.store(true, Ordering::Release);
            }
        }
    });
    let (saw_gate, returned_while_held, paths_held, release_result, result) = block_on(async {
        // Exercise the real public owner, whose internal-disposal call
        // supplies None. Timeout borrows this same future; it never drops it.
        let mut disposal = Box::pin(manager.close_session(CONNECTION, SESSION));
        let mut completed = None;
        let saw_gate = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::select! {
                started = &mut started_rx => started.is_ok(),
                result = disposal.as_mut() => {
                    completed = Some(result);
                    matches!(tokio::time::timeout(Duration::from_secs(2), &mut started_rx).await,
                        Ok(Ok(())))
                }
            }
        })
        .await
        .unwrap_or(false);
        let returned_while_held = if completed.is_some() {
            true
        } else {
            match tokio::time::timeout(Duration::from_millis(250), disposal.as_mut()).await {
                Ok(result) => {
                    completed = Some(result);
                    true
                }
                Err(_) => false,
            }
        };
        let paths_held = scratch.exists() && unix.exists();
        // Release on both outcomes before assertions, then finish the same
        // owning future and supervised worker. Sender drop also releases on
        // unwind; the gate has its independent two-second finite fallback.
        let release_result = release_tx.send(());
        let result = match completed {
            Some(result) => result,
            None => tokio::time::timeout(Duration::from_secs(5), disposal.as_mut())
                .await
                .expect("internal disposal finishes after gate release"),
        };
        (
            saw_gate,
            returned_while_held,
            paths_held,
            release_result,
            result,
        )
    });
    // This observes the existing manager-owned cleanup record after its
    // supervisor terminal callback releases the task reservation.
    block_on(manager.finish_host_path_cleanup_before_shutdown(
        tokio::time::Instant::now() + Duration::from_secs(5),
    ))
    .expect("physical internal host cleanup completes after release");
    manager.prune_retired_host_path_cleanups();
    manager.reap_reconciled_quarantined_vms();
    assert!(
        release_result.is_ok() && !gate_timed_out.load(Ordering::Acquire),
        "internal cleanup gate must be explicitly released before fallback"
    );
    assert!(
        saw_gate && paths_held,
        "real internal cleanup must be held before removal"
    );
    assert!(!scratch.exists() && !unix.exists());
    assert!(!manager.vms.contains_key(&vm_id));
    assert!(!manager.sessions.contains_key(SESSION));
    assert!(!manager
        .connections
        .get(CONNECTION)
        .unwrap()
        .sessions
        .contains(SESSION));
    assert!(!manager.host_path_cleanups.contains_key(&generation));
    assert!(!manager.quarantined_vms.contains_key(&generation));
    assert!(manager.bridge.permissions.lock().unwrap().is_empty());
    let typed_cleanup_failure = matches!(&result,
            Err(crate::VmError::Host(error)) if error.code == "ERR_AGENTOS_VM_PATH_CLEANUP_UNCONFIRMED")
        || matches!(&result, Err(crate::VmError::VmTeardownDeadline { .. }));
    assert!(returned_while_held && typed_cleanup_failure,
            "internal session disposal must return a typed cleanup failure before gate release; returned={returned_while_held} result={result:?}");
}
