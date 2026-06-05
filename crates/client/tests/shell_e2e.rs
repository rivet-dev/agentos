//! Shell / PTY e2e against a real `agent-os-sidecar`.
//!
//! `open_shell` spawns a PTY-backed `sh` (a WASM command) which is NOT checked into git, so this
//! suite is self-gating: it first probes a trivial `exec` of the shell command and, if it cannot be
//! resolved, treats that as "WASM shell not present" and skips. The suite still compiles and passes
//! as a skip in that environment.
//!
//! When the shell IS available the suite asserts the real TS contract: open returns a synthetic
//! `shell-N` id (NOT a pid), `on_shell_data` carries stdout, `write_shell` reaches the shell,
//! `resize_shell` validates existence, and `close_shell` plus the ShellNotFound error contract hold.

mod common;

use agent_os_client::{ClientError, OpenShellOptions, StdinInput};
use futures::StreamExt;

#[tokio::test]
async fn shell_surface_open_write_data_resize_close() {
    if !common::sidecar_available() {
        eprintln!("skipping shell_surface_open_write_data_resize_close: sidecar binary not built");
        return;
    }
    let os = common::new_vm().await;

    // --- Runtime-independent ShellNotFound contract (no WASM needed) ------------------------------
    // Every shell operation on an unknown id returns ShellNotFound, asserted against the real sidecar
    // regardless of whether a PTY-backed WASM shell is available.
    assert!(
        matches!(
            os.write_shell("shell-missing", StdinInput::Text("x".to_string())),
            Err(ClientError::ShellNotFound(_))
        ),
        "write_shell(unknown) must return ShellNotFound"
    );
    assert!(
        matches!(os.resize_shell("shell-missing", 80, 24), Err(ClientError::ShellNotFound(_))),
        "resize_shell(unknown) must return ShellNotFound"
    );
    assert!(
        matches!(os.close_shell("shell-missing"), Err(ClientError::ShellNotFound(_))),
        "close_shell(unknown) must return ShellNotFound"
    );
    assert!(
        matches!(os.on_shell_data("shell-missing"), Err(ClientError::ShellNotFound(_))),
        "on_shell_data(unknown) must return ShellNotFound"
    );

    if !common::wasm_commands_available(&os).await {
        eprintln!(
            "skipping shell PTY assertions: WASM PTY shell (sh) not present in this environment \
             (ShellNotFound contract above still executed)"
        );
        os.shutdown().await.expect("shutdown");
        return;
    }

    // --- open_shell: synthetic id, NOT a pid ------------------------------------------------------
    let shell = os
        .open_shell(OpenShellOptions {
            cols: Some(80),
            rows: Some(24),
            ..Default::default()
        })
        .expect("open_shell");
    assert!(
        shell.shell_id.starts_with("shell-"),
        "open_shell must return a synthetic shell-N id (not a pid), got {}",
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
    .expect("write_shell");

    let saw_marker = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut acc = Vec::<u8>::new();
        while let Some(chunk) = data.next().await {
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
        .expect("resize_shell on a live shell must succeed");

    // --- close_shell: removes the entry; subsequent shell calls report ShellNotFound --------------
    os.close_shell(&shell.shell_id).expect("close_shell");
    let err = os
        .write_shell(&shell.shell_id, StdinInput::Text("x".to_string()))
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
