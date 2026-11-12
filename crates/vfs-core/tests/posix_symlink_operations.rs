use agentos_vfs_core::posix::{
    resolve_path_for_creation, MemoryFileSystem, MountTable, OverlayFileSystem, OverlayMode,
    VirtualFileSystem,
};

#[test]
fn creation_resolution_follows_chained_links_and_missing_parents() {
    let mut fs = MemoryFileSystem::new();
    fs.mkdir("/allowed", true).unwrap();
    fs.mkdir("/private", true).unwrap();
    fs.symlink("../private/missing", "/allowed/link").unwrap();
    fs.symlink("link", "/allowed/chain").unwrap();
    assert_eq!(
        resolve_path_for_creation(&fs, "/allowed/chain").unwrap(),
        "/private/missing"
    );
    assert_eq!(
        resolve_path_for_creation(&fs, "/allowed/chain/child").unwrap(),
        "/private/missing/child"
    );
    assert_eq!(
        resolve_path_for_creation(&fs, "/allowed/chain/../escape")
            .unwrap_err()
            .code(),
        "ENOENT"
    );
    assert_eq!(
        resolve_path_for_creation(&fs, "/allowed/chain/")
            .unwrap_err()
            .code(),
        "ENOENT"
    );
    fs.symlink("cycle", "/allowed/cycle").unwrap();
    assert_eq!(
        resolve_path_for_creation(&fs, "/allowed/cycle")
            .unwrap_err()
            .code(),
        "ELOOP"
    );
}

#[test]
fn overlay_exists_preserves_component_and_trailing_slash_semantics() {
    for lower_backed in [false, true] {
        let mut layer = MemoryFileSystem::new();
        layer.mkdir("/a/deep", true).unwrap();
        layer.mkdir("/b", true).unwrap();
        layer.write_file("/a/file", b"data").unwrap();
        layer.symlink("/a/deep", "/b/link").unwrap();
        let mut fs = if lower_backed {
            OverlayFileSystem::new(vec![layer], OverlayMode::Ephemeral)
        } else {
            OverlayFileSystem::with_upper(vec![], layer)
        };
        assert!(fs.exists("/b/link/../file"), "lower_backed={lower_backed}");
        assert!(!fs.exists("/a/file/"), "lower_backed={lower_backed}");
        fs.rename("/b/link", "/b/link").unwrap();
        fs.remove_file("/a/file").unwrap();
        assert!(!fs.exists("/a/file"));
        assert_eq!(
            fs.rename("/a/file", "/a/file").unwrap_err().code(),
            "ENOENT"
        );
    }
}

#[test]
fn overlay_reads_preserve_symlink_cycle_errors_and_upper_shadowing() {
    for lower_backed in [false, true] {
        let mut layer = MemoryFileSystem::new();
        layer.symlink("cycle", "/cycle").unwrap();
        let mut fs = if lower_backed {
            OverlayFileSystem::new(vec![layer], OverlayMode::Ephemeral)
        } else {
            OverlayFileSystem::with_upper(vec![], layer)
        };
        assert_eq!(fs.read_file("/cycle").unwrap_err().code(), "ELOOP");
        assert_eq!(fs.stat("/cycle").unwrap_err().code(), "ELOOP");
        assert_eq!(fs.realpath("/cycle").unwrap_err().code(), "ELOOP");
    }
    let mut lower = MemoryFileSystem::new();
    lower.write_file("/shadow", b"lower").unwrap();
    let mut upper = MemoryFileSystem::new();
    upper.symlink("/missing", "/shadow").unwrap();
    let mut fs = OverlayFileSystem::with_upper(vec![lower], upper);
    assert_eq!(fs.read_file("/shadow").unwrap_err().code(), "ENOENT");
    assert_eq!(fs.stat("/shadow").unwrap_err().code(), "ENOENT");
    assert_eq!(fs.realpath("/shadow").unwrap_err().code(), "ENOENT");
}

#[test]
fn overlay_recreation_through_symlink_clears_target_whiteout() {
    let mut fs = OverlayFileSystem::new(vec![], OverlayMode::Ephemeral);
    fs.write_file("/target", b"old").unwrap();
    fs.symlink("/target", "/link").unwrap();
    fs.remove_file("/target").unwrap();
    fs.write_file("/link", b"new").unwrap();
    assert_eq!(fs.read_file("/target").unwrap(), b"new");
    assert_eq!(fs.read_file("/link").unwrap(), b"new");
}

#[test]
fn overlay_exclusive_creation_respects_lower_entries_and_whiteouts() {
    let mut lower = MemoryFileSystem::new();
    lower.write_file("/source", b"source").unwrap();
    lower.write_file("/taken", b"original").unwrap();
    let mut fs = OverlayFileSystem::new(vec![lower], OverlayMode::Ephemeral);
    assert_eq!(
        fs.create_file_exclusive("/taken", b"replacement")
            .unwrap_err()
            .code(),
        "EEXIST"
    );
    fs.remove_file("/taken").unwrap();
    fs.create_file_exclusive("/taken", b"replacement").unwrap();
    assert_eq!(fs.read_file("/taken").unwrap(), b"replacement");
}

#[test]
fn overlay_link_destinations_cannot_replace_lower_entries() {
    for symbolic in [false, true] {
        let mut lower = MemoryFileSystem::new();
        lower.write_file("/source", b"source").unwrap();
        lower.write_file("/taken", b"original").unwrap();
        let mut fs = OverlayFileSystem::new(vec![lower], OverlayMode::Ephemeral);
        assert_eq!(fs.link("/missing", "/taken").unwrap_err().code(), "ENOENT");
        let result = if symbolic {
            fs.symlink("/source", "/taken")
        } else {
            fs.link("/source", "/taken")
        };
        assert_eq!(result.unwrap_err().code(), "EEXIST", "symbolic={symbolic}");
        assert_eq!(fs.read_file("/taken").unwrap(), b"original");
    }
}

fn missing_self_rename<F: VirtualFileSystem>(mut fs: F) {
    assert_eq!(
        fs.rename("/missing", "/missing").unwrap_err().code(),
        "ENOENT"
    );
    fs.symlink("/missing", "/link").unwrap();
    fs.rename("/link", "/link").unwrap();
    assert_eq!(fs.read_link("/link").unwrap(), "/missing");
}

fn dangling_exclusive_create<F: VirtualFileSystem>(mut fs: F) {
    fs.symlink("/missing", "/link").unwrap();
    assert_eq!(
        fs.create_file_exclusive("/link", b"data")
            .unwrap_err()
            .code(),
        "EEXIST"
    );
    assert!(!fs.exists("/missing"));
    assert!(fs.lstat("/link").unwrap().is_symbolic_link);
}

fn create_through_dangling_symlink<F: VirtualFileSystem>(mut fs: F) {
    fs.symlink("/missing", "/link").unwrap();
    assert!(!fs.exists("/link"));
    fs.write_file("/link", b"data").unwrap();
    assert_eq!(fs.read_file("/missing").unwrap(), b"data");
    assert_eq!(fs.read_link("/link").unwrap(), "/missing");
}

fn hard_link_preserves_symlink<F: VirtualFileSystem>(mut fs: F) {
    fs.write_file("/target", b"data").unwrap();
    fs.symlink("/target", "/link").unwrap();
    fs.link("/link", "/alias").unwrap();
    assert!(fs.lstat("/alias").unwrap().is_symbolic_link);
    assert_eq!(fs.read_link("/alias").unwrap(), "/target");
    assert_eq!(fs.lstat("/alias").unwrap().nlink, 2);
    assert_eq!(fs.stat("/target").unwrap().nlink, 1);
    fs.symlink("/missing", "/dangling").unwrap();
    fs.link("/dangling", "/dangling-alias").unwrap();
    assert_eq!(fs.read_link("/dangling-alias").unwrap(), "/missing");
}

fn rename_between_links_to_same_inode_is_noop<F: VirtualFileSystem>(mut fs: F) {
    fs.write_file("/file", b"data").unwrap();
    fs.symlink("/missing", "/symlink").unwrap();
    for source in ["/file", "/symlink"] {
        let alias = format!("{source}-alias");
        fs.link(source, &alias).unwrap();
        let original = fs.lstat(source).unwrap();
        fs.rename(source, &alias).unwrap();
        for path in [source, alias.as_str()] {
            let stat = fs.lstat(path).unwrap();
            assert_eq!((stat.ino, stat.nlink), (original.ino, 2));
        }
    }
}

macro_rules! filesystem_cases {
    ($module:ident, $filesystem:expr) => {
        mod $module {
            use super::*;
            #[test]
            fn rename_requires_source() {
                missing_self_rename($filesystem);
            }
            #[test]
            fn exclusive_create_rejects_dangling_symlink() {
                dangling_exclusive_create($filesystem);
            }
            #[test]
            fn write_creates_dangling_target() {
                create_through_dangling_symlink($filesystem);
            }
            #[test]
            fn hard_link_uses_symlink_inode() {
                hard_link_preserves_symlink($filesystem);
            }
            #[test]
            fn rename_preserves_both_names_for_same_inode() {
                rename_between_links_to_same_inode_is_noop($filesystem);
            }
        }
    };
}

filesystem_cases!(memory, MemoryFileSystem::new());
filesystem_cases!(
    overlay,
    OverlayFileSystem::new(vec![], OverlayMode::Ephemeral)
);
filesystem_cases!(
    mounted_overlay,
    MountTable::new(OverlayFileSystem::new(vec![], OverlayMode::Ephemeral))
);

#[test]
fn overlay_symlinks_resolve_targets_across_layers() {
    let mut lower = MemoryFileSystem::new();
    lower.write_file("/lower-target", b"lower bytes").unwrap();
    lower.symlink("/upper-target", "/lower-link").unwrap();
    let mut upper = MemoryFileSystem::new();
    upper.write_file("/upper-target", b"upper bytes").unwrap();
    upper.symlink("/lower-target", "/upper-link").unwrap();
    let mut fs = OverlayFileSystem::with_upper(vec![lower], upper);
    for (link, target, bytes) in [
        ("/upper-link", "/lower-target", b"lower bytes"),
        ("/lower-link", "/upper-target", b"upper bytes"),
    ] {
        assert_eq!(fs.realpath(link).unwrap(), target);
        assert_eq!(fs.read_file(link).unwrap(), bytes);
        assert_eq!(fs.stat(link).unwrap().size, bytes.len() as u64);
    }
}

#[test]
fn overlay_symlink_reads_observe_target_overrides_and_whiteouts() {
    let mut lower = MemoryFileSystem::new();
    lower.write_file("/target", b"lower bytes").unwrap();
    lower.symlink("/target", "/link").unwrap();
    let mut fs = OverlayFileSystem::new(vec![lower], OverlayMode::Ephemeral);
    fs.write_file("/target", b"replacement").unwrap();
    assert_eq!(fs.read_file("/link").unwrap(), b"replacement");
    fs.remove_file("/target").unwrap();
    assert_eq!(fs.read_file("/link").unwrap_err().code(), "ENOENT");
    assert_eq!(fs.stat("/link").unwrap_err().code(), "ENOENT");
    assert_eq!(fs.realpath("/link").unwrap_err().code(), "ENOENT");
    assert!(fs.lstat("/link").unwrap().is_symbolic_link);
}

#[test]
fn overlay_directory_alias_reads_observe_merged_children() {
    let mut lower = MemoryFileSystem::new();
    lower.mkdir("/directory", true).unwrap();
    lower.write_file("/directory/deleted", b"old").unwrap();
    lower.symlink("/directory", "/alias").unwrap();
    let mut fs = OverlayFileSystem::new(vec![lower], OverlayMode::Ephemeral);
    fs.remove_file("/directory/deleted").unwrap();
    fs.write_file("/directory/new", b"new").unwrap();
    fs.symlink("new", "/directory/child-link").unwrap();
    assert_eq!(fs.read_dir("/alias").unwrap(), vec!["child-link", "new"]);
    assert_eq!(
        fs.read_dir_limited("/alias", 2).unwrap(),
        vec!["child-link", "new"]
    );
    assert_eq!(fs.lstat("/alias/new").unwrap().size, 3);
    assert_eq!(fs.read_link("/alias/child-link").unwrap(), "new");
}
