use crate::vfs::{VfsError, VfsResult, VirtualDirEntry, VirtualStat};
struct CommandProjectionFailureFileSystem {
    inner: MemoryFileSystem,
    fail_chmod_path: String,
    fail_chmod_once: bool,
}

impl CommandProjectionFailureFileSystem {
    fn new(inner: MemoryFileSystem, fail_chmod_path: impl Into<String>) -> Self {
        Self {
            inner,
            fail_chmod_path: fail_chmod_path.into(),
            fail_chmod_once: true,
        }
    }
}

impl VirtualFileSystem for CommandProjectionFailureFileSystem {
    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        self.inner.read_file(path)
    }

    fn read_dir(&mut self, path: &str) -> VfsResult<Vec<String>> {
        self.inner.read_dir(path)
    }

    fn read_dir_limited(&mut self, path: &str, max_entries: usize) -> VfsResult<Vec<String>> {
        self.inner.read_dir_limited(path, max_entries)
    }

    fn read_dir_with_types(&mut self, path: &str) -> VfsResult<Vec<VirtualDirEntry>> {
        self.inner.read_dir_with_types(path)
    }

    fn write_file(&mut self, path: &str, content: impl Into<Vec<u8>>) -> VfsResult<()> {
        self.inner.write_file(path, content)
    }

    fn create_file_exclusive(&mut self, path: &str, content: impl Into<Vec<u8>>) -> VfsResult<()> {
        self.inner.create_file_exclusive(path, content)
    }

    fn append_file(&mut self, path: &str, content: impl Into<Vec<u8>>) -> VfsResult<u64> {
        self.inner.append_file(path, content)
    }

    fn create_dir(&mut self, path: &str) -> VfsResult<()> {
        self.inner.create_dir(path)
    }

    fn mkdir(&mut self, path: &str, recursive: bool) -> VfsResult<()> {
        self.inner.mkdir(path, recursive)
    }

    fn exists(&self, path: &str) -> bool {
        self.inner.exists(path)
    }

    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat> {
        self.inner.stat(path)
    }

    fn remove_file(&mut self, path: &str) -> VfsResult<()> {
        self.inner.remove_file(path)
    }

    fn remove_dir(&mut self, path: &str) -> VfsResult<()> {
        self.inner.remove_dir(path)
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        self.inner.rename(old_path, new_path)
    }

    fn realpath(&self, path: &str) -> VfsResult<String> {
        self.inner.realpath(path)
    }

    fn symlink(&mut self, target: &str, link_path: &str) -> VfsResult<()> {
        self.inner.symlink(target, link_path)
    }

    fn read_link(&self, path: &str) -> VfsResult<String> {
        self.inner.read_link(path)
    }

    fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        self.inner.lstat(path)
    }

    fn link(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        self.inner.link(old_path, new_path)
    }

    fn chmod(&mut self, path: &str, mode: u32) -> VfsResult<()> {
        if path == self.fail_chmod_path && self.fail_chmod_once {
            self.fail_chmod_once = false;
            return Err(VfsError::new(
                "EIO",
                "injected second projection chmod failure",
            ));
        }
        self.inner.chmod(path, mode)
    }

    fn chown(&mut self, path: &str, uid: u32, gid: u32) -> VfsResult<()> {
        self.inner.chown(path, uid, gid)
    }

    fn utimes(&mut self, path: &str, atime_ms: u64, mtime_ms: u64) -> VfsResult<()> {
        self.inner.utimes(path, atime_ms, mtime_ms)
    }

    fn truncate(&mut self, path: &str, length: u64) -> VfsResult<()> {
        self.inner.truncate(path, length)
    }

    fn pread(&mut self, path: &str, offset: u64, length: usize) -> VfsResult<Vec<u8>> {
        self.inner.pread(path, offset, length)
    }
}

#[test]
fn command_projection_admits_whole_batch_before_mutation() {
    for operator in [false, true] {
        let mut config = KernelVmConfig::new("command-projection-quota");
        config.permissions = Permissions::allow_all();
        let mut kernel = KernelVm::new(MountTable::new(MemoryFileSystem::new()), config);
        let before = kernel.filesystem_usage().unwrap();
        // /bin plus one distinct stub fit; duplicate names consume one inode.
        kernel.resources = ResourceAccountant::new(ResourceLimits {
            max_inode_count: Some(before.inode_count + 2),
            ..ResourceLimits::default()
        });
        let register = |kernel: &mut KernelVm<MountTable>, commands: Vec<&str>| {
            let driver = CommandDriver::new("quota-driver", commands);
            if operator {
                kernel.register_driver_for_operator(driver)
            } else {
                kernel.register_driver(driver)
            }
        };
        let error = register(&mut kernel, vec!["first", "second"]).unwrap_err();
        assert_eq!(error.code(), "ENOSPC");
        assert!(error.to_string().contains("limits.resources.maxInodeCount"));
        assert_eq!(
            kernel.filesystem_usage().unwrap().inode_count,
            before.inode_count
        );
        assert!(kernel.commands.resolve("first").is_none());
        assert!(kernel.storage_stat("/bin/first").unwrap().is_none());
        register(&mut kernel, vec!["first", "first"]).unwrap();
        assert_eq!(
            kernel.filesystem_usage().unwrap().inode_count,
            before.inode_count + 2
        );
        register(&mut kernel, vec!["first"]).unwrap();
        assert_eq!(
            register(&mut kernel, vec!["second"]).unwrap_err().code(),
            "ENOSPC"
        );
        assert!(kernel.commands.resolve("first").is_some());
        assert!(kernel.commands.resolve("second").is_none());
        kernel.dispose().unwrap();
    }
}

#[test]
fn command_projection_rejects_preexisting_overquota_root_without_new_commands() {
    let mut config = KernelVmConfig::new("overquota-projection");
    config.permissions = Permissions::allow_all();
    config.resources.max_inode_count = Some(0);
    let mut kernel = KernelVm::new(MountTable::new(MemoryFileSystem::new()), config);
    let error = kernel
        .register_driver_for_operator(CommandDriver::new("empty", Vec::<String>::new()))
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(kernel.storage_stat("/bin").unwrap().is_none());
    kernel.dispose().unwrap();
}

#[test]
fn command_projection_byte_admission_is_atomic_and_exact() {
    let mut config = KernelVmConfig::new("projection-bytes");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MountTable::new(MemoryFileSystem::new()), config);
    let before = kernel.filesystem_usage().unwrap();
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        max_filesystem_bytes: Some(before.total_bytes + COMMAND_STUB.len() as u64),
        ..ResourceLimits::default()
    });
    let error = kernel
        .register_driver_for_operator(CommandDriver::new("quota", ["first", "second"]))
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(error
        .to_string()
        .contains("limits.resources.maxFilesystemBytes"));
    assert!(kernel.storage_stat("/bin/first").unwrap().is_none());
    assert!(kernel.commands.resolve("first").is_none());
    kernel
        .register_driver_for_operator(CommandDriver::new("quota", ["first", "first"]))
        .unwrap();
    assert_eq!(
        kernel.filesystem_usage().unwrap().total_bytes,
        before.total_bytes + COMMAND_STUB.len() as u64
    );
}

#[test]
fn command_projection_resolves_root_symlink_and_excludes_mounted_storage() {
    for mounted in [false, true] {
        let mut root = MemoryFileSystem::new();
        root.mkdir("/tools", true).unwrap();
        root.symlink("/tools", "/bin").unwrap();
        let mut mounts = MountTable::new(root);
        if mounted {
            mounts
                .mount(
                    "/tools",
                    MemoryFileSystem::new(),
                    MountOptions::new("memory"),
                )
                .unwrap();
        }
        let mut config = KernelVmConfig::new("projection-mount-symlink");
        config.permissions = Permissions::allow_all();
        let mut kernel = KernelVm::new(mounts, config);
        let before = kernel.filesystem_usage().unwrap();
        kernel.resources = ResourceAccountant::new(ResourceLimits {
            max_inode_count: Some(before.inode_count + usize::from(!mounted)),
            max_filesystem_bytes: Some(
                before.total_bytes
                    + if mounted {
                        0
                    } else {
                        COMMAND_STUB.len() as u64
                    },
            ),
            ..ResourceLimits::default()
        });
        kernel
            .register_driver_for_operator(CommandDriver::new("quota", ["first"]))
            .unwrap();
        assert_eq!(kernel.read_file("/tools/first").unwrap(), COMMAND_STUB);
        assert_eq!(
            kernel.filesystem_usage().unwrap().inode_count,
            before.inode_count + usize::from(!mounted)
        );
        assert_eq!(
            kernel.raw_filesystem_mut().read_link("/bin").unwrap(),
            "/tools"
        );
        kernel.resources = ResourceAccountant::new(ResourceLimits {
            max_inode_count: Some(before.inode_count + 2 * usize::from(!mounted)),
            max_filesystem_bytes: Some(
                before.total_bytes
                    + if mounted {
                        0
                    } else {
                        2 * COMMAND_STUB.len() as u64
                    },
            ),
            ..ResourceLimits::default()
        });
        kernel
            .replace_driver(CommandDriver::new("quota", ["second"]))
            .unwrap();
        assert!(kernel.storage_stat("/tools/first").unwrap().is_none());
        assert_eq!(kernel.read_file("/tools/second").unwrap(), COMMAND_STUB);
        assert_eq!(
            kernel.filesystem_usage().unwrap().inode_count,
            before.inode_count + usize::from(!mounted)
        );
    }
}

// Public embedded-kernel API controls. No managed VM callsite currently uses
// replace_driver; these must be run against the preimage before extending its fix.
#[test]
fn replace_driver_quota_rejection_preserves_registry_and_files() {
    let mut config = KernelVmConfig::new("replacement-quota");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MountTable::new(MemoryFileSystem::new()), config);
    kernel
        .register_driver(CommandDriver::new("quota", ["old"]))
        .unwrap();
    let before = kernel.filesystem_usage().unwrap();
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        max_inode_count: Some(before.inode_count),
        ..ResourceLimits::default()
    });
    let error = kernel
        .replace_driver(CommandDriver::new("quota", ["old", "new"]))
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(kernel.commands.resolve("old").is_some());
    assert!(kernel.commands.resolve("new").is_none());
    assert!(kernel.storage_stat("/bin/new").unwrap().is_none());
    assert_eq!(
        kernel.filesystem_usage().unwrap().inode_count,
        before.inode_count
    );
}

#[test]
fn replace_driver_refreshes_warmed_filesystem_usage_cache() {
    let mut config = KernelVmConfig::new("replacement-cache");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MountTable::new(MemoryFileSystem::new()), config);
    kernel
        .register_driver(CommandDriver::new("quota", ["old"]))
        .unwrap();
    let before = kernel.filesystem_usage().unwrap();
    kernel
        .replace_driver(CommandDriver::new("quota", ["old", "new"]))
        .unwrap();
    let after = kernel.filesystem_usage().unwrap();
    assert_eq!(after.inode_count, before.inode_count + 1);
    assert_eq!(
        after.total_bytes,
        before.total_bytes + COMMAND_STUB.len() as u64
    );
}

#[test]
fn replace_driver_permission_failure_does_not_publish_new_registry() {
    let mut config = KernelVmConfig::new("replacement-permission");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MountTable::new(MemoryFileSystem::new()), config);
    kernel
        .register_driver(CommandDriver::new("quota", ["old"]))
        .unwrap();
    kernel.set_permissions(Permissions::default());
    let error = kernel
        .replace_driver(CommandDriver::new("quota", ["new"]))
        .unwrap_err();
    assert_eq!(error.code(), "EACCES");
    assert!(kernel.commands.resolve("old").is_some());
    assert!(kernel.commands.resolve("new").is_none());
    assert!(kernel.storage_stat("/bin/old").unwrap().is_some());
    assert!(kernel.storage_stat("/bin/new").unwrap().is_none());
}

// The injected VFS error is distinct from quota rejection: earlier writes may
// remain, but must stay accounted and must not publish a partial registry.
#[test]
fn command_projection_partial_write_failure_retains_accounted_usage() {
    let mut config = KernelVmConfig::new("projection-partial-write");
    config.permissions = Permissions::allow_all();
    let filesystem =
        CommandProjectionFailureFileSystem::new(MemoryFileSystem::new(), "/bin/second");
    let mut kernel = KernelVm::new(filesystem, config);
    let before = kernel.filesystem_usage().unwrap();
    let error = kernel
        .register_driver(CommandDriver::new("partial", ["first", "second"]))
        .unwrap_err();
    assert_eq!(error.code(), "EIO");
    assert!(kernel.commands.resolve("first").is_none());
    assert!(kernel.commands.resolve("second").is_none());
    assert!(kernel.storage_stat("/bin/first").unwrap().is_some());
    let retained = kernel.filesystem_usage().unwrap();
    assert_eq!(retained.inode_count, before.inode_count + 2);
    assert_eq!(
        retained.total_bytes,
        before.total_bytes + COMMAND_STUB.len() as u64
    );
    kernel.resources = ResourceAccountant::new(ResourceLimits {
        max_inode_count: Some(retained.inode_count + 1),
        ..ResourceLimits::default()
    });
    kernel
        .register_driver(CommandDriver::new("partial", ["first", "second"]))
        .unwrap();
    let error = kernel
        .register_driver(CommandDriver::new("extra", ["third"]))
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(kernel.commands.resolve("third").is_none());
    assert!(kernel.storage_stat("/bin/third").unwrap().is_none());
}

#[test]
fn replace_driver_partial_failure_preserves_prior_command_bytes_and_registry() {
    let mut config = KernelVmConfig::new("replacement-partial-write");
    config.permissions = Permissions::allow_all();
    let filesystem =
        CommandProjectionFailureFileSystem::new(MemoryFileSystem::new(), "/bin/failure");
    let mut kernel = KernelVm::new(filesystem, config);
    kernel
        .register_driver(CommandDriver::new("driver", ["old", "retired"]))
        .unwrap();
    // Existing caller content must never be rewritten to a generated stub.
    kernel
        .raw_filesystem_mut()
        .write_file("/bin/old", b"caller-owned executable".to_vec())
        .unwrap();
    kernel.invalidate_filesystem_usage_cache();
    let before = kernel.filesystem_usage().unwrap();
    let error = kernel
        .replace_driver(CommandDriver::new("driver", ["old", "new", "failure"]))
        .unwrap_err();
    assert_eq!(error.code(), "EIO");
    assert_eq!(
        kernel.read_file("/bin/old").unwrap(),
        b"caller-owned executable"
    );
    assert_eq!(kernel.read_file("/bin/retired").unwrap(), COMMAND_STUB);
    assert!(kernel.commands.resolve("old").is_some());
    assert!(kernel.commands.resolve("retired").is_some());
    assert!(kernel.commands.resolve("new").is_none());
    assert!(kernel.commands.resolve("failure").is_none());
    let retained = kernel.filesystem_usage().unwrap();
    assert_eq!(retained.inode_count, before.inode_count + 1);
    assert_eq!(
        retained.total_bytes,
        before.total_bytes + COMMAND_STUB.len() as u64
    );
}

#[test]
fn replace_driver_preserves_final_symlink_root_target() {
    replacement_final_symlink_control(false);
}

#[test]
fn replace_driver_preserves_final_symlink_mounted_target() {
    replacement_final_symlink_control(true);
}

fn replacement_final_symlink_control(mounted_parent: bool) {
    let mut root = MemoryFileSystem::new();
    root.mkdir("/tools", true).unwrap();
    root.symlink("/tools", "/bin").unwrap();
    let mut mounts = MountTable::new(root);
    if mounted_parent {
        mounts
            .mount(
                "/tools",
                MemoryFileSystem::new(),
                MountOptions::new("memory"),
            )
            .unwrap();
    }
    let mut config = KernelVmConfig::new("replacement-final-symlink");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(mounts, config);
    kernel
        .register_driver(CommandDriver::new("driver", ["old", "keep"]))
        .unwrap();
    kernel
        .raw_filesystem_mut()
        .remove_file("/tools/old")
        .unwrap();
    kernel
        .raw_filesystem_mut()
        .symlink("keep", "/tools/old")
        .unwrap();
    kernel.invalidate_filesystem_usage_cache();
    kernel
        .replace_driver(CommandDriver::new("driver", ["keep"]))
        .unwrap();
    assert_eq!(kernel.read_file("/tools/keep").unwrap(), COMMAND_STUB);
    assert!(kernel.commands.resolve("keep").is_some());
    assert!(kernel.commands.resolve("old").is_none());
    assert_eq!(
        kernel.raw_filesystem_mut().read_link("/tools/old").unwrap(),
        "keep"
    );
}
