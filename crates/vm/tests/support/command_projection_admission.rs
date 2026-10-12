struct CommandProjectionScratchObserver;

impl CommandProjectionScratchObserver {
    fn begin() -> Self {
        super::COMMAND_PROJECTION_SCRATCH_OBSERVER.with(|observer| {
            assert!(
                observer.borrow().is_none(),
                "scratch observer already active"
            );
            *observer.borrow_mut() = Some(None);
        });
        Self
    }

    fn observed(&self) -> std::path::PathBuf {
        super::COMMAND_PROJECTION_SCRATCH_OBSERVER.with(|observer| {
            observer
                .borrow()
                .as_ref()
                .and_then(Clone::clone)
                .expect("creation must allocate scratch on the observed thread")
        })
    }
}

impl Drop for CommandProjectionScratchObserver {
    fn drop(&mut self) {
        super::COMMAND_PROJECTION_SCRATCH_OBSERVER.with(|observer| {
            *observer.borrow_mut() = None;
        });
        super::COMMAND_PROJECTION_FAIL_SOCKET_NAMESPACE.with(|fail| fail.set(false));
    }
}

#[test]
fn command_projection_direct_create_rejects_before_publication_and_releases_owners() {
    command_projection_create_control(false, 128, false);
}

#[test]
fn command_projection_prepared_create_rejects_before_publication_and_releases_owners() {
    command_projection_create_control(true, 128, false);
}

#[test]
fn command_projection_direct_create_with_headroom_keeps_owners_until_disposal() {
    command_projection_create_control(false, 1024, false);
}

#[test]
fn command_projection_direct_namespace_failure_does_not_publish_session_membership() {
    command_projection_create_control(false, 1024, true);
}

fn command_projection_create_control(
    prepared_path: bool,
    inode_limit: usize,
    fail_namespace: bool,
) {
    let mut manager = VmManager::new(LocalBridge::default()).expect("manager");
    manager.connections.insert(
        "quota-connection".into(),
        ConnectionState {
            auth_token: String::new(),
            sessions: BTreeSet::from(["quota-session".into()]),
        },
    );
    manager.sessions.insert(
        "quota-session".into(),
        SessionState {
            connection_id: "quota-connection".into(),
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
    config.bootstrap_commands = Some(
        (0..140)
            .map(|index| format!("quota-command-{index}"))
            .collect(),
    );
    config.limits = Some(
        serde_json::from_value(serde_json::json!({
            "resources": { "maxInodeCount": inode_limit }
        }))
        .unwrap(),
    );
    payload.config = serde_json::to_string(&config).unwrap();
    let request = RequestFrame::new(
        1,
        OwnershipScope::session("quota-connection", "quota-session"),
        RequestPayload::CreateVm(payload.clone()),
    );
    let observer = CommandProjectionScratchObserver::begin();
    super::COMMAND_PROJECTION_FAIL_SOCKET_NAMESPACE.with(|fail| fail.set(fail_namespace));
    let result = if prepared_path {
        block_on(
            manager
                .prepare_create_vm(&request, payload)
                .unwrap()
                .execute(),
        )
        .map(|_| ())
    } else {
        block_on(manager.create_vm(&request, payload)).map(|_| ())
    };
    let namespace_failure_observed =
        !super::COMMAND_PROJECTION_FAIL_SOCKET_NAMESPACE.with(|fail| fail.get());
    let scratch = observer.observed();
    drop(observer);
    if inode_limit == 1024 && !fail_namespace {
        result.expect("creation with headroom must succeed");
        assert!(scratch.exists(), "published VM retains its scratch owner");
        assert!(!manager.bridge.permissions.lock().unwrap().is_empty());
        let vm_id = manager.sessions["quota-session"]
            .vm_ids
            .iter()
            .next()
            .unwrap()
            .clone();
        let payload = DisposeVmRequest {
            reason: DisposeReason::Requested,
        };
        let request = RequestFrame::new(
            2,
            OwnershipScope::vm("quota-connection", "quota-session", &vm_id),
            RequestPayload::DisposeVm(payload.clone()),
        );
        block_on(manager.dispose_vm(&request, payload))
            .expect("dispose VM with admitted projection");
        assert!(!scratch.exists());
        assert!(manager.bridge.permissions.lock().unwrap().is_empty());
        return;
    }
    let scratch_retained = scratch.exists();
    // Keep an intentionally failing baseline control from leaking its own fixture.
    let cleanup_error = if scratch_retained {
        std::fs::remove_dir_all(&scratch).err()
    } else {
        None
    };
    assert!(
        result.is_err() && cleanup_error.is_none(),
        "creation result={result:?}; fixture cleanup error={cleanup_error:?}"
    );
    let error = result.unwrap_err();
    if fail_namespace {
        assert!(
            namespace_failure_observed,
            "namespace failure hook must be consumed"
        );
        assert!(
            error
                .to_string()
                .contains("ERR_AGENTOS_TEST_UNIX_NAMESPACE"),
            "{error}"
        );
    } else {
        assert!(error.to_string().contains("ENOSPC"), "{error}");
        assert!(
            error.to_string().contains("limits.resources.maxInodeCount"),
            "{error}"
        );
    }
    assert!(manager.vms.is_empty());
    assert!(manager.sessions["quota-session"].vm_ids.is_empty());
    let permissions_retained = !manager.bridge.permissions.lock().unwrap().is_empty();
    assert!(!scratch_retained && !permissions_retained,
            "failed creation retained owners: scratch={scratch_retained}, permissions={permissions_retained}");
}
