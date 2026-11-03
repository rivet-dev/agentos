//! Shared-sidecar pooling e2e against a real sidecar. Verifies that two VMs created with the default
//! (shared "default" pool) config reuse a single sidecar process, report a shared `active_vm_count`,
//! stay isolated, and release independently.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use agentos_client::config::{
    AgentOsConfig, AgentOsSidecarConfig, FsPermissions, HostFunction, HostFunctionCollections,
    PatternPermissions, PermissionMode, Permissions,
};
use agentos_client::fs::FileContent;
use agentos_client::process::ExecOptions;
use agentos_client::sidecar::AgentOsSidecar;
use agentos_client::AgentOs;
use serde_json::json;

#[tokio::test]
async fn shared_sidecar_pooling_reuses_one_process() {
    if !common::require_sidecar("shared_sidecar_pooling_reuses_one_process") {
        return;
    }

    // Default config => shared "default" pool, so both VMs land on one sidecar process.
    let a = common::new_vm().await;
    let b = common::new_vm().await;

    let desc_a = a.sidecar().describe();
    let desc_b = b.sidecar().describe();
    assert_eq!(
        desc_a.sidecar_id, desc_b.sidecar_id,
        "both VMs should share the same pooled sidecar"
    );
    assert_eq!(
        desc_a.active_vm_count, 2,
        "the shared sidecar should report 2 active VMs"
    );

    // Sharing a process must not break VM isolation.
    a.write_file("/tmp/who", FileContent::Text("A".to_string()))
        .await
        .expect("write A");
    b.write_file("/tmp/who", FileContent::Text("B".to_string()))
        .await
        .expect("write B");
    assert_eq!(a.read_file("/tmp/who").await.expect("read A"), b"A");
    assert_eq!(b.read_file("/tmp/who").await.expect("read B"), b"B");

    // Releasing one VM leaves the sibling working and drops the shared count to 1.
    a.shutdown().await.expect("shutdown A");
    assert_eq!(
        a.sidecar().describe().active_vm_count,
        1,
        "active_vm_count should drop to 1 after one VM releases"
    );
    assert_eq!(b.read_file("/tmp/who").await.expect("B still live"), b"B");

    b.shutdown().await.expect("shutdown B");
}

async fn vm_with_tenant_host_function(
    sidecar: &Arc<AgentOsSidecar>,
    tenant: &'static str,
) -> AgentOs {
    common::ensure_sidecar_env();
    let who = HostFunction {
        input_schema: json!({
            "type": "object",
            "properties": { "marker": { "type": "string" } }
        }),
        timeout_ms: None,
        execute: Arc::new(move |input| {
            Box::pin(async move { Ok(json!({ "tenant": tenant, "marker": input["marker"] })) })
        }),
    };
    let host_functions: HostFunctionCollections =
        BTreeMap::from([("t".to_owned(), BTreeMap::from([("who".to_owned(), who)]))]);
    AgentOs::create(AgentOsConfig {
        permissions: Some(Permissions {
            fs: Some(FsPermissions::Mode(PermissionMode::Allow)),
            child_process: Some(PatternPermissions::Mode(PermissionMode::Allow)),
            host_function: Some(PatternPermissions::Mode(PermissionMode::Allow)),
            ..Default::default()
        }),
        host_functions,
        sidecar: Some(AgentOsSidecarConfig::Explicit {
            handle: sidecar.clone(),
        }),
        ..Default::default()
    })
    .await
    .expect("create VM against real sidecar")
}

async fn guest_calls_who(vm: &AgentOs, args: &[&str]) -> serde_json::Value {
    let mut argv = vec!["who".to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let result = vm
        .exec_argv_process("agentos-t", &argv, ExecOptions::default())
        .await
        .expect("guest runs the host function command");
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    serde_json::from_str::<serde_json::Value>(&result.stdout).expect("JSON stdout")["result"]
        .clone()
}

// Every sidecar process numbers its VMs from `vm-1`, so `a` and `b` have the same VM id.
#[tokio::test]
async fn guest_reaches_only_its_own_host_functions_and_files() {
    if !common::require_sidecar("guest_reaches_only_its_own_host_functions_and_files") {
        return;
    }
    let first_sidecar = AgentOs::create_sidecar(None).await.expect("first sidecar");
    let second_sidecar = AgentOs::create_sidecar(None).await.expect("second sidecar");
    let a = vm_with_tenant_host_function(&first_sidecar, "a").await;
    let b = vm_with_tenant_host_function(&second_sidecar, "b").await;
    let same_sidecar_as_a = vm_with_tenant_host_function(&first_sidecar, "c").await;

    assert_eq!(guest_calls_who(&a, &[]).await["tenant"], "a");
    assert_eq!(guest_calls_who(&b, &[]).await["tenant"], "b");
    assert_eq!(
        guest_calls_who(&same_sidecar_as_a, &[]).await["tenant"],
        "c"
    );

    // `--json-file` input comes from the calling guest's filesystem.
    for (vm, marker) in [(&a, "file of a"), (&b, "file of b")] {
        vm.write_file(
            "/tmp/in.json",
            FileContent::Text(format!(r#"{{"marker":"{marker}"}}"#)),
        )
        .await
        .expect("write input file");
    }
    assert_eq!(
        guest_calls_who(&b, &["--json-file", "/tmp/in.json"]).await["marker"],
        "file of b"
    );

    // Shutting down a VM must not remove the routes of a VM with the same id.
    b.shutdown().await.expect("shutdown b");
    assert_eq!(guest_calls_who(&a, &[]).await["tenant"], "a");
    same_sidecar_as_a
        .shutdown()
        .await
        .expect("shutdown same_sidecar_as_a");
    assert_eq!(guest_calls_who(&a, &[]).await["tenant"], "a");
    a.shutdown().await.expect("shutdown a");
}
