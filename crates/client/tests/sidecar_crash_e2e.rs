//! A shared sidecar process can die (OOM kill, crash, silence watchdog). VMs created afterwards in
//! the same pool must get a working sidecar, and VMs that lived in the dead process must fail fast
//! and still shut down. Kept in its own test binary: it kills sidecar children of this process.

mod common;

use std::time::Duration;

use agentos_client::fs::FileContent;
use agentos_client::{ClientError, SpawnOptions};

#[tokio::test]
async fn a_pool_recovers_after_its_sidecar_process_dies() {
    if !common::require_sidecar("a_pool_recovers_after_its_sidecar_process_dies") {
        return;
    }
    let pool = format!("crash-{}", std::process::id());
    let orphan = common::new_vm_with_sidecar_pool(pool.clone()).await;
    orphan
        .write_file("/tmp/who", FileContent::Text("orphan".to_string()))
        .await
        .expect("write before the crash");
    let running = orphan
        .spawn(
            "node",
            vec![
                "-e".into(),
                "console.log('ready'); setInterval(() => {}, 1000)".into(),
            ],
            SpawnOptions {
                retain_output: true,
                ..Default::default()
            },
        )
        .expect("spawn before the crash");
    tokio::time::timeout(Duration::from_secs(20), async {
        while orphan
            .read_process_output(running.pid, None, None, None)
            .await
            .expect("read output before the crash")
            .events
            .is_empty()
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the process starts before the crash");

    let killed = kill_sidecar_children();
    assert_eq!(killed, 1, "expected exactly one sidecar child to kill");

    let orphan_call = tokio::time::timeout(Duration::from_secs(5), orphan.read_file("/tmp/who"))
        .await
        .expect("a call on a VM whose sidecar died must fail fast, not hang");
    assert!(
        orphan_call.is_err(),
        "a VM whose sidecar died cannot serve calls"
    );
    let orphan_wait =
        tokio::time::timeout(Duration::from_secs(5), orphan.wait_process(running.pid))
            .await
            .expect("waiting on a process whose sidecar died must fail fast, not hang");
    assert!(
        matches!(orphan_wait, Err(ClientError::SidecarExited(_))),
        "a process whose sidecar died has no exit code: {orphan_wait:?}"
    );

    let fresh = tokio::time::timeout(
        Duration::from_secs(30),
        common::new_vm_with_sidecar_pool(pool.clone()),
    )
    .await
    .expect("a new VM in the pool must not wait on the dead sidecar");
    fresh
        .write_file("/tmp/who", FileContent::Text("fresh".to_string()))
        .await
        .expect("a new VM in the pool works after the crash");
    assert_eq!(fresh.read_file("/tmp/who").await.expect("read"), b"fresh");

    tokio::time::timeout(Duration::from_secs(10), orphan.shutdown())
        .await
        .expect("shutting down a VM whose sidecar died must not hang")
        .expect("a VM whose sidecar died shuts down cleanly");
    fresh.shutdown().await.expect("shutdown the fresh VM");
}

/// SIGKILL every direct child of this test process that runs the sidecar binary.
fn kill_sidecar_children() -> usize {
    let binary = std::fs::canonicalize(std::env::var("AGENTOS_SIDECAR_BIN").expect("sidecar env"))
        .expect("canonical sidecar path");
    let me = std::process::id();
    let mut killed = 0;
    for entry in std::fs::read_dir("/proc").expect("read /proc").flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        // Field 4 (ppid) follows the parenthesized command name.
        let ppid = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(1))
            .and_then(|ppid| ppid.parse::<u32>().ok());
        let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
        if ppid == Some(me) && exe.as_deref() == Some(binary.as_path()) {
            let status = std::process::Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status()
                .expect("run kill");
            assert!(status.success(), "kill sidecar {pid}");
            killed += 1;
        }
    }
    killed
}
