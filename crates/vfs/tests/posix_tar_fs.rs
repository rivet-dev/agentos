#![cfg(not(target_arch = "wasm32"))]

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tar::{Builder, EntryType, Header};
use vfs::posix::{TarFileSystem, VirtualFileSystem};

#[test]
fn tar_filesystem_reads_files_dirs_symlinks_and_realpaths() {
    let tar_path = write_fixture_tar();
    let mut fs = TarFileSystem::open(&tar_path, "fixture-digest").expect("open tar filesystem");

    assert_eq!(
        fs.read_file("/pkg/bin/pi").expect("read file"),
        b"#!/bin/sh\necho pi\n".to_vec()
    );

    let root_entries = fs.read_dir("/pkg").expect("read package dir");
    assert!(root_entries.contains(&String::from("bin")));
    assert!(root_entries.contains(&String::from("lib")));

    let typed_entries = fs
        .read_dir_with_types("/pkg/lib")
        .expect("read typed package dir");
    assert!(typed_entries
        .iter()
        .any(|entry| entry.name == "target.txt" && !entry.is_directory));
    assert!(typed_entries
        .iter()
        .any(|entry| entry.name == "target-link.txt" && entry.is_symbolic_link));

    assert_eq!(
        fs.read_link("/pkg/lib/target-link.txt")
            .expect("read symlink"),
        "target.txt"
    );
    assert_eq!(
        fs.realpath("/pkg/lib/target-link.txt")
            .expect("resolve symlink"),
        "/pkg/lib/target.txt"
    );
    assert_eq!(
        fs.read_file("/pkg/lib/target-link.txt")
            .expect("read through symlink"),
        b"target\n".to_vec()
    );

    let stat = fs.stat("/pkg/bin/pi").expect("stat executable");
    assert_eq!(stat.mode & 0o777, 0o755);
    assert!(fs.exists("/pkg/lib/target.txt"));
    assert!(!fs.exists("/pkg/missing"));
}

#[test]
fn tar_filesystem_rejects_writes_as_read_only() {
    let tar_path = write_fixture_tar();
    let mut fs = TarFileSystem::open(&tar_path, "fixture-readonly").expect("open tar filesystem");

    let error = fs
        .write_file("/pkg/new.txt", b"nope".to_vec())
        .expect_err("tar filesystem is read-only");
    assert_eq!(error.code(), "EROFS");
}

fn write_fixture_tar() -> PathBuf {
    let mut path = std::env::temp_dir();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    path.push(format!("secure-exec-tar-fs-fixture-{nonce}.tar"));

    let file = File::create(&path).expect("create fixture tar");
    let mut builder = Builder::new(file);
    append_dir(&mut builder, "pkg");
    append_dir(&mut builder, "pkg/bin");
    append_file(&mut builder, "pkg/bin/pi", b"#!/bin/sh\necho pi\n", 0o755);
    append_dir(&mut builder, "pkg/lib");
    append_file(&mut builder, "pkg/lib/target.txt", b"target\n", 0o644);
    append_symlink(&mut builder, "pkg/lib/target-link.txt", "target.txt");
    builder.finish().expect("finish fixture tar");
    builder
        .into_inner()
        .expect("finish file")
        .flush()
        .expect("flush tar");

    path
}

fn append_dir(builder: &mut Builder<File>, path: &str) {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Directory);
    header.set_mode(0o755);
    header.set_uid(0);
    header.set_gid(0);
    header.set_size(0);
    header.set_cksum();
    builder
        .append_data(&mut header, path, std::io::empty())
        .expect("append directory");
}

fn append_file(builder: &mut Builder<File>, path: &str, content: &[u8], mode: u32) {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_mode(mode);
    header.set_uid(0);
    header.set_gid(0);
    header.set_size(content.len() as u64);
    header.set_cksum();
    builder
        .append_data(&mut header, path, content)
        .expect("append file");
}

fn append_symlink(builder: &mut Builder<File>, path: &str, target: &str) {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Symlink);
    header.set_mode(0o777);
    header.set_uid(0);
    header.set_gid(0);
    header.set_size(0);
    header.set_link_name(target).expect("set link target");
    header.set_cksum();
    builder
        .append_data(&mut header, path, std::io::empty())
        .expect("append symlink");
}
