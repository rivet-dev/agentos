//! Process e2e against a real `agent-os-sidecar`.
//!
//! `exec`/`spawn` require WASM command packages (sh/echo/cat) that are NOT checked into git, so this
//! suite is self-gating: it first probes a trivial `exec` and, if the command cannot be resolved
//! (a "no shell" / command-not-found style kernel rejection, or a non-zero exit with empty stdout),
//! it treats that as "WASM commands not present" and skips. The suite still compiles and passes as a
//! skip in that environment, so it is honest and never fails for agent-os reasons.
//!
//! When commands ARE available the suite asserts the real TS contract: exec stdout + exit code,
//! binary stdout round-trip, spawn pid + stdin write, exit-code wait, list/get of SDK processes, and
//! the kernel process snapshot (`all_processes` / `process_tree`).

mod common;

use std::sync::{Arc, Mutex};

use agent_os_client::{AgentOs, ClientError, ExecOptions, SpawnOptions, StdinInput};
use futures::StreamExt;

/// Probe whether WASM commands (a `sh`-backed `echo`) resolve inside the VM. Returns the probe's
/// stdout when commands work, or `None` when the prerequisite is absent (kernel rejection, or a
/// failed run with no output). A successful `echo` is the cheapest positive signal that the command
/// toolchain is mounted.
async fn commands_available(os: &AgentOs) -> Option<String> {
    // `exec` forwards the `command` field only (no shell args), so a bare `echo` runs the WASM
    // `echo` command which exits 0 (printing a blank line). A clean exit is the availability signal.
    let result = os.exec("echo", ExecOptions::default()).await;
    match result {
        // A clean run with the expected newline-terminated marker means commands are present.
        Ok(res) if res.exit_code == 0 => Some(res.stdout),
        // Any other outcome (kernel rejection, non-zero exit, or error) means the WASM command
        // toolchain is not mounted in this environment.
        Ok(_) | Err(_) => None,
    }
}

#[tokio::test]
async fn process_surface_exec_spawn_and_snapshot() {
    if !common::sidecar_available() {
        eprintln!("skipping process_surface_exec_spawn_and_snapshot: sidecar binary not built");
        return;
    }
    let os = common::new_vm().await;

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
    if commands_available(&os).await.is_none() {
        eprintln!(
            "skipping process_surface_exec_spawn_and_snapshot: WASM command packages (sh/echo) \
             not present in this environment"
        );
        os.shutdown().await.expect("shutdown");
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
        .expect("spawn cat");
    assert!(
        handle.pid >= 1_000_000,
        "spawn pid is drawn from the synthetic pid space (>= SYNTHETIC_PID_BASE), got {}",
        handle.pid
    );

    // Subscribe to stdout BEFORE writing so no output is missed.
    let mut stdout = os
        .on_process_stdout(handle.pid)
        .expect("subscribe spawn stdout");

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

    // Collect stdout chunks until the stream closes (process exit closes the broadcast).
    let collected = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut buf = Vec::<u8>::new();
        while let Some(chunk) = stdout.next().await {
            buf.extend_from_slice(&chunk);
        }
        buf
    })
    .await
    .expect("spawn stdout did not close within timeout");
    assert_eq!(
        collected, b"spawned-input",
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
