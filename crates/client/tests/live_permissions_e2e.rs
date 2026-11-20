mod common;
use agentos_client::config::{
    FsPermissionRule, FsPermissions, PermissionMode, Permissions, RulePermissions,
};
use agentos_client::{fs::FileContent, ExecOptions};

#[tokio::test]
async fn complete_policy_replacement_preserves_guest_files_and_changes_read_admission() {
    assert!(common::sidecar_available(), "set AGENTOS_SIDECAR_BIN");
    let vm = common::new_vm_with_sidecar_pool("live-policy-control").await;
    vm.write_file("/tmp/private", FileContent::Text("private".into()))
        .await
        .expect("seed private file");
    let denied = Permissions {
        fs: Some(FsPermissions::Rules(RulePermissions {
            default: Some(PermissionMode::Allow),
            rules: vec![FsPermissionRule {
                mode: PermissionMode::Deny,
                operations: Some(vec!["read".into()]),
                paths: Some(vec!["/tmp/private".into()]),
            }],
        })),
        ..Default::default()
    };
    let result = async {
        vm.configure_permissions(denied, Some(vec![])).await?;
        let args = vec!["--input-type=module".into(), "-e".into(), "import fs from 'node:fs';try{fs.readFileSync('/tmp/private');process.exit(9);}catch(e){console.log(e.code);}".into()];
        let output = vm.exec_argv_process("node", &args, ExecOptions { timeout: Some(5000.0), ..Default::default() }).await?;
        anyhow::ensure!(output.stdout.contains("EACCES"), "guest denial missing: {output:?}");
        vm.configure_permissions(Permissions::default(), Some(vec![])).await?;
        anyhow::ensure!(vm.read_file_range("/tmp/private", 0, 7).await? == b"private", "replacement lost file contents");
        Ok::<_,anyhow::Error>(())
    }.await;
    vm.shutdown().await.expect("shutdown");
    result.expect("live native policy assertions");
}
