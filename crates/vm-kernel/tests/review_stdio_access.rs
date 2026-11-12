use agentos_vm_kernel::fd_table::{F_GETFL, O_CREAT, O_RDONLY, O_RDWR, O_WRONLY};
use agentos_vm_kernel::kernel::{KernelVm, KernelVmConfig, VirtualProcessOptions};
use agentos_vm_kernel::permissions::Permissions;
use agentos_vm_kernel::vfs::MemoryFileSystem;

#[test]
fn linux_access_mode_three_does_not_allow_data_io() {
    let mut config = KernelVmConfig::new("access-mode-three");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MemoryFileSystem::new(), config);
    let process = kernel
        .create_virtual_process(
            "test",
            "test",
            "access-mode-three",
            vec![],
            VirtualProcessOptions::default(),
        )
        .unwrap();
    let pid = process.pid();
    let source = kernel
        .fd_open("test", pid, "/file", O_CREAT | O_RDWR, None)
        .unwrap();
    kernel.fd_write("test", pid, source, b"original").unwrap();
    // Linux reserves access mode 3 for descriptors that cannot read or write.
    let restricted = kernel.fd_open("test", pid, "/file", 3, None).unwrap();
    let duplicated = kernel.fd_dup("test", pid, restricted).unwrap();
    let alias = kernel
        .fd_open("test", pid, &format!("/dev/fd/{source}"), 3, None)
        .unwrap();
    for fd in [restricted, duplicated, alias] {
        assert_eq!(kernel.fd_fcntl("test", pid, fd, F_GETFL, 0).unwrap() & 3, 3);
        assert_eq!(
            kernel.fd_read("test", pid, fd, 1).unwrap_err().code(),
            "EBADF"
        );
        assert_eq!(
            kernel.fd_pread("test", pid, fd, 1, 0).unwrap_err().code(),
            "EBADF"
        );
        assert_eq!(
            kernel.fd_write("test", pid, fd, b"x").unwrap_err().code(),
            "EBADF"
        );
        assert_eq!(
            kernel
                .fd_pwrite("test", pid, fd, b"x", 0)
                .unwrap_err()
                .code(),
            "EBADF"
        );
    }
    assert_eq!(
        kernel.fd_pread("test", pid, source, 8, 0).unwrap(),
        b"original"
    );
}

#[test]
fn readonly_stdout_alias_does_not_retain_write_access() {
    let mut config = KernelVmConfig::new("stdio-access-review");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MemoryFileSystem::new(), config);
    let process = kernel
        .create_virtual_process(
            "test",
            "test",
            "stdio-access-review",
            vec![],
            VirtualProcessOptions::default(),
        )
        .unwrap();
    let pid = process.pid();
    let (reader, writer) = kernel.open_pipe("test", pid).unwrap();
    kernel.fd_dup2("test", pid, writer, 1).unwrap();
    let alias = kernel
        .fd_open("test", pid, "/dev/stdout", O_RDONLY, None)
        .unwrap();
    let error = kernel
        .fd_write("test", pid, alias, b"x")
        .expect_err("a read-only open must not permit writes");
    assert_eq!(error.code(), "EBADF");
    assert_eq!(
        kernel.fd_fcntl("test", pid, alias, F_GETFL, 0).unwrap() & 3,
        O_RDONLY
    );
    let duplicated = kernel.fd_dup("test", pid, alias).unwrap();
    let transfer = kernel.fd_transfer("test", pid, alias).unwrap();
    let installed = kernel
        .fd_install_transfer("test", pid, &transfer, false)
        .unwrap();
    let reopened = kernel
        .fd_open("test", pid, &format!("/dev/fd/{alias}"), O_WRONLY, None)
        .unwrap();
    for fd in [duplicated, installed, reopened] {
        assert_eq!(
            kernel.fd_write("test", pid, fd, b"x").unwrap_err().code(),
            "EBADF"
        );
        assert_eq!(
            kernel
                .fd_write_nonblocking_pipe("test", pid, fd, b"x")
                .unwrap_err()
                .code(),
            "EBADF"
        );
        kernel.fd_close("test", pid, fd).unwrap();
    }
    let child = kernel
        .create_virtual_process(
            "test",
            "test",
            "stdio-access-child",
            vec![],
            VirtualProcessOptions {
                parent_pid: Some(pid),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        kernel
            .fd_write("test", child.pid(), alias, b"x")
            .unwrap_err()
            .code(),
        "EBADF"
    );
    kernel.fd_close("test", pid, alias).unwrap();
    kernel.fd_write("test", pid, 1, b"ok").unwrap();
    assert_eq!(kernel.fd_read("test", pid, reader, 2).unwrap(), b"ok");
}

#[test]
fn readonly_regular_alias_stays_readonly_after_unlink() {
    let mut config = KernelVmConfig::new("stdio-access-regular");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MemoryFileSystem::new(), config);
    let process = kernel
        .create_virtual_process(
            "test",
            "test",
            "stdio-access-regular",
            vec![],
            VirtualProcessOptions::default(),
        )
        .unwrap();
    let pid = process.pid();
    let source = kernel
        .fd_open("test", pid, "/file", O_CREAT | O_RDWR, None)
        .unwrap();
    kernel.fd_write("test", pid, source, b"ok").unwrap();
    let alias = kernel
        .fd_open("test", pid, &format!("/dev/fd/{source}"), O_RDONLY, None)
        .unwrap();
    for unlinked in [false, true] {
        if unlinked {
            kernel.remove_file("/file").unwrap();
        }
        assert_eq!(kernel.fd_pread("test", pid, alias, 2, 0).unwrap(), b"ok");
        assert_eq!(
            kernel
                .fd_write("test", pid, alias, b"x")
                .unwrap_err()
                .code(),
            "EBADF"
        );
        assert_eq!(
            kernel
                .fd_pwrite("test", pid, alias, b"x", 0)
                .unwrap_err()
                .code(),
            "EBADF"
        );
        assert_eq!(
            kernel
                .fd_truncate("test", pid, alias, 0)
                .unwrap_err()
                .code(),
            "EBADF"
        );
        assert_eq!(kernel.fd_pread("test", pid, source, 2, 0).unwrap(), b"ok");
    }
    kernel.fd_pwrite("test", pid, source, b"OK", 0).unwrap();
    assert_eq!(kernel.fd_pread("test", pid, alias, 2, 0).unwrap(), b"OK");
}
