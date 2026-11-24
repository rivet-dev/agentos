// Included by child_event_claim_tests to exercise the actual private owner routes.
    struct BootstrapReply {
        allow_claim: bool,
        claims: AtomicUsize,
        results: Mutex<Vec<Result<HostCallReply, HostServiceError>>>,
    }
    impl crate::executor::backend::DirectHostReplyTarget for BootstrapReply {
        fn claim(&self, _: u64) -> Result<bool, HostServiceError> {
            self.claims.fetch_add(1, Ordering::SeqCst);
            Ok(self.allow_claim)
        }
        fn respond(
            &self,
            _: u64,
            _: bool,
            result: Result<HostCallReply, HostServiceError>,
        ) -> Result<(), HostServiceError> {
            self.results.lock().unwrap().push(result);
            Ok(())
        }
    }

    // Real root/descendant dispatch paths with no executor/native workload.
    // A rejected reply must never initialize capabilities; successful service
    // snapshots descriptor reuse after admission instead of caching old state.
    #[tokio::test(flavor = "current_thread")]
    async fn descriptor_bootstrap_preserves_claim_identity_freshness_and_reply_bound() {
        use agentos_vm_kernel::fd_table::O_RDONLY;
        tokio::task::LocalSet::new()
            .run_until(async {
                for descendant in [false, true] {
                    let (mut sidecar, vm_id) = sidecar_with_test_vm(64).await;
                    let root_id = String::from("bootstrap-root");
                    let path = if descendant {
                        vec![String::from("bootstrap-child")]
                    } else {
                        Vec::new()
                    };
                    let (pid, generation) = {
                        let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
                        let mut root = host_function_process(&mut vm, "bootstrap root", None);
                        let pid = if descendant {
                            let child = host_function_process(
                                &mut vm,
                                "bootstrap child",
                                Some(root.kernel_pid),
                            );
                            let pid = child.kernel_pid;
                            root.child_processes.insert(path[0].clone(), child);
                            pid
                        } else {
                            root.kernel_pid
                        };
                        vm.active_processes.insert(root_id.clone(), root);
                        assert!(vm
                            .kernel
                            .wasi_preopens(EXECUTION_DRIVER_NAME, pid)
                            .unwrap()
                            .is_empty());
                        (pid, vm.generation)
                    };
                    for mode in [
                        "cancelled",
                        "wrong-generation",
                        "wrong-pid",
                        "fresh",
                        "reply-bound",
                    ] {
                        let before = sidecar
                            .vms
                            .get(&vm_id)
                            .unwrap()
                            .kernel
                            .fd_snapshot(EXECUTION_DRIVER_NAME, pid)
                            .unwrap();
                        let target = Arc::new(BootstrapReply {
                            allow_claim: mode != "cancelled",
                            claims: AtomicUsize::new(0),
                            results: Mutex::new(Vec::new()),
                        });
                        let reply = DirectHostReplyHandle::new(
                            HostCallIdentity {
                                generation: if mode == "wrong-generation" {
                                    generation + 1
                                } else {
                                    generation
                                },
                                pid: if mode == "wrong-pid" { u32::MAX } else { pid },
                                call_id: 702,
                            },
                            target.clone(),
                            if mode == "reply-bound" { 8 } else { 64 * 1024 },
                        )
                        .unwrap();
                        // Simulate descriptor mutation after request admission but
                        // before owner service. No request carries cached fd state.
                        let expected = if mode == "fresh" {
                            let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
                            vm.kernel
                                .write_file("/workspace/bootstrap-current", b"fresh".to_vec())
                                .unwrap();
                            let old = vm
                                .kernel
                                .fd_open(
                                    EXECUTION_DRIVER_NAME,
                                    pid,
                                    "/workspace/bootstrap-current",
                                    O_RDONLY,
                                    None,
                                )
                                .unwrap();
                            let old_id = vm
                                .kernel
                                .fd_description_identity(EXECUTION_DRIVER_NAME, pid, old)
                                .unwrap()
                                .0;
                            vm.kernel.fd_close(EXECUTION_DRIVER_NAME, pid, old).unwrap();
                            let reused = vm
                                .kernel
                                .fd_open(
                                    EXECUTION_DRIVER_NAME,
                                    pid,
                                    "/workspace/bootstrap-current",
                                    O_RDONLY,
                                    None,
                                )
                                .unwrap();
                            let new_id = vm
                                .kernel
                                .fd_description_identity(EXECUTION_DRIVER_NAME, pid, reused)
                                .unwrap()
                                .0;
                            assert_eq!(old, reused);
                            assert_ne!(old_id, new_id);
                            Some((reused, new_id))
                        } else {
                            None
                        };
                        let handle = sidecar.vms.handle(&vm_id).unwrap();
                        let operation =
                            HostOperation::Filesystem(FilesystemOperation::BootstrapDescriptors);
                        let work = if descendant {
                            sidecar.prepare_owned_descendant_host_call(
                                &vm_id,
                                &root_id,
                                &path,
                                handle,
                                operation,
                                reply.clone(),
                            )
                        } else {
                            sidecar.prepare_owned_root_host_call(
                                &vm_id,
                                &root_id,
                                handle,
                                operation,
                                reply.clone(),
                            )
                        };
                        tokio::time::timeout(Duration::from_secs(2), work)
                            .await
                            .unwrap()
                            .unwrap();
                        let mut replies = target.results.lock().unwrap();
                        if mode == "cancelled" {
                            assert_eq!(target.claims.load(Ordering::SeqCst), 1);
                            assert!(replies.is_empty());
                        } else if mode.starts_with("wrong-") {
                            assert_eq!(target.claims.load(Ordering::SeqCst), 0);
                            assert_eq!(replies.pop().unwrap().unwrap_err().code, "ESTALE");
                        } else if mode == "reply-bound" {
                            let error = replies.pop().unwrap().unwrap_err();
                            assert_eq!(error.code, "E2BIG");
                            assert_eq!(
                                error.details.unwrap()["limitName"],
                                "limits.reactor.maxBridgeResponseBytes"
                            );
                        } else {
                            let HostCallReply::Json(value) = replies.pop().unwrap().unwrap() else {
                                panic!("bootstrap returns structured metadata")
                            };
                            let descriptors = value["descriptors"].as_array().unwrap();
                            let (fd, identity) = expected.unwrap();
                            let expected_identity = identity.to_string();
                            assert!(descriptors.iter().any(|entry| entry["fd"] == fd
                                && entry["descriptionId"].as_str() == Some(expected_identity.as_str())));
                            for preopen in value["preopens"].as_array().unwrap() {
                                assert!(descriptors.iter().any(|entry| entry["fd"]
                                    == preopen["fd"]
                                    && entry["rightsBase"] == preopen["rightsBase"]
                                    && entry["rightsInheriting"] == preopen["rightsInheriting"]));
                            }
                        }
                        assert!(replies.is_empty());
                        drop(replies);
                        if mode == "cancelled" || mode.starts_with("wrong-") {
                            let vm = sidecar.vms.get(&vm_id).unwrap();
                            assert!(vm
                                .kernel
                                .wasi_preopens(EXECUTION_DRIVER_NAME, pid)
                                .unwrap()
                                .is_empty());
                            assert_eq!(
                                vm.kernel.fd_snapshot(EXECUTION_DRIVER_NAME, pid).unwrap(),
                                before
                            );
                        }
                        drop(reply);
                    }
                }
            })
            .await;
    }

