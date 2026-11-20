#[tokio::test]
async fn terminal_close_preserves_pending_wait_without_admitting_writes() {
    for (code, signal) in [(0, None), (137, Some(9))] {
        // Actual open/close/wait methods; only launch and eventual exit are injected.
        // The bounded fixture child is a transport owner, not a real guest/sidecar.
        let (vm, transport, peer) = injected_disposal_vm().await;
        let shell = vm
            .open_shell(crate::shell::OpenShellOptions::default())
            .unwrap();
        let (spawned, exit, process_id) = vm
            .inner
            .shells
            .read(&shell.shell_id, |_, entry| {
                (
                    entry.spawned_tx.clone(),
                    entry.exit_tx.clone(),
                    entry.process_id.clone(),
                )
            })
            .unwrap();
        vm.inner
            .shells
            .update(&shell.shell_id, |_, entry| entry.pid = 42);
        spawned.send_replace(Some(Ok(())));
        vm.close_shell(&shell.shell_id).unwrap();
        assert_eq!(
            vm.inner.pending_shell_exits.len(),
            1,
            "close lost bounded task ownership"
        );
        let write = vm
            .write_shell_awaited(
                &shell.shell_id,
                crate::process::StdinInput::Bytes(b"after-close".to_vec()),
            )
            .await;
        let expected = crate::process::ProcessExit {
            pid: 42,
            exit_code: code,
            exit_signal: signal,
        };
        let (pending_until_exit, observed) = {
            let waiting = vm.wait_shell_exit(&shell.shell_id);
            tokio::pin!(waiting);
            let early = tokio::time::timeout(Duration::from_millis(10), &mut waiting).await;
            let pending = early.is_err();
            vm.inner.closed_shells.lock().push_back(ClosedShellEntry {
                shell_id: shell.shell_id.clone(),
                process_id,
                pid: 42,
                result: Ok(expected),
            });
            exit.send_replace(Some(Ok(expected)));
            let observed = match early {
                Ok(result) => result,
                Err(_) => tokio::time::timeout(Duration::from_secs(1), &mut waiting)
                    .await
                    .unwrap(),
            };
            (pending, observed)
        };
        let late = vm.wait_shell_exit(&shell.shell_id).await;
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
        assert!(
            vm.inner.pending_shell_exits.is_empty(),
            "dispose retained terminal tasks"
        );
        assert_eq!(vm.inner.terminal_count.load(Ordering::SeqCst), 0);
        assert!(
            matches!(write, Err(ClientError::ShellNotFound(_))),
            "close admitted a new write"
        );
        assert!(
            pending_until_exit,
            "close discarded the wait owner before actual exit: {observed:?}"
        );
        let observed = observed.unwrap();
        assert_eq!((observed.exit_code, observed.exit_signal), (code, signal));
        let late = late.unwrap();
        assert_eq!((late.exit_code, late.exit_signal), (code, signal));
    }
}
