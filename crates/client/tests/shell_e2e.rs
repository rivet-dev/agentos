//! Shell / PTY e2e against a real `agentos-sidecar`.
//!
//! `open_shell` spawns a PTY-backed process through the real sidecar. The ordered-stream assertion
//! uses guest Node so stdout and stderr remain distinct wire channels; separate package coverage
//! exercises the real WASM `sh` command.
//!
//! When the shell IS available the suite asserts the real TS contract: open returns a synthetic
//! `shell-N` id (NOT a pid), `on_shell_data` carries ordered PTY output, `write_shell` reaches the shell,
//! `resize_shell` validates existence, and `close_shell` plus the ShellNotFound error contract hold.

mod common;

use agentos_client::{ClientError, OpenShellOptions, StdinInput};

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
            os.write_shell("shell-missing", StdinInput::Text("x".to_string())),
            Err(ClientError::ShellNotFound(_))
        ),
        "write_shell(unknown) must return ShellNotFound"
    );
    assert!(
        matches!(
            os.resize_shell("shell-missing", 80, 24),
            Err(ClientError::ShellNotFound(_))
        ),
        "resize_shell(unknown) must return ShellNotFound"
    );
    assert!(
        matches!(
            os.close_shell("shell-missing"),
            Err(ClientError::ShellNotFound(_))
        ),
        "close_shell(unknown) must return ShellNotFound"
    );
    assert!(
        matches!(
            os.on_shell_data("shell-missing", |_| {}),
            Err(ClientError::ShellNotFound(_))
        ),
        "on_shell_data(unknown) must return ShellNotFound"
    );
    // --- open_shell: synthetic id, NOT a pid ------------------------------------------------------
    let shell = os
        .open_shell(OpenShellOptions {
            command: Some("node".to_string()),
            args: vec![
                "-e".to_string(),
                [
                    "process.stdin.setEncoding('utf8');",
                    "process.stdin.once('data', (chunk) => {",
                    "  process.stdout.write(`OUT:${chunk}`);",
                    "  process.stderr.write(`ERR:${chunk}`);",
                    "});",
                    "setInterval(() => {}, 1000);",
                ]
                .join("\n"),
            ],
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

    // --- on_shell_data: subscribe to the ordered PTY stream ---------------------------------------
    let (data_tx, mut data_rx) = tokio::sync::mpsc::unbounded_channel();
    let _data = os
        .on_shell_data(&shell.shell_id, move |event| {
            let _ = data_tx.send(event.data);
        })
        .expect("on_shell_data for live shell");
    // Stderr remains available as a channel-specific diagnostic tap.
    let (stderr_tx, mut stderr_rx) = tokio::sync::mpsc::unbounded_channel();
    let _stderr = os
        .on_shell_stderr(&shell.shell_id, move |event| {
            let _ = stderr_tx.send(event.data);
        })
        .expect("on_shell_stderr for live shell");

    // --- write_shell: prove execution and stdout/stderr ordering on the PTY stream ----------------
    // Neither output marker occurs in the input, so PTY input echo alone cannot satisfy this
    // assertion. The process writes stdout before stderr, proving the combined stream retains wire
    // order while the stderr bytes remain available through the diagnostic tap.
    os.write_shell(
        &shell.shell_id,
        StdinInput::Text("hello-shell\n".to_string()),
    )
    .expect("write_shell");

    let ordered_output = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut acc = Vec::<u8>::new();
        while let Some(chunk) = data_rx.recv().await {
            acc.extend_from_slice(&chunk);
            let text = String::from_utf8_lossy(&acc);
            if text.contains("OUT:hello-shell") && text.contains("ERR:hello-shell") {
                return acc;
            }
        }
        acc
    })
    .await
    .expect("timed out waiting for ordered shell output");
    let ordered_output = String::from_utf8_lossy(&ordered_output);
    let stdout_index = ordered_output.find("OUT:hello-shell").unwrap_or_else(|| {
        panic!("combined PTY stream should contain executed stdout: {ordered_output:?}")
    });
    let stderr_index = ordered_output.find("ERR:hello-shell").unwrap_or_else(|| {
        panic!("combined PTY stream should contain executed stderr: {ordered_output:?}")
    });
    assert!(
        stdout_index < stderr_index,
        "combined PTY stream reordered stdout/stderr: {ordered_output:?}"
    );

    let diagnostic_saw_stderr = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut acc = Vec::<u8>::new();
        while let Some(chunk) = stderr_rx.recv().await {
            acc.extend_from_slice(&chunk);
            if String::from_utf8_lossy(&acc).contains("ERR:hello-shell") {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(
        diagnostic_saw_stderr,
        "stderr diagnostic stream should retain the executed stderr output"
    );

    // --- resize_shell: validates existence and forwards the native PTY resize ----------------------
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
    match os.on_shell_data("shell-does-not-exist", |_| {}) {
        Err(ClientError::ShellNotFound(_)) => {}
        Ok(_) => panic!("unknown shell id must error"),
        Err(other) => panic!("expected ShellNotFound, got {other:?}"),
    }

    os.shutdown().await.expect("shutdown");
}
