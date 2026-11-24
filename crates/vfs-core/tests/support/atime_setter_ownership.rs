use super::{RootFileSystem, RootFilesystemMode};
use crate::posix::{
    MemoryFileSystem, MountTable, MountedFileSystem, MountedVirtualFileSystem, OverlayFileSystem,
    OverlayMode, VirtualFileSystem,
};

#[derive(Clone, Copy, Debug)]
enum Backend {
    Memory,
    OverlayUpper,
    OverlayLower,
    RootUpper,
    RootLower,
}
const BACKENDS: [Backend; 5] = [
    Backend::Memory,
    Backend::OverlayUpper,
    Backend::OverlayLower,
    Backend::RootUpper,
    Backend::RootLower,
];
const READS: [&str; 5] = [
    "read_file",
    "pread",
    "read_dir",
    "read_dir_limited",
    "read_dir_with_types",
];
const OLD_ATIME: u64 = 1_000;
const PRECISE_MTIME: (u64, u32) = (2_123, 123_456_789);
const OLD_CTIME: (u64, u32) = (3_222, 222_333_444);

fn memory(extra_lower_child: bool) -> MemoryFileSystem {
    let mut filesystem = MemoryFileSystem::new();
    filesystem.mkdir("/data", true).unwrap();
    filesystem
        .write_file("/data/file", b"content".to_vec())
        .unwrap();
    if extra_lower_child {
        filesystem
            .write_file("/data/lower-only", b"lower".to_vec())
            .unwrap();
    }
    // Use the real snapshot restoration API to set old ctime deterministically;
    // utimes would set ctime to the current clock and recreate the same-ms hole.
    let mut snapshot = filesystem.snapshot();
    for inode in snapshot.inodes.values_mut() {
        inode.metadata.atime_ms = OLD_ATIME;
        inode.metadata.atime_nsec = 0;
        inode.metadata.mtime_ms = PRECISE_MTIME.0;
        inode.metadata.mtime_nsec = PRECISE_MTIME.1;
        inode.metadata.ctime_ms = OLD_CTIME.0;
        inode.metadata.ctime_nsec = OLD_CTIME.1;
    }
    MemoryFileSystem::from_snapshot(snapshot)
}

fn overlay(upper: bool) -> OverlayFileSystem {
    if upper {
        OverlayFileSystem::with_upper(vec![memory(true)], memory(false))
    } else {
        OverlayFileSystem::new(vec![memory(false), memory(true)], OverlayMode::Ephemeral)
    }
}

fn root(upper: bool) -> RootFileSystem {
    RootFileSystem {
        overlay: overlay(upper),
        mode: RootFilesystemMode::Ephemeral,
        bootstrap_finished: true,
    }
}

fn table(backend: Backend, policy: &str) -> MountTable {
    let mut table = match backend {
        Backend::Memory => MountTable::new(memory(true)),
        Backend::OverlayUpper => MountTable::new(overlay(true)),
        Backend::OverlayLower => MountTable::new(overlay(false)),
        Backend::RootUpper => MountTable::new(root(true)),
        Backend::RootLower => MountTable::new(root(false)),
    };
    table.remount("/", policy).unwrap();
    // Seed AFTER the actual construction/remount census. Counts and content
    // remain the same; the owning mutable-root API invalidates derived caches.
    match backend {
        Backend::Memory => {
            *table
                .root_virtual_filesystem_mut::<MemoryFileSystem>()
                .unwrap() = memory(true)
        }
        Backend::OverlayUpper => {
            *table
                .root_virtual_filesystem_mut::<OverlayFileSystem>()
                .unwrap() = overlay(true)
        }
        Backend::OverlayLower => {
            *table
                .root_virtual_filesystem_mut::<OverlayFileSystem>()
                .unwrap() = overlay(false)
        }
        Backend::RootUpper => {
            *table
                .root_virtual_filesystem_mut::<RootFileSystem>()
                .unwrap() = root(true)
        }
        Backend::RootLower => {
            *table
                .root_virtual_filesystem_mut::<RootFileSystem>()
                .unwrap() = root(false)
        }
    }
    table
}

fn read(table: &mut MountTable, operation: &str) {
    match operation {
        "read_file" => assert_eq!(table.read_file("/data/file").unwrap(), b"content"),
        "pread" => assert_eq!(table.pread("/data/file", 1, 3).unwrap(), b"ont"),
        "read_dir" => assert_eq!(table.read_dir("/data").unwrap(), ["file", "lower-only"]),
        "read_dir_limited" => assert_eq!(
            table.read_dir_limited("/data", 2).unwrap(),
            ["file", "lower-only"]
        ),
        "read_dir_with_types" => {
            let mut names = table
                .read_dir_with_types("/data")
                .unwrap()
                .into_iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>();
            names.sort();
            assert_eq!(names, ["file", "lower-only"]);
        }
        _ => unreachable!(),
    }
}

#[test]
fn mounted_atime_setter_preserves_precise_metadata_and_merged_contents() {
    use std::time::{SystemTime, UNIX_EPOCH};
    assert!(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            > u128::from(OLD_CTIME.0 + 1_000)
    );
    let mut deviations = Vec::new();
    let mut checked = 0;
    for backend in BACKENDS {
        for policy in ["remount,strictatime", "remount,relatime"] {
            for operation in READS {
                let mut table = table(backend, policy);
                let directory = !matches!(operation, "read_file" | "pread");
                let path = if directory { "/data" } else { "/data/file" };
                let before = table.stat(path).unwrap();
                assert_eq!(
                    (before.atime_ms, before.atime_nsec),
                    (OLD_ATIME, 0),
                    "qualified atime {backend:?}/{policy}/{operation}"
                );
                assert_eq!(
                    (before.mtime_ms, before.mtime_nsec),
                    PRECISE_MTIME,
                    "qualified mtime {backend:?}/{policy}/{operation}"
                );
                assert_eq!(
                    (before.ctime_ms, before.ctime_nsec),
                    OLD_CTIME,
                    "qualified ctime {backend:?}/{policy}/{operation}"
                );
                read(&mut table, operation);
                let after = table.stat(path).unwrap();
                if after.atime_ms == OLD_ATIME {
                    deviations.push(format!(
                        "{backend:?}/{policy}/{operation}: atime did not advance"
                    ));
                }
                let mut non_access_metadata = after.clone();
                non_access_metadata.atime_ms = before.atime_ms;
                non_access_metadata.atime_nsec = before.atime_nsec;
                if non_access_metadata != before {
                    deviations.push(format!("{backend:?}/{policy}/{operation}: non-access metadata changed: before={before:?}; after={after:?}"));
                }
                // A metadata-only access must not turn a merged directory opaque.
                if directory {
                    let visible = table.read_dir("/data").unwrap();
                    if visible != ["file", "lower-only"] {
                        deviations.push(format!("{backend:?}/{policy}/{operation}: lower children disappeared: {visible:?}"));
                    }
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 50);
    assert!(
        deviations.is_empty(),
        "mounted access-time setter changed unrelated state: {deviations:?}"
    );
}

#[test]
fn mounted_atime_setter_keeps_lookup_and_sealed_root_errors() {
    let mut missing = MountedVirtualFileSystem::new(memory(true));
    assert_eq!(
        missing.set_atime("/missing", 9_000).unwrap_err().code(),
        "ENOENT"
    );
    let mut sealed = RootFileSystem {
        overlay: overlay(false),
        mode: RootFilesystemMode::ReadOnly,
        bootstrap_finished: false,
    };
    sealed.finish_bootstrap();
    let mut sealed = MountedVirtualFileSystem::new(sealed);
    assert_eq!(
        sealed.set_atime("/data/file", 9_000).unwrap_err().code(),
        "EROFS"
    );
    assert_eq!(
        sealed.set_atime("/missing", 9_000).unwrap_err().code(),
        "ENOENT"
    );
    let mut standalone = memory(true);
    standalone.utimes("/data/file", 9_000, 8_000).unwrap();
    let changed = standalone.stat("/data/file").unwrap();
    assert_eq!((changed.mtime_ms, changed.mtime_nsec), (8_000, 0));
    assert_ne!(
        (changed.ctime_ms, changed.ctime_nsec),
        OLD_CTIME,
        "explicit utimes retains its ordinary change-time semantics"
    );
}
