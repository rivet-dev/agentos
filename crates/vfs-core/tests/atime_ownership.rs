// All fixture timestamps are set after construction/remount, then explicitly verified.
use agentos_vfs_core::posix::{
    FilesystemEntry, MemoryFileSystem, MountTable, OverlayFileSystem, OverlayMode, RootFileSystem,
    RootFilesystemDescriptor, RootFilesystemMode, RootFilesystemSnapshot, VirtualFileSystem,
};
const AO_ATIME: u64 = 1_000;
const AO_MTIME: u64 = 2_000;
const AO_BACKENDS: [&str; 3] = ["memory", "overlay", "root"];
const AO_READS: [&str; 5] = [
    "read_file",
    "pread",
    "read_dir",
    "read_dir_limited",
    "read_dir_with_types",
];
fn ao_memory() -> MemoryFileSystem {
    let mut memory = MemoryFileSystem::new();
    memory.mkdir("/data", true).unwrap();
    memory
        .write_file("/data/file", b"content".to_vec())
        .unwrap();
    memory
}
fn ao_root(readonly: bool) -> RootFileSystem {
    RootFileSystem::from_descriptor(RootFilesystemDescriptor {
        mode: if readonly {
            RootFilesystemMode::ReadOnly
        } else {
            RootFilesystemMode::Ephemeral
        },
        disable_default_base_layer: true,
        lowers: vec![RootFilesystemSnapshot {
            entries: vec![
                FilesystemEntry::directory("/data"),
                FilesystemEntry::file("/data/file", b"content".to_vec()),
            ],
        }],
        bootstrap_entries: vec![],
    })
    .unwrap()
}
fn ao_table(backend: &str, readonly_root: bool) -> MountTable {
    match backend {
        "memory" => MountTable::new(ao_memory()),
        "overlay" => MountTable::new(OverlayFileSystem::new(
            vec![ao_memory()],
            OverlayMode::Ephemeral,
        )),
        "root" => MountTable::new(ao_root(readonly_root)),
        _ => unreachable!(),
    }
}
fn ao_seed<F: VirtualFileSystem>(filesystem: &mut F) {
    filesystem.utimes("/data/file", AO_ATIME, AO_MTIME).unwrap();
    filesystem.utimes("/data", AO_ATIME, AO_MTIME).unwrap();
}
fn ao_seed_owned_backend(table: &mut MountTable, backend: &str, seal_root: bool) {
    // Fixture owner deliberately bypasses mount policy ONLY during timestamp setup.
    // A sealed readonly RootFileSystem is seeded before finish_bootstrap, never unlocked.
    match backend {
        "memory" => ao_seed(
            table
                .root_virtual_filesystem_mut::<MemoryFileSystem>()
                .unwrap(),
        ),
        "overlay" => ao_seed(
            table
                .root_virtual_filesystem_mut::<OverlayFileSystem>()
                .unwrap(),
        ),
        "root" => {
            let root = table
                .root_virtual_filesystem_mut::<RootFileSystem>()
                .unwrap();
            ao_seed(root);
            if seal_root {
                root.finish_bootstrap();
            }
        }
        _ => unreachable!(),
    }
}
fn ao_path(operation: &str) -> &'static str {
    if matches!(operation, "read_file" | "pread") {
        "/data/file"
    } else {
        "/data"
    }
}
fn ao_read<F: VirtualFileSystem>(filesystem: &mut F, operation: &str) {
    let path = ao_path(operation);
    match operation {
        "read_file" => assert_eq!(filesystem.read_file(path).unwrap(), b"content"),
        "pread" => assert_eq!(filesystem.pread(path, 1, 3).unwrap(), b"ont"),
        "read_dir" => assert_eq!(
            filesystem.read_dir(path).unwrap(),
            vec![String::from("file")]
        ),
        "read_dir_limited" => assert_eq!(
            filesystem.read_dir_limited(path, 1).unwrap(),
            vec![String::from("file")]
        ),
        "read_dir_with_types" => {
            let entries = filesystem.read_dir_with_types(path).unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].name, "file");
        }
        _ => unreachable!(),
    }
}
#[test]
fn atime_ownership_mounted_memory_overlay_and_root_preserve_suppression() {
    let mut deviations = Vec::new();
    let mut checked = 0;
    for backend in AO_BACKENDS {
        for policy in [
            "remount,ro,strictatime",
            "remount,noatime",
            "remount,relatime,nodiratime",
        ] {
            for operation in AO_READS {
                if policy.ends_with("nodiratime") && matches!(operation, "read_file" | "pread") {
                    continue;
                }
                let readonly_root = backend == "root" && policy.contains(",ro,");
                let mut table = ao_table(backend, readonly_root);
                table.remount("/", policy).unwrap();
                ao_seed_owned_backend(&mut table, backend, true);
                let path = ao_path(operation);
                let before = table.stat(path).unwrap();
                assert_eq!(
                    (before.atime_ms, before.atime_nsec),
                    (AO_ATIME, 0),
                    "fixture precondition {backend}/{policy}/{operation}"
                );
                ao_read(&mut table, operation);
                let after = table.stat(path).unwrap();
                if (after.atime_ms, after.atime_nsec) != (before.atime_ms, before.atime_nsec) {
                    deviations.push(format!(
                        "{backend}/{policy}/{operation}: {:?} -> {:?}",
                        (before.atime_ms, before.atime_nsec),
                        (after.atime_ms, after.atime_nsec)
                    ));
                }
                assert_eq!(
                    (after.mtime_ms, after.mtime_nsec),
                    (before.mtime_ms, before.mtime_nsec)
                );
                assert_eq!(
                    (after.ctime_ms, after.ctime_nsec),
                    (before.ctime_ms, before.ctime_nsec)
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 39);
    assert!(
        deviations.is_empty(),
        "mounted access bypassed atime ownership: {deviations:?}"
    );
}
fn ao_standalone<F: VirtualFileSystem>(mut filesystem: F, operation: &str, backend: &str) {
    ao_seed(&mut filesystem);
    let before = filesystem.stat(ao_path(operation)).unwrap();
    assert_eq!(
        (before.atime_ms, before.atime_nsec),
        (AO_ATIME, 0),
        "standalone fixture precondition"
    );
    ao_read(&mut filesystem, operation);
    let after = filesystem.stat(ao_path(operation)).unwrap();
    assert_ne!(
        after.atime_ms, AO_ATIME,
        "standalone {backend}/{operation} keeps automatic atime"
    );
    assert_eq!(
        (after.mtime_ms, after.mtime_nsec),
        (before.mtime_ms, before.mtime_nsec)
    );
    assert_eq!(
        (after.ctime_ms, after.ctime_nsec),
        (before.ctime_ms, before.ctime_nsec)
    );
}
#[test]
fn atime_ownership_standalone_memory_overlay_and_root_still_touch_access_times() {
    for operation in AO_READS {
        ao_standalone(ao_memory(), operation, "memory");
        ao_standalone(
            OverlayFileSystem::new(vec![ao_memory()], OverlayMode::Ephemeral),
            operation,
            "overlay",
        );
        ao_standalone(ao_root(false), operation, "root");
    }
}
#[test]
fn atime_ownership_remount_census_does_not_touch_content_metadata() {
    let mut deviations = Vec::new();
    for backend in AO_BACKENDS {
        for reject in [false, true] {
            let mut table = ao_table(backend, false);
            // Seed after initial construction; no setup read is allowed to hide mutation.
            ao_seed_owned_backend(&mut table, backend, false);
            let before = table.stat("/data").unwrap();
            assert_eq!(before.atime_ms, AO_ATIME, "census fixture precondition");
            let mount_before = table.get_mounts();
            let result = table.remount(
                "/",
                if reject {
                    "remount,inodes=1"
                } else {
                    "remount,ro,noatime"
                },
            );
            if reject {
                assert_eq!(result.unwrap_err().code(), "ENOSPC");
                assert_eq!(
                    table.get_mounts(),
                    mount_before,
                    "failed admission preserves mount policy"
                );
            } else {
                result.unwrap();
            }
            let after = table.stat("/data").unwrap();
            if (after.atime_ms, after.atime_nsec) != (before.atime_ms, before.atime_nsec) {
                deviations.push(format!(
                    "{backend}/rejected={reject}: {:?} -> {:?}",
                    (before.atime_ms, before.atime_nsec),
                    (after.atime_ms, after.atime_nsec)
                ));
            }
            assert_eq!(
                (after.mtime_ms, after.mtime_nsec),
                (before.mtime_ms, before.mtime_nsec)
            );
            assert_eq!(
                (after.ctime_ms, after.ctime_nsec),
                (before.ctime_ms, before.ctime_nsec)
            );
        }
    }
    assert!(
        deviations.is_empty(),
        "internal remount census touched directory atime: {deviations:?}"
    );
}

#[test]
fn atime_ownership_mounted_writable_policies_still_update_access_times() {
    for backend in AO_BACKENDS {
        for policy in ["remount,strictatime", "remount,relatime"] {
            for operation in AO_READS {
                let mut table = ao_table(backend, false);
                table.remount("/", policy).unwrap();
                ao_seed_owned_backend(&mut table, backend, false);
                let path = ao_path(operation);
                let before = table.stat(path).unwrap();
                assert_eq!(
                    before.atime_ms, AO_ATIME,
                    "positive fixture {backend}/{policy}/{operation}"
                );
                ao_read(&mut table, operation);
                let after = table.stat(path).unwrap();
                assert_ne!(
                    after.atime_ms, AO_ATIME,
                    "mounted positive {backend}/{policy}/{operation}"
                );
                assert_eq!(
                    (after.mtime_ms, after.mtime_nsec),
                    (before.mtime_ms, before.mtime_nsec)
                );
                assert_eq!(
                    (after.ctime_ms, after.ctime_nsec),
                    (before.ctime_ms, before.ctime_nsec)
                );
            }
        }
    }
}

fn ao_seed_future<F: VirtualFileSystem>(filesystem: &mut F, future: u64) {
    filesystem.utimes("/data/file", future, AO_MTIME).unwrap();
    filesystem.utimes("/data", future, AO_MTIME).unwrap();
}

#[test]
fn atime_ownership_relatime_preserves_access_time_newer_than_modification_and_change() {
    use std::time::{SystemTime, UNIX_EPOCH};
    for backend in AO_BACKENDS {
        for operation in AO_READS {
            let mut table = ao_table(backend, false);
            table.remount("/", "remount,relatime").unwrap();
            let future = u64::try_from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
            )
            .unwrap()
            .checked_add(60_000)
            .unwrap();
            match backend {
                "memory" => ao_seed_future(
                    table
                        .root_virtual_filesystem_mut::<MemoryFileSystem>()
                        .unwrap(),
                    future,
                ),
                "overlay" => ao_seed_future(
                    table
                        .root_virtual_filesystem_mut::<OverlayFileSystem>()
                        .unwrap(),
                    future,
                ),
                "root" => ao_seed_future(
                    table
                        .root_virtual_filesystem_mut::<RootFileSystem>()
                        .unwrap(),
                    future,
                ),
                _ => unreachable!(),
            }
            let path = ao_path(operation);
            let before = table.stat(path).unwrap();
            assert_eq!((before.atime_ms, before.atime_nsec), (future, 0));
            assert!(
                before.atime_ms > before.ctime_ms && before.atime_ms > before.mtime_ms,
                "relatime precondition {backend}/{operation}"
            );
            ao_read(&mut table, operation);
            let after = table.stat(path).unwrap();
            assert_eq!(
                (after.atime_ms, after.atime_nsec),
                (before.atime_ms, before.atime_nsec)
            );
            assert_eq!(
                (after.mtime_ms, after.mtime_nsec),
                (before.mtime_ms, before.mtime_nsec)
            );
            assert_eq!(
                (after.ctime_ms, after.ctime_nsec),
                (before.ctime_ms, before.ctime_nsec)
            );
        }
    }
}

fn ao_read_errors<F: VirtualFileSystem>(filesystem: &mut F) -> Vec<String> {
    vec![
        filesystem.read_file("/data").unwrap_err().code().to_owned(),
        filesystem
            .pread("/data", 0, 1)
            .unwrap_err()
            .code()
            .to_owned(),
        filesystem
            .read_file("/missing")
            .unwrap_err()
            .code()
            .to_owned(),
        filesystem
            .read_dir("/data/file")
            .unwrap_err()
            .code()
            .to_owned(),
        filesystem
            .read_dir_limited("/data", 0)
            .unwrap_err()
            .code()
            .to_owned(),
    ]
}

#[test]
fn atime_ownership_mounted_reads_preserve_existing_error_results() {
    let expected = ["EISDIR", "EISDIR", "ENOENT", "ENOTDIR", "ENOMEM"];
    for backend in AO_BACKENDS {
        let mut table = ao_table(backend, false);
        table.remount("/", "remount,strictatime").unwrap();
        ao_seed_owned_backend(&mut table, backend, false);
        assert_eq!(ao_read_errors(&mut table), expected, "mounted {backend}");
        ao_read(&mut table, "read_file");
    }
    assert_eq!(ao_read_errors(&mut ao_memory()), expected);
    assert_eq!(
        ao_read_errors(&mut OverlayFileSystem::new(
            vec![ao_memory()],
            OverlayMode::Ephemeral
        )),
        expected
    );
    assert_eq!(ao_read_errors(&mut ao_root(false)), expected);
}
