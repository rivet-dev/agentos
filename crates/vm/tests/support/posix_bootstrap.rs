#[test]
fn prepared_readonly_creation_preserves_reads_and_mount_policy() {
    rp_with_vm(
        true,
        serde_json::json!([
            {"path": "/readonly-content", "kind": "directory"},
            {"path": "/readonly-content/file", "kind": "file", "content": "sealed content"}
        ]),
        |manager, id| {
            let mut vm = manager.vms.get_mut(id).unwrap();
            let file = vm.kernel.read_file("/readonly-content/file");
            let range = vm.kernel.pread_file("/readonly-content/file", 7, 7);
            let entries = vm.kernel.read_dir("/readonly-content");
            let root_read_only = vm
                .kernel
                .mounted_filesystems()
                .iter()
                .find(|mount| mount.path == "/")
                .unwrap()
                .read_only;
            let write = vm
                .kernel
                .write_file("/readonly-content/file", b"changed".to_vec());
            assert!(
                file.as_ref().is_ok_and(|value| value == b"sealed content")
                    && range.as_ref().is_ok_and(|value| value == b"content")
                    && entries
                        .as_ref()
                        .is_ok_and(|value| value == &[String::from("file")])
                    && root_read_only
                    && write.as_ref().is_err_and(|error| error.code() == "EROFS"),
                "file={file:?}; range={range:?}; entries={entries:?}; root_read_only={root_read_only}; write={write:?}"
            );
        },
    );
}

#[test]
fn prepared_readonly_creation_completes_product_directories_before_sealing() {
    for profile in [
        agentos_vm_config::VmDefaultsProfile::AgentOs,
        agentos_vm_config::VmDefaultsProfile::Secure,
    ] {
        let mut sidecar = VmManager::new(LocalBridge::default()).expect("test sidecar");
        sidecar.connections.insert(
            "bootstrap-connection".into(),
            ConnectionState {
                auth_token: String::new(),
                sessions: BTreeSet::from(["bootstrap-session".into()]),
            },
        );
        sidecar.sessions.insert(
            "bootstrap-session".into(),
            SessionState {
                connection_id: "bootstrap-connection".into(),
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
            None,
        );
        let mut config: agentos_vm_config::CreateVmConfig =
            serde_json::from_str(&payload.config).unwrap();
        config.defaults_profile = Some(profile);
        config.root_filesystem.mode = agentos_vm_config::RootFilesystemMode::ReadOnly;
        payload.config = serde_json::to_string(&config).unwrap();
        let request = RequestFrame::new(
            1,
            OwnershipScope::session("bootstrap-connection", "bootstrap-session"),
            RequestPayload::CreateVm(payload.clone()),
        );
        let prepared = sidecar
            .prepare_create_vm(&request, payload)
            .expect("prepare root");
        let mut completed =
            block_on(prepared.execute()).expect("complete readonly root before publication");
        // Capture body results before asserting: even the expected RED must
        // publish and run owning disposal, because VmState has no Drop cleanup.
        let boot = completed.vm.kernel.stat_for_operator("/boot");
        let readonly = completed
            .vm
            .kernel
            .root_filesystem_mut()
            .and_then(|root| root.write_file("/post-bootstrap", vec![1]).err())
            .map(|error| error.code().to_owned());
        let unpublished = sidecar.vms.is_empty();
        let vm_id = completed.vm_id.clone();
        let socket_dir = completed.vm.unix_socket_host_dir.clone();
        let scratch_root = completed.vm.runtime_scratch_root.clone();
        let publish = sidecar.complete_create_vm(completed);
        // Failed finalization already invokes cleanup_unpublished_vm.
        let cleanup = if publish.is_ok() {
            block_on(sidecar.dispose_vm_internal(
                "bootstrap-connection",
                "bootstrap-session",
                &vm_id,
                DisposeReason::Requested,
            ))
            .map(|_| ())
        } else {
            Ok(())
        };
        assert!(
            publish.is_ok() && cleanup.is_ok(),
            "body={boot:?}/{readonly:?}; publish={publish:?}; cleanup={cleanup:?}"
        );
        assert!(
            !socket_dir.exists() && !scratch_root.exists(),
            "owning cleanup removed test paths"
        );
        assert!(
            unpublished
                && boot.as_ref().is_ok_and(|stat| stat.is_directory)
                && readonly.as_deref() == Some("EROFS"),
            "body={boot:?}/{readonly:?}; publish={publish:?}; cleanup={cleanup:?}"
        );
    }
}
