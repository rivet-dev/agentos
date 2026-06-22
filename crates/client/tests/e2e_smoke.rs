//! Smoke e2e: the client spawns a real `agentos-sidecar`, runs the full create handshake, does a
//! filesystem round-trip through the kernel VFS, and shuts down cleanly.
//!
//! Filesystem ops are used (not `exec`) because they go straight through the kernel VFS and do not
//! require WASM command packages, which are not checked into git.
//!
//! Requires the sidecar binary. Resolve order: `AGENT_OS_SIDECAR_BIN`, else
//! `<workspace>/target/debug/agentos-sidecar`. Build it first: `cargo build -p agentos-sidecar`.

use std::path::PathBuf;

use agentos_client::config::AgentOsConfig;
use agentos_client::fs::FileContent;
use agentos_client::AgentOs;

fn sidecar_bin() -> PathBuf {
    if let Ok(path) = std::env::var("AGENT_OS_SIDECAR_BIN") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/agentos-sidecar")
}

#[tokio::test]
async fn smoke_connect_and_filesystem_round_trip() {
    let bin = sidecar_bin();
    assert!(
        bin.exists(),
        "sidecar binary not found at {} (run: cargo build -p agentos-sidecar)",
        bin.display()
    );
    std::env::set_var("AGENT_OS_SIDECAR_BIN", &bin);

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
