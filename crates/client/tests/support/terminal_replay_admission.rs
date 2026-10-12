#[tokio::test]
async fn terminal_snapshot_waits_for_launch_and_preserves_typed_failure() {
    // Actual snapshot method and transport; only launch acknowledgement is injected.
    // The existing bounded fixture child does not implement a sidecar or reply to RPCs.
    let (vm, transport, peer) = injected_disposal_vm().await;
    let (spawned, _) = tokio::sync::watch::channel(None);
    let (exit, _) = tokio::sync::watch::channel(None);
    let (data, _) = broadcast::channel(1);
    let (stderr, _) = broadcast::channel(1);
    let (events, _) = broadcast::channel(1);
    assert!(vm
        .inner
        .shells
        .insert(
            "shell-gate".into(),
            ShellEntry {
                pid: 0,
                data_tx: data,
                stderr_tx: stderr,
                event_tx: events,
                process_id: "shell-pending".into(),
                spawned_tx: spawned.clone(),
                exit_tx: exit,
            }
        )
        .is_ok());
    let expected = ClientError::Kernel {
        code: "EACCES".into(),
        message: "injected launch denied".into(),
    };
    let (was_pending, result) = {
        let snapshot = vm.snapshot_shell("shell-gate", None, None);
        tokio::pin!(snapshot);
        let was_pending = tokio::time::timeout(Duration::from_millis(10), &mut snapshot)
            .await
            .is_err();
        spawned.send_replace(Some(Err(expected.clone())));
        let result = tokio::time::timeout(Duration::from_millis(100), &mut snapshot).await;
        (was_pending, result)
    };
    vm.inner.shells.remove("shell-gate");
    tokio::time::timeout(Duration::from_secs(2), transport.terminate_child())
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !transport.is_disconnected() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let cleanup = vm.shutdown().await;
    peer.dispose().await.unwrap();
    vm.inner.sidecar.dispose().await.unwrap();
    cleanup.unwrap();
    assert!(was_pending, "snapshot must wait for launch acknowledgement");
    let result =
        result.expect("snapshot ignored launch failure and waited on premature output RPC");
    assert!(matches!(result, Err(ClientError::Kernel { code, .. }) if code == "EACCES"));
}
