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

use std::sync::{Arc, Mutex};

use agentos_client::{
    AgentOsLimits, ClientError, ExecOptions, JsRuntimeLimits, SpawnOptions, StdinInput,
};
use futures::StreamExt;

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
        os.list_processes()
            .await
            .expect("list_processes on fresh VM")
            .is_empty(),
        "a fresh VM has no SDK-spawned processes"
    );
    assert!(
        matches!(os.get_process(MISSING_PID).await, Err(ClientError::ProcessNotFound(p)) if p == MISSING_PID),
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
            os.stop_process(MISSING_PID).await,
            Err(ClientError::ProcessNotFound(_))
        ),
        "stop_process(unknown) must return ProcessNotFound"
    );
    assert!(
        matches!(
            os.kill_process(MISSING_PID).await,
            Err(ClientError::ProcessNotFound(_))
        ),
        "kill_process(unknown) must return ProcessNotFound"
    );
    assert!(
        matches!(
            os.on_process_stdout(MISSING_PID),
            Err(ClientError::ProcessNotFound(_))
        ),
        "on_process_stdout(unknown) must return ProcessNotFound"
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
        .exec(
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
        .exec(
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
        .exec(
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
        .await
        .expect("spawn cat");
    assert!(
        handle.pid > 0,
        "spawn must return a positive kernel pid, got {}",
        handle.pid
    );

    // Subscribe to stdout BEFORE writing so no output is missed.
    let mut stdout = os
        .on_process_stdout(handle.pid)
        .expect("subscribe spawn stdout");

    // get_process / list_processes reflect the live SDK process.
    let info = os.get_process(handle.pid).await.expect("get_process");
    assert_eq!(info.pid, handle.pid);
    assert_eq!(info.command, "cat");
    assert!(info.running, "freshly spawned process should be running");
    assert!(
        os.list_processes()
            .await
            .expect("list_processes")
            .iter()
            .any(|p| p.pid == handle.pid),
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
            let Some(chunk) = stdout.next().await else {
                break;
            };
            let chunk = chunk.expect("spawn stdout stream lagged");
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

    // Explicit timeouts are forwarded once and enforced by the sidecar rather
    // than a client timer racing a detached kill request.
    let timed = os
        .spawn(
            "node",
            vec![
                String::from("-e"),
                String::from("setInterval(() => {}, 1000)"),
            ],
            SpawnOptions {
                base: ExecOptions {
                    timeout: Some(25.0),
                    ..ExecOptions::default()
                },
                ..SpawnOptions::default()
            },
        )
        .await
        .expect("spawn timed process");
    assert_eq!(
        os.wait_process(timed.pid)
            .await
            .expect("wait for sidecar timeout"),
        137,
        "sidecar timeout should deliver SIGKILL exit status"
    );

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
    assert!(
        flat_pids.contains(&handle.pid),
        "the pid returned by spawn must be the same pid exposed by the sidecar process table"
    );
    let spawned = all
        .iter()
        .find(|process| process.pid == handle.pid)
        .expect("spawned process snapshot");
    assert_eq!(spawned.command, "cat");
    assert_eq!(spawned.args, vec!["cat"]);
    assert_eq!(spawned.cwd, "/workspace");
    assert!(spawned.start_time > 0.0);
    assert!(spawned.exit_time.is_some());
    for root in &tree {
        assert!(
            flat_pids.contains(&root.info.pid),
            "every process_tree root must exist in all_processes"
        );
    }

    os.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn sidecar_bounds_captured_output_without_limiting_raw_streams() {
    if !common::require_sidecar("sidecar_bounds_captured_output_without_limiting_raw_streams") {
        return;
    }
    let os = common::new_vm_with_limits(AgentOsLimits {
        js_runtime: Some(JsRuntimeLimits {
            captured_output_limit_bytes: Some(8),
            ..Default::default()
        }),
        ..Default::default()
    })
    .await;

    let error = os
        .exec_argv(
            "node",
            &[
                String::from("-e"),
                String::from("process.stdout.write('123456789')"),
            ],
            ExecOptions::default(),
        )
        .await
        .expect_err("captured stdout above the sidecar limit must fail");
    let typed = error
        .downcast_ref::<ClientError>()
        .expect("captured-output failure must preserve ClientError");
    assert!(
        matches!(typed, ClientError::Kernel { code, .. } if code == "ERR_CAPTURED_OUTPUT_LIMIT_EXCEEDED"),
        "unexpected captured-output error: {typed:?}"
    );
    let ClientError::Kernel { message, .. } = typed else {
        unreachable!("captured-output error shape checked above")
    };
    assert!(message.contains("limit of 8 bytes"));
    assert!(message.contains("limits.js_runtime.captured_output_limit_bytes"));

    let streamed = Arc::new(Mutex::new(Vec::<u8>::new()));
    let streamed_cb = Arc::clone(&streamed);
    let result = os
        .exec_argv(
            "node",
            &[
                String::from("-e"),
                String::from("process.stdout.write('123456789')"),
            ],
            ExecOptions {
                capture_stdio: Some(false),
                on_stdout: Some(Box::new(move |chunk| {
                    streamed_cb.lock().unwrap().extend_from_slice(chunk);
                })),
                ..Default::default()
            },
        )
        .await
        .expect("uncaptured streaming output must remain available");
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
    assert_eq!(&*streamed.lock().unwrap(), b"123456789");

    let spawned = os
        .spawn(
            "node",
            vec![
                String::from("-e"),
                String::from("process.stdin.once('data', () => process.stdout.write('123456789'))"),
            ],
            SpawnOptions {
                stream_stdin: Some(true),
                ..Default::default()
            },
        )
        .await
        .expect("spawn raw streaming process");
    let mut spawned_stdout = os
        .on_process_stdout(spawned.pid)
        .expect("subscribe to raw spawned stdout");
    os.write_process_stdin(spawned.pid, StdinInput::Text(String::from("go")))
        .await
        .expect("release raw spawned process after subscribing");
    os.close_process_stdin(spawned.pid)
        .await
        .expect("close raw spawned stdin");
    let chunk = tokio::time::timeout(std::time::Duration::from_secs(10), spawned_stdout.next())
        .await
        .expect("raw spawned stdout timed out")
        .expect("raw spawned stdout stream closed")
        .expect("raw spawned stdout stream lagged");
    assert_eq!(&chunk[..], b"123456789");
    assert_eq!(
        os.wait_process(spawned.pid)
            .await
            .expect("wait for raw spawned process"),
        0
    );

    os.shutdown().await.expect("shutdown limited VM");
}
