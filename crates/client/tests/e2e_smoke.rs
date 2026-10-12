//! Smoke e2e: the client spawns a real `agentos-sidecar`, runs the full create handshake, does a
//! filesystem round-trip through the kernel VFS, and shuts down cleanly.
//!
//! Filesystem ops are used (not `exec`) because they go straight through the kernel VFS and do not
//! require WASM command packages, which are not checked into git.
//!
//! Requires the sidecar binary. Resolve order: `AGENTOS_SIDECAR_BIN`, then `CARGO_TARGET_DIR`, else
//! `<workspace>/target/debug/agentos-sidecar`. Build it first: `cargo build -p agentos-sidecar`.

use std::path::PathBuf;

use agentos_client::config::AgentOsConfig;
use agentos_client::fs::FileContent;
use agentos_client::AgentOs;

fn sidecar_bin() -> PathBuf {
    if let Ok(path) = std::env::var("AGENTOS_SIDECAR_BIN") {
        return PathBuf::from(path);
    }
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(path)
            }
        })
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .join("debug/agentos-sidecar")
}

#[tokio::test]
async fn smoke_connect_and_filesystem_round_trip() {
    let bin = sidecar_bin();
    assert!(
        bin.exists(),
        "sidecar binary not found at {} (run: cargo build -p agentos-sidecar)",
        bin.display()
    );
    std::env::set_var("AGENTOS_SIDECAR_BIN", &bin);

    let os = AgentOs::create(AgentOsConfig::default())
        .await
        .expect("create VM against real sidecar");

    os.write_file(
        "/tmp/smoke.txt",
        FileContent::Bytes(b"hello agent-os".to_vec()),
    )
    .await
    .expect("write_file");

    let contents = os.read_file("/tmp/smoke.txt").await.expect("read_file");
    assert_eq!(contents, b"hello agent-os");

    os.shutdown().await.expect("shutdown");
}
