use agentos_vm_kernel::command_registry::CommandDriver;
use agentos_vm_kernel::kernel::{KernelVm, KernelVmConfig, VirtualProcessOptions};
use agentos_vm_kernel::mount_table::{MountOptions, MountTable, MountedVirtualFileSystem};
use agentos_vm_kernel::permissions::Permissions;
use agentos_vm_kernel::vfs::{MemoryFileSystem, VirtualFileSystem};

#[test]
fn virtual_mount_ancestors_preserve_real_ancestor_dac() {
    let mut root = MemoryFileSystem::new();
    root.mkdir("/private", true).unwrap();
    root.chmod("/private", 0o700).unwrap();
    root.chown("/private", 0, 0).unwrap();
    root.mkdir("/bin", true).unwrap();
    root.write_file("/bin/node", b"#!/bin/sh\nexit 0\n".to_vec())
        .unwrap();
    root.chmod("/bin/node", 0o755).unwrap();
    let table = MountTable::new_boxed_root(
        Box::new(MountedVirtualFileSystem::new(root)),
        MountOptions::new("root").read_only(true),
    );
    let mut config = KernelVmConfig::new("namespace-dac");
    config.permissions = Permissions::allow_all();
    config.user.uid = Some(1000);
    config.user.gid = Some(1000);
    let mut kernel = KernelVm::new(table, config);
    for path in ["/public/deep/payload", "/private/deep/payload"] {
        let mut payload = MemoryFileSystem::new();
        payload
            .write_file("/command", b"mounted-data".to_vec())
            .unwrap();
        kernel
            .mount_boxed_filesystem_for_operator(
                path,
                Box::new(MountedVirtualFileSystem::new(payload)),
                MountOptions::new("payload").read_only(true),
            )
            .unwrap();
    }
    kernel
        .register_driver(CommandDriver::new("node", ["node"]))
        .unwrap();
    let process = kernel
        .create_virtual_process(
            "node",
            "node",
            "node",
            Vec::new(),
            VirtualProcessOptions::default(),
        )
        .unwrap();
    let pid = process.pid();
    assert_eq!(
        kernel
            .read_file_for_process("node", pid, "/public/deep/payload/command")
            .unwrap(),
        b"mounted-data"
    );
    assert_eq!(
        kernel
            .read_file_for_process("node", pid, "/private/deep/payload/command")
            .unwrap_err()
            .code(),
        "EACCES"
    );
    assert_eq!(
        kernel
            .stat_for_process("node", pid, "/public/deep")
            .unwrap()
            .mode
            & 0o7777,
        0o555
    );
    assert_eq!(
        kernel
            .write_file_for_process("node", pid, "/public/deep/new", vec![1], None)
            .unwrap_err()
            .code(),
        "EACCES"
    );
}
