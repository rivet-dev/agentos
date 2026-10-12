// Included in kernel.rs::tests; uses existing real filesystems and kernel owners.
fn trusted_write_kernel(limit: usize) -> KernelVm<MemoryFileSystem> {
    let mut config = KernelVmConfig::new("trusted-write-quota");
    config.permissions = Permissions::allow_all();
    config.resources = ResourceLimits {
        max_filesystem_bytes: None,
        max_inode_count: Some(limit),
        ..Default::default()
    };
    KernelVm::new(MemoryFileSystem::new(), config)
}
#[test]
fn trusted_write_parent_quota_rejects_before_any_mutation() {
    let mut kernel = trusted_write_kernel(2);
    let before = kernel.filesystem_usage().unwrap();
    assert_eq!(before.inode_count, 1);
    let error = kernel
        .write_file("/a/file", b"x".to_vec())
        .expect_err("parent+file require two additional inodes");
    assert_eq!(error.code(), "ENOSPC");
    assert_eq!(kernel.filesystem_usage().unwrap(), before);
    assert!(!kernel.raw_filesystem_mut().exists("/a"));
}
#[test]
fn trusted_write_parent_cache_matches_independent_census() {
    let mut kernel = trusted_write_kernel(5);
    kernel.write_file("/a/b/file", b"x".to_vec()).unwrap();
    let census = measure_filesystem_usage(kernel.raw_filesystem_mut()).unwrap();
    assert_eq!(census.inode_count, 4);
    assert_eq!(
        kernel.filesystem_usage().unwrap(),
        census,
        "auto-created parents must enter cached inode usage"
    );
    kernel.write_file("/a/b/second", Vec::new()).unwrap();
    assert_eq!(
        kernel
            .write_file("/a/b/third", Vec::new())
            .unwrap_err()
            .code(),
        "ENOSPC"
    );
}
#[test]
fn trusted_write_dangling_symlink_target_parents_are_preflighted() {
    let mut fs = MemoryFileSystem::new();
    fs.symlink("/target/deep", "/alias").unwrap();
    let mut config = KernelVmConfig::new("trusted-symlink-quota");
    config.permissions = Permissions::allow_all();
    config.resources = ResourceLimits {
        max_filesystem_bytes: None,
        max_inode_count: Some(4),
        ..Default::default()
    };
    let mut kernel = KernelVm::new(fs, config);
    let before = kernel.filesystem_usage().unwrap();
    assert_eq!(before.inode_count, 2);
    assert_eq!(
        kernel
            .write_file("/alias/file", Vec::new())
            .expect_err("resolved target needs two parents plus file")
            .code(),
        "ENOSPC"
    );
    assert_eq!(kernel.filesystem_usage().unwrap(), before);
    assert!(!kernel.raw_filesystem_mut().exists("/target"));
}
#[test]
fn trusted_write_mounted_parent_cache_retains_mount_quota() {
    let mut mounts = MountTable::new(MemoryFileSystem::new());
    mounts
        .mount(
            "/mnt",
            MemoryFileSystem::new(),
            MountOptions::new("write-parent").max_inodes(Some(3)),
        )
        .unwrap();
    let mut config = KernelVmConfig::new("trusted-mounted-quota");
    config.permissions = Permissions::allow_all();
    config.resources = ResourceLimits {
        max_filesystem_bytes: None,
        max_inode_count: None,
        ..Default::default()
    };
    let mut kernel = KernelVm::new(mounts, config);
    let root = kernel.raw_filesystem_mut().root_usage().unwrap();
    kernel.write_file("/mnt/a/file", Vec::new()).unwrap();
    let outcome = kernel.write_file("/mnt/a/second", Vec::new());
    assert_eq!(
        kernel.raw_filesystem_mut().root_usage().unwrap(),
        root,
        "mounted parents must not charge root"
    );
    assert_eq!(
        outcome
            .expect_err("mounted root+parent+file already uses three inodes")
            .code(),
        "ENOSPC"
    );
}
#[test]
fn trusted_write_mounted_symlink_uses_selected_backend_parent_quota() {
    let mut backend = MemoryFileSystem::new();
    backend.symlink("/target/deep", "/alias").unwrap();
    let mut mounts = MountTable::new(MemoryFileSystem::new());
    mounts
        .mount(
            "/mnt",
            backend,
            MountOptions::new("write-symlink-parent").max_inodes(Some(4)),
        )
        .unwrap();
    let mut config = KernelVmConfig::new("trusted-mounted-symlink");
    config.permissions = Permissions::allow_all();
    config.resources = ResourceLimits {
        max_filesystem_bytes: None,
        max_inode_count: None,
        ..Default::default()
    };
    let mut kernel = KernelVm::new(mounts, config);
    let root = kernel.raw_filesystem_mut().root_usage().unwrap();
    assert_eq!(
        kernel
            .write_file("/mnt/alias/file", Vec::new())
            .expect_err("selected backend root+symlink+two parents+file exceeds four")
            .code(),
        "ENOSPC"
    );
    assert_eq!(kernel.raw_filesystem_mut().root_usage().unwrap(), root);
    assert!(kernel.storage_stat("/mnt/target").unwrap().is_none());
}

struct TrustedWritePartialBackend {
    inner: MemoryFileSystem,
    fail: bool,
}
impl VirtualFileSystem for TrustedWritePartialBackend {
    fn read_file(&mut self, p: &str) -> VfsResult<Vec<u8>> {
        self.inner.read_file(p)
    }
    fn read_dir(&mut self, p: &str) -> VfsResult<Vec<String>> {
        self.inner.read_dir(p)
    }
    fn read_dir_with_types(&mut self, p: &str) -> VfsResult<Vec<crate::vfs::VirtualDirEntry>> {
        self.inner.read_dir_with_types(p)
    }
    fn write_file(&mut self, p: &str, data: impl Into<Vec<u8>>) -> VfsResult<()> {
        if self.fail {
            self.inner.mkdir(&parent_path(p), true)?;
            return Err(VfsError::new(
                "EIO",
                "injected after parent creation; no file bytes written",
            ));
        }
        self.inner.write_file(p, data)
    }
    fn create_dir(&mut self, p: &str) -> VfsResult<()> {
        self.inner.create_dir(p)
    }
    fn mkdir(&mut self, p: &str, r: bool) -> VfsResult<()> {
        self.inner.mkdir(p, r)
    }
    fn exists(&self, p: &str) -> bool {
        self.inner.exists(p)
    }
    fn stat(&mut self, p: &str) -> VfsResult<VirtualStat> {
        self.inner.stat(p)
    }
    fn remove_file(&mut self, p: &str) -> VfsResult<()> {
        self.inner.remove_file(p)
    }
    fn remove_dir(&mut self, p: &str) -> VfsResult<()> {
        self.inner.remove_dir(p)
    }
    fn rename(&mut self, p: &str, q: &str) -> VfsResult<()> {
        self.inner.rename(p, q)
    }
    fn realpath(&self, p: &str) -> VfsResult<String> {
        self.inner.realpath(p)
    }
    fn symlink(&mut self, t: &str, p: &str) -> VfsResult<()> {
        self.inner.symlink(t, p)
    }
    fn read_link(&self, p: &str) -> VfsResult<String> {
        self.inner.read_link(p)
    }
    fn lstat(&self, p: &str) -> VfsResult<VirtualStat> {
        self.inner.lstat(p)
    }
    fn link(&mut self, p: &str, q: &str) -> VfsResult<()> {
        self.inner.link(p, q)
    }
    fn chmod(&mut self, p: &str, m: u32) -> VfsResult<()> {
        self.inner.chmod(p, m)
    }
    fn chown(&mut self, p: &str, u: u32, g: u32) -> VfsResult<()> {
        self.inner.chown(p, u, g)
    }
    fn utimes(&mut self, p: &str, a: u64, m: u64) -> VfsResult<()> {
        self.inner.utimes(p, a, m)
    }
    fn truncate(&mut self, p: &str, l: u64) -> VfsResult<()> {
        self.inner.truncate(p, l)
    }
    fn pread(&mut self, p: &str, o: u64, l: usize) -> VfsResult<Vec<u8>> {
        self.inner.pread(p, o, l)
    }
}
#[test]
fn trusted_write_partial_backend_failure_invalidates_cached_parent_usage() {
    let mut config = KernelVmConfig::new("trusted-partial-parent");
    config.permissions = Permissions::allow_all();
    config.resources = ResourceLimits {
        max_filesystem_bytes: None,
        max_inode_count: Some(4),
        ..Default::default()
    };
    let mut kernel = KernelVm::new(
        TrustedWritePartialBackend {
            inner: MemoryFileSystem::new(),
            fail: false,
        },
        config,
    );
    assert_eq!(kernel.filesystem_usage().unwrap().inode_count, 1);
    kernel.raw_filesystem_mut().fail = true;
    assert_eq!(
        kernel
            .write_file("/a/b/file", b"x".to_vec())
            .unwrap_err()
            .code(),
        "EIO"
    );
    assert!(!kernel.raw_filesystem_mut().inner.exists("/a/b/file"));
    let census = measure_filesystem_usage(&mut kernel.raw_filesystem_mut().inner).unwrap();
    assert_eq!(census.inode_count, 3);
    assert_eq!(census.total_bytes, 0);
    assert_eq!(
        kernel.filesystem_usage().unwrap(),
        census,
        "failed raw write may have created parents; no stale cache may survive"
    );
}

fn cross_mount_existing_target_case(parents_exist: bool) {
    fn setup(parents_exist: bool) -> KernelVm<MountTable> {
        let mut root = MemoryFileSystem::new();
        if parents_exist { root.mkdir("/mnt/tree", true).unwrap(); }
        root.symlink("/mnt/tree/file", "/alias").unwrap();
        let mut mounted = MemoryFileSystem::new();
        mounted.write_file("/tree/file", b"mounted-original".to_vec()).unwrap();
        let mut table = MountTable::new(root);
        table.mount("/mnt", mounted, MountOptions::new("global-existing-root-missing")).unwrap();
        let mut config = KernelVmConfig::new("cross-mount-trusted-write");
        config.permissions = Permissions::allow_all();
        config.resources = ResourceLimits { max_filesystem_bytes: None, max_inode_count: None, ..Default::default() };
        KernelVm::new(table, config)
    }
    let growth = if parents_exist { 1 } else { 2 };
    let mut rejected = setup(parents_exist);
    let before = rejected.filesystem_usage().unwrap();
    rejected.resources = ResourceAccountant::new(ResourceLimits { max_filesystem_bytes: None, max_inode_count: Some(before.inode_count + growth - 1), ..Default::default() });
    assert_eq!(rejected.storage_stat("/alias").unwrap().unwrap().size, 16, "global lookup must see independent mounted file");
    let result = rejected.write_file("/alias", b"x".to_vec());
    assert_eq!(rejected.raw_filesystem_mut().read_file("/mnt/tree/file").unwrap(), b"mounted-original", "raw trusted write must not be rerouted to mounted target");
    assert_eq!(result.expect_err("existing global target cannot hide selected root inode growth").code(), "ENOSPC");
    assert_eq!(rejected.raw_filesystem_mut().root_usage().unwrap(), before);
    assert_eq!(rejected.filesystem_usage().unwrap(), before);

    let mut admitted = setup(parents_exist);
    let before = admitted.filesystem_usage().unwrap();
    admitted.resources = ResourceAccountant::new(ResourceLimits { max_filesystem_bytes: None, max_inode_count: Some(before.inode_count + growth), ..Default::default() });
    admitted.write_file("/alias", b"x".to_vec()).unwrap();
    assert_eq!(admitted.raw_filesystem_mut().read_file("/mnt/tree/file").unwrap(), b"mounted-original");
    let physical = admitted.raw_filesystem_mut().root_usage().unwrap();
    assert_eq!(physical.inode_count, before.inode_count + growth);
    assert_eq!(physical.total_bytes, before.total_bytes + 1);
    assert_eq!(admitted.filesystem_usage().unwrap(), physical, "selected root metadata must update root cache even though global target already existed");
}
#[test]
fn trusted_write_cross_mount_existing_target_does_not_hide_root_parents() {
    cross_mount_existing_target_case(false);
}
#[test]
fn trusted_write_cross_mount_existing_target_does_not_hide_root_leaf() {
    cross_mount_existing_target_case(true);
}
