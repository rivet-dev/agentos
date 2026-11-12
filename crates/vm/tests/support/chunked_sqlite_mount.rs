use super::*;
use agentos_vm_host_interface::LocalVmHost;
use agentos_vm_kernel::command_registry::CommandDriver;
use agentos_vm_kernel::kernel::{KernelVm, KernelVmConfig, VirtualProcessOptions};
use agentos_vm_kernel::mount_table::{MountOptions, MountTable};
use agentos_vm_kernel::permissions::Permissions;
use agentos_vm_kernel::vfs::{MemoryFileSystem, VirtualFileSystem};

fn open_mount(
    manager: &crate::VmManager<LocalVmHost>,
    database: SharedVmSqliteDatabase,
    initial_root_metadata: Option<MountRootMetadata>,
    config: serde_json::Value,
) -> Box<dyn MountedFileSystem> {
    ChunkedSqliteMountPlugin
        .open(OpenFileSystemPluginRequest {
            vm_id: "sqlite-mount-test",
            guest_path: "/workspace",
            read_only: false,
            config: &config,
            context: &MountPluginContext {
                bridge: manager.bridge.clone(),
                runtime_context: manager.runtime_context.clone().unwrap(),
                connection_id: "test".into(),
                session_id: "test".into(),
                vm_id: "sqlite-mount-test".into(),
                sidecar_requests: manager.sidecar_requests.clone(),
                database: Some(database),
                max_pread_bytes: None,
                initial_root_metadata,
            },
        })
        .unwrap()
}

fn guest_write(
    filesystem: Box<dyn MountedFileSystem>,
    uid: u32,
    read_only: bool,
) -> Result<(), String> {
    let mut base = MemoryFileSystem::new();
    base.mkdir("/workspace", true).unwrap();
    let mut config = KernelVmConfig::new("sqlite-mount-guest");
    config.permissions = Permissions::allow_all();
    config.user.uid = Some(uid);
    config.user.gid = Some(uid);
    let mut kernel = KernelVm::new(MountTable::new(base), config);
    kernel
        .mount_boxed_filesystem_for_operator(
            "/workspace",
            filesystem,
            MountOptions::new("chunked_sqlite").read_only(read_only),
        )
        .unwrap();
    kernel
        .register_driver(CommandDriver::new("node", ["node"]))
        .unwrap();
    let process = kernel
        .create_virtual_process(
            "node",
            "node",
            "node",
            Vec::new(),
            VirtualProcessOptions::default(),
        )
        .unwrap();
    kernel
        .write_file_for_process(
            "node",
            process.pid(),
            "/workspace/guest-file",
            b"persistent guest data".to_vec(),
            None,
        )
        .map_err(|error| error.code().to_owned())
}

#[test]
fn sqlite_mount_fresh_ownership_and_stored_permissions() {
    let manager = crate::VmManager::new(LocalVmHost::default()).unwrap();
    let runtime = manager.runtime_context.clone().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("vm.sqlite");
    let open_database = || {
        runtime.tokio_handle().block_on(async {
            let database = crate::vm_sqlite::open_local_vm_sqlite(
                path.clone(),
                runtime.clone(),
                128 * 1024 * 1024,
            )
            .await
            .unwrap();
            bootstrap_schema(database.as_ref()).await.unwrap();
            database
        })
    };
    let database = open_database();
    let mut covered = MemoryFileSystem::new();
    covered.mkdir("/workspace", true).unwrap();
    covered.chown("/workspace", 1000, 1000).unwrap();
    covered.chmod("/workspace", 0o2770).unwrap();
    covered.write_file("/regular", b"data".to_vec()).unwrap();
    covered.symlink("/workspace", "/link").unwrap();
    covered.symlink("/absent", "/dangling").unwrap();
    let mut kernel = KernelVm::new(
        MountTable::new(covered),
        KernelVmConfig::new("mount-source"),
    );
    let inherited =
        crate::vm::initial_chunked_sqlite_root_metadata(&mut kernel, "/workspace").unwrap();
    let linked = crate::vm::initial_chunked_sqlite_root_metadata(&mut kernel, "/link")
        .unwrap()
        .unwrap();
    assert_eq!((linked.uid, linked.gid, linked.mode), (1000, 1000, 0o2770));
    for path in ["/missing", "/dangling"] {
        assert!(
            crate::vm::initial_chunked_sqlite_root_metadata(&mut kernel, path)
                .unwrap()
                .is_none()
        );
    }
    let error = crate::vm::initial_chunked_sqlite_root_metadata(&mut kernel, "/regular")
        .err()
        .expect("file mountpoint rejected");
    assert!(matches!(error, crate::VmError::Host(ref error) if error.code == "ENOTDIR"));
    let settings = serde_json::json!({"namespace":"workspace"});
    let mut filesystem = open_mount(&manager, database.clone(), inherited, settings.clone());
    let root = filesystem.stat("/").unwrap();
    assert_eq!(
        (root.uid, root.gid, root.mode & 0o7777),
        (1000, 1000, 0o2770)
    );
    assert_eq!(guest_write(filesystem, 1000, false), Ok(()));
    runtime.tokio_handle().block_on(database.close()).unwrap();
    drop(database);

    let database = open_database();
    // Reopening with different hints must not reset persisted ownership or mode.
    let changed = Some(MountRootMetadata {
        uid: 2000,
        gid: 2000,
        mode: 0o777,
    });
    let mut filesystem = open_mount(
        &manager,
        database.clone(),
        changed,
        serde_json::json!({"namespace":"workspace","uid":2000,"gid":2000,"dirMode":0o777}),
    );
    let root = filesystem.stat("/").unwrap();
    assert_eq!(
        (root.uid, root.gid, root.mode & 0o7777),
        (1000, 1000, 0o2770)
    );
    assert_eq!(
        filesystem.read_file("/guest-file").unwrap(),
        b"persistent guest data"
    );
    let file = filesystem.stat("/guest-file").unwrap();
    assert_eq!((file.uid, file.gid), (1000, 1000));
    assert_eq!(guest_write(filesystem, 2000, false), Err("EACCES".into()));
    assert_eq!(
        guest_write(
            open_mount(&manager, database.clone(), changed, settings),
            1000,
            true
        ),
        Err("EROFS".into())
    );

    // Explicit plugin ownership wins over the covered directory for new data.
    let mut explicit = open_mount(
        &manager,
        database.clone(),
        inherited,
        serde_json::json!({"namespace":"explicit", "uid":1234,"gid":2345,"dirMode":0o750}),
    );
    let root = explicit.stat("/").unwrap();
    assert_eq!(
        (root.uid, root.gid, root.mode & 0o7777),
        (1234, 2345, 0o750)
    );
    explicit.shutdown().unwrap();
    drop(explicit);
    // A mount without a covered directory retains the ordinary root defaults.
    let mut default = open_mount(
        &manager,
        database.clone(),
        None,
        serde_json::json!({"namespace":"default"}),
    );
    let root = default.stat("/").unwrap();
    assert_eq!((root.uid, root.gid, root.mode & 0o7777), (0, 0, 0o755));
    default.shutdown().unwrap();
    drop(default);
    runtime.tokio_handle().block_on(database.close()).unwrap();
}
