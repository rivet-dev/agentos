// General mount and storage-adapter POSIX coverage lives in
// agentos-vfs-storage/tests/posix_mount_table.rs. These tests cover the
// engine-independent ownership of detached and rejected mount backends.
use agentos_vfs_core::posix::{
    MemoryFileSystem, MountOptions, MountTable, MountedFileSystem, VfsResult, VirtualDirEntry,
    VirtualStat, VirtualUtimeSpec,
};
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct ShutdownTrackingFileSystem {
    shutdown: Arc<AtomicBool>,
}

impl ShutdownTrackingFileSystem {
    fn new(shutdown: Arc<AtomicBool>) -> Self {
        Self { shutdown }
    }
}

impl MountedFileSystem for ShutdownTrackingFileSystem {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        unreachable!("failed mount should not read {path}")
    }

    fn read_dir(&mut self, path: &str) -> VfsResult<Vec<String>> {
        unreachable!("failed mount should not read dir {path}")
    }

    fn read_dir_with_types(&mut self, path: &str) -> VfsResult<Vec<VirtualDirEntry>> {
        unreachable!("failed mount should not read dir types {path}")
    }

    fn write_file(&mut self, path: &str, _content: Vec<u8>) -> VfsResult<()> {
        unreachable!("failed mount should not write {path}")
    }

    fn create_dir(&mut self, path: &str) -> VfsResult<()> {
        unreachable!("failed mount should not create dir {path}")
    }

    fn mkdir(&mut self, path: &str, _recursive: bool) -> VfsResult<()> {
        unreachable!("failed mount should not mkdir {path}")
    }

    fn exists(&self, _path: &str) -> bool {
        false
    }

    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat> {
        unreachable!("failed mount should not stat {path}")
    }

    fn remove_file(&mut self, path: &str) -> VfsResult<()> {
        unreachable!("failed mount should not remove file {path}")
    }

    fn remove_dir(&mut self, path: &str) -> VfsResult<()> {
        unreachable!("failed mount should not remove dir {path}")
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        unreachable!("failed mount should not rename {old_path} to {new_path}")
    }

    fn realpath(&self, path: &str) -> VfsResult<String> {
        unreachable!("failed mount should not realpath {path}")
    }

    fn symlink(&mut self, target: &str, link_path: &str) -> VfsResult<()> {
        unreachable!("failed mount should not symlink {target} to {link_path}")
    }

    fn read_link(&self, path: &str) -> VfsResult<String> {
        unreachable!("failed mount should not readlink {path}")
    }

    fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        unreachable!("failed mount should not lstat {path}")
    }

    fn link(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        unreachable!("failed mount should not link {old_path} to {new_path}")
    }

    fn chmod(&mut self, path: &str, _mode: u32) -> VfsResult<()> {
        unreachable!("failed mount should not chmod {path}")
    }

    fn chown(&mut self, path: &str, _uid: u32, _gid: u32) -> VfsResult<()> {
        unreachable!("failed mount should not chown {path}")
    }

    fn utimes(&mut self, path: &str, _atime_ms: u64, _mtime_ms: u64) -> VfsResult<()> {
        unreachable!("failed mount should not utimes {path}")
    }

    fn utimes_spec(
        &mut self,
        path: &str,
        _atime: VirtualUtimeSpec,
        _mtime: VirtualUtimeSpec,
        _follow_symlinks: bool,
    ) -> VfsResult<()> {
        unreachable!("failed mount should not utimes_spec {path}")
    }

    fn truncate(&mut self, path: &str, _length: u64) -> VfsResult<()> {
        unreachable!("failed mount should not truncate {path}")
    }

    fn insert_range(&mut self, path: &str, _offset: u64, _length: u64) -> VfsResult<()> {
        unreachable!("failed mount should not insert range in {path}")
    }

    fn collapse_range(&mut self, path: &str, _offset: u64, _length: u64) -> VfsResult<()> {
        unreachable!("failed mount should not collapse range in {path}")
    }

    fn pread(&mut self, path: &str, _offset: u64, _length: usize) -> VfsResult<Vec<u8>> {
        unreachable!("failed mount should not pread {path}")
    }

    fn shutdown(&mut self) -> VfsResult<()> {
        self.shutdown.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn detached_mount_shutdown_follows_ownership_and_restore() {
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut table = MountTable::new(MemoryFileSystem::new());
    table
        .mount_boxed(
            "/data",
            Box::new(ShutdownTrackingFileSystem::new(shutdown.clone())),
            MountOptions::new("tracking"),
        )
        .unwrap();
    let detached = table.detach("/data").unwrap();
    assert!(!shutdown.load(Ordering::SeqCst));
    table.restore_detached(detached).unwrap();
    assert!(
        !shutdown.load(Ordering::SeqCst),
        "restoration must transfer ownership without shutdown"
    );
    let detached = table.detach("/data").unwrap();
    drop(detached);
    assert!(
        shutdown.load(Ordering::SeqCst),
        "abandoned detached backend must shut down"
    );
}

#[test]
fn rejected_mount_and_restore_shut_down_the_unpublished_backend() {
    let mut table = MountTable::new(MemoryFileSystem::new());
    table
        .mount(
            "/data",
            MemoryFileSystem::new(),
            MountOptions::new("existing"),
        )
        .unwrap();
    for path in ["/", "/data"] {
        let shutdown = Arc::new(AtomicBool::new(false));
        assert!(table
            .mount_boxed(
                path,
                Box::new(ShutdownTrackingFileSystem::new(shutdown.clone())),
                MountOptions::new("rejected")
            )
            .is_err());
        assert!(shutdown.load(Ordering::SeqCst));
    }
    let shutdown = Arc::new(AtomicBool::new(false));
    table
        .mount_boxed(
            "/old",
            Box::new(ShutdownTrackingFileSystem::new(shutdown.clone())),
            MountOptions::new("old"),
        )
        .unwrap();
    let detached = table.detach("/old").unwrap();
    table
        .mount(
            "/old",
            MemoryFileSystem::new(),
            MountOptions::new("replacement"),
        )
        .unwrap();
    assert_eq!(
        table.restore_detached(detached).unwrap_err().code(),
        "EEXIST"
    );
    assert!(shutdown.load(Ordering::SeqCst));
    assert!(table
        .get_mounts()
        .iter()
        .any(|mount| mount.path == "/old" && mount.plugin_id == "replacement"));
}
