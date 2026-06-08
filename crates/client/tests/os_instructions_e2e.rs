mod common;

use agent_os_client::config::AgentOsConfig;
use agent_os_client::{AgentOs, ClientError};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_bootstraps_os_instructions() {
    if !common::sidecar_available() {
        panic!(
            "create_bootstraps_os_instructions: sidecar binary is not built; build it with `cargo build -p agent-os-sidecar`"
        );
    }

    common::ensure_sidecar_env();
    let os = AgentOs::create(AgentOsConfig {
        additional_instructions: Some("rust-client-extra-instructions".to_string()),
        ..Default::default()
    })
    .await
    .expect("create VM with OS instructions");

    let contents = os
        .read_file("/etc/agentos/instructions.md")
        .await
        .expect("read OS instructions");
    let text = String::from_utf8(contents).expect("instructions are utf8");

    assert!(
        text.contains("# agentOS"),
        "base OS instructions are present"
    );
    assert!(
        text.contains("rust-client-extra-instructions"),
        "create-time additional instructions are appended"
    );

    assert_path_read_only(
        os.write_file("/etc/agentos/instructions.md", "tampered")
            .await
            .expect_err("writing OS instructions should fail"),
        "/etc/agentos/instructions.md",
    );
    assert_path_read_only(
        os.mkdir("/etc/agentos/tamper", Default::default())
            .await
            .expect_err("creating files under /etc/agentos should fail"),
        "/etc/agentos/tamper",
    );
    assert_path_read_only(
        os.delete("/etc/agentos/instructions.md", Default::default())
            .await
            .expect_err("deleting OS instructions should fail"),
        "/etc/agentos/instructions.md",
    );

    os.shutdown().await.expect("shutdown VM");
}

fn assert_path_read_only(error: anyhow::Error, expected_path: &str) {
    assert!(
        error
            .downcast_ref::<ClientError>()
            .map(|error| matches!(error, ClientError::PathReadOnly(path) if path == expected_path))
            .unwrap_or(false),
        "expected PathReadOnly for {expected_path}, got {error:?}"
    );
}
