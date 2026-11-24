// Included from vm.rs::tests. Exercises owning manager operations, not alias helpers.
const RP_CONNECTION: &str = "projection-connection";
const RP_SESSION: &str = "projection-session";

fn rp_package(name: &str, commands: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("package fixture");
    fs::create_dir(dir.path().join("bin")).unwrap();
    fs::write(
        dir.path().join("agentos-package.json"),
        serde_json::to_vec(&serde_json::json!({"name": name, "version": "1.0.0"})).unwrap(),
    )
    .unwrap();
    for command in commands {
        let path = dir.path().join("bin").join(command);
        fs::write(&path, format!("#!/bin/sh\n# {name}/{command}\nexit 0\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir
}

fn rp_descriptor(package: &tempfile::TempDir) -> PackageDescriptor {
    PackageDescriptor {
        path: package.path().to_string_lossy().into_owned(),
    }
}

fn rp_configuration(packages: &[&tempfile::TempDir]) -> ConfigureVmRequest {
    ConfigureVmRequest {
        mounts: Vec::new(),
        software: Vec::new(),
        permissions: None,
        module_access_cwd: None,
        instructions: Vec::new(),
        projected_modules: Vec::new(),
        command_permissions: Default::default(),
        loopback_exempt_ports: Vec::new(),
        packages: packages
            .iter()
            .map(|package| rp_descriptor(package))
            .collect(),
        packages_mount_at: String::new(),
        bootstrap_commands: vec!["projection-explicit".into()],
        host_function_shim_commands: Vec::new(),
    }
}

fn rp_configure(
    manager: &mut VmManager<LocalBridge>,
    id: &str,
    config: ConfigureVmRequest,
) -> Result<(), crate::VmError> {
    let request = RequestFrame::new(
        10,
        OwnershipScope::vm(RP_CONNECTION, RP_SESSION, id),
        RequestPayload::ConfigureVm(config.clone()),
    );
    block_on(manager.configure_vm(&request, config)).map(|_| ())
}

fn rp_link(
    manager: &mut VmManager<LocalBridge>,
    id: &str,
    package: &tempfile::TempDir,
) -> Result<(), crate::VmError> {
    let payload = LinkPackageRequest {
        package: rp_descriptor(package),
        package_id: "live-projection".into(),
    };
    let request = RequestFrame::new(
        11,
        OwnershipScope::vm(RP_CONNECTION, RP_SESSION, id),
        RequestPayload::LinkPackage(payload.clone()),
    );
    block_on(manager.link_package(&request, payload)).map(|_| ())
}

fn rp_unlink(
    manager: &mut VmManager<LocalBridge>,
    id: &str,
    package_id: &str,
) -> Result<(), crate::VmError> {
    let payload = UnlinkPackageRequest {
        package_id: package_id.into(),
    };
    let request = RequestFrame::new(
        12,
        OwnershipScope::vm(RP_CONNECTION, RP_SESSION, id),
        RequestPayload::UnlinkPackage(payload.clone()),
    );
    block_on(manager.unlink_package(&request, payload)).map(|_| ())
}

// Clean up the actual VM even if an assertion or the expected pre-fix failure panics.
fn rp_with_vm(
    read_only: bool,
    entries: serde_json::Value,
    body: impl FnOnce(&mut VmManager<LocalBridge>, &str),
) {
    rp_with_vm_entries(read_only, entries, false, body);
}

fn rp_with_vm_entries(
    read_only: bool,
    entries: serde_json::Value,
    entries_are_lower: bool,
    body: impl FnOnce(&mut VmManager<LocalBridge>, &str),
) {
    let mut manager = VmManager::new(LocalBridge::default()).expect("projection manager");
    manager.connections.insert(
        RP_CONNECTION.into(),
        ConnectionState {
            auth_token: String::new(),
            sessions: BTreeSet::from([RP_SESSION.into()]),
        },
    );
    manager.sessions.insert(
        RP_SESSION.into(),
        SessionState {
            connection_id: RP_CONNECTION.into(),
            placement: SidecarPlacement::SidecarPlacementShared(SidecarPlacementShared {
                pool: None,
            }),
            metadata: BTreeMap::new(),
            vm_ids: BTreeSet::new(),
        },
    );
    let mut payload = CreateVmRequest::legacy_test_config(
        GuestRuntimeKind::JavaScript,
        Default::default(),
        Default::default(),
        Some(PermissionsPolicy::allow_all()),
    );
    let mut config: agentos_vm_config::CreateVmConfig =
        serde_json::from_str(&payload.config).unwrap();
    config.bootstrap_commands = Some(vec!["projection-explicit".into()]);
    let entries = serde_json::from_value(entries).unwrap();
    if entries_are_lower {
        // The snapshot owns /bin's inode kind; do not symlink over a bundled directory.
        config.root_filesystem.disable_default_base_layer = true;
        config.root_filesystem.lowers =
            vec![agentos_vm_config::RootFilesystemLowerDescriptor::Snapshot { entries }];
    } else {
        config.root_filesystem.bootstrap_entries = entries;
    }
    if read_only {
        config.root_filesystem.mode = agentos_vm_config::RootFilesystemMode::ReadOnly;
    }
    payload.config = serde_json::to_string(&config).unwrap();
    let request = RequestFrame::new(
        1,
        OwnershipScope::session(RP_CONNECTION, RP_SESSION),
        RequestPayload::CreateVm(payload.clone()),
    );
    let prepared = manager
        .prepare_create_vm(&request, payload)
        .expect("prepare projection VM");
    let completed = block_on(prepared.execute()).expect("create projection VM");
    let id = completed.vm_id.clone();
    let scratch = completed.vm.runtime_scratch_root.clone();
    let sockets = completed.vm.unix_socket_host_dir.clone();
    manager
        .complete_create_vm(completed)
        .expect("publish projection VM");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(&mut manager, &id)));
    let cleanup = block_on(manager.dispose_vm_internal(
        RP_CONNECTION,
        RP_SESSION,
        &id,
        DisposeReason::Requested,
    ));
    assert!(
        cleanup.is_ok(),
        "owning fixture cleanup failed: {cleanup:?}"
    );
    assert!(
        !scratch.exists() && !sockets.exists(),
        "fixture paths retired"
    );
    assert!(
        manager.bridge.permissions.lock().unwrap().is_empty(),
        "fixture policy retired"
    );
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[derive(Debug, PartialEq)]
struct RpState {
    mounts: Vec<agentos_vm_kernel::mount_table::MountEntry>,
    configured_mounts: Vec<MountDescriptor>,
    commands: BTreeMap<String, String>,
    command_paths: BTreeMap<String, String>,
    provided: BTreeMap<String, Vec<String>>,
    configured_provided: BTreeMap<String, Vec<String>>,
    env: BTreeMap<String, String>,
    packages: Vec<(String, crate::package_projection::PackageDescriptor)>,
    roots: BTreeMap<String, String>,
    paths: BTreeMap<String, BTreeSet<String>>,
    created: BTreeMap<String, BTreeSet<String>>,
    live: BTreeSet<String>,
}

fn rp_state(manager: &VmManager<LocalBridge>, id: &str) -> RpState {
    let vm = manager.vms.get(id).unwrap();
    RpState {
        mounts: vm.kernel.mounted_filesystems(),
        configured_mounts: vm.configuration.mounts.clone(),
        commands: vm.kernel.commands(),
        command_paths: vm.command_guest_paths.clone(),
        provided: vm.provided_commands.clone(),
        configured_provided: vm.configuration.provided_commands.clone(),
        env: vm.guest_env.clone(),
        packages: vm.package_descriptors.clone(),
        roots: vm.package_mount_roots.clone(),
        paths: vm.package_mount_paths.clone(),
        created: vm.package_created_mountpoints.clone(),
        live: vm.runtime_linked_package_ids.clone(),
    }
}

#[test]
fn readonly_package_projection_reconfigure_and_remove_preserve_seal() {
    let package = rp_package("projection-repeat", &["projection-a"]);
    rp_with_vm(true, serde_json::json!([]), |manager, id| {
        let config = rp_configuration(&[&package]);
        rp_configure(manager, id, config.clone()).expect("readonly package configuration");
        {
            let mut vm = manager.vms.get_mut(id).unwrap();
            assert!(
                vm.kernel
                    .filesystem()
                    .inner()
                    .lstat("/bin/projection-a")
                    .unwrap()
                    .is_symbolic_link
            );
            assert_eq!(
                vm.kernel
                    .filesystem()
                    .inner()
                    .read_link("/bin/projection-a")
                    .unwrap(),
                "/opt/agentos/bin/projection-a"
            );
            assert_eq!(
                vm.kernel.read_file("/bin/projection-a").unwrap(),
                fs::read(package.path().join("bin/projection-a")).unwrap()
            );
            assert!(
                vm.kernel
                    .read_dir("/bin")
                    .unwrap()
                    .contains(&"projection-a".into())
            );
            assert_eq!(
                vm.kernel
                    .write_file("/tmp/projection-write", vec![1])
                    .unwrap_err()
                    .code(),
                "EROFS"
            );
            assert_eq!(
                vm.kernel.read_file("/bin/projection-explicit").unwrap(),
                KERNEL_COMMAND_STUB
            );
        }
        let before = rp_state(manager, id);
        rp_configure(manager, id, config).expect("repeat configure retains alias");
        assert_eq!(rp_state(manager, id), before);
        rp_configure(manager, id, rp_configuration(&[])).expect("remove boot package");
        let vm = manager.vms.get(id).unwrap();
        assert!(!vm.kernel.exists_for_operator("/bin/projection-a").unwrap());
        assert!(!vm.kernel.commands().contains_key("projection-a"));
        assert!(vm.package_mount_paths.is_empty());
    });
}

#[test]
fn writable_package_projection_keeps_generated_and_explicit_stubs() {
    let package = rp_package("projection-writable", &["projection-a"]);
    rp_with_vm(false, serde_json::json!([]), |manager, id| {
        rp_configure(manager, id, rp_configuration(&[&package])).unwrap();
        let mut vm = manager.vms.get_mut(id).unwrap();
        for path in ["/bin/projection-a", "/bin/projection-explicit"] {
            assert!(
                !vm.kernel
                    .filesystem()
                    .inner()
                    .lstat(path)
                    .unwrap()
                    .is_symbolic_link
            );
            assert_eq!(vm.kernel.read_file(path).unwrap(), KERNEL_COMMAND_STUB);
            assert_eq!(
                vm.kernel.stat_for_operator(path).unwrap().mode & 0o777,
                0o755
            );
        }
        assert!(
            !vm.kernel
                .mounted_filesystems()
                .iter()
                .any(|m| m.path == "/bin/projection-a")
        );
        vm.kernel
            .write_file("/tmp/projection-write", vec![1])
            .unwrap();
    });
}

#[test]
fn readonly_package_projection_preserves_custom_file_and_final_symlink_at_exact_limit() {
    let package = rp_package("projection-custom", &["projection-file", "projection-link"]);
    rp_with_vm(
        true,
        serde_json::json!([
            {"path":"/bin/projection-file", "kind":"file", "content":"custom file", "mode":493},
            {"path":"/tmp/projection-target", "kind":"file", "content":"custom target", "mode":493},
            {"path":"/bin/projection-link", "kind":"symlink", "target":"/tmp/projection-target"}
        ]),
        |manager, id| {
            manager
                .vms
                .get_mut(id)
                .unwrap()
                .limits
                .agentos_packages
                .max_mounts = 4;
            rp_configure(manager, id, rp_configuration(&[&package]))
                .expect("no charge for unnecessary aliases");
            let mut vm = manager.vms.get_mut(id).unwrap();
            assert_eq!(
                vm.kernel.read_file("/bin/projection-file").unwrap(),
                b"custom file"
            );
            assert_eq!(
                vm.kernel.read_file("/bin/projection-link").unwrap(),
                b"custom target"
            );
            assert_eq!(
                vm.kernel
                    .filesystem()
                    .inner()
                    .read_link("/bin/projection-link")
                    .unwrap(),
                "/tmp/projection-target"
            );
            assert!(
                !vm.kernel
                    .mounted_filesystems()
                    .iter()
                    .any(|m| m.path.starts_with("/bin/projection-"))
            );
        },
    );
}

#[test]
fn readonly_package_projection_dangling_custom_leaf_rejects_without_clobbering() {
    let old = rp_package("projection-old", &["projection-old"]);
    let next = rp_package("projection-dangling", &["projection-dangling"]);
    rp_with_vm(
        true,
        serde_json::json!([
            {"path":"/bin/projection-dangling", "kind":"symlink", "target":"/tmp/projection-missing"}
        ]),
        |manager, id| {
            rp_configure(manager, id, rp_configuration(&[&old])).unwrap();
            let before = rp_state(manager, id);
            let error = rp_configure(manager, id, rp_configuration(&[&next])).unwrap_err();
            assert!(error.to_string().contains("EROFS"), "{error}");
            assert!(!error.to_string().contains("rollback failed"), "{error}");
            assert_eq!(rp_state(manager, id), before);
            let vm = manager.vms.get(id).unwrap();
            assert_eq!(
                vm.kernel
                    .filesystem()
                    .inner()
                    .read_link("/bin/projection-dangling")
                    .unwrap(),
                "/tmp/projection-missing"
            );
            assert!(
                !vm.kernel
                    .exists_for_operator("/tmp/projection-missing")
                    .unwrap()
            );
            assert!(
                vm.kernel
                    .exists_for_operator("/bin/projection-old")
                    .unwrap()
            );
        },
    );
}

#[test]
fn readonly_package_projection_alias_admission_rolls_back_configure_and_live_link() {
    let old = rp_package("projection-old", &["projection-old"]);
    let next = rp_package(
        "projection-next",
        &["projection-next-a", "projection-next-b"],
    );
    let live = rp_package("projection-live", &["projection-live"]);
    rp_with_vm(true, serde_json::json!([]), |manager, id| {
        rp_configure(manager, id, rp_configuration(&[&old])).unwrap();
        let before = rp_state(manager, id);
        manager
            .vms
            .get_mut(id)
            .unwrap()
            .limits
            .agentos_packages
            .max_mounts = 5;
        let error = rp_configure(manager, id, rp_configuration(&[&next])).unwrap_err();
        assert!(
            matches!(error, crate::VmError::PackageMountLimit { .. }),
            "{error}"
        );
        assert_eq!(rp_state(manager, id), before);
        manager
            .vms
            .get_mut(id)
            .unwrap()
            .limits
            .agentos_packages
            .max_mounts = 7;
        let error = rp_link(manager, id, &live).unwrap_err();
        assert!(
            matches!(error, crate::VmError::PackageMountLimit { .. }),
            "{error}"
        );
        assert_eq!(rp_state(manager, id), before);
        let vm = manager.vms.get(id).unwrap();
        assert!(
            vm.kernel
                .exists_for_operator("/bin/projection-old")
                .unwrap()
        );
        for command in ["projection-next-a", "projection-next-b", "projection-live"] {
            assert!(
                !vm.kernel
                    .exists_for_operator(&format!("/bin/{command}"))
                    .unwrap()
            );
            assert!(
                !vm.kernel
                    .exists_for_operator(&format!("/opt/agentos/bin/{command}"))
                    .unwrap()
            );
        }
    });
}

#[test]
fn readonly_package_projection_live_link_survives_configure_then_unlinks_owned_alias() {
    let boot = rp_package("projection-boot", &["projection-boot"]);
    let live = rp_package("projection-live", &["projection-live"]);
    rp_with_vm(true, serde_json::json!([]), |manager, id| {
        rp_configure(manager, id, rp_configuration(&[&boot])).unwrap();
        rp_link(manager, id, &live).unwrap();
        rp_configure(manager, id, rp_configuration(&[&boot])).unwrap();
        {
            let vm = manager.vms.get(id).unwrap();
            assert!(
                vm.kernel
                    .exists_for_operator("/bin/projection-live")
                    .unwrap()
            );
            assert!(vm.package_mount_paths["live-projection"].contains("/bin/projection-live"));
        }
        rp_unlink(manager, id, "live-projection").unwrap();
        let vm = manager.vms.get(id).unwrap();
        assert!(
            !vm.kernel
                .exists_for_operator("/bin/projection-live")
                .unwrap()
        );
        assert!(!vm.kernel.commands().contains_key("projection-live"));
        assert!(
            vm.kernel
                .exists_for_operator("/bin/projection-boot")
                .unwrap()
        );
        assert!(!vm.package_mount_paths.contains_key("live-projection"));
    });
}

#[test]
fn readonly_package_projection_canonical_bin_parent_and_root_mount_at() {
    let package = rp_package("projection-canonical", &["projection-a"]);
    rp_with_vm_entries(
        true,
        serde_json::json!([
            {"path":"/tools", "kind":"directory"},
            {"path":"/bin", "kind":"symlink", "target":"/tools"}
        ]),
        true,
        |manager, id| {
            rp_configure(manager, id, rp_configuration(&[&package])).unwrap();
            let vm = manager.vms.get(id).unwrap();
            assert!(
                vm.kernel
                    .mounted_filesystems()
                    .iter()
                    .any(|m| m.path == "/tools/projection-a")
            );
            assert_eq!(
                vm.kernel.filesystem().inner().read_link("/bin").unwrap(),
                "/tools"
            );
            assert!(vm.kernel.exists_for_operator("/bin/projection-a").unwrap());
        },
    );
    rp_with_vm(true, serde_json::json!([]), |manager, id| {
        let mut config = rp_configuration(&[&package]);
        config.packages_mount_at = "/".into();
        manager
            .vms
            .get_mut(id)
            .unwrap()
            .limits
            .agentos_packages
            .max_mounts = 3;
        rp_configure(manager, id, config)
            .expect("existing planned /bin leaf needs no duplicate alias");
        let mut vm = manager.vms.get_mut(id).unwrap();
        assert_eq!(
            vm.kernel
                .mounted_filesystems()
                .iter()
                .filter(|m| m.path == "/bin/projection-a")
                .count(),
            1
        );
        assert_eq!(
            vm.kernel.read_file("/bin/projection-a").unwrap(),
            fs::read(package.path().join("bin/projection-a")).unwrap()
        );
    });
}

fn rp_memory_bin() -> MountDescriptor {
    MountDescriptor {
        guest_path: "/bin".into(),
        guest_source: "projection-test".into(),
        guest_fstype: "memory".into(),
        read_only: false,
        plugin: MountPluginDescriptor {
            id: "memory".into(),
            config: "{}".into(),
        },
    }
}

#[test]
fn readonly_package_projection_unlink_removes_alias_mountpoint_from_writable_bin() {
    let package = rp_package("projection-writable-bin", &["projection-a"]);
    rp_with_vm(true, serde_json::json!([]), |manager, id| {
        let mut config = rp_configuration(&[&package]);
        config.mounts.push(rp_memory_bin());
        rp_configure(manager, id, config).unwrap();
        let package_id = format!("path:{}", package.path().display());
        {
            let mut vm = manager.vms.get_mut(id).unwrap();
            assert!(vm.package_created_mountpoints[&package_id].contains("/bin/projection-a"));
            vm.kernel
                .write_file("/bin/custom-neighbor", b"keep".to_vec())
                .unwrap();
        }
        rp_unlink(manager, id, &package_id).unwrap();
        let mut vm = manager.vms.get_mut(id).unwrap();
        assert_eq!(
            vm.kernel
                .filesystem()
                .inner()
                .lstat("/bin/projection-a")
                .unwrap_err()
                .code(),
            "ENOENT"
        );
        assert_eq!(
            vm.kernel.read_file("/bin/custom-neighbor").unwrap(),
            b"keep"
        );
        assert!(
            vm.kernel
                .mounted_filesystems()
                .iter()
                .any(|m| m.path == "/bin")
        );
    });
}

#[test]
fn readonly_package_projection_replacement_bin_custom_leaf_wins() {
    let package = rp_package("projection-custom-bin", &["projection-a"]);
    let bin = tempfile::tempdir().unwrap();
    rp_with_vm(true, serde_json::json!([]), |manager, id| {
        // A replacement /bin contains its own standard command files and a custom package leaf.
        for command in manager.vms.get(id).unwrap().kernel.commands().keys() {
            fs::write(bin.path().join(command), KERNEL_COMMAND_STUB).unwrap();
        }
        fs::write(bin.path().join("projection-a"), b"custom mounted command").unwrap();
        let mut config = rp_configuration(&[&package]);
        config.mounts.push(MountDescriptor {
            guest_path: "/bin".into(),
            guest_source: "projection-test".into(),
            guest_fstype: "agentos_packages".into(),
            read_only: true,
            plugin: MountPluginDescriptor {
                id: "agentos_packages".into(),
                config: serde_json::json!({
                    "kind":"hostDir", "hostPath":bin.path().to_string_lossy(), "readOnly":true
                })
                .to_string(),
            },
        });
        manager
            .vms
            .get_mut(id)
            .unwrap()
            .limits
            .agentos_packages
            .max_mounts = 3;
        rp_configure(manager, id, config).unwrap();
        let mut vm = manager.vms.get_mut(id).unwrap();
        assert_eq!(
            vm.kernel.read_file("/bin/projection-a").unwrap(),
            b"custom mounted command"
        );
        assert!(
            !vm.kernel
                .mounted_filesystems()
                .iter()
                .any(|m| m.path == "/bin/projection-a")
        );
    });
}

#[test]
fn readonly_package_projection_rejects_invalid_command_names_before_mount_publication() {
    let package = rp_package("projection-invalid", &["valid-image"]);
    for name in ["", "bad/name", "bad\u{0}name"] {
        fs::write(
            package.path().join("package.json"),
            serde_json::to_vec(&serde_json::json!({
                "name":"projection-invalid", "bin":BTreeMap::from([(name, "bin/valid-image")])
            }))
            .unwrap(),
        )
        .unwrap();
        rp_with_vm(true, serde_json::json!([]), |manager, id| {
            let before = rp_state(manager, id);
            let mut invalid = rp_configuration(&[&package]);
            // If descriptor validation is late, this valid mount descriptor fails to open first.
            invalid.mounts.push(MountDescriptor {
                guest_path: "/projection-invalid-open".into(),
                guest_source: "projection-test".into(),
                guest_fstype: "agentos_packages".into(),
                read_only: true,
                plugin: MountPluginDescriptor {
                    id: "agentos_packages".into(),
                    config: r#"{"kind":"singleSymlink"}"#.into(),
                },
            });
            let error = rp_configure(manager, id, invalid).unwrap_err();
            assert!(
                error.to_string().contains("invalid command name"),
                "{error}"
            );
            assert_eq!(rp_state(manager, id), before);
            let error = rp_link(manager, id, &package).unwrap_err();
            assert!(
                error.to_string().contains("invalid command name"),
                "{error}"
            );
            assert_eq!(rp_state(manager, id), before);
        });
    }
}
