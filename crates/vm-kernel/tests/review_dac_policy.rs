use agentos_vm_kernel::kernel::{KernelVm, KernelVmConfig, VirtualProcessOptions};
use agentos_vm_kernel::permissions::{
    FsAccessRequest, FsOperation, PermissionDecision, PermissionEvaluator, Permissions,
};
use agentos_vm_kernel::vfs::MemoryFileSystem;

const DRIVER: &str = "review-policy";

fn fixture() -> (KernelVm<MemoryFileSystem>, u32) {
    let mut config = KernelVmConfig::new("review-policy");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MemoryFileSystem::new(), config);
    kernel
        .write_file("/permitted.txt", b"permitted data")
        .unwrap();
    kernel.symlink("/permitted.txt", "/alias.txt").unwrap();
    let pid = kernel
        .create_virtual_process(
            DRIVER,
            DRIVER,
            "review-policy",
            Vec::new(),
            VirtualProcessOptions::default(),
        )
        .unwrap()
        .pid();
    (kernel, pid)
}

#[test]
fn permitted_root_file_read_does_not_require_root_stat_capability() {
    let (mut kernel, pid) = fixture();
    kernel.set_permissions(Permissions {
        filesystem: PermissionEvaluator::dynamic(|request: &FsAccessRequest| {
            if request.path == "/" {
                PermissionDecision::deny("root metadata is not granted")
            } else {
                PermissionDecision::allow()
            }
        }),
        ..Permissions::default()
    });
    assert_eq!(
        kernel
            .read_file_for_process(DRIVER, pid, "/permitted.txt")
            .unwrap(),
        b"permitted data"
    );
    assert_eq!(
        kernel
            .stat_for_process(DRIVER, pid, "/")
            .unwrap_err()
            .code(),
        "EACCES"
    );
}

#[test]
fn permitted_symlink_read_does_not_require_readlink_capability() {
    let (mut kernel, pid) = fixture();
    kernel.set_permissions(Permissions {
        filesystem: PermissionEvaluator::dynamic(|request: &FsAccessRequest| {
            if request.op == FsOperation::ReadLink {
                PermissionDecision::deny("link metadata is not granted")
            } else {
                PermissionDecision::allow()
            }
        }),
        ..Permissions::default()
    });
    assert_eq!(
        kernel
            .read_file_for_process(DRIVER, pid, "/alias.txt")
            .unwrap(),
        b"permitted data"
    );
    assert_eq!(
        kernel
            .read_link_for_process(DRIVER, pid, "/alias.txt")
            .unwrap_err()
            .code(),
        "EACCES"
    );
}

#[test]
fn internal_directory_acl_lookup_respects_dac_without_guest_metadata_grants() {
    for (search_allowed, content_allowed) in [(true, true), (false, true), (true, false)] {
        let (mut kernel, pid) = fixture();
        let identity = kernel.process_identity(DRIVER, pid).unwrap();
        assert_ne!(identity.euid, 0, "fixture must exercise non-root DAC");
        kernel.mkdir("/acl", false).unwrap();
        kernel.write_file("/acl/file", b"permitted data").unwrap();
        kernel
            .chown("/acl", identity.euid + 1, identity.egid + 1)
            .unwrap();
        let mut acl = 2u32.to_le_bytes().to_vec();
        for (tag, mode, id) in [
            (0x01u16, 7u16, u32::MAX),
            (0x02, u16::from(search_allowed), identity.euid),
            (0x04, 0, u32::MAX),
            (0x10, 1, u32::MAX),
            (0x20, 0, u32::MAX),
        ] {
            acl.extend_from_slice(&tag.to_le_bytes());
            acl.extend_from_slice(&mode.to_le_bytes());
            acl.extend_from_slice(&id.to_le_bytes());
        }
        kernel
            .set_xattr("/acl", "system.posix_acl_access", acl, 0, true)
            .unwrap();
        kernel.set_permissions(Permissions {
            filesystem: PermissionEvaluator::dynamic(move |request: &FsAccessRequest| {
                if request.path == "/acl" && request.op == FsOperation::Read {
                    PermissionDecision::deny("directory ACL metadata is not guest-readable")
                } else if !content_allowed
                    && request.path == "/acl/file"
                    && request.op == FsOperation::Read
                {
                    PermissionDecision::deny("file content denied")
                } else {
                    PermissionDecision::allow()
                }
            }),
            ..Permissions::default()
        });
        let result = kernel.read_file_for_process(DRIVER, pid, "/acl/file");
        if search_allowed && content_allowed {
            assert_eq!(result.unwrap(), b"permitted data");
        } else {
            assert_eq!(result.unwrap_err().code(), "EACCES");
        }
        assert_eq!(
            kernel
                .get_xattr_for_process(DRIVER, pid, "/acl", "system.posix_acl_access", true)
                .unwrap_err()
                .code(),
            "EACCES"
        );
    }
}
