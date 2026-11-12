use agentos_vm_kernel::fd_table::{O_CREAT, O_DIRECTORY, O_EXCL, O_NOFOLLOW, O_RDONLY, O_WRONLY};
use agentos_vm_kernel::kernel::{KernelVm, KernelVmConfig, VirtualProcessOptions};
use agentos_vm_kernel::permissions::Permissions;
use agentos_vm_kernel::vfs::MemoryFileSystem;

#[test]
fn opening_standard_stream_paths_preserves_redirected_pipe_descriptors() {
    for (path, stdio_fd, flags) in [
        ("/dev/stdin", 0, O_RDONLY),
        ("/dev/stdout", 1, O_WRONLY),
        ("/dev/stderr", 2, O_WRONLY),
    ] {
        let mut config = KernelVmConfig::new("stdio-alias");
        config.permissions = Permissions::allow_all();
        let mut kernel = KernelVm::new(MemoryFileSystem::new(), config);
        let process = kernel
            .create_virtual_process(
                "test",
                "test",
                "stdio-alias",
                vec![],
                VirtualProcessOptions::default(),
            )
            .unwrap();
        let pid = process.pid();
        let (read_fd, write_fd) = kernel.open_pipe("test", pid).unwrap();
        let source = if stdio_fd == 0 { read_fd } else { write_fd };
        kernel.fd_dup2("test", pid, source, stdio_fd).unwrap();
        let alias = kernel.fd_open("test", pid, path, flags, None).unwrap();
        assert!(kernel.fd_is_pipe("test", pid, alias).unwrap(), "{path}");
        let writer = if stdio_fd == 0 { write_fd } else { alias };
        let reader = if stdio_fd == 0 { alias } else { read_fd };
        kernel.fd_write("test", pid, writer, b"ok").unwrap();
        assert_eq!(kernel.fd_read("test", pid, reader, 2).unwrap(), b"ok");
        kernel.fd_close("test", pid, alias).unwrap();
        assert!(kernel.fd_is_pipe("test", pid, stdio_fd).unwrap());
    }
}

#[test]
fn standard_stream_aliases_preserve_open_guards_and_descriptor_rights() {
    let mut config = KernelVmConfig::new("stdio-alias-guards");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MemoryFileSystem::new(), config);
    let process = kernel
        .create_virtual_process(
            "test",
            "test",
            "stdio-alias-guards",
            vec![],
            VirtualProcessOptions::default(),
        )
        .unwrap();
    let pid = process.pid();
    for (path, source) in [("/dev/stdin", 0), ("/dev/stdout", 1), ("/dev/stderr", 2)] {
        for (flags, code) in [
            (O_NOFOLLOW, "ELOOP"),
            (O_CREAT | O_EXCL, "EEXIST"),
            (O_DIRECTORY, "ENOTDIR"),
        ] {
            assert_eq!(
                kernel
                    .fd_open("test", pid, path, flags, None)
                    .unwrap_err()
                    .code(),
                code
            );
        }
        let original = kernel.fd_stat("test", pid, source).unwrap();
        let alias = kernel.fd_open("test", pid, path, O_RDONLY, None).unwrap();
        let duplicate = kernel.fd_stat("test", pid, alias).unwrap();
        assert_eq!(
            duplicate.rights,
            original.rights
                & agentos_vm_kernel::fd_table::wasi_rights_for_open(O_RDONLY, original.filetype).0
        );
        assert_eq!(duplicate.rights_inheriting, original.rights_inheriting);
        kernel.fd_close("test", pid, alias).unwrap();
        kernel.fd_close("test", pid, source).unwrap();
        assert_eq!(
            kernel
                .fd_open("test", pid, path, O_RDONLY, None)
                .unwrap_err()
                .code(),
            "ENOENT"
        );
    }
}
