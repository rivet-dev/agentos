//! Shell / PTY e2e against a real `agentos-sidecar`.
//!
//! `open_shell` spawns a PTY-backed `sh` (a WASM command). This suite fails fast by default when
//! that command is unavailable; set `AGENT_OS_CLIENT_ALLOW_E2E_SKIPS=1` only for local skip-only
//! runs.
//!
//! When the shell IS available the suite asserts the real TS contract: open returns the
//! sidecar-owned process id (not a pid), `on_shell_data` carries stdout, `write_shell` reaches the
//! shell, `resize_shell` validates existence, and `close_shell` plus the ShellNotFound contract hold.

mod common;

use agentos_client::{ClientError, OpenShellOptions, StdinInput};
use futures::StreamExt;

#[tokio::test]
async fn shell_surface_open_write_data_resize_close() {
    if !common::require_sidecar("shell_surface_open_write_data_resize_close") {
        return;
    }
    let os = common::new_vm_with_wasm_commands().await;

    // --- Runtime-independent ShellNotFound contract (no WASM needed) ------------------------------
    // Every shell operation on an unknown id returns ShellNotFound, asserted against the real sidecar
    // regardless of whether a PTY-backed WASM shell is available.
    assert!(
        matches!(
            os.write_shell("shell-missing", StdinInput::Text("x".to_string()))
                .await,
            Err(ClientError::ShellNotFound(_))
        ),
        "write_shell(unknown) must return ShellNotFound"
    );
    assert!(
        matches!(
            os.resize_shell("shell-missing", 80, 24).await,
            Err(ClientError::ShellNotFound(_))
        ),
        "resize_shell(unknown) must return ShellNotFound"
    );
    assert!(
        matches!(
            os.close_shell("shell-missing").await,
            Err(ClientError::ShellNotFound(_))
        ),
        "close_shell(unknown) must return ShellNotFound"
    );
    assert!(
        matches!(
            os.on_shell_data("shell-missing"),
            Err(ClientError::ShellNotFound(_))
        ),
        "on_shell_data(unknown) must return ShellNotFound"
    );

    if !common::require_wasm_commands(&os, "shell_surface_open_write_data_resize_close").await {
        os.shutdown().await.expect("shutdown after local skip");
        return;
    }

    // --- open_shell: sidecar process id, NOT a pid ------------------------------------------------
    let shell = os
        .open_shell(OpenShellOptions {
            cols: Some(80),
            rows: Some(24),
            ..Default::default()
        })
        .await
        .expect("open_shell");
    assert!(
        shell.shell_id.starts_with("sidecar-process-"),
        "open_shell must return the sidecar process id (not a pid), got {}",
        shell.shell_id
    );

    // --- on_shell_data: subscribe to stdout (stderr is on a separate channel) ---------------------
    let mut data = os
        .on_shell_data(&shell.shell_id)
        .expect("on_shell_data for live shell");
    // A separate stderr channel must also be subscribable.
    let _stderr = os
        .on_shell_stderr(&shell.shell_id)
        .expect("on_shell_stderr for live shell");

    // --- write_shell: drive the shell, expect the echoed command/output on the data stream --------
    // A PTY shell echoes typed input and runs the command. `echo shell-marker` produces the literal
    // marker on stdout. We scan the data stream for the marker rather than asserting an exact frame,
    // because PTY line-discipline echo + prompts interleave.
    os.write_shell(
        &shell.shell_id,
        StdinInput::Text("echo shell-marker\n".to_string()),
    )
    .await
    .expect("write_shell");

    let saw_marker = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut acc = Vec::<u8>::new();
        while let Some(chunk) = data.next().await {
            let chunk = chunk.expect("shell data stream lagged");
            acc.extend_from_slice(&chunk);
            if String::from_utf8_lossy(&acc).contains("shell-marker") {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(
        saw_marker,
        "the shell's data stream should surface the echoed `shell-marker` output"
    );

    // --- resize_shell: validates existence (no native winsize op, so it is a best-effort no-op) ----
    os.resize_shell(&shell.shell_id, 120, 40)
        .await
        .expect("resize_shell on a live shell must succeed");

    // --- close_shell: removes the entry; subsequent shell calls report ShellNotFound --------------
    os.close_shell(&shell.shell_id).await.expect("close_shell");
    let exit_code = os.wait_shell(&shell.shell_id).await.expect("wait_shell");
    assert_eq!(
        os.wait_shell(&shell.shell_id)
            .await
            .expect("late wait_shell from sidecar snapshot"),
        exit_code
    );
    let err = os
        .write_shell(&shell.shell_id, StdinInput::Text("x".to_string()))
        .await
        .expect_err("write to a closed shell must error");
    assert!(
        matches!(err, ClientError::ShellNotFound(id) if id == shell.shell_id),
        "closed shell must report ShellNotFound"
    );

    // --- ShellNotFound contract for a never-opened id ---------------------------------------------
    match os.on_shell_data("shell-does-not-exist") {
        Err(ClientError::ShellNotFound(_)) => {}
        Ok(_) => panic!("unknown shell id must error"),
        Err(other) => panic!("expected ShellNotFound, got {other:?}"),
    }

    os.shutdown().await.expect("shutdown");
}
