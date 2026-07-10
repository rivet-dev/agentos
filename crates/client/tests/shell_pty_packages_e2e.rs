//! PTY shell round-trip against a packed `.aospkg` boot package — the exact
//! surface the actor plugin drives for the shell CLI's `--actor` mode:
//! `packages` config → sidecar `/opt/agentos` projection → `open_shell` (TTY)
//! → `write_shell` input (cooked echo) → `on_shell_data` output → `wait_shell`
//! exit code. Prefers the toolchain-built `package.aospkg` (the production
//! artifact); falls back to the unpacked transition dir. Skips cleanly when
//! neither is built.

mod common;

use std::path::PathBuf;

use agentos_client::{AgentOs, AgentOsConfig, OpenShellOptions, PackageRef, StdinInput};
use futures::StreamExt;

fn coreutils_package_path() -> Option<PathBuf> {
    // Prefer the packed .aospkg — the artifact registry packages actually ship.
    for aospkg in [
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../software/coreutils/dist/package.aospkg"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../node_modules/@agentos-software/coreutils/dist/package.aospkg"),
    ] {
        if aospkg.is_file() {
            return std::fs::canonicalize(aospkg).ok();
        }
    }
    for dir in [
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../software/coreutils/dist/package"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../node_modules/@agentos-software/coreutils/dist/package"),
    ] {
        if dir.join("agentos-package.json").is_file() {
            return std::fs::canonicalize(dir).ok();
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread")]
async fn pty_shell_round_trip_via_boot_packages() {
    if !common::require_sidecar("pty_shell_round_trip_via_boot_packages") {
        return;
    }
    let Some(package_path) = coreutils_package_path() else {
        eprintln!("skipping: coreutils package is not built");
        return;
    };

    common::ensure_sidecar_env();
    let os = AgentOs::create(AgentOsConfig {
        packages: vec![PackageRef {
            path: package_path.to_string_lossy().into_owned(),
        }],
        ..Default::default()
    })
    .await
    .expect("create VM with boot packages");

    let shell = os
        .open_shell(OpenShellOptions {
            command: Some(String::from("sh")),
            args: vec![
                String::from("-c"),
                String::from("echo before-read; read line; echo got:$line"),
            ],
            cwd: Some(String::from("/")),
            cols: Some(80),
            rows: Some(24),
            ..Default::default()
        })
        .expect("open_shell");

    let mut data = os
        .on_shell_data(&shell.shell_id)
        .expect("on_shell_data subscription");

    // Wait for the guest to reach its read() before writing.
    let saw_before = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let mut acc = Vec::<u8>::new();
        while let Some(chunk) = data.next().await {
            acc.extend_from_slice(&chunk);
            if String::from_utf8_lossy(&acc).contains("before-read") {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(saw_before, "guest banner must arrive on the data stream");

    // Give the guest time to enter its blocking stdin read before writing, so
    // this test also covers the delayed-write path (a host write must not be
    // starved by the guest's in-flight blocking read).
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    os.write_shell(
        &shell.shell_id,
        StdinInput::Text(String::from("marker-input\n")),
    )
    .expect("write_shell");

    // The cooked PTY must deliver the line to the guest's read() and surface
    // `got:marker-input` (plus the input echo) on the data stream.
    let saw_roundtrip = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let mut acc = Vec::<u8>::new();
        while let Some(chunk) = data.next().await {
            acc.extend_from_slice(&chunk);
            if String::from_utf8_lossy(&acc).contains("got:marker-input") {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(
        saw_roundtrip,
        "write_shell input must reach the guest read() and its output must surface"
    );

    let exit_code = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        os.wait_shell(&shell.shell_id),
    )
    .await
    .expect("wait_shell must resolve after the guest exits")
    .expect("wait_shell result");
    assert_eq!(exit_code, 0, "clean guest exit");

    os.shutdown().await.expect("shutdown");
}
