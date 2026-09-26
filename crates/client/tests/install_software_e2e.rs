use agentos_client::config::{
    AgentOsConfig, FsPermissions, PatternPermissions, PermissionMode, Permissions,
};
use agentos_client::{AgentOs, PackageSource};

mod common;

fn allow_all() -> Permissions {
    Permissions {
        fs: Some(FsPermissions::Mode(PermissionMode::Allow)),
        network: Some(PatternPermissions::Mode(PermissionMode::Allow)),
        child_process: Some(PatternPermissions::Mode(PermissionMode::Allow)),
        process: Some(PatternPermissions::Mode(PermissionMode::Allow)),
        env: Some(PatternPermissions::Mode(PermissionMode::Allow)),
        host_function: Some(PatternPermissions::Mode(PermissionMode::Allow)),
    }
}

fn test_package() -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::<u8>::new());
    let manifest = br#"{"name":"installed-tool","version":"1.0.0"}"#;
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "agentos-package.json", &manifest[..])
        .unwrap();
    let command = b"#!/usr/bin/env node\nprocess.stdout.write('installed-ok\\n');\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(command.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder
        .append_data(&mut header, "bin/installed-cmd", &command[..])
        .unwrap();
    let tar = builder.into_inner().unwrap();
    vfs::package_format::pack::pack_aospkg_from_tar_bytes(&tar)
        .unwrap()
        .0
}

#[tokio::test]
async fn content_addressed_install_and_uninstall_update_the_live_projection() {
    if !common::require_sidecar(
        "content_addressed_install_and_uninstall_update_the_live_projection",
    ) {
        return;
    }
    let package = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(package.path(), test_package()).unwrap();
    let os = AgentOs::create(AgentOsConfig {
        permissions: Some(allow_all()),
        ..AgentOsConfig::default()
    })
    .await
    .expect("create VM");

    let installed = os
        .install_software(PackageSource::Path {
            path: package.path().to_string_lossy().into_owned(),
            expected_digest: None,
        })
        .await
        .expect("install package");
    assert!(installed.package_id.starts_with("sha256:"));
    assert_eq!(installed.package_name, "installed-tool");
    assert_eq!(os.installed_software(), vec![installed.clone()]);
    assert!(os
        .exists("/opt/agentos/bin/installed-cmd")
        .await
        .expect("check linked command"));

    assert_eq!(
        os.uninstall_software(&installed.package_id)
            .await
            .expect("uninstall package"),
        installed
    );
    assert!(os.installed_software().is_empty());
    assert!(!os
        .exists("/opt/agentos/bin/installed-cmd")
        .await
        .expect("check removed command"));

    os.shutdown().await.expect("shutdown VM");
}
