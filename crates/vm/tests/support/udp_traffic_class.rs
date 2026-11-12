#[test]
fn udp_traffic_class_unbound_and_transfer_alias_state() {
    use agentos_vm_kernel::{command_registry::CommandDriver, kernel::KernelVmConfig};
    use agentos_vm_kernel::{mount_table::MountTable, vfs::MemoryFileSystem};
    let process_runtime = agentos_driver_tokio::TokioDriver::process(&Default::default()).unwrap();
    let runtime = process_runtime.handle();
    let resources = Arc::clone(runtime.resources());
    let mut config = KernelVmConfig::new("udp-options");
    config.permissions = agentos_vm_kernel::permissions::Permissions::allow_all();
    let mut kernel = SidecarKernel::new(MountTable::new(MemoryFileSystem::new()), config);
    kernel
        .register_driver(CommandDriver::new(EXECUTION_DRIVER_NAME, [WASM_COMMAND]))
        .unwrap();
    let process = kernel
        .create_virtual_process(
            EXECUTION_DRIVER_NAME,
            EXECUTION_DRIVER_NAME,
            WASM_COMMAND,
            vec![WASM_COMMAND.to_owned()],
            Default::default(),
        )
        .unwrap();
    for family in [UdpFamily::Ipv4, UdpFamily::Ipv6] {
        let mut socket = ActiveUdpSocket::new(
            &mut kernel,
            process.pid(),
            family,
            Arc::clone(&resources),
            runtime.clone(),
            reactor_io_limits(&crate::limits::VmLimits::default()),
        )
        .unwrap();
        let mut alias = socket.clone_for_fd_transfer().unwrap();
        socket.set_traffic_class(false, 257).unwrap();
        assert_eq!(alias.traffic_class(false).unwrap(), 1);
        assert!(socket.local_addr().is_none(), "unbound set must not bind");
        assert!(socket.connection_snapshot().native_commands.is_none());
        if family == UdpFamily::Ipv6 {
            alias.set_traffic_class(true, 255).unwrap();
            assert_eq!(socket.traffic_class(true).unwrap(), 255);
            alias.set_traffic_class(true, -1).unwrap();
            assert_eq!(socket.traffic_class(true).unwrap(), 0);
            for invalid in [-2, 256] {
                assert!(alias.set_traffic_class(true, invalid).is_err());
            }
            assert_eq!(socket.traffic_class(true).unwrap(), 0);
        } else {
            assert!(alias.set_traffic_class(true, 0).is_err());
        }
        drop(socket);
        assert_eq!(alias.traffic_class(false).unwrap(), 1);
        alias.close(&mut kernel, process.pid());
    }
}

#[test]
fn udp_traffic_class_native_commit_and_error_atomicity() {
    for family in [UdpFamily::Ipv4, UdpFamily::Ipv6] {
        let process_runtime = agentos_driver_tokio::TokioDriver::process(
            &agentos_driver_tokio::DriverConfig::default(),
        )
        .expect("create UDP owner test runtime");
        let resources = Arc::new(ResourceLedger::child(
            "udp-owner-test",
            [
                (
                    ResourceClass::BufferedBytes,
                    ResourceLimit::new(64, "test.maxBufferedBytes"),
                ),
                (
                    ResourceClass::Datagrams,
                    ResourceLimit::new(2, "test.maxDatagrams"),
                ),
                (
                    ResourceClass::UdpBytes,
                    ResourceLimit::new(64, "test.maxUdpBytes"),
                ),
                (
                    ResourceClass::UdpDatagrams,
                    ResourceLimit::new(2, "test.maxUdpDatagrams"),
                ),
            ],
            Arc::clone(process_runtime.handle().resources()),
        ));
        let runtime = process_runtime
            .handle()
            .scoped_for_vm(Arc::clone(&resources), 7001);
        let socket = UdpSocket::bind(if family == UdpFamily::Ipv4 {
            "127.0.0.1:0"
        } else {
            "[::1]:0"
        })
        .expect("bind owner socket");
        let observed = socket.try_clone().unwrap();
        let read_event_notify = Arc::new(tokio::sync::Notify::new());
        let wake_pending = Arc::new(AtomicBool::new(false));
        let fairness_identity = Arc::new(OnceLock::new());
        fairness_identity
            .set((8001, 7001))
            .expect("commit test fairness identity");
        let fairness_identity_committed = Arc::new(tokio::sync::Notify::new());
        let mut limits = reactor_io_limits(&crate::limits::VmLimits::default());
        limits.byte_quantum = 64;
        let commands = {
            let _runtime_guard = runtime.tokio_handle().enter();
            spawn_native_udp_owner(
                &runtime,
                socket,
                NativeUdpOwnerRegistration {
                    receive_options: Default::default(),
                    transition_error: Arc::new(Mutex::new(None)),
                    native_receive_enabled: Arc::new(AtomicBool::new(true)),
                    family,
                    resources: Arc::clone(&resources),
                    limits,
                    fairness_identity: Arc::clone(&fairness_identity),
                    fairness_identity_committed: Arc::clone(&fairness_identity_committed),
                    event_pusher: SocketReadinessSubscribers::new(&resources),
                    read_event_notify: Arc::clone(&read_event_notify),
                    wake_pending: Arc::clone(&wake_pending),
                },
            )
            .expect("spawn UDP owner")
        };
        let connection = Arc::new(Mutex::new(crate::state::ActiveUdpConnectionState::default()));
        let poll_handle = ActiveUdpPollHandle {
            transition_error: Arc::new(Mutex::new(None)),
            connection: Arc::new(Mutex::new(crate::state::ActiveUdpConnectionState {
                native_commands: Some(commands.clone()),
                ..Default::default()
            })),
            resources: Arc::clone(&resources),
            runtime_context: runtime.clone(),
            reactor_limits: limits,
            fairness_identity,
            fairness_identity_committed,
            read_event_notify: Arc::clone(&read_event_notify),
            pending_datagram: Arc::new(Mutex::new(None)),
        };
        runtime.tokio_handle().block_on(async {
            for (ipv6, value, expected_error) in
                [(false, 42, false), (true, 43, family == UdpFamily::Ipv4)]
            {
                let (completion, receiver) = tokio::sync::oneshot::channel();
                commands
                    .try_send(NativeUdpCommand::SetTrafficClass {
                        _command_reservation: reserve_udp_command(&resources).unwrap(),
                        ipv6,
                        value,
                        connection: Arc::clone(&connection),
                        completion,
                    })
                    .unwrap();
                let result = tokio::time::timeout(Duration::from_secs(2), receiver)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(result.is_err(), expected_error);
                let state = connection.lock().unwrap();
                assert_eq!(state.ip_tos, 42);
                assert_eq!(
                    state.ipv6_traffic_class,
                    if ipv6 && !expected_error { 43 } else { 0 },
                    "native option state must commit only on success"
                );
                if family == UdpFamily::Ipv6 {
                    assert_eq!(
                        SockRef::from(&observed).tclass_v6().unwrap(),
                        u32::from(state.ipv6_traffic_class)
                    );
                }
                assert_eq!(SockRef::from(&observed).tos_v4().unwrap(), 42);
            }
        });
        drop(observed);
        drop(poll_handle);
        drop(commands);
        runtime.tokio_handle().block_on(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                while !resources.is_zero() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        });
    }
}
