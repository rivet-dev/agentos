use agentos_vm_kernel::kernel::{KernelError, KernelVm, KernelVmConfig};
use agentos_vm_kernel::permissions::Permissions;
use agentos_vm_kernel::resource_accounting::{FileSystemUsage, ResourceAccountant, ResourceLimits};
use agentos_vm_kernel::vfs::{MemoryFileSystem, VfsError};

#[test]
fn filesystem_quota_diagnostics_keep_exact_u64_and_errno() {
    let accounting = ResourceAccountant::new(ResourceLimits {
        max_filesystem_bytes: Some(u64::MAX - 1),
        ..Default::default()
    });
    let error = accounting
        .check_filesystem_usage(&FileSystemUsage::default(), u64::MAX, 0)
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    let typed = error.filesystem_quota().expect("owning quota data");
    assert_eq!(typed.setting(), "limits.resources.maxFilesystemBytes");
    assert_eq!(typed.unit(), "bytes");
    assert_eq!(typed.limit(), u64::MAX - 1);
    assert_eq!(typed.observed(), u64::MAX);
    let kernel: KernelError = error.into();
    assert_eq!(kernel.code(), "ENOSPC");
    assert_eq!(kernel.filesystem_quota(), Some(typed));
    let backend: KernelError = VfsError::new("ENOSPC", "ordinary backend full").into();
    assert_eq!(backend.code(), "ENOSPC");
    assert_eq!(backend.filesystem_quota(), None);
}

#[test]
fn inode_quota_rejection_keeps_original_file_and_usage() {
    let mut config = KernelVmConfig::new("quota-owner");
    config.permissions = Permissions::allow_all();
    config.resources = ResourceLimits {
        max_filesystem_bytes: None,
        max_inode_count: Some(4),
        ..Default::default()
    };
    let mut kernel = KernelVm::new(MemoryFileSystem::new(), config);
    kernel.write_file("/tmp/a.txt", b"hello".to_vec()).unwrap();
    kernel.create_dir("/tmp/dir").unwrap();
    let first = kernel.write_file("/tmp/b.txt", vec![1]).unwrap_err();
    let quota = first.filesystem_quota().unwrap();
    assert_eq!(first.code(), "ENOSPC");
    assert_eq!(quota.setting(), "limits.resources.maxInodeCount");
    assert_eq!(quota.unit(), "inodes");
    assert_eq!(quota.limit(), 4);
    assert_eq!(quota.observed(), 5);
    let second = kernel.write_file("/tmp/b.txt", vec![1]).unwrap_err();
    assert_eq!(
        second.filesystem_quota(),
        Some(quota),
        "rejection must not charge usage"
    );
    let mut byte_config = KernelVmConfig::new("byte-quota-owner");
    byte_config.permissions = Permissions::allow_all();
    byte_config.resources = ResourceLimits {
        max_filesystem_bytes: Some(5),
        ..Default::default()
    };
    let mut byte_kernel = KernelVm::new(MemoryFileSystem::new(), byte_config);
    byte_kernel
        .write_file("/tmp/a.txt", b"hello".to_vec())
        .unwrap();
    let grow = byte_kernel.truncate("/tmp/a.txt", 6).unwrap_err();
    let bytes = grow.filesystem_quota().unwrap();
    assert_eq!(grow.code(), "ENOSPC");
    assert_eq!(bytes.setting(), "limits.resources.maxFilesystemBytes");
    assert_eq!(bytes.limit(), 5);
    assert_eq!(bytes.observed(), 6);
    assert_eq!(kernel.read_file("/tmp/a.txt").unwrap(), b"hello");
    assert_eq!(byte_kernel.read_file("/tmp/a.txt").unwrap(), b"hello");
}
