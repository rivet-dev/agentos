use agentos_vfs_core::posix::{
    MemoryFileSystem, MountOptions, MountTable, SingleSymlinkFileSystem, SymlinkDirFileSystem,
    VirtualFileSystem,
};

fn package_table() -> MountTable {
    let mut table = MountTable::new(MemoryFileSystem::new());
    let mut content = MemoryFileSystem::new();
    content.mkdir("/bin", true).unwrap();
    content
        .write_file("/bin/pi", b"#!/bin/sh\n".to_vec())
        .unwrap();
    table
        .mount(
            "/opt/agentos/pkgs/pi/1.2.3",
            content,
            MountOptions::new("tar").read_only(true),
        )
        .unwrap();
    table
        .mount(
            "/opt/agentos/pkgs/pi/current",
            SingleSymlinkFileSystem::new("1.2.3"),
            MountOptions::new("single-symlink").read_only(true),
        )
        .unwrap();
    let commands = SymlinkDirFileSystem::new([(
        String::from("pi"),
        String::from("../pkgs/pi/current/bin/pi"),
    )])
    .unwrap();
    table
        .mount(
            "/opt/agentos/bin",
            commands,
            MountOptions::new("symlink-dir")
                .read_only(true)
                .entry_overlay(true),
        )
        .unwrap();
    table
}

#[test]
fn command_dir_overlay_keeps_its_directory_writable_for_guest_files() {
    let mut table = package_table();

    table
        .write_file("/opt/agentos/bin/user-tool", b"user".to_vec())
        .expect("guest installs a command beside the managed links");
    assert_eq!(
        table.read_file("/opt/agentos/bin/user-tool").unwrap(),
        b"user"
    );
    let mut entries = table
        .read_dir_with_types("/opt/agentos/bin")
        .unwrap()
        .into_iter()
        .map(|entry| (entry.name, entry.is_symbolic_link, entry.is_directory))
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        [
            (String::from("pi"), true, false),
            (String::from("user-tool"), false, false),
        ]
    );
    assert_eq!(
        table.realpath("/opt/agentos/bin/pi").unwrap(),
        "/opt/agentos/pkgs/pi/1.2.3/bin/pi"
    );
    assert_eq!(
        table.read_file("/opt/agentos/bin/pi").unwrap(),
        b"#!/bin/sh\n"
    );

    // Managed links stay read-only, and the directory never looks empty.
    for error in [
        table.write_file("/opt/agentos/bin/pi", b"x".to_vec()),
        table.remove_file("/opt/agentos/bin/pi"),
        table.symlink("/tmp/x", "/opt/agentos/bin/pi"),
    ] {
        assert_eq!(error.unwrap_err().code(), "EROFS");
    }
    table.remove_file("/opt/agentos/bin/user-tool").unwrap();
    assert_eq!(
        table.remove_dir("/opt/agentos/bin").unwrap_err().code(),
        "ENOTEMPTY"
    );
}
