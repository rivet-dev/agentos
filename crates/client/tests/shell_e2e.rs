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
async fn exited_terminal_replay_and_late_subscriptions_survive_registry_removal() {
    if !common::require_sidecar(
        "exited_terminal_replay_and_late_subscriptions_survive_registry_removal",
    ) {
        return;
    }
    let os = common::new_vm().await;
    let result = async {
        let shell = os.open_shell(OpenShellOptions {
            command: Some("node".into()),
            args: vec![
                "-e".into(),
                "process.stdout.write('final-output'); process.exit(7)".into(),
            ],
            ..Default::default()
        })?;
        let code = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            os.wait_shell(&shell.shell_id),
        )
        .await??;
        anyhow::ensure!(code == 7, "terminal exit code changed");
        let snapshot = os.snapshot_shell(&shell.shell_id, None, None).await?;
        anyhow::ensure!(
            snapshot.exit_code == Some(7),
            "terminal replay lost its end state"
        );
        anyhow::ensure!(
            snapshot
                .events
                .iter()
                .flat_map(|event| event.data.clone())
                .collect::<Vec<_>>()
                .windows(12)
                .any(|bytes| bytes == b"final-output"),
            "terminal replay lost its final output"
        );
        let _output = os.on_shell_output(&shell.shell_id, |_| {})?;
        let (exit_tx, exit_rx) = tokio::sync::oneshot::channel();
        let _exit = os.on_shell_exit(&shell.shell_id, move |exit| {
            exit_tx.send(exit.exit_code).expect("late exit receiver");
        })?;
        anyhow::ensure!(exit_rx.await? == 7, "late subscription lost terminal exit");
        let bad = os.open_shell(OpenShellOptions {
            command: Some("agentos-command-that-does-not-exist".into()),
            ..Default::default()
        })?;
        // A rejected Execute must fail the wait, not become PID 0 and wait forever.
        anyhow::ensure!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                os.wait_shell(&bad.shell_id)
            )
            .await?
            .is_err(),
            "rejected terminal must not report an exit code"
        );
        Ok::<(), anyhow::Error>(())
    }
    .await;
    os.shutdown().await.expect("shutdown terminal test VM");
    result.unwrap();
}

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

/// Output chunks with the time each one arrived.
type TimedChunks = Vec<(std::time::Instant, Vec<u8>)>;

/// Shell output collected with arrival times, as a host-side client sees it.
#[derive(Clone, Default)]
struct ShellOutput(std::sync::Arc<std::sync::Mutex<TimedChunks>>);

impl ShellOutput {
    fn subscribe(
        os: &agentos_client::AgentOs,
        shell_id: &str,
    ) -> Result<(Self, agentos_client::stream::Subscription), ClientError> {
        let output = Self::default();
        let sink = output.clone();
        let subscription = os.on_shell_data(shell_id, move |event| {
            sink.0
                .lock()
                .expect("shell output lock")
                .push((std::time::Instant::now(), event.data));
        })?;
        Ok((output, subscription))
    }

    fn bytes(&self) -> Vec<u8> {
        self.0
            .lock()
            .expect("shell output lock")
            .iter()
            .flat_map(|(_, chunk)| chunk.clone())
            .collect()
    }

    fn count(&self, needle: &str) -> usize {
        String::from_utf8_lossy(&self.bytes())
            .matches(needle)
            .count()
    }

    fn first_arrival(&self, needle: &str) -> Option<std::time::Instant> {
        let mut seen = Vec::new();
        for (at, chunk) in self.0.lock().expect("shell output lock").iter() {
            seen.extend_from_slice(chunk);
            if String::from_utf8_lossy(&seen).contains(needle) {
                return Some(*at);
            }
        }
        None
    }
}

async fn wait_for_shell_exit(os: &agentos_client::AgentOs, shell_id: &str) -> anyhow::Result<i32> {
    Ok(tokio::time::timeout(std::time::Duration::from_secs(60), os.wait_shell(shell_id)).await??)
}

/// Runs a test body against a fresh VM and shuts the VM down before reporting
/// the result, so a failure cannot leak the VM into later tests.
async fn with_wasm_vm<F, Fut>(body: F)
where
    F: FnOnce(agentos_client::AgentOs) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let os = common::new_vm_with_wasm_commands().await;
    let result = body(os.clone()).await;
    os.shutdown().await.expect("shutdown terminal test VM");
    result.unwrap();
}

#[tokio::test]
async fn a_command_run_from_a_terminal_shows_its_output_once_and_while_the_shell_runs() {
    if !common::require_sidecar(
        "a_command_run_from_a_terminal_shows_its_output_once_and_while_the_shell_runs",
    ) {
        return;
    }
    with_wasm_vm(|os| async move {
        os.write_file("/tmp/early.txt", "EARLY-LINE\n").await?;
        os.write_file("/tmp/late.txt", "LATE-LINE\n").await?;

        // `cat` runs as a child of the shell and writes to the terminal they share.
        let shell = os.open_shell(OpenShellOptions {
            command: Some("sh".into()),
            args: vec![
                "-c".into(),
                "cat /tmp/early.txt; sleep 2; cat /tmp/late.txt".into(),
            ],
            ..Default::default()
        })?;
        let (output, _subscription) = ShellOutput::subscribe(&os, &shell.shell_id)?;
        anyhow::ensure!(
            wait_for_shell_exit(&os, &shell.shell_id).await? == 0,
            "shell failed"
        );

        anyhow::ensure!(
            output.count("EARLY-LINE") == 1,
            "child output must appear exactly once"
        );
        anyhow::ensure!(
            output.count("LATE-LINE") == 1,
            "child output must appear exactly once"
        );
        let early = output.first_arrival("EARLY-LINE").expect("early output");
        let late = output.first_arrival("LATE-LINE").expect("late output");
        anyhow::ensure!(
            late.duration_since(early) >= std::time::Duration::from_millis(1500),
            "the first child's output must reach the host before the shell finishes"
        );
        Ok(())
    })
    .await;
}

#[tokio::test]
async fn large_terminal_output_reaches_the_host_intact() {
    if !common::require_sidecar("large_terminal_output_reaches_the_host_intact") {
        return;
    }
    with_wasm_vm(|os| async move {
        // 32,000 lines of 64 bytes: far more than the PTY's 64 KiB output queue,
        // written by the child in large chunks.
        let line = format!("{}\n", &"0123456789abcdef".repeat(4)[..63]);
        let content = line.repeat(32_000);
        os.write_file("/tmp/big.txt", content.clone()).await?;

        let shell = os.open_shell(OpenShellOptions {
            command: Some("sh".into()),
            args: vec!["-c".into(), "cat /tmp/big.txt".into()],
            ..Default::default()
        })?;
        let (output, _subscription) = ShellOutput::subscribe(&os, &shell.shell_id)?;
        anyhow::ensure!(
            wait_for_shell_exit(&os, &shell.shell_id).await? == 0,
            "shell failed"
        );

        // The terminal turns each "\n" into "\r\n".
        let expected = content.replace('\n', "\r\n");
        let received = output.bytes();
        anyhow::ensure!(
            received.len() == expected.len(),
            "terminal output lost or duplicated bytes: got {} of {}",
            received.len(),
            expected.len()
        );
        anyhow::ensure!(received == expected.as_bytes(), "terminal output changed");
        Ok(())
    })
    .await;
}

#[tokio::test]
async fn a_single_large_write_to_a_terminal_is_written_whole() {
    if !common::require_sidecar("a_single_large_write_to_a_terminal_is_written_whole") {
        return;
    }
    with_wasm_vm(|os| async move {
        // dd makes one write() per block and does not retry a short one, so a
        // terminal that accepts part of a blocking write loses the rest. Linux
        // completes a blocking write to a terminal. Each 192 KiB block is
        // larger than the terminal's 64 KiB output queue and fits in one read.
        let shell = os.open_shell(OpenShellOptions {
            command: Some("sh".into()),
            args: vec!["-c".into(), "dd if=/dev/zero bs=196608 count=8".into()],
            ..Default::default()
        })?;
        let (output, _subscription) = ShellOutput::subscribe(&os, &shell.shell_id)?;
        anyhow::ensure!(
            wait_for_shell_exit(&os, &shell.shell_id).await? == 0,
            "dd failed"
        );

        let received = output.bytes();
        let zeros = received.iter().filter(|&&byte| byte == 0).count();
        let report = String::from_utf8_lossy(&received).replace('\0', "");
        anyhow::ensure!(
            zeros == 8 * 196_608,
            "dd wrote {zeros} of 1572864 bytes to the terminal; dd reported: {report:?}"
        );
        Ok(())
    })
    .await;
}

fn screen(rows: u16, cols: u16) -> vt100::Parser {
    let size = |n: u16| std::num::NonZeroU16::new(n).expect("terminal size");
    vt100::Parser::new(size(rows), size(cols), 0)
}

fn screen_lines(bytes: &[u8]) -> Vec<String> {
    let mut parser = screen(24, 80);
    parser.process(bytes);
    parser
        .screen()
        .rows(0, 80)
        .map(|row| row.trim_end().to_string())
        .collect()
}

/// Waits until `done` holds, the shell exits, or `limit` passes. Returns the
/// shell's exit code if it exited.
async fn wait_until(
    os: &agentos_client::AgentOs,
    shell_id: &str,
    limit: std::time::Duration,
    mut done: impl FnMut() -> bool,
) -> anyhow::Result<Option<i32>> {
    let deadline = std::time::Instant::now() + limit;
    while std::time::Instant::now() < deadline && !done() {
        if let Ok(code) = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            os.wait_shell(shell_id),
        )
        .await
        {
            return Ok(Some(code?));
        }
    }
    Ok(None)
}

/// Waits until the shell has written something and then been quiet for a
/// moment, as a line editor waiting for keys is.
async fn wait_until_quiet(
    os: &agentos_client::AgentOs,
    shell_id: &str,
    output: &ShellOutput,
) -> anyhow::Result<Option<i32>> {
    let quiet = std::time::Duration::from_millis(700);
    let mut seen = (0, std::time::Instant::now());
    wait_until(os, shell_id, std::time::Duration::from_secs(15), || {
        let length = output.bytes().len();
        if length != seen.0 {
            seen = (length, std::time::Instant::now());
        }
        length > 0 && seen.1.elapsed() >= quiet
    })
    .await
}

/// Opens the interactive shell, whose line editor asks the terminal for the
/// cursor position before every prompt, runs one command in it, and returns
/// the rendered screen and whether the shell exited.
async fn run_interactive_command(
    os: &agentos_client::AgentOs,
    answering_clients: usize,
) -> anyhow::Result<(Vec<String>, Option<i32>)> {
    let shell = os.open_shell(OpenShellOptions {
        command: Some("sh".into()),
        cols: Some(80),
        rows: Some(24),
        env: [("TERM".to_string(), "xterm-256color".to_string())].into(),
        ..Default::default()
    })?;
    let (output, _subscription) = ShellOutput::subscribe(os, &shell.shell_id)?;
    // Each answering client renders the stream with its own emulator and
    // answers every cursor query it sees, as a browser terminal does.
    let clients = std::sync::Arc::new(std::sync::Mutex::new(
        (0..answering_clients)
            .map(|_| screen(24, 80))
            .collect::<Vec<_>>(),
    ));
    let answer_errors = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (answering, errors) = (clients.clone(), answer_errors.clone());
    let writer = os.clone();
    let shell_id = shell.shell_id.clone();
    let _answers = os.on_shell_data(&shell.shell_id, move |event| {
        let mut clients = answering.lock().expect("clients lock");
        for client in clients.iter_mut() {
            let mut applied = 0;
            for (index, _) in event
                .data
                .windows(4)
                .enumerate()
                .filter(|(_, bytes)| *bytes == b"\x1b[6n")
            {
                client.process(&event.data[applied..index + 4]);
                applied = index + 4;
                let (row, col) = client.screen().cursor_position();
                let report = StdinInput::Text(format!("\x1b[{};{}R", row + 1, col + 1));
                if let Err(error) = writer.write_shell(&shell_id, report) {
                    errors.lock().expect("errors lock").push(error.to_string());
                }
            }
            client.process(&event.data[applied..]);
        }
    })?;

    let mut exited = wait_until_quiet(os, &shell.shell_id, &output).await?;
    if exited.is_none() {
        os.write_shell(
            &shell.shell_id,
            StdinInput::Text("cat /tmp/early.txt\r".into()),
        )?;
        exited = wait_until(
            os,
            &shell.shell_id,
            std::time::Duration::from_secs(15),
            || {
                screen_lines(&output.bytes())
                    .iter()
                    .any(|line| line == "EARLY-LINE")
            },
        )
        .await?;
    }
    if exited.is_none() {
        exited = wait_until_quiet(os, &shell.shell_id, &output).await?;
    }
    let errors = answer_errors.lock().expect("errors lock");
    anyhow::ensure!(
        errors.is_empty() || exited.is_some(),
        "a client's answer could not be written: {errors:?}"
    );
    Ok((screen_lines(&output.bytes()), exited))
}

#[tokio::test]
async fn an_interactive_shell_keeps_running_when_no_client_answers_its_cursor_queries() {
    if !common::require_sidecar(
        "an_interactive_shell_keeps_running_when_no_client_answers_its_cursor_queries",
    ) {
        return;
    }
    with_wasm_vm(|os| async move {
        os.write_file("/tmp/early.txt", "EARLY-LINE\n").await?;
        let (lines, exited) = run_interactive_command(&os, 0).await?;
        anyhow::ensure!(
            exited.is_none(),
            "the shell must not exit while no client is attached"
        );
        anyhow::ensure!(
            lines
                .iter()
                .filter(|line| line.as_str() == "EARLY-LINE")
                .count()
                == 1,
            "the command's output must be on screen once: {lines:#?}"
        );
        Ok(())
    })
    .await;
}

#[tokio::test]
async fn cursor_reports_from_attached_clients_do_not_corrupt_the_shell_screen() {
    if !common::require_sidecar(
        "cursor_reports_from_attached_clients_do_not_corrupt_the_shell_screen",
    ) {
        return;
    }
    with_wasm_vm(|os| async move {
        os.write_file("/tmp/early.txt", "EARLY-LINE\n").await?;
        // Two clients answer every query they see; only one answer may reach the shell.
        let (lines, exited) = run_interactive_command(&os, 2).await?;
        anyhow::ensure!(exited.is_none(), "the shell must keep running");
        anyhow::ensure!(
            lines
                .iter()
                .filter(|line| line.as_str() == "EARLY-LINE")
                .count()
                == 1,
            "the command's output must stay on screen once: {lines:#?}"
        );
        Ok(())
    })
    .await;
}

#[tokio::test]
async fn hostile_terminal_output_does_not_stall_the_sidecar() {
    if !common::require_sidecar("hostile_terminal_output_does_not_stall_the_sidecar") {
        return;
    }
    with_wasm_vm(|os| async move {
        // Each sequence asks a terminal to repeat or insert 65,535 characters.
        let hostile = "x\x1b[65535b\x1b[65535@\x1b[2J".repeat(40_000);
        os.write_file("/tmp/hostile.txt", hostile).await?;
        let shell = os.open_shell(OpenShellOptions {
            command: Some("sh".into()),
            args: vec!["-c".into(), "cat /tmp/hostile.txt; echo DONE".into()],
            ..Default::default()
        })?;
        let (output, _subscription) = ShellOutput::subscribe(&os, &shell.shell_id)?;
        let code = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            os.wait_shell(&shell.shell_id),
        )
        .await
        .map_err(|_| anyhow::anyhow!("the terminal took over 20 s to pass 1 MB of output"))??;
        anyhow::ensure!(code == 0 && output.count("DONE") == 1, "cat failed");
        Ok(())
    })
    .await;
}
