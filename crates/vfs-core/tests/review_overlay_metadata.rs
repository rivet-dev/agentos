use agentos_vfs_core::posix::{
    MemoryFileSystem, OverlayFileSystem, OverlayMode, VirtualFileSystem, VirtualTimeSpec,
    VirtualUtimeSpec,
};

fn fixture() -> OverlayFileSystem {
    let mut lower = MemoryFileSystem::new();
    lower.write_file("/target", b"lower").unwrap();
    lower
        .set_xattr("/target", "user.old", b"lower".to_vec(), 0, true)
        .unwrap();
    lower.symlink("/target", "/link").unwrap();
    let mut fs = OverlayFileSystem::new(vec![lower], OverlayMode::Ephemeral);
    fs.set_xattr("/target", "user.old", b"upper".to_vec(), 0, true)
        .unwrap();
    fs.set_xattr("/target", "user.new", b"new".to_vec(), 0, true)
        .unwrap();
    fs
}

#[test]
fn overlay_alias_xattrs_observe_merged_target() {
    let mut fs = fixture();
    assert_eq!(fs.get_xattr("/link", "user.old", true).unwrap(), b"upper");
    assert_eq!(
        fs.list_xattrs("/link", true).unwrap(),
        vec!["user.new", "user.old"]
    );
    assert!(fs.list_xattrs("/link", false).unwrap().is_empty());
    fs.remove_file("/target").unwrap();
    assert_eq!(
        fs.get_xattr("/link", "user.old", true).unwrap_err().code(),
        "ENOENT"
    );
    assert!(fs.list_xattrs("/link", false).unwrap().is_empty());
}

#[test]
fn overlay_alias_xattr_mutations_change_merged_target_only() {
    let mut fs = fixture();
    fs.set_xattr("/link", "user.old", b"changed".to_vec(), 0, true)
        .unwrap();
    assert_eq!(
        fs.get_xattr("/target", "user.old", true).unwrap(),
        b"changed"
    );
    fs.remove_xattr("/link", "user.new", true).unwrap();
    assert_eq!(
        fs.get_xattr("/target", "user.new", true)
            .unwrap_err()
            .code(),
        "ENODATA"
    );
    fs.set_xattr("/link", "user.link", b"link".to_vec(), 0, false)
        .unwrap();
    assert_eq!(fs.get_xattr("/link", "user.link", false).unwrap(), b"link");
    assert_eq!(
        fs.get_xattr("/target", "user.link", true)
            .unwrap_err()
            .code(),
        "ENODATA"
    );
}

#[test]
fn overlay_upper_link_xattr_mutations_reach_lower_target() {
    let mut lower = MemoryFileSystem::new();
    lower.write_file("/target", b"lower").unwrap();
    lower
        .set_xattr("/target", "user.remove", b"old".to_vec(), 0, true)
        .unwrap();
    let mut fs = OverlayFileSystem::new(vec![lower], OverlayMode::Ephemeral);
    fs.symlink("/target", "/link").unwrap();
    fs.set_xattr("/link", "user.new", b"new".to_vec(), 0, true)
        .unwrap();
    assert_eq!(fs.get_xattr("/target", "user.new", true).unwrap(), b"new");
    fs.remove_xattr("/link", "user.remove", true).unwrap();
    assert_eq!(
        fs.get_xattr("/target", "user.remove", true)
            .unwrap_err()
            .code(),
        "ENODATA"
    );
    assert!(fs.list_xattrs("/link", false).unwrap().is_empty());
}

fn parent_alias_fixture() -> OverlayFileSystem {
    let mut lower = MemoryFileSystem::new();
    lower.mkdir("/directory", false).unwrap();
    lower.write_file("/directory/file", b"lower").unwrap();
    lower.symlink("file", "/directory/link").unwrap();
    let mut upper = MemoryFileSystem::new();
    upper.symlink("/directory", "/alias").unwrap();
    OverlayFileSystem::with_upper(vec![lower], upper)
}

#[test]
fn overlay_chmod_cross_layer_parent_alias() {
    let mut fs = parent_alias_fixture();
    fs.chmod("/alias/link", 0o600).unwrap();
    assert_eq!(fs.stat("/directory/file").unwrap().mode & 0o777, 0o600);
    assert!(fs.lstat("/directory/link").unwrap().is_symbolic_link);
}

#[test]
fn overlay_chown_cross_layer_parent_alias() {
    let mut fs = parent_alias_fixture();
    fs.chown("/alias/link", 123, 456).unwrap();
    let stat = fs.stat("/directory/file").unwrap();
    assert_eq!((stat.uid, stat.gid), (123, 456));
}

#[test]
fn overlay_lchown_cross_layer_parent_alias() {
    let mut fs = parent_alias_fixture();
    let target_before = fs.stat("/directory/file").unwrap();
    fs.lchown("/alias/link", 123, 456).unwrap();
    let link = fs.lstat("/directory/link").unwrap();
    assert_eq!((link.uid, link.gid), (123, 456));
    let target = fs.stat("/directory/file").unwrap();
    assert_eq!(
        (target.uid, target.gid),
        (target_before.uid, target_before.gid)
    );
}

#[test]
fn overlay_chown_spec_nofollow_cross_layer_parent_alias() {
    let mut fs = parent_alias_fixture();
    fs.chown_spec("/alias/link", 123, 456, false).unwrap();
    let stat = fs.lstat("/directory/link").unwrap();
    assert_eq!((stat.uid, stat.gid), (123, 456));
    assert_ne!(fs.stat("/directory/file").unwrap().uid, 123);
}

#[test]
fn overlay_truncate_cross_layer_parent_alias() {
    let mut fs = parent_alias_fixture();
    fs.truncate("/alias/link", 2).unwrap();
    assert_eq!(fs.read_file("/directory/file").unwrap(), b"lo");
}

#[test]
fn overlay_utimes_cross_layer_parent_alias() {
    let mut fs = parent_alias_fixture();
    fs.utimes("/alias/link", 12_000, 34_000).unwrap();
    let stat = fs.stat("/directory/file").unwrap();
    assert_eq!((stat.atime_ms, stat.mtime_ms), (12_000, 34_000));
}

#[test]
fn overlay_utimes_spec_nofollow_cross_layer_parent_alias() {
    let mut fs = parent_alias_fixture();
    let target_before = fs.stat("/directory/file").unwrap();
    fs.utimes_spec(
        "/alias/link",
        VirtualUtimeSpec::Set(VirtualTimeSpec::from_millis(12_000)),
        VirtualUtimeSpec::Omit,
        false,
    )
    .unwrap();
    assert_eq!(fs.lstat("/directory/link").unwrap().atime_ms, 12_000);
    let target = fs.stat("/directory/file").unwrap();
    assert_eq!(
        (target.atime_ms, target.mtime_ms),
        (target_before.atime_ms, target_before.mtime_ms)
    );
}

fn exercise_alias_operation<F: VirtualFileSystem>(
    fs: &mut F,
    operation: &str,
) -> Result<String, String> {
    let result = (|| -> agentos_vfs_core::posix::VfsResult<String> {
        match operation {
            "pread" => return Ok(format!("{:?}", fs.pread("/alias/link", 1, 3)?)),
            "allocated_ranges" => return Ok(format!("{:?}", fs.allocated_ranges("/alias/link")?)),
            "unwritten_ranges" => return Ok(format!("{:?}", fs.unwritten_ranges("/alias/link")?)),
            "extent_at" => return Ok(format!("{:?}", fs.extent_at("/alias/link", 0)?)),
            "write" => fs.write_file("/alias/link", b"new")?,
            "write_new" => fs.write_file("/alias/new", b"new")?,
            "append" => {
                fs.append_file("/alias/link", b"new")?;
            }
            "pwrite" => fs.pwrite("/alias/link", b"X", 1)?,
            "allocate" => fs.allocate("/alias/link", 5, 3)?,
            "insert_range_valid" => fs.insert_range("/alias/link", 4096, 4096)?,
            "collapse_range_valid" => fs.collapse_range("/alias/link", 4096, 4096)?,
            "insert_range" => fs.insert_range("/alias/link", 1, 1)?,
            "collapse_range" => fs.collapse_range("/alias/link", 1, 1)?,
            "zero_range" => fs.zero_range("/alias/link", 1, 2, true)?,
            "punch_hole" => fs.punch_hole("/alias/link", 1, 2)?,
            "exclusive" => fs.create_file_exclusive("/alias/new", b"new")?,
            "create_dir" => fs.create_dir("/alias/newdir")?,
            "mkdir" => fs.mkdir("/alias/newdir", false)?,
            "mkdir_recursive" => fs.mkdir("/alias/newdir/child", true)?,
            "mknod" => fs.mknod("/alias/new", 0o010600, 0)?,
            "symlink" => fs.symlink("file", "/alias/newlink")?,
            "link" => fs.link("/alias/file", "/alias/new")?,
            "link_symlink" => fs.link("/alias/link", "/alias/newlink")?,
            "unlink" => fs.remove_file("/alias/file")?,
            "unlink_symlink" => fs.remove_file("/alias/link")?,
            "rmdir" => fs.remove_dir("/alias/empty")?,
            "rename_file" => fs.rename("/alias/file", "/alias/new")?,
            "rename_link" => fs.rename("/alias/link", "/alias/newlink")?,
            "rename_dir" => fs.rename("/alias/empty", "/alias/newdir")?,
            _ => panic!("unknown operation"),
        }
        let mut names = fs.read_dir("/directory")?;
        names.sort();
        let file = fs
            .read_file("/directory/file")
            .map_err(|e| e.code().to_owned());
        let new = fs
            .lstat("/directory/new")
            .map(|s| (s.size, s.mode & 0o170000))
            .map_err(|e| e.code().to_owned());
        let link = fs
            .read_link("/directory/link")
            .map_err(|e| e.code().to_owned());
        let newlink = fs
            .read_link("/directory/newlink")
            .map_err(|e| e.code().to_owned());
        Ok(format!("{names:?}|{file:?}|{new:?}|{link:?}|{newlink:?}"))
    })();
    result.map_err(|e| e.code().to_owned())
}

#[test]
fn overlay_parent_alias_operation_matrix_matches_single_layer() {
    let mut failures = Vec::new();
    for operation in [
        "pread",
        "allocated_ranges",
        "unwritten_ranges",
        "extent_at",
        "write",
        "write_new",
        "append",
        "pwrite",
        "allocate",
        "insert_range",
        "insert_range_valid",
        "collapse_range_valid",
        "collapse_range",
        "zero_range",
        "punch_hole",
        "exclusive",
        "create_dir",
        "mkdir",
        "mkdir_recursive",
        "mknod",
        "symlink",
        "link",
        "link_symlink",
        "unlink",
        "unlink_symlink",
        "rmdir",
        "rename_file",
        "rename_link",
        "rename_dir",
    ] {
        let mut lower = MemoryFileSystem::new();
        lower.mkdir("/directory/empty", true).unwrap();
        if operation.ends_with("_valid") {
            lower
                .write_file("/directory/file", vec![b'x'; 12_288])
                .unwrap();
        } else {
            lower.write_file("/directory/file", b"lower").unwrap();
        }
        lower.symlink("file", "/directory/link").unwrap();
        let mut reference = MemoryFileSystem::from_snapshot(lower.snapshot());
        reference.symlink("/directory", "/alias").unwrap();
        let mut upper = MemoryFileSystem::new();
        upper.symlink("/directory", "/alias").unwrap();
        let mut overlay = OverlayFileSystem::with_upper(vec![lower], upper);
        let expected = exercise_alias_operation(&mut reference, operation);
        let actual = exercise_alias_operation(&mut overlay, operation);
        if expected != actual {
            failures.push(format!(
                "{operation}: expected {expected:?}; actual {actual:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn overlay_nofollow_namespace_operations_reject_trailing_symlink() {
    for operation in ["unlink", "rmdir", "rename"] {
        let mut lower = MemoryFileSystem::new();
        lower.mkdir("/directory", false).unwrap();
        let mut upper = MemoryFileSystem::new();
        upper.symlink("/directory", "/alias").unwrap();
        let mut fs = OverlayFileSystem::with_upper(vec![lower], upper);
        let result = match operation {
            "unlink" => fs.remove_file("/alias/"),
            "rmdir" => fs.remove_dir("/alias/"),
            _ => fs.rename("/alias/", "/new"),
        };
        assert_eq!(result.unwrap_err().code(), "ENOTDIR", "{operation}");
        assert!(fs.lstat("/alias").unwrap().is_symbolic_link);
        assert!(fs.stat("/directory").unwrap().is_directory);
    }
}

#[test]
fn overlay_nofollow_utimes_omit_preserves_lower_link_mtime() {
    let mut lower = MemoryFileSystem::new();
    lower.write_file("/target", b"x").unwrap();
    lower.symlink("/target", "/link").unwrap();
    lower
        .utimes_spec(
            "/link",
            VirtualUtimeSpec::Set(VirtualTimeSpec::from_millis(3_000)),
            VirtualUtimeSpec::Set(VirtualTimeSpec::from_millis(4_000)),
            false,
        )
        .unwrap();
    lower.lchown("/link", 123, 456).unwrap();
    let mut fs = OverlayFileSystem::new(vec![lower], OverlayMode::Ephemeral);
    fs.utimes_spec(
        "/link",
        VirtualUtimeSpec::Set(VirtualTimeSpec::from_millis(12_000)),
        VirtualUtimeSpec::Omit,
        false,
    )
    .unwrap();
    let stat = fs.lstat("/link").unwrap();
    assert_eq!((stat.atime_ms, stat.mtime_ms), (12_000, 4_000));
    assert_eq!((stat.uid, stat.gid), (123, 456));
}
