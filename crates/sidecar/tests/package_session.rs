use agentos_client::{
    configure_shared_sidecar_package_cache, AgentOs, PackageSource, ProcessPackageCacheOptions,
};
use std::time::Duration;

#[tokio::test]
async fn pre_vm_package_session_uses_child_sidecar_cache() {
    let cache = tempfile::tempdir().expect("cache directory");
    let source_directory = tempfile::tempdir().expect("source directory");
    configure_shared_sidecar_package_cache(ProcessPackageCacheOptions {
        root: Some(cache.path().to_path_buf()),
        ..ProcessPackageCacheOptions::default()
    })
    .expect("configure child cache");

    let source = source_directory.path().join("source.aospkg");
    let mut tar = tar::Builder::new(Vec::new());
    let manifest = br#"{"name":"pre-vm-test","version":"1.0.0"}"#;
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "agentos-package.json", &manifest[..])
        .expect("append package manifest");
    let bytes = agentos_vfs_core::package_format::pack::pack_aospkg_from_tar_bytes(
        &tar.into_inner().expect("finish package tar"),
    )
    .expect("pack package")
    .0;
    std::fs::write(&source, bytes).expect("write package source");

    let sidecar = AgentOs::get_shared_sidecar(
        Some("pre-vm-package-session-test".into()),
        Some(env!("CARGO_BIN_EXE_agentos-sidecar").into()),
    )
    .await
    .expect("get sidecar");
    let test_sidecar = sidecar.clone();
    let mut task = tokio::spawn(async move {
        let session = test_sidecar
            .open_package_session()
            .await
            .expect("open session");
        let installed = session
            .acquire(
                PackageSource::Path {
                    path: source.to_string_lossy().into_owned(),
                    expected_digest: None,
                },
                false,
                None,
                None,
            )
            .await
            .expect("acquire package before VM creation");
        assert_eq!(installed.package_name, "pre-vm-test");
        let stats = session.cache_stats().await.expect("read child cache stats");
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.acquisitions, 1);
        assert_eq!(stats.pinned_entries, 0);
        // Repeated handles share one wire session and closing a transient handle
        // must not reap the worker while the original handle is still alive.
        for _ in 0..3 {
            let transient = test_sidecar
                .open_package_session()
                .await
                .expect("reopen package session");
            transient
                .close()
                .await
                .expect("close transient package session");
            assert_eq!(
                session
                    .cache_stats()
                    .await
                    .expect("original session remains live")
                    .entries,
                1
            );
        }
        session.close().await.expect("close package session");
        assert_eq!(
            test_sidecar.describe().state,
            agentos_client::SidecarState::Disposed
        );
        session
            .close()
            .await
            .expect("close is idempotent after confirmed reaping");
    });
    let result = tokio::time::timeout(Duration::from_secs(30), &mut task).await;
    if result.is_err() {
        task.abort();
        if let Err(error) = task.await {
            if !error.is_cancelled() {
                eprintln!("package session task failed while stopping it: {error}");
            }
        }
    }
    let cleanup = tokio::time::timeout(Duration::from_secs(5), sidecar.dispose()).await;
    if !matches!(cleanup, Ok(Ok(()))) {
        // Never delete cache files if this regression failed to reap its child.
        let path = cache.keep();
        panic!(
            "package session cleanup failed: {cleanup:?}; retained cache at {}",
            path.display()
        );
    }
    result
        .expect("package session test deadline")
        .expect("package session assertions");
}
