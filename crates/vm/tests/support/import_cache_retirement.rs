#[cfg(feature = "node-v8")]
mod import_cache_retirement_tests {
    use super::super::{
        HostPathCleanupTicket, ImportCacheRetirementLeases, ImportCacheRetirements,
        PreparedImportCacheRetirement, VmHostPaths,
    };
    use super::*;
    use agentos_executor_node_v8::{
        CreateJavascriptContextRequest, JavascriptExecutionEngine, StartJavascriptExecutionRequest,
    };
    use agentos_executor_v8_runtime::asset_cache::NodeImportCache;

    fn process() -> DriverHandle {
        TokioDriver::process(&agentos_driver_tokio::DriverConfig::default())
            .unwrap()
            .handle()
    }

    fn paths(temporary: &tempfile::TempDir) -> VmHostPaths {
        let paths = VmHostPaths {
            scratch: temporary.path().join("scratch"),
            staging: Some(temporary.path().join("staging")),
            unix: temporary.path().join("unix"),
            #[cfg(feature = "test-support")]
            gate: std::sync::Mutex::new(None),
        };
        for path in [&paths.scratch, &paths.unix]
            .into_iter()
            .chain(paths.staging.as_ref())
        {
            fs::create_dir_all(path).unwrap();
        }
        paths
    }

    #[test]
    fn actual_prepared_execution_keeps_cache_until_owned_cleanup_can_finish() {
        let process = process();
        let temporary = tempfile::tempdir().unwrap();
        let mut engine = JavascriptExecutionEngine::new(process.clone());
        let context = engine.create_context(CreateJavascriptContextRequest {
            vm_id: "retirement-real-execution".into(),
            bootstrap_module: None,
            compile_cache_root: None,
        });
        let mut execution = engine
            .prepare_execution(StartJavascriptExecutionRequest {
                limits: Default::default(),
                argv0: None,
                guest_runtime: Default::default(),
                vm_id: "retirement-real-execution".into(),
                context_id: context.context_id,
                argv: vec!["./entry.mjs".into()],
                env: BTreeMap::new(),
                cwd: temporary.path().to_path_buf(),
                wasm_module_bytes: None,
                inline_code: Some("process.stdout.write('retained\\n');".into()),
            })
            .expect("prepare a real execution with its own import-cache lease");
        let guards = engine.import_cache_cleanup_guards("retirement-real-execution");
        assert_eq!(guards.len(), 1);
        let record = guards[0].prepare_retirement();
        let root = record.root_path().to_path_buf();
        let host_paths = paths(&temporary);
        let scratch = host_paths.scratch.clone();
        let ticket = HostPathCleanupTicket::prepare(
            &process,
            process.allocate_vm_generation().unwrap(),
            host_paths,
        )
        .unwrap();
        ticket.mark_detached();
        let leases = PreparedImportCacheRetirement {
            records: ImportCacheRetirements {
                retained_bytes: record.retained_bytes(),
                records: vec![record.clone()],
            },
            leases: ImportCacheRetirementLeases { _guards: guards },
        }
        .attach(&ticket);
        engine.dispose_vm("retirement-real-execution");
        drop(engine);
        leases.release();
        // The remaining owner is the actual execution, not a synthetic Arc in this test.
        let retained = root.exists() && record.remove_released().is_err();
        block_on(async {
            let cleanup = ticket.activate_and_wait(None);
            tokio::pin!(cleanup);
            let pending = tokio::time::timeout(Duration::from_millis(25), cleanup.as_mut())
                .await
                .is_err();
            let still_present = root.exists() && scratch.exists();
            execution
                .start_prepared()
                .expect("start retained execution");
            let result = execution.wait(); // Consumes the real execution and releases its lease.
            let cleaned = tokio::time::timeout(Duration::from_secs(5), cleanup.as_mut()).await;
            assert!(
                retained && pending && still_present,
                "cleanup must wait for the real execution lease"
            );
            let result = result.expect("execution remains usable until its owner retires");
            assert_eq!(result.exit_code, 0);
            assert_eq!(result.stdout, b"retained\n");
            cleaned
                .expect("cleanup finishes after execution retirement")
                .expect("physical cleanup succeeds");
        });
        assert!(!root.exists() && !scratch.exists());
    }

    #[test]
    fn metadata_and_task_admission_rejection_leave_original_cache_authority_intact() {
        let process = process();
        let temporary = tempfile::tempdir().unwrap();
        let cache = NodeImportCache::new_in(temporary.path().join("cache-base"));
        let record = cache.cleanup_guard().prepare_retirement();
        fs::create_dir_all(record.root_path()).unwrap();
        let root = record.root_path().to_path_buf();
        let host_paths = paths(&temporary);
        let bytes = host_paths.bytes();
        let scope = process.scoped(Arc::new(ResourceLedger::child(
            "cache-retirement-metadata-rejection",
            [(
                ResourceClass::ExecutorBytes,
                ResourceLimit::new(bytes, "test.cleanupMetadataBytes"),
            )],
            Arc::clone(process.resources()),
        )));
        let error =
            HostPathCleanupTicket::check_retained_bytes(&scope, bytes + record.retained_bytes())
                .unwrap_err();
        assert!(matches!(error, crate::VmError::ResourceLimit(_)));
        assert!(error.to_string().contains("test.cleanupMetadataBytes"));
        scope.close_admission();
        let rejected = HostPathCleanupTicket::prepare(
            &scope,
            process.allocate_vm_generation().unwrap(),
            host_paths,
        );
        assert!(
            rejected.is_err(),
            "closed task admission must fail before authority transfers"
        );
        assert!(root.exists() && record.remove_released().is_err());
        drop(record);
        drop(cache);
        assert!(
            !root.exists(),
            "unarmed failure must preserve original standalone Drop cleanup"
        );
    }

    #[test]
    fn failed_cache_deletion_retains_authority_and_does_not_skip_other_paths() {
        let process = process();
        let temporary = tempfile::tempdir().unwrap();
        let failed = NodeImportCache::new_in(temporary.path().join("failed-base"));
        let good = NodeImportCache::new_in(temporary.path().join("good-base"));
        let guards = vec![failed.cleanup_guard(), good.cleanup_guard()];
        let records = guards
            .iter()
            .map(|guard| guard.prepare_retirement())
            .collect::<Vec<_>>();
        let failed_root = records[0].root_path().to_path_buf();
        let good_root = records[1].root_path().to_path_buf();
        fs::create_dir_all(failed_root.parent().unwrap()).unwrap();
        fs::write(&failed_root, b"force ENOTDIR even when tests run as root").unwrap();
        fs::create_dir_all(&good_root).unwrap();
        let host_paths = paths(&temporary);
        let scratch = host_paths.scratch.clone();
        let generation = process.allocate_vm_generation().unwrap();
        let ticket = HostPathCleanupTicket::prepare(&process, generation, host_paths).unwrap();
        let cleanup = Arc::clone(&ticket.cleanup);
        ticket.mark_detached();
        let leases = PreparedImportCacheRetirement {
            records: ImportCacheRetirements {
                retained_bytes: records.iter().map(|record| record.retained_bytes()).sum(),
                records,
            },
            leases: ImportCacheRetirementLeases { _guards: guards },
        }
        .attach(&ticket);
        drop(failed);
        drop(good);
        leases.release();
        let result = block_on(ticket.activate_and_wait(None));
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("ERR_AGENTOS_IMPORT_CACHE_CLEANUP"));
        assert!(
            !good_root.exists() && !scratch.exists(),
            "a failed cache cannot skip later cache and VM paths"
        );
        assert!(failed_root.is_file());
        let mut manager = VmManager::new(LocalBridge::default()).unwrap();
        manager
            .host_path_cleanups
            .insert(generation, Arc::clone(&cleanup));
        manager.prune_retired_host_path_cleanups();
        assert!(manager.host_path_cleanups.contains_key(&generation));
        assert!(cleanup.retired_at().is_none());
        assert_eq!(cleanup.cache_records().records[0].root_path(), failed_root);
        // Repair the injected fault, then use the retained authority for physical cleanup.
        fs::remove_file(&failed_root).unwrap();
        let retained = cleanup.cache_records().clone();
        process
            .blocking()
            .run_sync(1024, Duration::from_secs(5), move || retained.remove())
            .expect("bounded retry worker")
            .expect("retained authority remains usable");
    }

    fn ic_with_vm(body: impl FnOnce(&mut VmManager<LocalBridge>, &str) -> Vec<std::path::PathBuf>) {
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
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(&mut manager, &id)));
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
        let roots = match result {
            Ok(roots) => roots,
            Err(panic) => std::panic::resume_unwind(panic),
        };
        assert!(
            roots.iter().all(|root| !root.exists()),
            "all engine-owned cache paths retire while manager remains alive: {roots:?}"
        );
        assert!(manager
            .javascript_engine
            .import_cache_cleanup_guards(&id)
            .is_empty());
        #[cfg(feature = "wasm-v8")]
        assert!(manager
            .wasm_engine
            .import_cache_cleanup_guards(&id)
            .is_empty());
        #[cfg(feature = "python-v8-pyodide")]
        assert!(manager
            .python_engine
            .import_cache_cleanup_guards(&id)
            .is_empty());
    }

    fn ic_roots(
        guards: Vec<Arc<agentos_executor_v8_runtime::asset_cache::NodeImportCacheCleanup>>,
        expected_slots: usize,
    ) -> Vec<std::path::PathBuf> {
        assert_eq!(guards.len(), expected_slots, "real engine slot inventory");
        let roots = guards
            .iter()
            .map(|guard| guard.prepare_retirement().root_path().to_path_buf())
            .collect::<Vec<_>>();
        assert_eq!(
            roots.iter().collect::<BTreeSet<_>>().len(),
            expected_slots,
            "outer and nested engines own distinct caches"
        );
        assert!(
            roots.iter().all(|root| root.is_dir()),
            "preparation must physically materialize every owned cache: {roots:?}"
        );
        roots
    }

    fn ic_check_collected(
        manager: &VmManager<LocalBridge>,
        vm_id: &str,
        expected: &[std::path::PathBuf],
    ) {
        // Inspect the actual manager collector without arming or detaching anything.
        // This separately owned ticket is drained below; the VM's real disposal
        // still performs its own transfer and physical cleanup in ic_with_vm.
        let runtime = process();
        let temporary = tempfile::tempdir().unwrap();
        let ticket = HostPathCleanupTicket::prepare(
            &runtime,
            runtime.allocate_vm_generation().unwrap(),
            paths(&temporary),
        )
        .unwrap();
        let inventory = manager
            .prepare_import_cache_retirement(vm_id, &ticket)
            .map(|inventory| {
                let actual = inventory
                    .records
                    .records
                    .iter()
                    .map(|record| record.root_path().to_path_buf())
                    .collect::<BTreeSet<_>>();
                (actual, inventory.records.records.len())
            });
        ticket.mark_detached();
        block_on(ticket.activate_and_wait(None)).expect("drain inventory inspection ticket");
        let (actual, record_count) = inventory.expect("collect actual engine slots");
        assert_eq!(
            actual.len(),
            record_count,
            "collector deduplicates retirement authority"
        );
        assert!(expected.iter().all(|root| actual.contains(root)),
            "collector omitted actual engine-owned roots: expected {expected:?}, collected {actual:?}");
        assert!(actual.len() <= super::super::MAX_VM_IMPORT_CACHE_RETIREMENTS);
        assert!(
            expected.iter().all(|root| root.is_dir()),
            "inventory inspection does not retire live owners"
        );
    }

    fn ic_prepare_node(engine: &mut JavascriptExecutionEngine, vm_id: &str, cwd: &std::path::Path) {
        let context = engine.create_context(CreateJavascriptContextRequest {
            vm_id: vm_id.into(),
            bootstrap_module: None,
            compile_cache_root: None,
        });
        let execution = engine
            .prepare_execution(StartJavascriptExecutionRequest {
                limits: Default::default(),
                argv0: None,
                guest_runtime: Default::default(),
                vm_id: vm_id.into(),
                context_id: context.context_id,
                argv: vec!["./cache-owner.mjs".into()],
                env: BTreeMap::new(),
                cwd: cwd.to_path_buf(),
                wasm_module_bytes: None,
                inline_code: Some(
                    "throw new Error('prepared ownership control must not execute');".into(),
                ),
            })
            .expect("materialize actual manager-global JavaScript cache without guest startup");
        drop(execution);
    }

    #[test]
    fn manager_global_execve_cache_is_collected_and_physically_retired() {
        ic_with_vm(|manager, vm_id| {
            let cwd = tempfile::tempdir().unwrap();
            ic_prepare_node(&mut manager.javascript_engine, vm_id, cwd.path());
            let roots = ic_roots(
                manager.javascript_engine.import_cache_cleanup_guards(vm_id),
                1,
            );
            ic_check_collected(manager, vm_id, &roots);
            roots
        });
    }

    #[cfg(feature = "wasm-v8")]
    fn ic_prepare_wasm(
        engine: &mut crate::executor::WasmExecutionEngine,
        vm_id: &str,
        module: &std::path::Path,
    ) {
        use agentos_executor_wasm_v8::{
            CreateWasmContextRequest, StandaloneWasmBackend, StartWasmExecutionRequest,
            WasmPermissionTier,
        };
        let context = engine.create_context(CreateWasmContextRequest {
            vm_id: vm_id.into(),
            module_path: Some(module.to_string_lossy().into_owned()),
        });
        let execution = engine
            .prepare_execution_with_runtime_for_backend(
                StartWasmExecutionRequest {
                    vm_id: vm_id.into(),
                    context_id: context.context_id,
                    managed_kernel_host: false,
                    argv: vec![module.to_string_lossy().into_owned()],
                    env: BTreeMap::new(),
                    cwd: module.parent().unwrap().to_path_buf(),
                    permission_tier: WasmPermissionTier::Full,
                    limits: Default::default(),
                    guest_runtime: Default::default(),
                },
                process(),
                StandaloneWasmBackend::V8,
            )
            .expect("prepare actual WASM execution and nested JavaScript owner");
        assert!(execution.is_prepared_for_start());
        drop(execution);
    }

    #[cfg(feature = "wasm-v8")]
    #[test]
    fn actual_wasm_outer_and_nested_cache_slots_retire_in_both_engine_families() {
        ic_with_vm(|manager, vm_id| {
            let temporary = tempfile::tempdir().unwrap();
            let module = temporary.path().join("cache-owner.wasm");
            fs::write(
                &module,
                wat::parse_str("(module (func (export \"_start\")))").unwrap(),
            )
            .unwrap();
            ic_prepare_wasm(&mut manager.wasm_engine, vm_id, &module);
            let mut roots = ic_roots(manager.wasm_engine.import_cache_cleanup_guards(vm_id), 2);
            let engines = manager.vms.get(vm_id).unwrap().execution_engines.clone();
            {
                let mut engine = engines.wasm("cache retirement owner control").unwrap();
                ic_prepare_wasm(&mut engine, vm_id, &module);
                roots.extend(ic_roots(engine.import_cache_cleanup_guards(vm_id), 2));
            }
            assert_eq!(roots.iter().collect::<BTreeSet<_>>().len(), 4);
            ic_check_collected(manager, vm_id, &roots);
            roots
        });
    }

    #[cfg(feature = "python-v8-pyodide")]
    fn ic_prepare_python(
        engine: &mut agentos_executor_python_v8_pyodide::PythonExecutionEngine,
        vm_id: &str,
        cwd: &std::path::Path,
        limits: crate::executor::PythonExecutionLimits,
        guest_runtime: crate::executor::GuestRuntimeConfig,
    ) {
        use agentos_executor_python_v8_pyodide::{
            CreatePythonContextRequest, StartPythonExecutionRequest,
        };
        let dist = engine
            .bundled_pyodide_dist_path_for_vm(vm_id)
            .expect("materialize real bundled Python assets");
        let context = engine.create_context(CreatePythonContextRequest {
            vm_id: vm_id.into(),
            pyodide_dist_path: dist,
        });
        let execution = engine
            .prepare_execution(StartPythonExecutionRequest {
                vm_id: vm_id.into(),
                context_id: context.context_id,
                code: "raise RuntimeError('prepared ownership control must not execute')".into(),
                file_path: None,
                env: BTreeMap::new(),
                cwd: cwd.to_path_buf(),
                limits,
                guest_runtime,
            })
            .expect(
                "prepare actual Python execution and nested JavaScript owner without Python boot",
            );
        assert!(execution.is_prepared_for_start());
        drop(execution);
    }

    #[cfg(feature = "python-v8-pyodide")]
    #[test]
    fn actual_python_outer_and_nested_cache_slots_retire_in_both_engine_families() {
        ic_with_vm(|manager, vm_id| {
            let temporary = tempfile::tempdir().unwrap();
            let (limits, guest_runtime) = crate::execution::python_cache_retirement_test_policy(
                &manager.vms.get(vm_id).unwrap(),
            );
            ic_prepare_python(
                &mut manager.python_engine,
                vm_id,
                temporary.path(),
                limits.clone(),
                guest_runtime.clone(),
            );
            let mut roots = ic_roots(manager.python_engine.import_cache_cleanup_guards(vm_id), 2);
            let engines = manager.vms.get(vm_id).unwrap().execution_engines.clone();
            {
                let mut engine = engines.python("cache retirement owner control").unwrap();
                ic_prepare_python(&mut engine, vm_id, temporary.path(), limits, guest_runtime);
                roots.extend(ic_roots(engine.import_cache_cleanup_guards(vm_id), 2));
            }
            assert_eq!(roots.iter().collect::<BTreeSet<_>>().len(), 4);
            ic_check_collected(manager, vm_id, &roots);
            roots
        });
    }
}
