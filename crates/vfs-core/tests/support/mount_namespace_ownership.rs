use super::super::vfs::MemoryFileSystem;
use super::*;
fn storage(index: &MountNamespaceIndex) -> (usize, usize) {
    index
        .children
        .iter()
        .fold((0, 0), |(bytes, nodes), (key, child)| {
            let (nested_bytes, nested_nodes) = storage(child);
            (bytes + key.len() + nested_bytes, nodes + 1 + nested_nodes)
        })
}
#[test]
fn namespace_churn_keeps_only_live_component_storage_and_stable_identity() {
    let mut table = MountTable::new_boxed_root(
        Box::new(MountedVirtualFileSystem::new(MemoryFileSystem::new())),
        MountOptions::new("root").read_only(true),
    );
    let mut prefix = String::from("/ancestor");
    let mut previous: Option<String> = None;
    let mut identity = None;
    for step in 0..6 {
        let path = format!("{prefix}/{}-{step}", "x".repeat(1024));
        let mut payload = MemoryFileSystem::new();
        payload.write_file("/command", b"live".to_vec()).unwrap();
        table
            .mount(&path, payload, MountOptions::new("payload"))
            .unwrap();
        if let Some(old) = previous.take() {
            table.unmount(&old).unwrap();
        }
        let stat = table.lstat("/ancestor").unwrap();
        let current = (stat.dev, stat.ino);
        if let Some(before) = identity {
            assert_eq!(current, before);
        } else {
            identity = Some(current);
        }
        let (bytes, nodes) = storage(&table.namespace_directories);
        assert!(
            bytes <= path.len(),
            "no discarded full-path backing remains in prefix index"
        );
        assert!(nodes <= path.split('/').filter(|part| !part.is_empty()).count());
        assert_eq!(
            table.read_file(&format!("{path}/command")).unwrap(),
            b"live"
        );
        assert_eq!(table.read_dir(&prefix).unwrap().len(), 1);
        previous = Some(path);
        prefix.push_str("/next");
    }
    table.unmount(previous.as_ref().unwrap()).unwrap();
    assert_eq!(storage(&table.namespace_directories), (0, 0));
    assert!(!table.exists("/ancestor"));
    assert_eq!(table.lstat("/ancestor").unwrap_err().code(), "ENOENT");
}
#[test]
fn ordinary_writable_paths_skip_namespace_and_deep_virtual_walk_is_linear() {
    let mut ordinary = MountTable::new(MemoryFileSystem::new());
    let mut payload = MemoryFileSystem::new();
    payload.write_file("/command", b"live".to_vec()).unwrap();
    ordinary
        .mount(
            "/ordinary/deep/payload",
            payload,
            MountOptions::new("payload"),
        )
        .unwrap();
    assert!(ordinary.namespace_directories.children.is_empty());
    assert!(ordinary
        .mounts
        .iter()
        .all(|mount| !mount.namespace_requires_walk && mount.namespace_ancestors.is_empty()));
    let before = ordinary.namespace_component_probes.get();
    assert_eq!(
        ordinary
            .read_file("/ordinary/deep/payload/command")
            .unwrap(),
        b"live"
    );
    assert_eq!(
        ordinary.namespace_component_probes.get(),
        before,
        "existing writable exact-mount read retains its fast path"
    );
    let mut readonly = MountTable::new_boxed_root(
        Box::new(MountedVirtualFileSystem::new(MemoryFileSystem::new())),
        MountOptions::new("root").read_only(true),
    );
    let path = format!("/ancestor{}/payload", "/segment".repeat(32));
    let mut payload = MemoryFileSystem::new();
    payload.write_file("/command", b"live".to_vec()).unwrap();
    readonly
        .mount(&path, payload, MountOptions::new("payload"))
        .unwrap();
    let before = readonly.namespace_component_probes.get();
    assert_eq!(
        readonly.read_file(&format!("{path}/command")).unwrap(),
        b"live"
    );
    let probes = readonly.namespace_component_probes.get() - before;
    assert!(
        probes <= 36,
        "canonical walk must not recursively rewalk parent prefixes: {probes}"
    );
}

#[test]
fn successful_detach_invalidates_topology_and_failed_detach_does_not() {
    let mut table = MountTable::new_boxed_root(
        Box::new(MountedVirtualFileSystem::new(MemoryFileSystem::new())),
        MountOptions::new("root").read_only(true),
    );
    let mut payload = MemoryFileSystem::new();
    payload.write_file("/command", b"owned".to_vec()).unwrap();
    table
        .mount(
            "/ancestor/deep/payload",
            payload,
            MountOptions::new("payload"),
        )
        .unwrap();
    assert_eq!(
        table.read_file("/ancestor/deep/payload/command").unwrap(),
        b"owned"
    );
    let before = table.mutation_generation();
    let detached = table.detach("/ancestor/deep/payload").unwrap();
    let detached_generation = table.mutation_generation();
    assert_ne!(
        detached_generation, before,
        "topology token must invalidate cached module resolution"
    );
    assert!(!table.exists("/ancestor"));
    assert_eq!(table.lstat("/ancestor").unwrap_err().code(), "ENOENT");
    table.restore_detached(detached).unwrap();
    assert_ne!(table.mutation_generation(), detached_generation);
    assert_eq!(
        table.read_file("/ancestor/deep/payload/command").unwrap(),
        b"owned"
    );
    let before_rejection = table.mutation_generation();
    assert!(table.detach("/not-a-mount").is_err());
    assert_eq!(
        table.mutation_generation(),
        before_rejection,
        "rejected detach must not mutate topology"
    );
}
