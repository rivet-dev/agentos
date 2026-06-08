mod common;

use std::fs;
use std::path::Path;

use agent_os_client::config::{AgentOsConfig, MountConfig, MountPlugin};
use agent_os_client::AgentOs;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_forwards_native_mounts() {
    if !common::sidecar_available() {
        panic!(
            "create_forwards_native_mounts: sidecar binary is not built; build it with `cargo build -p agent-os-sidecar`"
        );
    }

    let host_root = std::env::temp_dir().join(format!("agent-os-client-mount-{}", Uuid::new_v4()));
    fs::create_dir_all(&host_root).expect("create host mount root");
    fs::write(host_root.join("marker.txt"), b"mounted").expect("write host marker");

    let os = create_vm_with_host_mount(&host_root).await;
    let contents = os
        .read_file("/mnt/host/marker.txt")
        .await
        .expect("read mounted host file");

    assert_eq!(contents, b"mounted");

    os.shutdown().await.expect("shutdown VM");
    fs::remove_dir_all(host_root).expect("remove host mount root");
}

async fn create_vm_with_host_mount(host_root: &Path) -> AgentOs {
    common::ensure_sidecar_env();
    AgentOs::create(AgentOsConfig {
        mounts: vec![MountConfig::Native {
            path: "/mnt/host".to_string(),
            plugin: MountPlugin {
                id: "host_dir".to_string(),
                config: Some(serde_json::json!({
                    "hostPath": host_root.to_string_lossy().into_owned(),
                    "readOnly": true,
                })),
            },
            read_only: true,
        }],
        ..Default::default()
    })
    .await
    .expect("create VM with native host-dir mount")
}
