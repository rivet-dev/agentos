//! Process e2e against a real `agentos-sidecar`.
//!
//! `exec`/`spawn` require WASM command packages (sh/echo/cat). This suite fails fast by default when
//! those packages are unavailable; set `AGENT_OS_CLIENT_ALLOW_E2E_SKIPS=1` only for local skip-only
//! runs.
//!
//! When commands ARE available the suite asserts the real TS contract: exec stdout + exit code,
//! binary stdout round-trip, spawn pid + stdin write, exit-code wait, list/get of SDK processes, and
//! the kernel process snapshot (`all_processes` / `process_tree`).

mod common;

#[tokio::test]
async fn language_spawn_retains_output_and_completion_from_fast_scripts() {
    if !common::require_sidecar("language_spawn_retains_output_and_completion_from_fast_scripts") {
        return;
    }
    let vm = common::new_vm().await;
    let result = async {
        for index in 0..3 {
            let marker = format!("fast-language-{index}");
            let process = vm
                .spawn_javascript(
                    format!("console.log('{marker}')"),
                    agentos_client::LanguageSpawnOptions {
                        retain_events: true,
                        ..Default::default()
                    },
                )
                .await?;
            anyhow::ensure!(
                tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    vm.wait_process(process.pid)
                )
                .await??
                    == 0,
                "fast language process must report a confirmed successful exit"
            );
            let replay = vm
                .read_process_output(process.pid, None, None, None)
                .await?;
            let output = replay
                .events
                .into_iter()
                .flat_map(|event| event.data)
                .collect::<Vec<_>>();
            anyhow::ensure!(
                String::from_utf8_lossy(&output).contains(&marker),
                "fast language replay lost its output"
            );
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    vm.shutdown().await.expect("shutdown fast language test VM");
    result.unwrap();
}

use std::sync::{Arc, Mutex};

use agentos_client::{ClientError, ExecOptions, SpawnOptions, StdinInput};

#[tokio::test]
async fn spawn_rejection_and_fast_exit_remain_distinct_for_late_waiters() {
    if !common::require_sidecar("spawn_rejection_and_fast_exit_remain_distinct_for_late_waiters") {
        return;
    }
    let os = common::new_vm().await;
    let result = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let rejected = os.spawn(
            "agentos-nonexistent-review-command",
            Vec::new(),
            SpawnOptions::default(),
        )?;
        anyhow::ensure!(
            matches!(
                os.wait_process(rejected.pid).await,
                Err(ClientError::Kernel { .. })
            ),
            "a rejected launch must be an error, not exit 1"
        );
        anyhow::ensure!(
            os.get_process(rejected.pid)?.exit_code.is_none(),
            "rejection must not invent an exit status"
        );
        anyhow::ensure!(
            matches!(
                os.wait_process(rejected.pid).await,
                Err(ClientError::Kernel { .. })
            ),
            "late waiters must retain the typed rejection"
        );

        let fast = os.spawn(
            "node",
            vec!["-e".into(), "process.exit(23)".into()],
            SpawnOptions::default(),
        )?;
        // Deliberately do not subscribe to exit while this guest runs.
        loop {
            if os.get_process(fast.pid)?.exit_code.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        anyhow::ensure!(
            os.wait_process(fast.pid).await? == 23,
            "fast exit must survive until a late wait"
        );
        Ok::<_, anyhow::Error>(())
    })
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(4), os.shutdown())
        .await
        .expect("bounded VM cleanup")
        .expect("shutdown VM");
    result
        .expect("bounded process observation probe")
        .expect("process outcome parity");
}

/// VMs in one process share a sidecar. A guest blocked reading an open stdin must not stall it:
/// calls on the same VM and on another VM keep answering, and closing the stdin ends the reader.
#[tokio::test]
async fn a_guest_waiting_on_stdin_does_not_stall_the_shared_sidecar() {
    const TEST: &str = "a_guest_waiting_on_stdin_does_not_stall_the_shared_sidecar";
    if !common::require_sidecar(TEST) {
        return;
    }
    let (Some(blocked_vm), Some(other_vm)) = (
        common::new_vm_with_commands().await,
        common::new_vm_with_commands().await,
    ) else {
        if common::allow_local_e2e_skips() {
            eprintln!("skipping {TEST}: coreutils package absent");
            return;
        }
        panic!("{TEST}: coreutils package absent");
    };
    let result = async {
        let reader = blocked_vm.spawn("cat", Vec::new(), SpawnOptions::default())?;
        // Give `cat` time to enter its stdin read.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        for (label, vm) in [("same", &blocked_vm), ("other", &other_vm)] {
            let probe = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                vm.exec_process("echo probe", ExecOptions::default()),
            )
            .await
            .map_err(|_| anyhow::anyhow!("exec on the {label} VM stalled behind a stdin read"))??;
            anyhow::ensure!(probe.stdout == "probe\n", "unexpected probe: {probe:?}");
        }
        blocked_vm.close_process_stdin(reader.pid).await?;
        let exit = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            blocked_vm.wait_process(reader.pid),
        )
        .await
        .map_err(|_| anyhow::anyhow!("cat did not exit after its stdin closed"))??;
        anyhow::ensure!(exit == 0, "cat exited with {exit}");
        Ok::<_, anyhow::Error>(())
    }
    .await;
    for vm in [blocked_vm, other_vm] {
        tokio::time::timeout(std::time::Duration::from_secs(10), vm.shutdown())
            .await
            .expect("bounded VM cleanup")
            .expect("shutdown VM");
    }
    result.unwrap();
}

/// A pipeline writer fills its pipe before the reader starts, then waits for room. The wait must
/// end when the reader drains the pipe, as on Linux, not when the writer's wait times out.
#[tokio::test]
async fn a_pipeline_writer_resumes_when_a_late_reader_drains_the_pipe() {
    const TEST: &str = "a_pipeline_writer_resumes_when_a_late_reader_drains_the_pipe";
    if !common::require_sidecar(TEST) {
        return;
    }
    let Some(vm) = common::new_vm_with_commands().await else {
        if common::allow_local_e2e_skips() {
            eprintln!("skipping {TEST}: coreutils package absent");
            return;
        }
        panic!("{TEST}: coreutils package absent");
    };
    // 200 KB is several pipe buffers. Linux finishes in about a second.
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        vm.exec_process(
            "head -c 200000 /dev/zero | { sleep 1; wc -c; }",
            ExecOptions::default(),
        ),
    )
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(10), vm.shutdown())
        .await
        .expect("bounded VM cleanup")
        .expect("shutdown VM");
    let result = result
        .expect("the pipeline must finish once its reader drains the pipe")
        .expect("pipeline runs");
    assert_eq!(result.exit_code, 0, "{result:?}");
    assert_eq!(result.stdout.trim(), "200000", "{result:?}");
}

/// A pipeline writer waits for its reader for as long as the reader takes, as a blocking write
/// does on Linux, and loses nothing.
#[tokio::test]
#[ignore = "waits past the 30 s limits.reactor.operationDeadlineMs to show pipe writes have no deadline"]
async fn a_pipeline_writer_waits_for_a_slow_reader_without_losing_data() {
    const TEST: &str = "a_pipeline_writer_waits_for_a_slow_reader_without_losing_data";
    if !common::require_sidecar(TEST) {
        return;
    }
    let Some(vm) = common::new_vm_with_commands().await else {
        panic!("{TEST}: coreutils package absent");
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        vm.exec_process(
            "seq 1 200000 | { sleep 35; wc -l; }",
            ExecOptions::default(),
        ),
    )
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(10), vm.shutdown())
        .await
        .expect("bounded VM cleanup")
        .expect("shutdown VM");
    let result = result
        .expect("the pipeline must finish")
        .expect("pipeline runs");
    assert_eq!(result.exit_code, 0, "{result:?}");
    assert_eq!(result.stdout.trim(), "200000", "{result:?}");
}

/// Stdin written after a process exits has no reader, so the write fails with `EPIPE`, as a pipe
/// write does on Linux. Closing that stdin still succeeds.
#[tokio::test]
async fn stdin_written_after_a_process_exits_fails_with_epipe() {
    if !common::require_sidecar("stdin_written_after_a_process_exits_fails_with_epipe") {
        return;
    }
    let os = common::new_vm().await;
    let result = async {
        let process = os.spawn(
            "node",
            vec!["-e".into(), "process.exit(0)".into()],
            SpawnOptions::default(),
        )?;
        let exit = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            os.wait_process(process.pid),
        )
        .await
        .map_err(|_| anyhow::anyhow!("the process did not exit"))??;
        anyhow::ensure!(exit == 0, "the process exited with {exit}");
        let late = os
            .write_process_stdin(process.pid, StdinInput::Text("late\n".into()))
            .await;
        anyhow::ensure!(
            matches!(&late, Err(ClientError::Kernel { code, .. }) if code == "EPIPE"),
            "write after exit returned {late:?}, not EPIPE"
        );
        os.close_process_stdin(process.pid).await?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(10), os.shutdown())
        .await
        .expect("bounded VM cleanup")
        .expect("shutdown VM");
    result.unwrap();
}

/// A caller that does not stream stdin gets a process that reads EOF, as with `</dev/null`, so a
/// command that reads stdin cannot wait forever.
#[tokio::test]
async fn a_process_spawned_without_streamed_stdin_reads_eof() {
    if !common::require_sidecar("a_process_spawned_without_streamed_stdin_reads_eof") {
        return;
    }
    let os = common::new_vm().await;
    let result = async {
        let reader = os.spawn(
            "node",
            vec![
                "-e".into(),
                "process.stdin.resume(); process.stdin.on('end', () => process.exit(7))".into(),
            ],
            SpawnOptions {
                stream_stdin: Some(false),
                ..Default::default()
            },
        )?;
        let exit = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            os.wait_process(reader.pid),
        )
        .await
        .map_err(|_| anyhow::anyhow!("the reader never saw EOF on stdin"))??;
        anyhow::ensure!(exit == 7, "the reader exited with {exit}, not at EOF");
        Ok::<_, anyhow::Error>(())
    }
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(10), os.shutdown())
        .await
        .expect("bounded VM cleanup")
        .expect("shutdown VM");
    result.unwrap();
}

/// Stdin written after the caller closed it cannot reach the process, so the write fails with
/// `EBADF`, as a write to a closed descriptor does on Linux, instead of dropping the bytes.
#[tokio::test]
async fn stdin_written_after_it_is_closed_is_rejected() {
    const TEST: &str = "stdin_written_after_it_is_closed_is_rejected";
    if !common::require_sidecar(TEST) {
        return;
    }
    let Some(os) = common::new_vm_with_commands().await else {
        if common::allow_local_e2e_skips() {
            eprintln!("skipping {TEST}: coreutils package absent");
            return;
        }
        panic!("{TEST}: coreutils package absent");
    };
    let result = async {
        let process = os.spawn(
            "sh",
            vec![
                "-c".into(),
                "echo ready; cat >/dev/null; echo closed; sleep 30".into(),
            ],
            SpawnOptions::default(),
        )?;
        let (stdout_tx, mut stdout_rx) = tokio::sync::mpsc::unbounded_channel();
        let _output = os.on_process_output(process.pid, move |event| {
            if event.stream == agentos_client::ProcessStream::Stdout {
                let _ = stdout_tx.send(event.data);
            }
        })?;
        let mut stdout = Vec::new();
        anyhow::ensure!(
            read_stdout_until(&mut stdout_rx, &mut stdout, b"ready\n").await,
            "the process never started"
        );
        os.write_process_stdin(process.pid, StdinInput::Text("early\n".into()))
            .await?;
        os.close_process_stdin(process.pid).await?;
        anyhow::ensure!(
            read_stdout_until(&mut stdout_rx, &mut stdout, b"closed\n").await,
            "the process never saw EOF on stdin"
        );
        let late = os
            .write_process_stdin(process.pid, StdinInput::Text("late\n".into()))
            .await;
        anyhow::ensure!(
            matches!(&late, Err(ClientError::Kernel { code, .. }) if code == "EBADF"),
            "a write after close returned {late:?}, not EBADF"
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(10), os.shutdown())
        .await
        .expect("bounded VM cleanup")
        .expect("shutdown VM");
    result.unwrap();
}

/// Collects stdout until it ends with `marker`; `false` if that takes more than 10 seconds.
async fn read_stdout_until(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    stdout: &mut Vec<u8>,
    marker: &[u8],
) -> bool {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !stdout.ends_with(marker) {
            match rx.recv().await {
                Some(chunk) => stdout.extend(chunk),
                None => return false,
            }
        }
        true
    })
    .await
    .unwrap_or(false)
}

/// `trap '' TERM` makes a shell ignore SIGTERM, and `trap - TERM` restores the default action, as
/// on Linux. A trap set inside a subshell does not change the parent shell.
#[tokio::test]
async fn a_shell_ignores_sigterm_only_while_it_traps_it_with_an_empty_action() {
    const TEST: &str = "a_shell_ignores_sigterm_only_while_it_traps_it_with_an_empty_action";
    if !common::require_sidecar(TEST) {
        return;
    }
    let Some(vm) = common::new_vm_with_commands().await else {
        if common::allow_local_e2e_skips() {
            eprintln!("skipping {TEST}: coreutils package absent");
            return;
        }
        panic!("{TEST}: coreutils package absent");
    };
    let result = async {
        for (script, survives) in [
            ("trap '' TERM; echo ready; sleep 600", true),
            ("trap '' TERM; trap - TERM; echo ready; sleep 600", false),
            ("(trap '' TERM); echo ready; sleep 600", false),
        ] {
            let shell = spawn_shell_until_ready(&vm, script).await?;
            vm.signal_process_awaited(shell, "SIGTERM").await?;
            let exit =
                tokio::time::timeout(std::time::Duration::from_secs(2), vm.wait_process(shell))
                    .await;
            vm.signal_process_awaited(shell, "SIGKILL").await?;
            match (survives, exit) {
                (true, Err(_)) | (false, Ok(Ok(_))) => {}
                (true, Ok(exit)) => anyhow::bail!("`{script}` ended on SIGTERM: {exit:?}"),
                (false, exit) => anyhow::bail!("`{script}` survived SIGTERM: {exit:?}"),
            }
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(10), vm.shutdown())
        .await
        .expect("bounded VM cleanup")
        .expect("shutdown VM");
    result.unwrap();
}

/// A shell that traps SIGTERM with a command runs the trap once its foreground command finishes, as
/// on Linux. `wait` returns early for the trap, and subshells do not take the parent's signal.
#[tokio::test]
async fn a_shell_runs_its_sigterm_trap_after_the_foreground_command() {
    const TEST: &str = "a_shell_runs_its_sigterm_trap_after_the_foreground_command";
    if !common::require_sidecar(TEST) {
        return;
    }
    let Some(vm) = common::new_vm_with_commands().await else {
        if common::allow_local_e2e_skips() {
            eprintln!("skipping {TEST}: coreutils package absent");
            return;
        }
        panic!("{TEST}: coreutils package absent");
    };
    let result = async {
        for script in [
            "trap 'echo caught; exit 3' TERM; echo ready; sleep 2; echo after",
            "trap 'echo caught; exit 3' TERM; sleep 600 & echo ready; wait $!; echo after",
            "trap 'echo caught; exit 3' TERM; echo ready; while :; do x=$(sleep 0.2); done",
        ] {
            let shell = spawn_shell_until_ready(&vm, script).await?;
            vm.signal_process_awaited(shell, "SIGTERM").await?;
            let exit =
                tokio::time::timeout(std::time::Duration::from_secs(10), vm.wait_process(shell))
                    .await;
            let Ok(exit) = exit else {
                vm.signal_process_awaited(shell, "SIGKILL").await?;
                anyhow::bail!("`{script}` never ran its trap");
            };
            let exit = exit?;
            let output = vm.read_process_output(shell, None, None, None).await?;
            let stdout = output
                .events
                .into_iter()
                .flat_map(|event| event.data)
                .collect::<Vec<_>>();
            let stdout = String::from_utf8_lossy(&stdout);
            anyhow::ensure!(
                exit == 3 && stdout == "ready\ncaught\n",
                "`{script}`: expected the trap to run and exit 3, got exit {exit} with {stdout:?}"
            );
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(10), vm.shutdown())
        .await
        .expect("bounded VM cleanup")
        .expect("shutdown VM");
    result.unwrap();
}

/// Spawns `sh -c script` with retained output and waits until the script prints `ready`.
async fn spawn_shell_until_ready(
    vm: &agentos_client::AgentOs,
    script: &str,
) -> anyhow::Result<u32> {
    let shell = vm.spawn(
        "sh",
        vec!["-c".into(), script.into()],
        SpawnOptions {
            retain_output: true,
            ..Default::default()
        },
    )?;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let replay = vm.read_process_output(shell.pid, None, None, None).await?;
            if replay.events.iter().any(|event| event.data == b"ready\n") {
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("`{script}` never printed ready"))??;
    Ok(shell.pid)
}

#[tokio::test]
async fn exec_argv_timeout_confirms_node_guest_exit() {
    if !common::require_sidecar("exec_argv_timeout_confirms_node_guest_exit") {
        return;
    }
    let os = common::new_vm().await;
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        let args = vec!["-e".to_owned(), "setInterval(() => {}, 1000)".to_owned()];
        let run = os.exec_argv_process("node", &args, ExecOptions {
            timeout: Some(10_000.0),
            ..Default::default()
        });
        tokio::pin!(run);
        // Observe a concrete running kernel PID first. Checking only that all matching entries
        // are exited after the timeout would pass vacuously if snapshot matching were broken.
        let mut last_snapshot = Vec::new();
        let pid = loop {
            tokio::select! {
                result = &mut run => anyhow::bail!("run ended before a running guest was observed: {result:?}; last snapshot: {last_snapshot:?}"),
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                    last_snapshot = os.all_processes().await?;
                    if let Some(process) = last_snapshot.iter().find(|process| {
                        process.status == agentos_client::ProcessStatus::Running
                            && process.command == "node"
                            && process.args.iter().any(|arg| arg == "-e")
                    }) {
                        break process.pid;
                    }
                }
            }
        };
        let error = run.await.expect_err("long-running guest must time out");
        anyhow::ensure!(
            matches!(error.downcast_ref::<ClientError>(), Some(ClientError::ExecutionTimedOut { .. })),
            "timeout must confirm guest exit, not merely issue a signal: {error:#}"
        );
        let processes = os.all_processes().await?;
        anyhow::ensure!(
            processes.iter().filter(|process| process.pid == pid)
                .all(|process| process.status == agentos_client::ProcessStatus::Exited),
            "previously running PID {pid} must be exited or reaped: {processes:?}"
        );
        Ok::<_, anyhow::Error>(())
    }).await;
    tokio::time::timeout(std::time::Duration::from_secs(4), os.shutdown())
        .await
        .expect("VM teardown must not wait for the five-second reconciliation deadline")
        .expect("shutdown after timeout");
    outcome
        .expect("bounded timeout probe")
        .expect("timeout termination proof");
}

#[tokio::test]
async fn process_surface_exec_spawn_and_snapshot() {
    if !common::require_sidecar("process_surface_exec_spawn_and_snapshot") {
        return;
    }
    let os = common::new_vm_with_wasm_commands().await;

    // --- Runtime-independent process-management surface (no WASM needed) --------------------------
    // These execute real assertions against the real sidecar regardless of whether WASM command
    // packages are present: the SDK process registry starts empty, every map-backed operation on an
    // unknown pid returns ProcessNotFound, and the kernel process snapshot is always obtainable.
    const MISSING_PID: u32 = 999_999;
    assert!(
        os.list_processes().is_empty(),
        "a fresh VM has no SDK-spawned processes"
    );
    assert!(
        matches!(os.get_process(MISSING_PID), Err(ClientError::ProcessNotFound(p)) if p == MISSING_PID),
        "get_process(unknown) must return ProcessNotFound"
    );
    assert!(
        matches!(
            os.write_process_stdin(MISSING_PID, StdinInput::Text("x".to_string()))
                .await,
            Err(ClientError::ProcessNotFound(_))
        ),
        "write_process_stdin(unknown) must return ProcessNotFound"
    );
    assert!(
        matches!(
            os.close_process_stdin(MISSING_PID).await,
            Err(ClientError::ProcessNotFound(_))
        ),
        "close_process_stdin(unknown) must return ProcessNotFound"
    );
    assert!(
        matches!(
            os.stop_process(MISSING_PID),
            Err(ClientError::ProcessNotFound(_))
        ),
        "stop_process(unknown) must return ProcessNotFound"
    );
    assert!(
        matches!(
            os.kill_process(MISSING_PID),
            Err(ClientError::ProcessNotFound(_))
        ),
        "kill_process(unknown) must return ProcessNotFound"
    );
    assert!(
        matches!(
            os.on_process_output(MISSING_PID, |_| {}),
            Err(ClientError::ProcessNotFound(_))
        ),
        "on_process_output(unknown) must return ProcessNotFound"
    );
    assert!(
        matches!(
            os.wait_process(MISSING_PID).await,
            Err(ClientError::ProcessNotFound(_))
        ),
        "wait_process(unknown) must return ProcessNotFound"
    );
    // Kernel-wide process snapshot is always obtainable (no WASM required).
    let all = os.all_processes().await.expect("all_processes snapshot");
    let tree = os.process_tree().await.expect("process_tree snapshot");
    assert!(
        all.len() >= tree.len(),
        "the process forest cannot have more roots than total processes"
    );

    // Gate: probe for the WASM command toolchain. Bare `echo` with no args prints an empty line, so
    // a clean exit (code 0) is the availability signal even though stdout is just "\n".
    if !common::require_wasm_commands(&os, "process_surface_exec_spawn_and_snapshot").await {
        os.shutdown().await.expect("shutdown after local skip");
        return;
    }

    // --- exec: stdout + exit code -----------------------------------------------------------------
    // `exec` forwards only the `command` field (no args), so to get deterministic stdout we use a
    // command that echoes its stdin: `cat` round-trips its input to stdout and exits 0 on EOF.
    let echoed = os
        .exec_process(
            "cat",
            ExecOptions {
                stdin: Some(StdinInput::Text("hello-stdout".to_string())),
                ..Default::default()
            },
        )
        .await
        .expect("exec cat");
    assert_eq!(echoed.exit_code, 0, "cat should exit 0");
    assert_eq!(
        echoed.stdout, "hello-stdout",
        "cat must echo its stdin verbatim to stdout"
    );
    assert!(echoed.stderr.is_empty(), "cat should not write stderr");

    // --- exec: streaming on_stdout callback fires with raw bytes ----------------------------------
    let streamed = Arc::new(Mutex::new(Vec::<u8>::new()));
    let streamed_cb = Arc::clone(&streamed);
    let res = os
        .exec_process(
            "cat",
            ExecOptions {
                stdin: Some(StdinInput::Text("stream-me".to_string())),
                on_stdout: Some(Box::new(move |chunk: &[u8]| {
                    streamed_cb.lock().unwrap().extend_from_slice(chunk);
                })),
                ..Default::default()
            },
        )
        .await
        .expect("exec cat with on_stdout");
    assert_eq!(res.exit_code, 0);
    assert_eq!(
        &*streamed.lock().unwrap(),
        b"stream-me",
        "on_stdout must receive the streamed bytes during the exec"
    );

    // --- exec: binary stdout round-trip (non-UTF-8 bytes survive) ---------------------------------
    // `exec` decodes stdout lossily into a String, so we assert the byte-exact path via the
    // on_stdout callback (which delivers raw bytes), feeding non-UTF-8 input through `cat`.
    let binary: Vec<u8> = vec![0, 159, 146, 150, 255, 254, 1, 2, 3];
    let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
    let captured_cb = Arc::clone(&captured);
    let bin_input = binary.clone();
    let res = os
        .exec_process(
            "cat",
            ExecOptions {
                stdin: Some(StdinInput::Bytes(bin_input)),
                on_stdout: Some(Box::new(move |chunk: &[u8]| {
                    captured_cb.lock().unwrap().extend_from_slice(chunk);
                })),
                ..Default::default()
            },
        )
        .await
        .expect("exec cat binary");
    assert_eq!(res.exit_code, 0);
    assert_eq!(
        &*captured.lock().unwrap(),
        &binary,
        "binary stdout must round-trip byte-for-byte through on_stdout"
    );

    // --- spawn: pid + stdin write + stdout stream + exit wait -------------------------------------
    let handle = os
        .spawn("cat", Vec::new(), SpawnOptions::default())
        .expect("spawn cat");
    assert!(
        handle.pid >= 1_000_000,
        "spawn pid is drawn from the synthetic pid space (>= SYNTHETIC_PID_BASE), got {}",
        handle.pid
    );

    // Subscribe to stdout BEFORE writing so no output is missed.
    let (stdout_tx, mut stdout_rx) = tokio::sync::mpsc::unbounded_channel();
    let _output = os
        .on_process_output(handle.pid, move |event| {
            if event.stream == agentos_client::ProcessStream::Stdout {
                let _ = stdout_tx.send(event.data);
            }
        })
        .expect("subscribe spawn output");

    // get_process / list_processes reflect the live SDK process.
    let info = os.get_process(handle.pid).expect("get_process");
    assert_eq!(info.pid, handle.pid);
    assert_eq!(info.command, "cat");
    assert!(info.running, "freshly spawned process should be running");
    assert!(
        os.list_processes().iter().any(|p| p.pid == handle.pid),
        "spawned process must appear in list_processes"
    );

    // Write to stdin, then close it so `cat` sees EOF and exits.
    os.write_process_stdin(handle.pid, StdinInput::Text("spawned-input".to_string()))
        .await
        .expect("write stdin");
    os.close_process_stdin(handle.pid)
        .await
        .expect("close stdin");

    // Collect the expected stdout bytes. The stdout subscription is a live multi-subscriber stream,
    // so process exit is observed through wait_process rather than channel closure.
    let expected_spawn_stdout = b"spawned-input";
    let collected = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut buf = Vec::<u8>::new();
        while buf.len() < expected_spawn_stdout.len() {
            let Some(chunk) = stdout_rx.recv().await else {
                break;
            };
            buf.extend_from_slice(&chunk);
        }
        buf
    })
    .await
    .expect("spawn stdout did not produce expected bytes within timeout");
    assert_eq!(
        collected, expected_spawn_stdout,
        "spawned cat must echo the written stdin to its stdout stream"
    );

    // wait_process resolves with the exit code (cat exits 0 on clean EOF).
    let exit_code = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        os.wait_process(handle.pid),
    )
    .await
    {
        Ok(result) => result.expect("wait_process"),
        Err(error) => {
            let sdk_process = os.get_process(handle.pid);
            let kernel_processes = os.all_processes().await;
            panic!(
                "wait_process timed out: {error}; sdk_process={sdk_process:?}; kernel_processes={kernel_processes:?}"
            );
        }
    };
    assert_eq!(exit_code, 0, "cat should exit 0 after EOF");

    // --- kernel snapshot: all_processes / process_tree -------------------------------------------
    // The snapshot is a kernel-wide view. It must at least be obtainable and well-formed; every node
    // in the tree must correspond to a process in the flat list (tree is built purely from the list).
    let all = os.all_processes().await.expect("all_processes");
    let tree = os.process_tree().await.expect("process_tree");
    assert!(
        all.len() >= tree.len(),
        "the process forest cannot contain more roots than total processes"
    );
    // Every tree node must correspond to an entry in the flat list (the forest is built purely from
    // it), and pgid/sid are self-consistent for the roots.
    let flat_pids: std::collections::BTreeSet<u32> = all.iter().map(|p| p.pid).collect();
    for root in &tree {
        assert!(
            flat_pids.contains(&root.info.pid),
            "every process_tree root must exist in all_processes"
        );
    }

    os.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn concurrent_launches_on_a_fresh_vm_all_run() {
    if !common::require_sidecar("concurrent_launches_on_a_fresh_vm_all_run") {
        return;
    }
    let Some(os) = common::new_vm_with_commands().await else {
        if common::allow_local_e2e_skips() {
            eprintln!(
                "skipping concurrent_launches_on_a_fresh_vm_all_run: coreutils package absent"
            );
            return;
        }
        panic!("concurrent_launches_on_a_fresh_vm_all_run: coreutils package absent");
    };
    // A fresh VM is cold: the first WebAssembly and Python launches await runtime setup while
    // the others arrive, so every launch here overlaps another one of the same runtime.
    let launches: Vec<(&str, Vec<String>)> = (0..3)
        .map(|index| ("echo", vec![format!("wasm-{index}")]))
        .chain((0..3).map(|index| {
            (
                "python3",
                vec!["-c".to_owned(), format!("print('python-{index}')")],
            )
        }))
        .collect();
    let result = tokio::time::timeout(std::time::Duration::from_secs(120), async {
        let mut handles = Vec::new();
        for (command, args) in &launches {
            handles.push(os.spawn(command, args.clone(), SpawnOptions::default())?);
        }
        let mut outcomes = Vec::new();
        for ((command, args), handle) in launches.iter().zip(&handles) {
            outcomes.push((command, args, os.wait_process(handle.pid).await));
        }
        let failed: Vec<_> = outcomes
            .iter()
            .filter(|(_, _, outcome)| !matches!(outcome, Ok(0)))
            .collect();
        anyhow::ensure!(
            failed.is_empty(),
            "every concurrent launch must run to exit 0; failed: {failed:?}"
        );
        Ok::<_, anyhow::Error>(())
    })
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(10), os.shutdown())
        .await
        .expect("bounded VM cleanup")
        .expect("shutdown VM");
    result
        .expect("concurrent launches finish within the timeout")
        .expect("concurrent launches all run");
}

#[tokio::test]
async fn shell_children_of_every_runtime_run_to_completion() {
    if !common::require_sidecar("shell_children_of_every_runtime_run_to_completion") {
        return;
    }
    let Some(os) = common::new_vm_with_commands().await else {
        if common::allow_local_e2e_skips() {
            eprintln!(
                "skipping shell_children_of_every_runtime_run_to_completion: coreutils package absent"
            );
            return;
        }
        panic!("shell_children_of_every_runtime_run_to_completion: coreutils package absent");
    };
    // The shell waits on each child before it runs the next line, so every line of output
    // proves the previous child ran, reported its exit status, and let the shell continue.
    let script = [
        r#"python3 -c "print(6*7)""#,
        r#"echo 'print(1+1)' | python3 -"#,
        r#"python3 -c "raise ValueError" 2>/dev/null; echo "python-exit-$?""#,
        "echo nested > /tmp/nested.txt",
        r#"sh -c "cat /tmp/nested.txt""#,
    ]
    .join("\n");
    let result = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        let handle = os.spawn(
            "sh",
            vec!["-c".to_owned(), script],
            SpawnOptions {
                retain_output: true,
                ..Default::default()
            },
        )?;
        let exit_code = os.wait_process(handle.pid).await?;
        let stdout = os
            .read_process_output(handle.pid, None, None, None)
            .await?
            .events
            .into_iter()
            .filter(|event| event.stream == agentos_client::ProcessStream::Stdout)
            .flat_map(|event| event.data)
            .collect::<Vec<_>>();
        Ok::<_, anyhow::Error>((exit_code, String::from_utf8_lossy(&stdout).into_owned()))
    })
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(10), os.shutdown())
        .await
        .expect("bounded VM cleanup")
        .expect("shutdown VM");
    let (exit_code, stdout) = result
        .expect("the shell and its children finish within the timeout")
        .expect("the shell runs");
    assert_eq!(stdout, "42\n2\npython-exit-1\nnested\n");
    assert_eq!(exit_code, 0);
}

#[tokio::test]
async fn output_read_right_after_a_failed_spawn_returns_the_launch_error() {
    if !common::require_sidecar("output_read_right_after_a_failed_spawn_returns_the_launch_error") {
        return;
    }
    let os = common::new_vm().await;
    let result = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let launch = os.spawn(
            "agentos-nonexistent-review-command",
            Vec::new(),
            SpawnOptions {
                retain_output: true,
                ..Default::default()
            },
        )?;
        // Read before the launch has settled: the caller must get the launch error, not a
        // missing-process error from asking the sidecar before it knows the process.
        let read = os.read_process_output(launch.pid, None, None, None).await;
        anyhow::ensure!(
            matches!(&read, Err(ClientError::Kernel { code, .. }) if code == "ENOENT"),
            "an output read right after a failed spawn must return the launch error: {read:?}"
        );
        Ok::<_, anyhow::Error>(())
    })
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(10), os.shutdown())
        .await
        .expect("bounded VM cleanup")
        .expect("shutdown VM");
    result
        .expect("output read finishes within the timeout")
        .expect("output read returns the launch error");
}
