use agent_os_kernel::mount_table::{MountOptions, MountTable, MountedFileSystem};
use agent_os_kernel::vfs::{
    MemoryFileSystem, VfsResult, VirtualDirEntry, VirtualFileSystem, VirtualStat, VirtualUtimeSpec,
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

    fn pread(&mut self, path: &str, _offset: u64, _length: usize) -> VfsResult<Vec<u8>> {
        unreachable!("failed mount should not pread {path}")
    }

    fn shutdown(&mut self) -> VfsResult<()> {
        self.shutdown.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn mount_table_prefers_mounted_filesystems_and_merges_mount_points() {
    let mut root = MemoryFileSystem::new();
    root.write_file("/data/root-only.txt", b"root".to_vec())
        .expect("seed root file");

    let mut mounted = MemoryFileSystem::new();
    mounted
        .write_file("/mounted.txt", b"mounted".to_vec())
        .expect("seed mounted file");

    let mut table = MountTable::new(root);
    table
        .mount("/data", mounted, MountOptions::new("memory"))
        .expect("mount memory filesystem");

    assert_eq!(
        table
            .read_file("/data/mounted.txt")
            .expect("read mounted file"),
        b"mounted".to_vec()
    );
    assert!(!table.exists("/data/root-only.txt"));

    let root_entries = table.read_dir("/").expect("read root directory");
    assert!(root_entries.contains(&String::from("data")));
}

#[test]
fn mount_table_enforces_read_only_and_cross_mount_boundaries() {
    let mut table = MountTable::new(MemoryFileSystem::new());
    table
        .mount(
            "/readonly",
            MemoryFileSystem::new(),
            MountOptions::new("memory").read_only(true),
        )
        .expect("mount readonly filesystem");
    table
        .mount(
            "/writable",
            MemoryFileSystem::new(),
            MountOptions::new("memory"),
        )
        .expect("mount writable filesystem");

    let read_only_error = table
        .write_file("/readonly/blocked.txt", b"blocked".to_vec())
        .expect_err("readonly mount should reject writes");
    assert_eq!(read_only_error.code(), "EROFS");

    table
        .write_file("/writable/file.txt", b"ok".to_vec())
        .expect("write mounted file");
    let cross_mount_error = table
        .rename("/writable/file.txt", "/file.txt")
        .expect_err("rename across mounts should fail");
    assert_eq!(cross_mount_error.code(), "EXDEV");
}

#[test]
fn mount_table_rejects_symlinks_that_cross_mount_boundaries() {
    let mut root = MemoryFileSystem::new();
    root.write_file("/root.txt", b"root".to_vec())
        .expect("seed root file");

    let mut mounted = MemoryFileSystem::new();
    mounted
        .write_file("/inside.txt", b"inside".to_vec())
        .expect("seed mounted file");

    let mut table = MountTable::new(root);
    table
        .mount("/mounted", mounted, MountOptions::new("memory"))
        .expect("mount memory filesystem");

    let error = table
        .symlink("../root.txt", "/mounted/root-link")
        .expect_err("cross-mount symlink should fail");
    assert_eq!(error.code(), "EXDEV");
}

#[test]
fn mount_table_rejects_hardlinks_that_cross_mount_boundaries() {
    let mut root = MemoryFileSystem::new();
    root.write_file("/root.txt", b"root".to_vec())
        .expect("seed root file");

    let mut mounted = MemoryFileSystem::new();
    mounted
        .write_file("/inside.txt", b"inside".to_vec())
        .expect("seed mounted file");

    let mut table = MountTable::new(root);
    table
        .mount("/mounted", mounted, MountOptions::new("memory"))
        .expect("mount memory filesystem");

    let error = table
        .link("/root.txt", "/mounted/root-link")
        .expect_err("cross-mount hardlink should fail");
    assert_eq!(error.code(), "EXDEV");
}

#[test]
fn mount_table_mounts_nested_filesystems_under_read_only_parents() {
    let mut table = MountTable::new(MemoryFileSystem::new());
    table
        .mount(
            "/root/node_modules",
            MemoryFileSystem::new(),
            MountOptions::new("memory").read_only(true),
        )
        .expect("mount read-only parent filesystem");

    let mut nested = MemoryFileSystem::new();
    nested
        .write_file("/package.json", b"{}".to_vec())
        .expect("seed nested package file");

    table
        .mount(
            "/root/node_modules/@scope/pkg",
            nested,
            MountOptions::new("memory").read_only(true),
        )
        .expect("read-only parents must still accept nested mounts");

    assert_eq!(
        table
            .read_file("/root/node_modules/@scope/pkg/package.json")
            .expect("read file through nested mount"),
        b"{}".to_vec()
    );
}

#[test]
fn mount_table_rejects_mount_when_mount_point_creation_fails() {
    let mut root = MemoryFileSystem::new();
    root.write_file("/blocked", b"not a directory".to_vec())
        .expect("seed file at parent path");
    let mut table = MountTable::new(root);

    let error = table
        .mount(
            "/blocked/child",
            MemoryFileSystem::new(),
            MountOptions::new("memory"),
        )
        .expect_err("mount point creation should fail through file parent");

    assert_eq!(error.code(), "ENOTDIR");
    assert!(!table
        .get_mounts()
        .iter()
        .any(|mount| mount.path == "/blocked/child"));
}

#[test]
fn mount_table_shuts_down_boxed_filesystem_when_mount_point_creation_fails() {
    let mut root = MemoryFileSystem::new();
    root.write_file("/blocked", b"not a directory".to_vec())
        .expect("seed file at parent path");
    let mut table = MountTable::new(root);
    let shutdown = Arc::new(AtomicBool::new(false));

    let error = table
        .mount_boxed(
            "/blocked/child",
            Box::new(ShutdownTrackingFileSystem::new(Arc::clone(&shutdown))),
            MountOptions::new("tracking"),
        )
        .expect_err("mount point creation should fail through file parent");

    assert_eq!(error.code(), "ENOTDIR");
    assert!(shutdown.load(Ordering::SeqCst));
}

#[test]
fn mount_table_unmount_rejects_parent_mounts_with_children() {
    let mut table = MountTable::new(MemoryFileSystem::new());
    table
        .mount("/a", MemoryFileSystem::new(), MountOptions::new("parent"))
        .expect("mount parent filesystem");
    table
        .mount("/a/b", MemoryFileSystem::new(), MountOptions::new("child"))
        .expect("mount child filesystem");

    let error = table
        .unmount("/a")
        .expect_err("parent mount should stay busy while child mount exists");
    assert_eq!(error.code(), "EBUSY");
}

#[test]
fn mount_table_unmount_succeeds_after_children_are_removed() {
    let mut table = MountTable::new(MemoryFileSystem::new());
    table
        .mount("/a", MemoryFileSystem::new(), MountOptions::new("parent"))
        .expect("mount parent filesystem");
    table
        .mount("/a/b", MemoryFileSystem::new(), MountOptions::new("child"))
        .expect("mount child filesystem");

    table.unmount("/a/b").expect("unmount child first");
    table.unmount("/a").expect("unmount parent after child");
}
