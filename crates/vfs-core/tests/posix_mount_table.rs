// General mount and storage-adapter POSIX coverage lives in
// agentos-vfs-storage/tests/posix_mount_table.rs. These tests cover the
// engine-independent ownership of detached and rejected mount backends.
use agentos_vfs_core::posix::{
    MemoryFileSystem, MountOptions, MountTable, MountedFileSystem, VfsResult, VirtualDirEntry,
    VirtualFileSystem, VirtualStat, VirtualUtimeSpec,
};
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[test]
fn symlink_parent_is_resolved_before_dotdot_across_mounts() {
    let mut table = MountTable::new(MemoryFileSystem::new());
    table.create_dir("/outside").unwrap();
    table
        .write_file("/outside/sibling", b"lexical-parent".to_vec())
        .unwrap();
    table.create_dir("/mounted").unwrap();
    table
        .mount(
            "/mounted",
            MemoryFileSystem::new(),
            MountOptions::new("memory"),
        )
        .unwrap();
    table.create_dir("/mounted/inner").unwrap();
    table
        .write_file("/mounted/sibling", b"resolved-parent".to_vec())
        .unwrap();
    table.symlink("/mounted/inner", "/outside/link").unwrap();

    assert_eq!(
        table.read_file("/outside/link/../sibling").unwrap(),
        b"resolved-parent"
    );
    assert_eq!(
        table.realpath("/outside/link/../sibling").unwrap(),
        "/mounted/sibling"
    );
    assert_eq!(
        table.lstat("/outside/link/../sibling").unwrap().size,
        b"resolved-parent".len() as u64
    );
    table
        .write_file("/outside/link/../created", b"target-mount".to_vec())
        .unwrap();
    assert_eq!(
        table.read_file("/mounted/created").unwrap(),
        b"target-mount"
    );
    assert!(!table.exists("/outside/created"));
}

#[test]
fn trailing_slash_requires_directory_across_mounts() {
    let mut table = MountTable::new(MemoryFileSystem::new());
    table.create_dir("/mounted").unwrap();
    table
        .mount(
            "/mounted",
            MemoryFileSystem::new(),
            MountOptions::new("memory"),
        )
        .unwrap();
    table.write_file("/mounted/file", b"data".to_vec()).unwrap();

    assert_eq!(table.stat("/mounted/file/").unwrap_err().code(), "ENOTDIR");
    assert_eq!(
        table.read_file("/mounted/file/").unwrap_err().code(),
        "ENOTDIR"
    );
    assert_eq!(table.lstat("/mounted/file/").unwrap_err().code(), "ENOTDIR");
}

#[test]
fn resolved_parent_mount_policy_applies_to_writes() {
    let mut table = MountTable::new(MemoryFileSystem::new());
    table.create_dir("/outside").unwrap();
    table.create_dir("/mounted").unwrap();
    let mut readonly = MemoryFileSystem::new();
    readonly.create_dir("/inner").unwrap();
    table
        .mount(
            "/mounted",
            readonly,
            MountOptions::new("memory").read_only(true),
        )
        .unwrap();
    table.symlink("/mounted/inner", "/outside/link").unwrap();

    assert_eq!(
        table
            .write_file("/outside/link/../created", b"blocked".to_vec())
            .unwrap_err()
            .code(),
        "EROFS"
    );
    assert!(!table.exists("/outside/created"));
}

struct ShutdownTrackingFileSystem {
    deny_metadata: bool,
    switchable_metadata: Option<(MemoryFileSystem, Arc<AtomicBool>)>,
    shutdown: Arc<AtomicBool>,
}

impl ShutdownTrackingFileSystem {
    fn new(shutdown: Arc<AtomicBool>) -> Self {
        Self {
            shutdown,
            deny_metadata: false,
            switchable_metadata: None,
        }
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
        if let Some((filesystem, denied)) = &mut self.switchable_metadata {
            if denied.load(Ordering::SeqCst) {
                return Err(agentos_vfs_core::posix::VfsError::new(
                    "EACCES",
                    "fixture backend metadata denied",
                ));
            }
            return filesystem.stat(path);
        }
        if self.deny_metadata {
            return Err(agentos_vfs_core::posix::VfsError::new(
                "EACCES",
                "fixture backend metadata denied",
            ));
        }
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
        if let Some((filesystem, denied)) = &self.switchable_metadata {
            if denied.load(Ordering::SeqCst) {
                return Err(agentos_vfs_core::posix::VfsError::new(
                    "EACCES",
                    "fixture backend metadata denied",
                ));
            }
            return filesystem.realpath(path);
        }
        if self.deny_metadata {
            return Err(agentos_vfs_core::posix::VfsError::new(
                "EACCES",
                "fixture backend metadata denied",
            ));
        }
        unreachable!("failed mount should not realpath {path}")
    }

    fn symlink(&mut self, target: &str, link_path: &str) -> VfsResult<()> {
        unreachable!("failed mount should not symlink {target} to {link_path}")
    }

    fn read_link(&self, path: &str) -> VfsResult<String> {
        unreachable!("failed mount should not readlink {path}")
    }

    fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        if let Some((filesystem, denied)) = &self.switchable_metadata {
            if denied.load(Ordering::SeqCst) {
                return Err(agentos_vfs_core::posix::VfsError::new(
                    "EACCES",
                    "fixture backend metadata denied",
                ));
            }
            return filesystem.lstat(path);
        }
        if self.deny_metadata {
            return Err(agentos_vfs_core::posix::VfsError::new(
                "EACCES",
                "fixture backend metadata denied",
            ));
        }
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

#[test]
fn rejected_root_mount_admission_shuts_down_backend_without_parent_mutation() {
    let mut table = MountTable::new(MemoryFileSystem::new());
    let shutdown = Arc::new(AtomicBool::new(false));
    let generation = table.mutation_generation();
    let mounts = table.get_mounts();
    let usage = table.root_usage().unwrap();
    let error = table
        .mount_boxed_with_root_admission(
            "/missing/leaf",
            Box::new(ShutdownTrackingFileSystem::new(shutdown.clone())),
            MountOptions::new("rejected"),
            |added| {
                assert_eq!(added, 2);
                Err(agentos_vfs_core::posix::VfsError::new(
                    "ENOSPC",
                    "root admission rejected",
                ))
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(shutdown.load(Ordering::SeqCst));
    assert!(!table.exists("/missing"));
    assert_eq!(table.root_usage().unwrap(), usage);
    assert_eq!(table.get_mounts(), mounts);
    assert_eq!(table.mutation_generation(), generation);
}

#[test]
fn mounted_parent_quota_rejection_shuts_down_child_backend() {
    let mut table = MountTable::new(MemoryFileSystem::new());
    table
        .mount(
            "/parent",
            MemoryFileSystem::new(),
            MountOptions::new("parent").max_inodes(Some(1)),
        )
        .unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let error = table
        .mount_boxed_with_root_admission(
            "/parent/child",
            Box::new(ShutdownTrackingFileSystem::new(shutdown.clone())),
            MountOptions::new("rejected"),
            |_| panic!("mounted payload must not consume root inode allowance"),
        )
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(shutdown.load(Ordering::SeqCst));
    assert!(!table.exists("/parent/child"));
}

#[test]
fn raw_root_handle_invalidates_mounted_quota_census() {
    let root = MemoryFileSystem::new();
    let mut table = MountTable::new_boxed_root(
        Box::new(agentos_vfs_core::posix::MountedVirtualFileSystem::new(root)),
        MountOptions::new("root").max_inodes(Some(2)),
    );
    // Prime the internal root backend's quota census without adding an inode.
    table.path_stats("/", None, None).unwrap();
    table
        .root_virtual_filesystem_mut::<MemoryFileSystem>()
        .unwrap()
        .mkdir("/trusted", false)
        .unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let error = table
        .mount_boxed_with_root_admission(
            "/missing",
            Box::new(ShutdownTrackingFileSystem::new(shutdown.clone())),
            MountOptions::new("child"),
            |_| Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(shutdown.load(Ordering::SeqCst));
    assert!(!table.exists("/missing"));
}

#[test]
fn sealed_readonly_parent_ignores_cosmetic_directory_quota() {
    use agentos_vfs_core::posix::{
        MountedVirtualFileSystem, RootFileSystem, RootFilesystemDescriptor, RootFilesystemMode,
    };
    let mut parent = RootFileSystem::from_descriptor(RootFilesystemDescriptor {
        mode: RootFilesystemMode::ReadOnly,
        disable_default_base_layer: true,
        lowers: Vec::new(),
        bootstrap_entries: Vec::new(),
    })
    .unwrap();
    parent.finish_bootstrap();
    let mut table = MountTable::new(MemoryFileSystem::new());
    table
        .mount_boxed(
            "/sealed",
            Box::new(MountedVirtualFileSystem::new(parent)),
            MountOptions::new("sealed").max_inodes(Some(1)),
        )
        .unwrap();
    let growth = table
        .mount_boxed_with_root_admission(
            "/sealed/missing/leaf",
            Box::new(MountedVirtualFileSystem::new(MemoryFileSystem::new())),
            MountOptions::new("child"),
            |_| panic!("non-root sealed parent must not charge root growth"),
        )
        .unwrap();
    assert_eq!(growth, Some(0));
}

#[test]
fn readonly_mode_before_seal_still_admits_writable_bootstrap_growth() {
    use agentos_vfs_core::posix::{RootFileSystem, RootFilesystemDescriptor, RootFilesystemMode};
    let root = RootFileSystem::from_descriptor(RootFilesystemDescriptor {
        mode: RootFilesystemMode::ReadOnly,
        disable_default_base_layer: true,
        lowers: Vec::new(),
        bootstrap_entries: Vec::new(),
    })
    .unwrap();
    let mut table = MountTable::new(root);
    let shutdown = Arc::new(AtomicBool::new(false));
    let error = table
        .mount_boxed_with_root_admission(
            "/missing/leaf",
            Box::new(ShutdownTrackingFileSystem::new(shutdown.clone())),
            MountOptions::new("child"),
            |added| {
                assert_eq!(added, 2);
                Err(agentos_vfs_core::posix::VfsError::new(
                    "ENOSPC",
                    "bootstrap growth denied",
                ))
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(shutdown.load(Ordering::SeqCst));
    assert!(!table.exists("/missing"));
}

fn dangling_parent_mount_with_quota(headroom: usize) -> MountTable {
    use agentos_vfs_core::posix::{
        measure_filesystem_usage, RootFileSystem, RootFilesystemDescriptor, RootFilesystemMode,
    };
    let mut root = MemoryFileSystem::new();
    // These global paths exist, but the selected backend's absolute target does not.
    root.mkdir("/missing/a/b", true).unwrap();
    let mut table = MountTable::new(root);
    let mut parent = RootFileSystem::from_descriptor(RootFilesystemDescriptor {
        mode: RootFilesystemMode::Ephemeral,
        disable_default_base_layer: true,
        lowers: Vec::new(),
        bootstrap_entries: Vec::new(),
    })
    .unwrap();
    parent.symlink("/missing/a/b", "/alias").unwrap();
    let usage = measure_filesystem_usage(&mut parent).unwrap();
    table
        .mount(
            "/parent",
            parent,
            MountOptions::new("parent").max_inodes(Some(usage.inode_count + headroom)),
        )
        .unwrap();
    table
}

#[test]
fn dangling_symlink_mount_uses_selected_parent_quota_before_mutation() {
    let mut table = dangling_parent_mount_with_quota(2);
    let root_usage = table.root_usage().unwrap();
    let mounts = table.get_mounts();
    let generation = table.mutation_generation();
    let shutdown = Arc::new(AtomicBool::new(false));
    let error = table
        .mount_boxed_with_root_admission(
            "/parent/alias/leaf",
            Box::new(ShutdownTrackingFileSystem::new(shutdown.clone())),
            MountOptions::new("child"),
            |_| panic!("selected mounted backend must not consume root headroom"),
        )
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(shutdown.load(Ordering::SeqCst));
    assert_eq!(table.get_mounts(), mounts);
    assert_eq!(table.mutation_generation(), generation);
    assert!(!table.exists("/parent/missing"));
    assert_eq!(table.root_usage().unwrap(), root_usage);
}

#[test]
fn dangling_symlink_mount_exact_parent_quota_retains_full_charge() {
    let mut table = dangling_parent_mount_with_quota(4);
    let root_usage = table.root_usage().unwrap();
    let growth = table
        .mount_boxed_with_root_admission(
            "/parent/alias/leaf",
            Box::new(agentos_vfs_core::posix::MountedVirtualFileSystem::new(
                MemoryFileSystem::new(),
            )),
            MountOptions::new("child"),
            |_| panic!("selected mounted backend must not consume root headroom"),
        )
        .unwrap();
    assert_eq!(growth, Some(0));
    assert!(table
        .get_mounts()
        .iter()
        .any(|mount| mount.path == "/parent/alias/leaf"));
    assert!(table.exists("/parent/missing/a/b/leaf"));
    assert!(!table.exists("/missing/a/b/leaf"));
    // Do not refresh path_stats first: a second admission must use the cached charge.
    let shutdown = Arc::new(AtomicBool::new(false));
    let error = table
        .mount_boxed_with_root_admission(
            "/parent/extra",
            Box::new(ShutdownTrackingFileSystem::new(shutdown.clone())),
            MountOptions::new("extra"),
            |_| panic!("selected mounted backend must not consume root headroom"),
        )
        .unwrap_err();
    assert_eq!(error.code(), "ENOSPC");
    assert!(shutdown.load(Ordering::SeqCst));
    assert!(!table.exists("/parent/extra"));
    assert_eq!(
        table.path_stats("/parent", None, None).unwrap().free_inodes,
        0
    );
    assert_eq!(table.root_usage().unwrap(), root_usage);
}

fn namespace_readonly_table(mut root: MemoryFileSystem) -> MountTable {
    root.mkdir("/bin", true).unwrap();
    root.symlink("/custom/deep/entry", "/bin/namespace-command")
        .unwrap();
    MountTable::new_boxed_root(
        Box::new(agentos_vfs_core::posix::MountedVirtualFileSystem::new(root)),
        MountOptions::new("readonly-root").read_only(true),
    )
}

#[test]
fn readonly_missing_mount_ancestors_are_live_directories_with_stable_identity() {
    use agentos_vfs_core::posix::SingleSymlinkFileSystem;
    let mut table = namespace_readonly_table(MemoryFileSystem::new());
    let root_usage = table.path_stats("/", None, None).unwrap();
    let mut payload = MemoryFileSystem::new();
    payload
        .write_file("/command", b"namespace-command".to_vec())
        .unwrap();
    table
        .mount(
            "/custom/deep/payload",
            payload,
            MountOptions::new("payload").read_only(true),
        )
        .unwrap();
    table
        .mount(
            "/custom/deep/entry",
            SingleSymlinkFileSystem::new("payload/command"),
            MountOptions::new("entry").read_only(true),
        )
        .unwrap();
    let before = table.lstat("/custom").unwrap();
    let nested = table.stat("/custom/deep").unwrap();
    assert_eq!(
        (before.uid, before.gid, before.mode & 0o7777),
        (0, 0, 0o555)
    );
    assert_eq!(before.dev, nested.dev);
    assert_ne!(before.ino, nested.ino);
    assert_ne!((before.dev, before.ino), {
        let root = table.lstat("/").unwrap();
        (root.dev, root.ino)
    });
    assert!(table.exists("/custom/deep"));
    assert_eq!(
        table.read_file("/custom/deep").unwrap_err().code(),
        "EISDIR"
    );
    assert_eq!(
        table.pread("/custom/deep", 0, 1).unwrap_err().code(),
        "EISDIR"
    );
    assert_eq!(table.realpath("/custom/deep").unwrap(), "/custom/deep");
    assert_eq!(
        table.realpath("/bin/namespace-command").unwrap(),
        "/custom/deep/payload/command"
    );
    assert_eq!(
        table.read_file("/bin/namespace-command").unwrap(),
        b"namespace-command"
    );
    assert_eq!(table.read_dir("/custom").unwrap(), ["deep"]);
    assert_eq!(
        table.read_dir("/custom/deep").unwrap(),
        ["entry", "payload"]
    );
    assert_eq!(
        table
            .read_dir_limited("/custom/deep", 1)
            .unwrap_err()
            .code(),
        "ENOMEM"
    );
    assert_eq!(
        table.read_dir_limited("/custom/deep", 2).unwrap(),
        ["entry", "payload"]
    );
    let typed = table.read_dir_with_types("/custom/deep").unwrap();
    assert!(typed
        .iter()
        .any(|e| e.name == "entry" && e.is_symbolic_link && !e.is_directory));
    assert!(typed
        .iter()
        .any(|e| e.name == "payload" && !e.is_symbolic_link && e.is_directory));
    assert!(table.read_dir("/").unwrap().contains(&"custom".into()));
    assert_eq!(table.path_stats("/", None, None).unwrap(), root_usage);
    assert_eq!(
        table.write_file("/custom/new", vec![1]).unwrap_err().code(),
        "EROFS"
    );
    table
        .mount(
            "/unrelated/longer/tree/leaf",
            MemoryFileSystem::new(),
            MountOptions::new("other"),
        )
        .unwrap();
    assert_eq!(table.lstat("/custom").unwrap(), before);
    let entry = table.detach("/custom/deep/entry").unwrap();
    assert_eq!(table.lstat("/custom").unwrap(), before);
    let payload = table.detach("/custom/deep/payload").unwrap();
    assert!(!table.exists("/custom"));
    assert_eq!(table.lstat("/custom").unwrap_err().code(), "ENOENT");
    assert!(!table.read_dir("/").unwrap().contains(&"custom".into()));
    table.restore_detached(payload).unwrap();
    table.restore_detached(entry).unwrap();
    assert_eq!(table.lstat("/custom").unwrap(), before);
    assert_eq!(
        table.read_file("/bin/namespace-command").unwrap(),
        b"namespace-command"
    );
    table.unmount("/custom/deep/entry").unwrap();
    table.unmount("/custom/deep/payload").unwrap();
    assert!(!table.exists("/custom"));
}

#[test]
fn missing_mount_ancestors_do_not_shadow_existing_files_or_dangling_symlinks() {
    let mut root = MemoryFileSystem::new();
    root.write_file("/obstacle", b"custom-file".to_vec())
        .unwrap();
    root.symlink("/absent-target", "/alias").unwrap();
    let mut table = namespace_readonly_table(root);
    assert_eq!(
        table
            .mount(
                "/obstacle/deep/payload",
                MemoryFileSystem::new(),
                MountOptions::new("payload").read_only(true)
            )
            .unwrap_err()
            .code(),
        "ENOTDIR"
    );
    let mut payload = MemoryFileSystem::new();
    payload.write_file("/command", vec![1]).unwrap();
    table
        .mount(
            "/alias/deep/payload",
            payload,
            MountOptions::new("payload").read_only(true),
        )
        .unwrap();
    assert_eq!(table.read_file("/obstacle").unwrap(), b"custom-file");
    assert!(!table.lstat("/obstacle").unwrap().is_directory);
    assert_eq!(
        table
            .read_file("/obstacle/deep/payload/command")
            .unwrap_err()
            .code(),
        "ENOTDIR"
    );
    assert_eq!(
        table.lstat("/obstacle/deep/payload").unwrap_err().code(),
        "ENOTDIR"
    );
    assert!(table.lstat("/alias").unwrap().is_symbolic_link);
    assert_eq!(table.read_link("/alias").unwrap(), "/absent-target");
    assert_eq!(
        table
            .read_file("/alias/deep/payload/command")
            .unwrap_err()
            .code(),
        "ENOENT"
    );
    assert_eq!(table.stat("/alias/deep").unwrap_err().code(), "ENOENT");
}

#[test]
fn missing_mount_ancestor_projection_preserves_backend_permission_errors() {
    let mut backend = ShutdownTrackingFileSystem::new(Arc::new(AtomicBool::new(false)));
    backend.deny_metadata = true;
    let mut table = MountTable::new_boxed_root(
        Box::new(backend),
        MountOptions::new("denied-root").read_only(true),
    );
    assert_eq!(
        table
            .mount(
                "/denied/deep/leaf",
                MemoryFileSystem::new(),
                MountOptions::new("leaf")
            )
            .unwrap_err()
            .code(),
        "EACCES"
    );
    for result in [
        table.realpath("/denied/deep/leaf"),
        table
            .read_file("/denied/deep/leaf/missing")
            .map(|_| String::new()),
        table.read_dir("/denied").map(|_| String::new()),
    ] {
        assert_eq!(result.unwrap_err().code(), "EACCES");
    }
    assert_eq!(table.lstat("/denied").unwrap_err().code(), "EACCES");
}

#[test]
fn detached_writable_mount_restores_into_fresh_readonly_namespace() {
    let mut source = MountTable::new(MemoryFileSystem::new());
    let mut payload = MemoryFileSystem::new();
    payload
        .write_file("/command", b"restored".to_vec())
        .unwrap();
    source
        .mount(
            "/transferred/deep/payload",
            payload,
            MountOptions::new("payload"),
        )
        .unwrap();
    let detached = source.detach("/transferred/deep/payload").unwrap();
    let mut target = namespace_readonly_table(MemoryFileSystem::new());
    target.restore_detached(detached).unwrap();
    assert!(target.stat("/transferred/deep").unwrap().is_directory);
    assert_eq!(
        target
            .read_file("/transferred/deep/payload/command")
            .unwrap(),
        b"restored"
    );
    assert_eq!(target.read_dir("/transferred").unwrap(), ["deep"]);
    target.unmount("/transferred/deep/payload").unwrap();
    assert!(!target.exists("/transferred"));
}

#[test]
fn cached_virtual_walk_preserves_later_backend_error_over_leaf_enoent() {
    let denied = Arc::new(AtomicBool::new(false));
    let mut backend = ShutdownTrackingFileSystem::new(Arc::new(AtomicBool::new(false)));
    backend.switchable_metadata = Some((MemoryFileSystem::new(), denied.clone()));
    let mut table =
        MountTable::new_boxed_root(Box::new(backend), MountOptions::new("root").read_only(true));
    table
        .mount(
            "/guard/deep/leaf",
            MemoryFileSystem::new(),
            MountOptions::new("leaf"),
        )
        .unwrap();
    denied.store(true, Ordering::SeqCst);
    assert_eq!(
        table
            .read_file("/guard/deep/leaf/missing")
            .unwrap_err()
            .code(),
        "EACCES"
    );
    assert_eq!(
        table
            .realpath("/guard/deep/leaf/missing")
            .unwrap_err()
            .code(),
        "EACCES"
    );
}

#[test]
fn virtual_namespace_xattrs_report_absence_and_reject_mutation() {
    let mut root = MemoryFileSystem::new();
    root.mkdir("/real", true).unwrap();
    root.set_xattr("/real", "user.retained", b"retained".to_vec(), 0, true)
        .unwrap();
    root.mkdir("/bin", true).unwrap();
    root.symlink("/custom/deep", "/bin/namespace-directory")
        .unwrap();
    let mut table = namespace_readonly_table(root);
    table
        .mount(
            "/custom/deep/payload",
            MemoryFileSystem::new(),
            MountOptions::new("payload"),
        )
        .unwrap();
    let before = table.lstat("/custom/deep").unwrap();
    let generation = table.mutation_generation();
    for follow in [false, true] {
        for name in [
            "system.posix_acl_access",
            "system.posix_acl_default",
            "user.absent",
        ] {
            assert_eq!(
                table
                    .get_xattr("/custom/deep", name, follow)
                    .unwrap_err()
                    .code(),
                "ENODATA"
            );
        }
        assert!(table
            .list_xattrs("/custom/deep", follow)
            .unwrap()
            .is_empty());
        assert_eq!(
            table
                .get_xattr("/custom/deep", "invalid", follow)
                .unwrap_err()
                .code(),
            "EINVAL"
        );
        assert_eq!(
            table
                .set_xattr("/custom/deep", "user.new", vec![1], 0, follow)
                .unwrap_err()
                .code(),
            "EROFS"
        );
        assert_eq!(
            table
                .remove_xattr("/custom/deep", "user.absent", follow)
                .unwrap_err()
                .code(),
            "EROFS"
        );
    }
    assert_eq!(
        table
            .get_xattr("/bin/namespace-directory", "system.posix_acl_access", true)
            .unwrap_err()
            .code(),
        "ENODATA"
    );
    assert!(table
        .list_xattrs("/bin/namespace-directory", true)
        .unwrap()
        .is_empty());
    assert_eq!(
        table.get_xattr("/real", "user.retained", true).unwrap(),
        b"retained"
    );
    assert_eq!(table.list_xattrs("/real", true).unwrap(), ["user.retained"]);
    assert_eq!(
        table
            .get_xattr("/not-present", "user.absent", true)
            .unwrap_err()
            .code(),
        "ENOENT"
    );
    assert_eq!(table.lstat("/custom/deep").unwrap(), before);
    assert_eq!(
        table.mutation_generation(),
        generation,
        "absent/read-only xattrs do not mutate namespace"
    );
}

#[test]
fn virtual_namespace_xattrs_preserve_backend_errors() {
    let denied = Arc::new(AtomicBool::new(false));
    let mut backend = ShutdownTrackingFileSystem::new(Arc::new(AtomicBool::new(false)));
    backend.switchable_metadata = Some((MemoryFileSystem::new(), denied.clone()));
    let mut table =
        MountTable::new_boxed_root(Box::new(backend), MountOptions::new("root").read_only(true));
    table
        .mount(
            "/guard/deep/leaf",
            MemoryFileSystem::new(),
            MountOptions::new("leaf"),
        )
        .unwrap();
    denied.store(true, Ordering::SeqCst);
    for follow in [false, true] {
        assert_eq!(
            table
                .get_xattr("/guard/deep", "system.posix_acl_access", follow)
                .unwrap_err()
                .code(),
            "EACCES"
        );
        assert_eq!(
            table.list_xattrs("/guard/deep", follow).unwrap_err().code(),
            "EACCES"
        );
        assert_eq!(
            table
                .set_xattr("/guard/deep", "user.new", vec![1], 0, follow)
                .unwrap_err()
                .code(),
            "EACCES"
        );
        assert_eq!(
            table
                .remove_xattr("/guard/deep", "user.absent", follow)
                .unwrap_err()
                .code(),
            "EACCES"
        );
    }
}
