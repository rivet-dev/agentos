// Included inside kernel.rs::tests, beside the existing command-projection controls.
fn mount_quota_kernel() -> KernelVm<MountTable> {
    let root = RootFileSystem::from_descriptor(RootFilesystemDescriptor {
        mode: RootFilesystemMode::Ephemeral,
        disable_default_base_layer: true,
        lowers: Vec::new(),
        bootstrap_entries: Vec::new(),
    })
    .expect("empty snapshottable root");
    let mut config = KernelVmConfig::new("trusted-mount-quota");
    config.permissions = Permissions::allow_all();
    KernelVm::new(MountTable::new(root), config)
}

#[test]
fn trusted_mount_projection_rejects_parent_inodes_before_mutation() {
    let mut kernel = mount_quota_kernel();
    let before_usage = kernel.filesystem_usage().unwrap();
    let before_snapshot = kernel.snapshot_root_filesystem().unwrap();
    let before_mounts = kernel.mounted_filesystems();
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        // MountTable currently materializes both /quota and /quota/leaf.
        // Neither guest writes nor a resource-saturation loop are involved.
        max_inode_count: Some(before_usage.inode_count + 1),
        ..ResourceLimits::default()
    });
    let result = kernel.mount_boxed_filesystem_for_operator(
        "/quota/leaf",
        Box::new(crate::mount_table::MountedVirtualFileSystem::new(
            MemoryFileSystem::new(),
        )),
        MountOptions::new("memory"),
    );
    let after_usage = kernel.filesystem_usage().unwrap();
    let after_snapshot = kernel.snapshot_root_filesystem();
    assert!(
        result.is_err(),
        "trusted mount admitted excess parent inodes: before={before_usage:?}, after={after_usage:?}, snapshot={after_snapshot:?}"
    );
    let error = result.unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(error.to_string().contains("maxInodeCount"));
    assert_eq!(after_usage, before_usage, "no partial parent creation");
    assert_eq!(
        kernel.mounted_filesystems(),
        before_mounts,
        "no new mount membership"
    );
    assert!(!kernel.exists_for_operator("/quota").unwrap());
    assert_eq!(
        after_snapshot.unwrap(),
        before_snapshot,
        "snapshot remains usable"
    );
}

#[test]
fn trusted_mount_projection_exact_headroom_preserves_root_accounting() {
    let mut kernel = mount_quota_kernel();
    let before_usage = kernel.filesystem_usage().unwrap();
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        max_inode_count: Some(before_usage.inode_count + 2),
        ..ResourceLimits::default()
    });
    let mut mounted = MemoryFileSystem::new();
    mounted.write_file("/payload", b"mounted".to_vec()).unwrap();
    kernel
        .mount_boxed_filesystem_for_operator(
            "/quota/leaf",
            Box::new(crate::mount_table::MountedVirtualFileSystem::new(mounted)),
            MountOptions::new("memory"),
        )
        .expect("two parent directories fit exactly");
    let after_usage = kernel.filesystem_usage().unwrap();
    assert_eq!(after_usage.inode_count, before_usage.inode_count + 2);
    assert_eq!(after_usage.total_bytes, before_usage.total_bytes);
    let snapshot = kernel
        .snapshot_root_filesystem()
        .expect("admitted root remains exportable");
    assert!(snapshot
        .entries
        .iter()
        .all(|entry| entry.path != "/quota/leaf/payload"));
    assert_eq!(kernel.read_file("/quota/leaf/payload").unwrap(), b"mounted");
    kernel
        .unmount_filesystem_for_operator("/quota/leaf")
        .unwrap();
    kernel
        .snapshot_root_filesystem()
        .expect("unmount does not corrupt accounting");
}

#[test]
fn trusted_mount_projection_keeps_known_root_usage_between_mounts() {
    let mut kernel = mount_quota_kernel();
    let before = kernel.filesystem_usage().unwrap();
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        max_inode_count: Some(before.inode_count + 17),
        ..ResourceLimits::default()
    });
    for index in 0..16 {
        kernel
            .mount_boxed_filesystem_for_operator(
                &format!("/series/item-{index}"),
                Box::new(crate::mount_table::MountedVirtualFileSystem::new(
                    MemoryFileSystem::new(),
                )),
                MountOptions::new("memory"),
            )
            .unwrap();
        let cached = kernel
            .filesystem_usage_cache
            .as_ref()
            .expect("successful mount retains census");
        assert_eq!(cached.inode_count, before.inode_count + index + 2);
    }
    let cached = kernel.filesystem_usage().unwrap();
    kernel.invalidate_filesystem_usage_cache();
    assert_eq!(
        kernel.filesystem_usage().unwrap(),
        cached,
        "delta equals independent final census"
    );
    kernel.snapshot_root_filesystem().unwrap();
}

#[test]
fn trusted_mount_projection_readonly_parent_needs_no_root_headroom() {
    let mut kernel = mount_quota_kernel();
    kernel
        .mount_boxed_filesystem_for_operator(
            "/readonly",
            Box::new(crate::mount_table::MountedVirtualFileSystem::new(
                MemoryFileSystem::new(),
            )),
            MountOptions::new("readonly")
                .read_only(true)
                .max_inodes(Some(1)),
        )
        .unwrap();
    let before = kernel.filesystem_usage().unwrap();
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        max_inode_count: Some(before.inode_count),
        ..ResourceLimits::default()
    });
    let snapshot = kernel.snapshot_root_filesystem().unwrap();
    let mut child = MemoryFileSystem::new();
    child.write_file("/payload", b"child".to_vec()).unwrap();
    kernel
        .mount_boxed_filesystem_for_operator(
            "/readonly/missing/child",
            Box::new(crate::mount_table::MountedVirtualFileSystem::new(child)),
            MountOptions::new("child"),
        )
        .unwrap();
    assert_eq!(
        kernel.read_file("/readonly/missing/child/payload").unwrap(),
        b"child"
    );
    assert_eq!(kernel.filesystem_usage().unwrap(), before);
    assert_eq!(kernel.snapshot_root_filesystem().unwrap(), snapshot);
}

#[test]
fn trusted_mount_projection_symlink_parent_counts_only_new_directories() {
    let mut kernel = mount_quota_kernel();
    kernel.mkdir("/target", true).unwrap();
    kernel.symlink("/target", "/alias").unwrap();
    let before = kernel.filesystem_usage().unwrap();
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        max_inode_count: Some(before.inode_count + 1),
        ..ResourceLimits::default()
    });
    kernel
        .mount_boxed_filesystem_for_operator(
            "/alias/leaf",
            Box::new(crate::mount_table::MountedVirtualFileSystem::new(
                MemoryFileSystem::new(),
            )),
            MountOptions::new("memory"),
        )
        .unwrap();
    assert_eq!(
        kernel.filesystem_usage().unwrap().inode_count,
        before.inode_count + 1
    );
    kernel.invalidate_filesystem_usage_cache();
    assert_eq!(
        kernel.filesystem_usage().unwrap().inode_count,
        before.inode_count + 1
    );
    assert!(kernel.exists_for_operator("/target/leaf").unwrap());
    kernel.snapshot_root_filesystem().unwrap();
}

#[test]
fn trusted_mount_projection_mounted_parent_uses_own_quota() {
    let mut kernel = mount_quota_kernel();
    kernel
        .mount_boxed_filesystem_for_operator(
            "/parent",
            Box::new(crate::mount_table::MountedVirtualFileSystem::new(
                MemoryFileSystem::new(),
            )),
            MountOptions::new("parent").max_inodes(Some(2)),
        )
        .unwrap();
    let before = kernel.filesystem_usage().unwrap();
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        max_inode_count: Some(before.inode_count),
        ..ResourceLimits::default()
    });
    let mounts = kernel.mounted_filesystems();
    let error = kernel
        .mount_boxed_filesystem_for_operator(
            "/parent/two/directories",
            Box::new(crate::mount_table::MountedVirtualFileSystem::new(
                MemoryFileSystem::new(),
            )),
            MountOptions::new("child"),
        )
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(!kernel.exists_for_operator("/parent/two").unwrap());
    assert_eq!(kernel.mounted_filesystems(), mounts);
    kernel
        .mount_boxed_filesystem_for_operator(
            "/parent/one",
            Box::new(crate::mount_table::MountedVirtualFileSystem::new(
                MemoryFileSystem::new(),
            )),
            MountOptions::new("child"),
        )
        .unwrap();
    assert_eq!(kernel.filesystem_usage().unwrap(), before);
    kernel.snapshot_root_filesystem().unwrap();
}

#[test]
fn trusted_mount_projection_sealed_readonly_root_needs_no_headroom() {
    // Seal through the backend API, leaving the mount registration's read_only
    // field false. The owner must observe backend capability, not just metadata.
    let mut kernel = mount_quota_kernel();
    let replacement = RootFileSystem::from_descriptor(RootFilesystemDescriptor {
        mode: RootFilesystemMode::ReadOnly,
        disable_default_base_layer: true,
        lowers: Vec::new(),
        bootstrap_entries: Vec::new(),
    })
    .unwrap();
    *kernel.root_filesystem_mut().unwrap() = replacement;
    kernel.root_filesystem_mut().unwrap().finish_bootstrap();
    let before = kernel.filesystem_usage().unwrap();
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        max_inode_count: Some(before.inode_count),
        ..ResourceLimits::default()
    });
    let snapshot = kernel.snapshot_root_filesystem().unwrap();
    kernel
        .mount_boxed_filesystem_for_operator(
            "/missing/leaf",
            Box::new(crate::mount_table::MountedVirtualFileSystem::new(
                MemoryFileSystem::new(),
            )),
            MountOptions::new("memory"),
        )
        .expect("sealed read-only root needs no cosmetic parent directories");
    assert_eq!(kernel.filesystem_usage().unwrap(), before);
    assert_eq!(kernel.snapshot_root_filesystem().unwrap(), snapshot);
}

#[test]
fn trusted_mount_projection_raw_root_mutation_invalidates_admission_census() {
    let mut kernel = mount_quota_kernel();
    let before = kernel.filesystem_usage().unwrap();
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        max_inode_count: Some(before.inode_count + 1),
        ..ResourceLimits::default()
    });
    kernel
        .root_filesystem_mut()
        .unwrap()
        .mkdir("/trusted", false)
        .unwrap();
    assert!(
        kernel.filesystem_usage_cache.is_none(),
        "raw root borrow invalidates census"
    );
    let error = kernel
        .mount_boxed_filesystem_for_operator(
            "/missing",
            Box::new(crate::mount_table::MountedVirtualFileSystem::new(
                MemoryFileSystem::new(),
            )),
            MountOptions::new("memory"),
        )
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(!kernel.exists_for_operator("/missing").unwrap());
    assert_eq!(
        kernel.filesystem_usage().unwrap().inode_count,
        before.inode_count + 1
    );
}

#[test]
fn trusted_mount_projection_dangling_symlink_rejects_resolved_parent_growth() {
    let mut kernel = mount_quota_kernel();
    kernel.symlink("/missing/a/b", "/alias").unwrap();
    let before = kernel.filesystem_usage().unwrap();
    let snapshot = kernel.snapshot_root_filesystem().unwrap();
    let mounts = kernel.mounted_filesystems();
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        max_inode_count: Some(before.inode_count + 2),
        ..ResourceLimits::default()
    });
    let result = kernel.mount_boxed_filesystem_for_operator(
        "/alias/leaf",
        Box::new(crate::mount_table::MountedVirtualFileSystem::new(
            MemoryFileSystem::new(),
        )),
        MountOptions::new("memory"),
    );
    let cached = kernel.filesystem_usage().unwrap();
    kernel.invalidate_filesystem_usage_cache();
    let measured = kernel.filesystem_usage().unwrap();
    assert!(
        result.is_err(),
        "mount exceeded resolved-parent allowance: before={before:?}, cached={cached:?}, measured={measured:?}"
    );
    let error = result.unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(error.to_string().contains("maxInodeCount"));
    assert_eq!(cached, before);
    assert_eq!(measured, before);
    assert_eq!(kernel.mounted_filesystems(), mounts);
    assert!(!kernel.exists_for_operator("/missing").unwrap());
    assert_eq!(kernel.snapshot_root_filesystem().unwrap(), snapshot);
}

#[test]
fn trusted_mount_projection_dangling_symlink_exact_headroom_matches_census() {
    let mut kernel = mount_quota_kernel();
    kernel.symlink("/missing/a/b", "/alias").unwrap();
    let before = kernel.filesystem_usage().unwrap();
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        max_inode_count: Some(before.inode_count + 4),
        ..ResourceLimits::default()
    });
    let mut mounted = MemoryFileSystem::new();
    mounted.write_file("/payload", b"mounted".to_vec()).unwrap();
    kernel
        .mount_boxed_filesystem_for_operator(
            "/alias/leaf",
            Box::new(crate::mount_table::MountedVirtualFileSystem::new(mounted)),
            MountOptions::new("memory"),
        )
        .expect("four resolved parent directories fit exactly");
    let cached = kernel.filesystem_usage().unwrap();
    kernel.invalidate_filesystem_usage_cache();
    let measured = kernel.filesystem_usage().unwrap();
    assert_eq!(measured.inode_count, before.inode_count + 4);
    assert_eq!(
        cached, measured,
        "retained usage must equal independent census"
    );
    assert_eq!(measured.total_bytes, before.total_bytes);
    assert!(kernel.exists_for_operator("/missing/a/b/leaf").unwrap());
    kernel.snapshot_root_filesystem().unwrap();
}
