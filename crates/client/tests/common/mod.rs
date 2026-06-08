//! Shared e2e helpers: resolve/point at the real `agent-os-sidecar` binary and build VMs.
//!
//! Resolve order for the binary: `AGENT_OS_SIDECAR_BIN`, else `<workspace>/target/debug/agent-os-sidecar`.
//! Build it first with `cargo build -p agent-os-sidecar`.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Once;

use agent_os_client::config::AgentOsConfig;
use agent_os_client::AgentOs;

static INIT: Once = Once::new();

pub fn ensure_sidecar_env() {
    INIT.call_once(|| {
        if std::env::var("AGENT_OS_SIDECAR_BIN").is_err() {
            let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/debug/agent-os-sidecar");
            std::env::set_var("AGENT_OS_SIDECAR_BIN", bin);
        }
    });
}

/// Whether the sidecar binary is present. e2e tests skip (return early) when it is not, so the suite
/// stays honest in environments where the binary was not built.
pub fn sidecar_available() -> bool {
    ensure_sidecar_env();
    std::env::var("AGENT_OS_SIDECAR_BIN")
        .map(|path| PathBuf::from(path).exists())
        .unwrap_or(false)
}

/// Create a VM with default config against the real sidecar.
pub async fn new_vm() -> AgentOs {
    ensure_sidecar_env();
    AgentOs::create(AgentOsConfig::default())
        .await
        .expect("create VM against real sidecar")
}

/// Locate the coreutils wasm command directory under the workspace `node_modules`. Returns its
/// canonical absolute path, or `None` when the artifacts have not been installed/built.
pub fn coreutils_wasm_dir() -> Option<PathBuf> {
    let pnpm = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../node_modules/.pnpm");
    for entry in std::fs::read_dir(&pnpm).ok()?.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("@rivet-dev+agent-os-coreutils@")
        {
            let wasm = entry
                .path()
                .join("node_modules/@rivet-dev/agent-os-coreutils/wasm");
            if wasm.is_dir() {
                return std::fs::canonicalize(&wasm).ok();
            }
        }
    }
    None
}

/// Create a VM with the coreutils wasm command package mounted, so `exec`/`spawn` can resolve real
/// commands (`echo`, `cat`, `sh`, ...). Returns `None` when the wasm artifacts are absent, so suites
/// can skip cleanly in unbuilt trees.
pub async fn new_vm_with_commands() -> Option<AgentOs> {
    ensure_sidecar_env();
    let wasm_dir = coreutils_wasm_dir()?;
    let config = AgentOsConfig {
        software: vec![agent_os_client::SoftwareInput {
            package: wasm_dir.to_string_lossy().into_owned(),
            version: None,
            kind: agent_os_client::SoftwareKind::WasmCommands,
        }],
        ..Default::default()
    };
    Some(
        AgentOs::create(config)
            .await
            .expect("create VM with coreutils command software"),
    )
}

/// Probe whether WASM-backed commands resolve in the VM (a trivial `exec`). Returns false when the
/// registry WASM command packages are absent (the common case in unbuilt trees), so the
/// process/shell/fetch suites can gate cleanly without each re-implementing the probe.
pub async fn wasm_commands_available(os: &AgentOs) -> bool {
    os.exec("sh", agent_os_client::ExecOptions::default())
        .await
        .is_ok()
}
