use agentos_vm_kernel::kernel::{KernelVm, KernelVmConfig, VirtualProcessOptions};
use agentos_vm_kernel::permissions::Permissions;
use agentos_vm_kernel::user::UserConfig;
use agentos_vm_kernel::vfs::{
    MemoryFileSystem, VfsResult, VirtualDirEntry, VirtualFileSystem, VirtualStat,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

const DRIVER: &str = "read-metadata-test";
const TARGET: &str = "/data/value.cjs";

struct CountingFileSystem {
    inner: MemoryFileSystem,
    target_stats: Arc<AtomicUsize>,
}

impl VirtualFileSystem for CountingFileSystem {
    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        self.inner.read_file(path)
    }
    fn read_dir(&mut self, path: &str) -> VfsResult<Vec<String>> {
        self.inner.read_dir(path)
    }
    fn read_dir_with_types(&mut self, path: &str) -> VfsResult<Vec<VirtualDirEntry>> {
        self.inner.read_dir_with_types(path)
    }
    fn write_file(&mut self, path: &str, content: impl Into<Vec<u8>>) -> VfsResult<()> {
        self.inner.write_file(path, content)
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
        if path == TARGET {
            self.target_stats.fetch_add(1, Ordering::Relaxed);
        }
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

fn fixture() -> (KernelVm<CountingFileSystem>, u32, Arc<AtomicUsize>) {
    let target_stats = Arc::new(AtomicUsize::new(0));
    let fs = CountingFileSystem {
        inner: MemoryFileSystem::new(),
        target_stats: Arc::clone(&target_stats),
    };
    let mut config = KernelVmConfig::new("read-metadata");
    config.permissions = Permissions::allow_all();
    config.user = UserConfig {
        uid: Some(1000),
        gid: Some(1000),
        ..UserConfig::default()
    };
    let mut kernel = KernelVm::new(fs, config);
    kernel.mkdir("/data", false).expect("create parent");
    kernel
        .write_file(TARGET, b"module.exports = 1;".to_vec())
        .expect("write module");
    kernel.chown(TARGET, 1000, 1000).expect("set file owner");
    kernel.chmod(TARGET, 0o600).expect("allow owner reads");
    let pid = kernel
        .create_virtual_process(
            DRIVER,
            DRIVER,
            "read-metadata",
            Vec::new(),
            VirtualProcessOptions::default(),
        )
        .expect("create non-root process")
        .pid();
    target_stats.store(0, Ordering::Relaxed);
    (kernel, pid, target_stats)
}

#[test]
fn process_read_reuses_checked_target_metadata_within_one_request() {
    let (mut kernel, pid, target_stats) = fixture();
    assert_eq!(
        kernel
            .read_file_for_process(DRIVER, pid, TARGET)
            .expect("read module"),
        b"module.exports = 1;"
    );
    assert_eq!(
        target_stats.load(Ordering::Relaxed),
        2,
        "one DAC/kind/size stat plus the device layer classification stat"
    );
}

#[test]
fn process_read_rechecks_permissions_after_chmod() {
    let (mut kernel, pid, _) = fixture();
    kernel
        .read_file_for_process(DRIVER, pid, TARGET)
        .expect("initial allowed read");
    kernel.chmod(TARGET, 0o000).expect("revoke read permission");
    assert_eq!(
        kernel
            .read_file_for_process(DRIVER, pid, TARGET)
            .expect_err("later request must observe revoked DAC permission")
            .code(),
        "EACCES"
    );
    kernel
        .chmod(TARGET, 0o600)
        .expect("restore read permission");
    assert_eq!(
        kernel
            .read_file_for_process(DRIVER, pid, TARGET)
            .expect("read after restore"),
        b"module.exports = 1;"
    );
}

#[test]
fn process_read_rejects_direct_and_symlinked_unix_socket_paths() {
    let (mut kernel, pid, _) = fixture();
    kernel.chmod("/data", 0o777).expect("allow pathname bind");
    kernel
        .bind_unix_socket_path_for_process(DRIVER, pid, "/", "/data/socket")
        .expect("materialize Unix socket inode");
    kernel
        .symlink("/data/socket", "/data/socket-link")
        .expect("link socket");
    for path in ["/data/socket", "/data/socket-link"] {
        assert_eq!(
            kernel
                .read_file_for_process(DRIVER, pid, path)
                .expect_err("socket path must not support file reads")
                .code(),
            "ENXIO"
        );
    }
}

#[test]
fn process_read_observes_replacement_and_symlink_retarget_between_requests() {
    let (mut kernel, pid, _) = fixture();
    kernel.symlink(TARGET, "/data/link").expect("link original");
    assert_eq!(
        kernel
            .read_file_for_process(DRIVER, pid, "/data/link")
            .expect("read original"),
        b"module.exports = 1;"
    );
    kernel
        .write_file(
            "/data/replacement",
            b"replacement with a different size".to_vec(),
        )
        .expect("write replacement");
    kernel
        .rename("/data/replacement", TARGET)
        .expect("replace target");
    assert_eq!(
        kernel
            .read_file_for_process(DRIVER, pid, "/data/link")
            .expect("read replacement"),
        b"replacement with a different size"
    );
    kernel
        .write_file("/data/other", b"other".to_vec())
        .expect("write other target");
    kernel
        .remove_file("/data/link")
        .expect("unlink old symlink");
    kernel
        .symlink("/data/other", "/data/link")
        .expect("retarget symlink");
    assert_eq!(
        kernel
            .read_file_for_process(DRIVER, pid, "/data/link")
            .expect("read new target"),
        b"other"
    );
}

#[test]
fn process_read_still_enforces_read_policy_after_allowed_stat() {
    use agentos_vm_kernel::permissions::{FsOperation, FsPermissionEvaluator, PermissionDecision};
    let (mut kernel, pid, _) = fixture();
    let mut permissions = Permissions::allow_all();
    permissions.filesystem_unrestricted = false;
    permissions.filesystem = FsPermissionEvaluator::dynamic(|request| {
        if request.op == FsOperation::Read {
            PermissionDecision::deny("reads disabled")
        } else {
            PermissionDecision::allow()
        }
    });
    kernel.set_permissions(permissions);
    kernel
        .stat_for_process(DRIVER, pid, TARGET)
        .expect("metadata remains allowed");
    assert_eq!(
        kernel
            .read_file_for_process(DRIVER, pid, TARGET)
            .expect_err("final pread must enforce Read policy")
            .code(),
        "EACCES"
    );
}

#[test]
fn process_read_preserves_missing_directory_and_symlink_loop_errors() {
    let (mut kernel, pid, _) = fixture();
    kernel
        .symlink("/data/loop", "/data/loop")
        .expect("create symlink loop");
    for (path, code) in [
        ("/data/missing", "ENOENT"),
        ("/data", "EISDIR"),
        ("/data/loop", "ELOOP"),
    ] {
        assert_eq!(
            kernel
                .read_file_for_process(DRIVER, pid, path)
                .expect_err("invalid read target must retain its errno")
                .code(),
            code,
            "{path}"
        );
    }
}
