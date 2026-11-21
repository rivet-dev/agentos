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

// The DAC search walk asks the overlay for each component with its parent
// already resolved. Deep paths must still see whiteouts, copy-up metadata,
// and renames made in the upper layer.
#[test]
fn guest_walk_through_overlay_sees_upper_whiteouts_metadata_and_renames() {
    use agentos_vm_kernel::kernel::VirtualProcessOptions;
    use agentos_vm_kernel::vfs::MemoryFileSystem;

    let mut lower = MemoryFileSystem::new();
    lower.mkdir("/pkg/lib/deep", true).unwrap();
    lower.mkdir("/pkg/sub", true).unwrap();
    for path in [
        "/pkg/lib/deep/old.js",
        "/pkg/lib/deep/keep.js",
        "/pkg/sub/x.js",
    ] {
        lower.write_file(path, b"lower".to_vec()).unwrap();
    }
    let fs = OverlayFileSystem::new(vec![lower], OverlayMode::Ephemeral);
    let mut config = KernelVmConfig::new("overlay-walk");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(fs, config);
    let process = kernel
        .create_virtual_process(
            "walk",
            "walk",
            "walk",
            Vec::new(),
            VirtualProcessOptions::default(),
        )
        .unwrap();
    let pid = process.pid();

    kernel.remove_file("/pkg/lib/deep/old.js").unwrap();
    kernel.rename("/pkg/sub", "/pkg/moved").unwrap();
    let code = |kernel: &mut KernelVm<_>, path: &str| {
        kernel
            .stat_for_process("walk", pid, path)
            .err()
            .map(|error| error.code().to_owned())
    };
    assert_eq!(
        code(&mut kernel, "/pkg/lib/deep/old.js").as_deref(),
        Some("ENOENT")
    );
    assert_eq!(code(&mut kernel, "/pkg/lib/deep/keep.js"), None);
    assert_eq!(
        code(&mut kernel, "/pkg/sub/x.js").as_deref(),
        Some("ENOENT")
    );
    assert_eq!(code(&mut kernel, "/pkg/moved/x.js"), None);

    // A copied-up chmod on an ancestor denies search below it.
    kernel.chown("/pkg/lib", 0, 0).unwrap();
    kernel.chmod("/pkg/lib", 0o700).unwrap();
    assert_eq!(
        code(&mut kernel, "/pkg/lib/deep/keep.js").as_deref(),
        Some("EACCES")
    );
}
