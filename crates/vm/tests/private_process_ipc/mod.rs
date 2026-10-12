//! Finite kernel/private-owner controls, including one actual Node startup
//! rejection control for root and nested spawn rollback.
use super::*;
use crate::executor::backend::{DirectHostReplyTarget, HostCallIdentity};
use agentos_vm_kernel::command_registry::CommandDriver;
use agentos_vm_kernel::kernel::KernelVmConfig;
use agentos_vm_kernel::mount_table::MountTable;
use agentos_vm_kernel::permissions::Permissions;
use agentos_vm_kernel::vfs::MemoryFileSystem;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

fn kernel(limit: Option<usize>) -> (SidecarKernel, KernelProcessHandle, KernelProcessHandle) {
    let mut config = KernelVmConfig::new("private-ipc-control");
    config.permissions = Permissions::allow_all();
    if let Some(limit) = limit {
        config.resources.max_open_fds = Some(limit);
    }
    let mut kernel = SidecarKernel::new(MountTable::new(MemoryFileSystem::new()), config);
    kernel
        .register_driver(CommandDriver::new(EXECUTION_DRIVER_NAME, [WASM_COMMAND]))
        .unwrap();
    let parent = kernel
        .spawn_process(
            WASM_COMMAND,
            Vec::new(),
            SpawnOptions {
                requester_driver: Some(EXECUTION_DRIVER_NAME.to_owned()),
                ..SpawnOptions::default()
            },
        )
        .unwrap();
    let child = kernel
        .spawn_process(
            WASM_COMMAND,
            Vec::new(),
            SpawnOptions {
                requester_driver: Some(EXECUTION_DRIVER_NAME.to_owned()),
                parent_pid: Some(parent.pid()),
                ..SpawnOptions::default()
            },
        )
        .unwrap();
    (kernel, parent, child)
}

fn options() -> ProcessLaunchOptions {
    ProcessLaunchOptions {
        stdio: vec!["ignore".into(), "pipe".into(), "pipe".into(), "ipc".into()],
        ..ProcessLaunchOptions::default()
    }
}

fn active(handle: KernelProcessHandle) -> ActiveProcess {
    let context =
        agentos_driver_tokio::TokioDriver::process(&agentos_driver_tokio::DriverConfig::default())
            .unwrap()
            .handle();
    ActiveProcess::new(
        handle.pid(),
        handle,
        context,
        crate::limits::VmLimits::default(),
        agentos_sidecar_protocol::config::DEFAULT_MAX_PROCESS_EVENTS,
        GuestRuntimeKind::WebAssembly,
        ActiveExecution::HostFunction(HostFunctionExecution::default()),
    )
}

#[test]
fn ipc_half_installed_transfer_respects_aggregate_fd_admission_and_rolls_back() {
    let (probe, _, _) = kernel(None);
    let count = probe.resource_snapshot().open_fds;
    drop(probe);
    let (mut kernel, parent, child) = kernel(Some(count + 2));
    let before = kernel.resource_snapshot();
    let mut pending = PendingProcessIpc::new(None);
    assert!(pending
        .install(&mut kernel, parent.pid(), child.pid(), &options())
        .is_err());
    let after = kernel.resource_snapshot();
    assert_eq!(after.open_fds, before.open_fds);
    assert_eq!(after.sockets, before.sockets);
    assert_eq!(after.socket_connections, before.socket_connections);
}

#[test]
fn ipc_descriptors_leave_ignored_and_piped_fd0_data_and_eof_independent() {
    let (mut kernel, parent, child) = kernel(None);
    let child_pid = child.pid();
    let parent_pid = parent.pid();
    let mut pending = PendingProcessIpc::new(None);
    pending
        .install(&mut kernel, parent_pid, child_pid, &options())
        .unwrap();
    let parent_endpoint = pending.endpoints[0].1.clone();
    let child_endpoint = pending.endpoints[1].1.clone();
    install_kernel_ignored_stdin(&mut kernel, child_pid).unwrap();
    kernel
        .fd_write_nonblocking(
            EXECUTION_DRIVER_NAME,
            parent_pid,
            parent_endpoint.fd,
            b"control",
        )
        .unwrap();
    assert!(kernel
        .fd_read_with_timeout_result(EXECUTION_DRIVER_NAME, child_pid, 0, 1, Some(Duration::ZERO))
        .unwrap()
        .unwrap_or_default()
        .is_empty());
    assert_eq!(
        kernel
            .fd_read_with_timeout_result(
                EXECUTION_DRIVER_NAME,
                child_pid,
                child_endpoint.fd,
                16,
                Some(Duration::ZERO)
            )
            .unwrap()
            .unwrap(),
        b"control"
    );
    let writer = install_kernel_stdin_pipe(&mut kernel, child_pid).unwrap();
    kernel
        .fd_write_nonblocking(EXECUTION_DRIVER_NAME, child_pid, writer, b"USER")
        .unwrap();
    assert_eq!(
        kernel
            .fd_read_with_timeout_result(
                EXECUTION_DRIVER_NAME,
                child_pid,
                0,
                16,
                Some(Duration::ZERO)
            )
            .unwrap()
            .unwrap(),
        b"USER"
    );
    kernel
        .fd_close(EXECUTION_DRIVER_NAME, child_pid, writer)
        .unwrap();
    assert!(kernel
        .fd_read_with_timeout_result(EXECUTION_DRIVER_NAME, child_pid, 0, 1, Some(Duration::ZERO))
        .unwrap()
        .unwrap_or_default()
        .is_empty());
    kernel
        .fd_close(EXECUTION_DRIVER_NAME, child_pid, 0)
        .unwrap();
    kernel
        .fd_write_nonblocking(
            EXECUTION_DRIVER_NAME,
            parent_pid,
            parent_endpoint.fd,
            b"still-ipc",
        )
        .unwrap();
    assert_eq!(
        kernel
            .fd_read_with_timeout_result(
                EXECUTION_DRIVER_NAME,
                child_pid,
                child_endpoint.fd,
                16,
                Some(Duration::ZERO)
            )
            .unwrap()
            .unwrap(),
        b"still-ipc"
    );
    kernel
        .fd_write_nonblocking(
            EXECUTION_DRIVER_NAME,
            child_pid,
            child_endpoint.fd,
            b"final",
        )
        .unwrap();
    kernel
        .fd_close(EXECUTION_DRIVER_NAME, child_pid, child_endpoint.fd)
        .unwrap();
    assert_eq!(
        kernel
            .fd_read_with_timeout_result(
                EXECUTION_DRIVER_NAME,
                parent_pid,
                parent_endpoint.fd,
                16,
                Some(Duration::ZERO)
            )
            .unwrap()
            .unwrap(),
        b"final"
    );
}

#[test]
fn ipc_stale_generation_and_fd_reuse_cannot_write_or_close_a_replacement() {
    let (mut kernel, parent, child) = kernel(None);
    let mut pending = PendingProcessIpc::new(None);
    pending
        .install(&mut kernel, parent.pid(), child.pid(), &options())
        .unwrap();
    let mut parent = active(parent);
    let mut child = active(child);
    pending.commit(&mut parent, &mut child, "child").unwrap();
    let old = parent.ipc.children["child"].clone();
    let target = ProcessIpcTarget {
        child_id: Some(
            crate::executor::host::BoundedString::try_new(
                "child".into(),
                &PayloadLimit::new("testId", 64).unwrap(),
            )
            .unwrap(),
        ),
        generation: old.generation + 1,
    };
    assert_eq!(
        error_code(&checked_endpoint(&kernel, &parent, &target).unwrap_err()),
        "ESTALE"
    );
    kernel
        .fd_close(EXECUTION_DRIVER_NAME, parent.kernel_pid, old.fd)
        .unwrap();
    let replacement = kernel
        .fd_open(
            EXECUTION_DRIVER_NAME,
            parent.kernel_pid,
            "/dev/null",
            agentos_vm_kernel::fd_table::O_RDONLY,
            None,
        )
        .unwrap();
    kernel
        .fd_dup2(
            EXECUTION_DRIVER_NAME,
            parent.kernel_pid,
            replacement,
            old.fd,
        )
        .unwrap();
    let target = ProcessIpcTarget {
        generation: old.generation,
        ..target
    };
    assert_eq!(
        error_code(&checked_endpoint(&kernel, &parent, &target).unwrap_err()),
        "ESTALE"
    );
    close_matching(&mut kernel, parent.kernel_pid, &old).unwrap();
    assert!(kernel
        .fd_stat(EXECUTION_DRIVER_NAME, parent.kernel_pid, old.fd)
        .is_ok());
}

#[derive(Default)]
struct Replies(Mutex<Vec<Result<HostCallReply, HostServiceError>>>);
impl DirectHostReplyTarget for Replies {
    fn claim(&self, _: u64) -> Result<bool, HostServiceError> {
        Ok(true)
    }
    fn respond(
        &self,
        _: u64,
        _: bool,
        result: Result<HostCallReply, HostServiceError>,
    ) -> Result<(), HostServiceError> {
        self.0.lock().unwrap().push(result);
        Ok(())
    }
}

#[test]
fn ipc_interest_change_wakes_the_owned_idle_wait_and_teardown_settles_once() {
    let (mut kernel, parent, child) = kernel(None);
    let mut pending = PendingProcessIpc::new(None);
    pending
        .install(&mut kernel, parent.pid(), child.pid(), &options())
        .unwrap();
    let mut parent = active(parent);
    let mut child = active(child);
    pending.commit(&mut parent, &mut child, "child").unwrap();
    let replies = Arc::new(Replies::default());
    let reply = DirectHostReplyHandle::new(
        HostCallIdentity {
            generation: 1,
            pid: parent.kernel_pid,
            call_id: 1,
        },
        replies.clone(),
        65536,
    )
    .unwrap();
    assert!(reply.claim().unwrap());
    let wait = Rc::new(IpcWait {
        reply: reply.clone(),
        id: 7,
        revision: Cell::new(0),
        interests: RefCell::new(Vec::new()),
        closed: Cell::new(false),
        changed: Arc::new(tokio::sync::Notify::new()),
        task: RefCell::new(None),
        wake: {
            let (sender, _) = tokio::sync::mpsc::channel(1);
            IpcWake {
                sender,
                notify: Arc::new(tokio::sync::Notify::new()),
                connection_id: "connection".into(),
                session_id: "session".into(),
                vm_id: "vm".into(),
                root: "root".into(),
                path: Vec::new(),
            }
        },
    });
    parent.ipc.wait = Some(Rc::clone(&wait));
    let mut notified = Box::pin(wait.changed.notified());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(notified.as_mut().poll(&mut cx).is_pending());
    let interests = crate::executor::host::BoundedVec::try_new(
        vec![ProcessIpcInterest {
            target: ProcessIpcTarget {
                child_id: Some(
                    crate::executor::host::BoundedString::try_new(
                        "child".into(),
                        &PayloadLimit::new("testId", 64).unwrap(),
                    )
                    .unwrap(),
                ),
                generation: parent.ipc.children["child"].generation,
            },
            writable: true,
        }],
        &PayloadLimit::new("testInterests", 1).unwrap(),
    )
    .unwrap();
    execute_operation(
        &mut kernel,
        &mut parent,
        ProcessIpcOperation::Update {
            poll_id: 7,
            expected_revision: 0,
            interests: interests.clone(),
        },
    )
    .unwrap();
    assert!(matches!(notified.as_mut().poll(&mut cx), Poll::Ready(())));
    assert_eq!(wait.revision.get(), 1);
    assert_eq!(
        error_code(
            &execute_operation(
                &mut kernel,
                &mut parent,
                ProcessIpcOperation::Update {
                    poll_id: 7,
                    expected_revision: 0,
                    interests
                }
            )
            .unwrap_err()
        ),
        "ESTALE"
    );
    parent.ipc.cancel_wait();
    parent.ipc.cancel_wait();
    assert!(wait.closed.get());
    assert!(reply.is_terminal());
    assert_eq!(replies.0.lock().unwrap().len(), 1);
}

fn child_target(child: &str, generation: u64) -> ProcessIpcTarget {
    ProcessIpcTarget {
        child_id: Some(
            crate::executor::host::BoundedString::try_new(
                child.into(),
                &PayloadLimit::new("testId", 64).unwrap(),
            )
            .unwrap(),
        ),
        generation,
    }
}

fn retirement_cycles(replace_fd: bool) {
    let (mut kernel, parent, child) = kernel(None);
    let mut parent = active(parent);
    let mut child = active(child);
    let baseline = kernel.resource_snapshot();
    let mut previous = None;
    for _ in 0..16 {
        let mut pending = PendingProcessIpc::new(None);
        pending
            .install(&mut kernel, parent.kernel_pid, child.kernel_pid, &options())
            .unwrap();
        pending.commit(&mut parent, &mut child, "child").unwrap();
        let endpoint = parent.ipc.children["child"].clone();
        let target = child_target("child", endpoint.generation);
        if let Some(old) = previous.take() {
            assert_eq!(
                error_code(
                    &execute_operation(
                        &mut kernel,
                        &mut parent,
                        ProcessIpcOperation::Close { target: old }
                    )
                    .unwrap_err()
                ),
                "ESTALE"
            );
            assert_eq!(parent.ipc.children.len(), 1);
            assert!(kernel
                .fd_stat(EXECUTION_DRIVER_NAME, parent.kernel_pid, endpoint.fd)
                .is_ok());
        }
        kernel
            .fd_close(EXECUTION_DRIVER_NAME, parent.kernel_pid, endpoint.fd)
            .unwrap();
        if replace_fd {
            let replacement = kernel
                .fd_open(
                    EXECUTION_DRIVER_NAME,
                    parent.kernel_pid,
                    "/dev/null",
                    agentos_vm_kernel::fd_table::O_RDONLY,
                    None,
                )
                .unwrap();
            if replacement != endpoint.fd {
                kernel
                    .fd_dup2(
                        EXECUTION_DRIVER_NAME,
                        parent.kernel_pid,
                        replacement,
                        endpoint.fd,
                    )
                    .unwrap();
                kernel
                    .fd_close(EXECUTION_DRIVER_NAME, parent.kernel_pid, replacement)
                    .unwrap();
            }
        }
        execute_operation(
            &mut kernel,
            &mut parent,
            ProcessIpcOperation::Close {
                target: target.clone(),
            },
        )
        .unwrap();
        assert!(
            parent.ipc.children.is_empty(),
            "generation-owned metadata must retire after ordinary close/reuse"
        );
        if replace_fd {
            assert!(
                kernel
                    .fd_stat(EXECUTION_DRIVER_NAME, parent.kernel_pid, endpoint.fd)
                    .is_ok(),
                "IPC retirement must preserve the replacement descriptor"
            );
            kernel
                .fd_close(EXECUTION_DRIVER_NAME, parent.kernel_pid, endpoint.fd)
                .unwrap();
        }
        execute_operation(
            &mut kernel,
            &mut child,
            ProcessIpcOperation::Close {
                target: ProcessIpcTarget {
                    child_id: None,
                    generation: endpoint.generation,
                },
            },
        )
        .unwrap();
        assert!(child.ipc.parent.is_none());
        let after = kernel.resource_snapshot();
        assert_eq!(after.open_fds, baseline.open_fds);
        assert_eq!(after.sockets, baseline.sockets);
        assert_eq!(after.socket_connections, baseline.socket_connections);
        previous = Some(target);
    }
}

#[test]
fn ipc_close_retires_metadata_after_repeated_ordinary_close() {
    retirement_cycles(false);
}

#[test]
fn ipc_close_retires_metadata_after_repeated_fd_replacement_without_closing_replacement() {
    retirement_cycles(true);
}

#[test]
fn ipc_metadata_admission_is_bounded_when_parent_cleanup_is_never_called() {
    let limit = 12;
    let (mut kernel, parent, child) = kernel(Some(limit));
    let mut parent = active(parent);
    let mut child = active(child);
    let baseline = kernel.resource_snapshot();
    for index in 0..=limit {
        let mut pending = PendingProcessIpc::new(None);
        pending
            .install(&mut kernel, parent.kernel_pid, child.kernel_pid, &options())
            .unwrap();
        let id = format!("child-{index}");
        let committed = pending.commit(&mut parent, &mut child, &id);
        if index == limit {
            let error = committed.unwrap_err();
            assert_eq!(error_code(&error), "EMFILE");
            assert!(error
                .to_string()
                .contains("raise limits.resources.maxOpenFds"));
            let VmError::Host(error) = error else {
                panic!("expected typed IPC admission error")
            };
            let details = error.details.expect("actionable IPC limit details");
            assert_eq!(details["configPath"], "limits.resources.maxOpenFds");
            assert_eq!(details["resource"], "ipcEndpointMetadata");
            assert_eq!(details["ownerPid"], parent.kernel_pid);
            assert_eq!(details["limit"], limit);
            assert_eq!(details["observed"], limit + 1);
            assert_eq!(
                parent.ipc.children.len(),
                limit,
                "closed descriptors must not allow unbounded IPC metadata"
            );
            assert!(
                child.ipc.parent.is_none(),
                "failed metadata admission must not publish a child endpoint"
            );
            assert_eq!(
                pending.endpoints.len(),
                2,
                "uncommitted endpoints remain owned for startup rollback"
            );
            // This fixture has no VmHandle. Retire its uncommitted descriptors
            // with the same description fence used by production rollback.
            for (pid, endpoint) in pending.endpoints.drain(..) {
                close_matching(&mut kernel, pid, &endpoint).unwrap();
            }
        } else {
            committed.unwrap();
            let endpoint = parent.ipc.children[&id].clone();
            kernel
                .fd_close(EXECUTION_DRIVER_NAME, parent.kernel_pid, endpoint.fd)
                .unwrap();
            execute_operation(
                &mut kernel,
                &mut child,
                ProcessIpcOperation::Close {
                    target: ProcessIpcTarget {
                        child_id: None,
                        generation: endpoint.generation,
                    },
                },
            )
            .unwrap();
            // Deliberately never retire the parent's metadata in this control.
            assert_eq!(parent.ipc.children.len(), index + 1);
        }
        let after = kernel.resource_snapshot();
        assert_eq!(after.open_fds, baseline.open_fds);
        assert_eq!(after.sockets, baseline.sockets);
        assert_eq!(after.socket_connections, baseline.socket_connections);
    }
}

#[derive(Clone)]
struct WarningCapture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for WarningCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn ipc_retained_metadata_warning_tracks_threshold_and_rearms_after_close() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_target(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(move || WarningCapture(Arc::clone(&sink)))
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let warning_count = || {
        String::from_utf8(captured.lock().unwrap().clone())
            .unwrap()
            .matches("IPC retained endpoint metadata is nearing")
            .count()
    };
    let (mut kernel, parent, child) = kernel(Some(12));
    let baseline = kernel.resource_snapshot();
    let mut parent = active(parent);
    let mut child = active(child);
    let mut add = |id: &str, parent: &mut ActiveProcess| {
        let mut pending = PendingProcessIpc::new(None);
        pending
            .install(&mut kernel, parent.kernel_pid, child.kernel_pid, &options())
            .unwrap();
        pending.commit(parent, &mut child, id).unwrap();
        let endpoint = parent.ipc.children[id].clone();
        // Free actual FDs while retaining the parent's metadata. The ordinary
        // FD gauge cannot warn about these retained records.
        kernel
            .fd_close(EXECUTION_DRIVER_NAME, parent.kernel_pid, endpoint.fd)
            .unwrap();
        execute_operation(
            &mut kernel,
            &mut child,
            ProcessIpcOperation::Close {
                target: ProcessIpcTarget {
                    child_id: None,
                    generation: endpoint.generation,
                },
            },
        )
        .unwrap();
        assert_eq!(kernel.resource_snapshot(), baseline);
    };
    for index in 0..9 {
        add(&format!("retained-{index}"), &mut parent);
    }
    assert_eq!(warning_count(), 0);
    add("retained-9", &mut parent);
    assert_eq!(warning_count(), 1);
    add("retained-10", &mut parent);
    assert_eq!(warning_count(), 1, "warn once while above the threshold");
    drop(add);
    for id in ["retained-9", "retained-10"] {
        let endpoint = parent.ipc.children[id].clone();
        execute_operation(
            &mut kernel,
            &mut parent,
            ProcessIpcOperation::Close {
                target: child_target(id, endpoint.generation),
            },
        )
        .unwrap();
    }
    assert_eq!(parent.ipc.metadata_count(), 9);
    assert!(
        !parent.ipc.metadata_warned,
        "falling below 80% resets the warning"
    );
    let mut pending = PendingProcessIpc::new(None);
    pending
        .install(&mut kernel, parent.kernel_pid, child.kernel_pid, &options())
        .unwrap();
    pending.commit(&mut parent, &mut child, "rearmed").unwrap();
    assert_eq!(warning_count(), 2);
    let warnings = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert!(warnings.contains("observed=10"));
    assert!(warnings.contains("limit=12"));
    assert!(warnings.contains("ipcEndpointMetadata"));
    assert!(warnings.contains("raise limits.resources.maxOpenFds"));
    let generation = child.ipc.parent.as_ref().unwrap().generation;
    execute_operation(
        &mut kernel,
        &mut parent,
        ProcessIpcOperation::Close {
            target: child_target("rearmed", generation),
        },
    )
    .unwrap();
    execute_operation(
        &mut kernel,
        &mut child,
        ProcessIpcOperation::Close {
            target: ProcessIpcTarget {
                child_id: None,
                generation,
            },
        },
    )
    .unwrap();
    assert_eq!(kernel.resource_snapshot(), baseline);
}

#[cfg(feature = "node-v8")]
#[tokio::test(flavor = "current_thread")]
async fn ipc_actual_spawn_metadata_rejection_rolls_back_root_and_nested_owners() {
    use crate::execution::child_process::child_event_claim_tests::{
        host_function_process, sidecar_with_test_vm,
    };
    tokio::task::LocalSet::new()
        .run_until(async {
            for nested in [false, true] {
                let (mut sidecar, vm_id) = sidecar_with_test_vm(64).await;
                let limit;
                let sentinel;
                {
                    let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
                    let env = vm.guest_env.clone();
                    let cwd = vm.runtime_scratch_root.clone();
                    let mut root = host_function_process(&mut vm, "IPC admission root", None)
                        .with_guest_cwd("/".into())
                        .with_env(env.clone())
                        .with_host_cwd(cwd.clone());
                    if nested {
                        let child = host_function_process(
                            &mut vm,
                            "IPC admission nested",
                            Some(root.kernel_pid),
                        )
                        .with_guest_cwd("/".into())
                        .with_env(env)
                        .with_host_cwd(cwd);
                        root.child_processes.insert("nested".into(), child);
                    }
                    let owner = if nested {
                        root.child_processes.get_mut("nested").unwrap()
                    } else {
                        &mut root
                    };
                    limit = metadata_limit(&vm.kernel);
                    let fd = vm
                        .kernel
                        .fd_open(
                            EXECUTION_DRIVER_NAME,
                            owner.kernel_pid,
                            "/dev/null",
                            agentos_vm_kernel::fd_table::O_RDONLY,
                            None,
                        )
                        .unwrap();
                    sentinel = (
                        owner.kernel_pid,
                        fd,
                        vm.kernel
                            .fd_description_identity(EXECUTION_DRIVER_NAME, owner.kernel_pid, fd)
                            .unwrap(),
                    );
                    // Seed a finite full retained-record owner. The earlier tests
                    // create such records through real close/reuse cycles; here
                    // the actual startup path must reject after descriptor setup.
                    for index in 0..limit {
                        owner.ipc.children.insert(
                            format!("retained-{index}"),
                            Endpoint {
                                fd: u32::MAX,
                                description: u64::MAX,
                                generation: 1,
                                serialization: "json".into(),
                            },
                        );
                    }
                    vm.active_processes.insert("root".into(), root);
                }
                let baseline = sidecar.vms.get(&vm_id).unwrap().kernel.resource_snapshot();
                let engines = sidecar.vms.get(&vm_id).unwrap().execution_engines.clone();
                let contexts_before = engines
                    .javascript("IPC rejection context baseline")
                    .unwrap()
                    .context_count_for_test();
                let request = crate::executor::host::ProcessLaunchRequest {
                    command: "node".into(),
                    args: vec!["-e".into(), "setInterval(()=>{},1000)".into()],
                    options: options(),
                };
                let result = tokio::time::timeout(Duration::from_secs(5), async {
                    if nested {
                        sidecar
                            .spawn_descendant_process_for_test(&vm_id, "root", &["nested"], request)
                            .await
                    } else {
                        sidecar.spawn_child_process(&vm_id, "root", request).await
                    }
                })
                .await
                .expect("finite metadata spawn rejection");
                let error = result.expect_err("full retained metadata must reject actual spawn");
                assert_eq!(error.code(), Some("EMFILE"));
                assert!(
                    error.to_string().contains("IPC retained endpoint metadata"),
                    "reject at metadata commit, not earlier FD admission: {error}"
                );
                assert!(error
                    .to_string()
                    .contains("raise limits.resources.maxOpenFds"));
                {
                    let vm = sidecar.vms.get(&vm_id).unwrap();
                    assert_eq!(
                        vm.kernel.resource_snapshot(),
                        baseline,
                        "rejected spawn must release its kernel process, FDs and sockets"
                    );
                    assert_eq!(
                        vm.kernel
                            .fd_description_identity(EXECUTION_DRIVER_NAME, sentinel.0, sentinel.1)
                            .unwrap(),
                        sentinel.2,
                        "rollback must preserve the parent's pre-existing descriptor description"
                    );
                    let root = vm.active_processes.get("root").unwrap();
                    let owner = if nested {
                        root.child_processes.get("nested").unwrap()
                    } else {
                        root
                    };
                    assert!(
                        owner.child_processes.is_empty(),
                        "rejected child cannot be published"
                    );
                    assert_eq!(
                        owner.ipc.metadata_count(),
                        limit,
                        "retain the pre-existing records only"
                    );
                }
                assert_eq!(
                    engines
                        .javascript("IPC rejection context rollback")
                        .unwrap()
                        .context_count_for_test(),
                    contexts_before
                );
                sidecar
                    .dispose_vm_internal(
                        "child-claim-connection",
                        "child-claim-session",
                        &vm_id,
                        crate::protocol::DisposeReason::Requested,
                    )
                    .await
                    .expect("dispose finite admission fixture");
            }
        })
        .await;
}
