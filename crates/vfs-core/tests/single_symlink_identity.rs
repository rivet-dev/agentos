use agentos_vfs_core::posix::{
    measure_filesystem_usage, MemoryFileSystem, MountOptions, MountTable, SingleSymlinkFileSystem,
    VirtualFileSystem,
};

#[test]
fn distinct_single_symlink_filesystems_have_distinct_stable_identities() {
    let memory = MemoryFileSystem::new();
    let first = SingleSymlinkFileSystem::new("/one");
    let second = SingleSymlinkFileSystem::new("/two-longer");
    let root = memory.lstat("/").unwrap();
    let first_stat = first.lstat("/").unwrap();
    let second_stat = second.lstat("/").unwrap();
    assert_ne!(
        (first_stat.dev, first_stat.ino),
        (second_stat.dev, second_stat.ino)
    );
    assert_ne!((root.dev, root.ino), (first_stat.dev, first_stat.ino));
    assert_ne!((root.dev, root.ino), (second_stat.dev, second_stat.ino));
    assert_eq!(first.lstat("/").unwrap(), first_stat);
    assert_eq!(second.lstat("/").unwrap(), second_stat);
}

#[test]
fn cloned_single_symlink_filesystem_retains_its_logical_inode_identity() {
    let original = SingleSymlinkFileSystem::new("/clone-target");
    let cloned = original.clone();
    assert_eq!(cloned.target(), original.target());
    assert_eq!(cloned.lstat("/").unwrap(), original.lstat("/").unwrap());
}

#[test]
fn generic_usage_counts_both_actual_mounted_single_symlink_inodes_and_bytes() {
    let mut table = MountTable::new(MemoryFileSystem::new());
    table
        .mount(
            "/first",
            SingleSymlinkFileSystem::new("/one"),
            MountOptions::new("single-link"),
        )
        .unwrap();
    table
        .mount(
            "/second",
            SingleSymlinkFileSystem::new("/two-longer"),
            MountOptions::new("single-link"),
        )
        .unwrap();
    let first = table.lstat("/first").unwrap();
    let second = table.lstat("/second").unwrap();
    assert!(first.is_symbolic_link && second.is_symbolic_link);
    let usage = measure_filesystem_usage(&mut table).unwrap();
    assert_eq!(
        usage.inode_count, 3,
        "actual root plus two distinct mounted links"
    );
    assert_eq!(
        usage.total_bytes,
        (b"/one".len() + b"/two-longer".len()) as u64
    );
    assert_eq!(table.lstat("/first").unwrap(), first);
    assert_eq!(table.lstat("/second").unwrap(), second);
    assert_eq!(measure_filesystem_usage(&mut table).unwrap(), usage);
}
