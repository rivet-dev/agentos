//! Agent-registry forwarding e2e against a real `agentos-sidecar`.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use agentos_client::config::{AgentOsConfig, PackageRef};
use agentos_client::AgentOs;

const MOCK_AGENT_TYPE: &str = "registry-mock-agent";

fn unique_dir() -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("agentos-registry-e2e-{nonce}"));
    std::fs::create_dir_all(&root).expect("create temp package root");
    root
}

fn write_mock_agent_package(root: &std::path::Path) -> PathBuf {
    let package = root.join("registry-mock-agent-package");
    std::fs::create_dir_all(package.join("bin")).expect("create package bin");
    std::fs::write(
        package.join("agentos-package.json"),
        r#"{"name":"registry-mock-agent","version":"1.0.0","agent":{"acpEntrypoint":"registry-mock-agent-acp"}}"#,
    )
    .expect("write agentos-package.json");
    let bin = package.join("bin/registry-mock-agent-acp");
    std::fs::write(&bin, "#!/usr/bin/env node\nprocess.stdin.resume();\n")
        .expect("write mock adapter");
    let mut permissions = std::fs::metadata(&bin)
        .expect("stat mock adapter")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&bin, permissions).expect("chmod mock adapter");
    package
}

#[tokio::test]
async fn list_agents_forwards_resolved_adapter_entrypoint() {
    if !common::require_sidecar("list_agents_forwards_resolved_adapter_entrypoint") {
        return;
    }
    let package_root = unique_dir();
    let package_dir = write_mock_agent_package(&package_root);
    common::ensure_sidecar_env();
    let os = AgentOs::create(AgentOsConfig {
        packages: vec![PackageRef {
            path: package_dir.to_string_lossy().into_owned(),
        }],
        ..Default::default()
    })
    .await
    .expect("create VM with registry mock package");

    let agents = os.list_agents().await.expect("list agents");
    let agent = agents
        .iter()
        .find(|agent| agent.id == MOCK_AGENT_TYPE)
        .unwrap_or_else(|| panic!("projected registry mock missing: {agents:?}"));
    assert_eq!(
        agent.adapter_entrypoint,
        "/opt/agentos/bin/registry-mock-agent-acp"
    );

    os.shutdown().await.expect("shutdown");
    std::fs::remove_dir_all(package_root).ok();
}
