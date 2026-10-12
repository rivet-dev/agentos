use agentos_vm_kernel::command_registry::CommandDriver;
use agentos_vm_kernel::fd_table::{O_APPEND, O_CREAT, O_EXCL, O_WRONLY};
use agentos_vm_kernel::kernel::{KernelVm, KernelVmConfig, SpawnOptions};
use agentos_vm_kernel::overlay_fs::{OverlayFileSystem, OverlayMode};
use agentos_vm_kernel::permissions::Permissions;
use agentos_vm_kernel::vfs::VirtualFileSystem;

#[test]
fn hard_link_preserves_a_symlink_with_a_non_directory_target_component() {
    let fs = OverlayFileSystem::new(vec![], OverlayMode::Ephemeral);
    let mut config = KernelVmConfig::new("symlink-link");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(fs, config);
    kernel.write_file("/file", "data").unwrap();
    kernel.symlink("/file/child", "/link").unwrap();
    kernel.link("/link", "/alias").unwrap();
    assert_eq!(kernel.read_link("/alias").unwrap(), "/file/child");
    assert_eq!(kernel.link("/link", "/link").unwrap_err().code(), "EEXIST");
}

#[test]
fn open_create_follows_dangling_link_but_exclusive_create_does_not() {
    let mut fs = OverlayFileSystem::new(vec![], OverlayMode::Ephemeral);
    fs.mkdir("/tmp", true).unwrap();
    fs.symlink("target", "/tmp/link").unwrap();
    fs.symlink("cycle", "/tmp/cycle").unwrap();
    let mut config = KernelVmConfig::new("symlink-open");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(fs, config);
    kernel
        .register_driver(CommandDriver::new("shell", ["sh"]))
        .unwrap();
    let process = kernel
        .spawn_process(
            "sh",
            vec![],
            SpawnOptions {
                requester_driver: Some("shell".into()),
                ..SpawnOptions::default()
            },
        )
        .unwrap();
    let fd = kernel
        .fd_open(
            "shell",
            process.pid(),
            "/tmp/link",
            O_CREAT | O_APPEND | O_WRONLY,
            None,
        )
        .expect("create dangling target");
    kernel
        .fd_write("shell", process.pid(), fd, b"bytes")
        .unwrap();
    assert_eq!(kernel.read_file("/tmp/target").unwrap(), b"bytes");
    assert_eq!(
        kernel
            .fd_open(
                "shell",
                process.pid(),
                "/tmp/cycle",
                O_CREAT | O_EXCL | O_WRONLY,
                None
            )
            .unwrap_err()
            .code(),
        "EEXIST"
    );
}
