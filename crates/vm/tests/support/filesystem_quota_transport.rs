use super::*;
use agentos_vm_kernel::kernel::KernelError;
use agentos_vm_kernel::resource_accounting::{FileSystemUsage, ResourceAccountant, ResourceLimits};
use agentos_vm_kernel::vfs::VfsError;

#[test]
fn typed_quota_crosses_core_and_host_without_backend_reclassification() {
    for (limits, bytes, inodes, setting, unit, limit, observed) in [
        (
            ResourceLimits {
                max_filesystem_bytes: Some(u64::MAX - 1),
                ..Default::default()
            },
            u64::MAX,
            0,
            "limits.resources.maxFilesystemBytes",
            "bytes",
            u64::MAX - 1,
            u64::MAX,
        ),
        (
            ResourceLimits {
                max_inode_count: Some(4),
                ..Default::default()
            },
            0,
            5,
            "limits.resources.maxInodeCount",
            "inodes",
            4,
            5,
        ),
    ] {
        let error = ResourceAccountant::new(limits)
            .check_filesystem_usage(&FileSystemUsage::default(), bytes, inodes)
            .unwrap_err();
        let core = crate::core::SidecarCoreError::from_kernel(error.into());
        assert_eq!(core.code(), Some("ENOSPC"));
        let VmError::Host(host) = native_guest_filesystem_core_error(core) else {
            panic!("typed host error expected")
        };
        assert_eq!(host.code, "ENOSPC");
        let details = host.details.unwrap();
        assert_eq!(details["limitName"], setting);
        assert_eq!(details["configPath"], setting);
        assert_eq!(details["unit"], unit);
        assert_eq!(details["limit"].as_u64(), Some(limit));
        assert_eq!(details["observed"].as_u64(), Some(observed));
    }
    let backend: KernelError = VfsError::new("ENOSPC", "backend private diagnostic").into();
    let core = crate::core::SidecarCoreError::from_kernel(backend);
    let VmError::Host(host) = native_guest_filesystem_core_error(core) else {
        panic!("backend errno expected")
    };
    assert_eq!(host.code, "ENOSPC");
    assert!(host.details.is_none());
}
