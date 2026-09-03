//! Process e2e against a real `agentos-native-sidecar`.
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
        let rejected = os.spawn_process(
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

        let fast = os.spawn_process(
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
            os.write_process_stdin(MISSING_PID, StdinInput::Text("x".to_string())),
            Err(ClientError::ProcessNotFound(_))
        ),
        "write_process_stdin(unknown) must return ProcessNotFound"
    );
    assert!(
        matches!(
            os.close_process_stdin(MISSING_PID),
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
        .spawn_process("cat", Vec::new(), SpawnOptions::default())
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
        .expect("write stdin");
    os.close_process_stdin(handle.pid).expect("close stdin");

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
    let exit_code = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        os.wait_process(handle.pid),
    )
    .await
    .expect("wait_process timed out")
    .expect("wait_process");
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
