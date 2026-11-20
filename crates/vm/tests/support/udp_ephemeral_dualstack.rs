#[test]
fn udp_ephemeral_dualstack_collector_avoids_cross_family_collisions() {
    use agentos_vm_host_interface::LocalVmHost;
    use agentos_vm_kernel::{
        command_registry::CommandDriver, kernel::KernelVmConfig, mount_table::MountTable,
        permissions::Permissions, vfs::MemoryFileSystem,
    };
    for (
        receiver_family,
        receiver_host,
        sender_family,
        sender_host,
        overlap,
        explicit_specific,
        reuse,
    ) in [
        (
            UdpFamily::Ipv6,
            "::",
            UdpFamily::Ipv4,
            "127.0.0.1",
            true,
            false,
            false,
        ),
        (
            UdpFamily::Ipv4,
            "0.0.0.0",
            UdpFamily::Ipv6,
            "::",
            true,
            false,
            false,
        ),
        (
            UdpFamily::Ipv6,
            "::1",
            UdpFamily::Ipv4,
            "127.0.0.1",
            false,
            false,
            false,
        ),
        (
            UdpFamily::Ipv4,
            "127.0.0.1",
            UdpFamily::Ipv6,
            "::1",
            false,
            false,
            false,
        ),
        (
            UdpFamily::Ipv6,
            "::1",
            UdpFamily::Ipv4,
            "127.0.0.1",
            false,
            true,
            false,
        ),
        (
            UdpFamily::Ipv4,
            "127.0.0.1",
            UdpFamily::Ipv6,
            "::1",
            false,
            true,
            false,
        ),
        (
            UdpFamily::Ipv6,
            "::",
            UdpFamily::Ipv4,
            "127.0.0.1",
            true,
            false,
            true,
        ),
        (
            UdpFamily::Ipv4,
            "0.0.0.0",
            UdpFamily::Ipv6,
            "::",
            true,
            false,
            true,
        ),
    ] {
        let driver = agentos_driver_tokio::TokioDriver::process(&Default::default()).unwrap();
        let handle = driver.handle();
        let resources = Arc::new(ResourceLedger::child(
            "udp-mapped-receive",
            [],
            Arc::clone(handle.resources()),
        ));
        let generation = handle.allocate_vm_generation().unwrap();
        let runtime = handle.scoped_for_vm(Arc::clone(&resources), generation);
        let mut config = KernelVmConfig::new("ephemeral-udp");
        config.permissions = Permissions::allow_all();
        let mut kernel = SidecarKernel::new(MountTable::new(MemoryFileSystem::new()), config);
        kernel
            .set_socket_resource_ledger(Arc::clone(&resources))
            .unwrap();
        kernel
            .register_driver(CommandDriver::new(EXECUTION_DRIVER_NAME, [WASM_COMMAND]))
            .unwrap();
        let process_handle = kernel
            .create_virtual_process(
                EXECUTION_DRIVER_NAME,
                EXECUTION_DRIVER_NAME,
                WASM_COMMAND,
                vec![WASM_COMMAND.into()],
                Default::default(),
            )
            .unwrap();
        let mut process = ActiveProcess::new(
            process_handle.pid(),
            process_handle,
            runtime.clone(),
            Default::default(),
            agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_PROCESS_EVENTS,
            GuestRuntimeKind::JavaScript,
            ActiveExecution::HostFunction(Default::default()),
        );
        let mut paths = mixed_udp_paths();
        let mut receiver = ActiveUdpSocket::new(
            &mut kernel,
            process.kernel_pid,
            receiver_family,
            Arc::clone(runtime.resources()),
            runtime.clone(),
            reactor_io_limits(&crate::limits::VmLimits::default()),
        )
        .unwrap();
        if reuse {
            kernel
                .socket_set_datagram_option(
                    EXECUTION_DRIVER_NAME,
                    process.kernel_pid,
                    receiver.kernel_socket_id.unwrap(),
                    agentos_vm_kernel::socket_table::DatagramSocketOption::ReusePort,
                    true,
                )
                .unwrap();
        }
        let receiver_addr = receiver
            .bind(
                &mut kernel,
                process.kernel_pid,
                Some(receiver_host),
                0,
                &paths,
            )
            .unwrap();
        receiver.set_fairness_identity(Some((1, generation)));
        if receiver_family == UdpFamily::Ipv6 && overlap && !reuse {
            receiver
                .set_receive_metadata_option(
                    &mut kernel,
                    process.kernel_pid,
                    RECEIVE_PKTINFO_V6,
                    true,
                )
                .unwrap();
        }
        process.udp_sockets.insert("receiver".into(), receiver);
        crate::execution::process::collect_socket_port_state(
            &kernel,
            "root",
            &process,
            &mut paths.tcp_loopback_guest_to_host_ports,
            &mut paths.http_loopback_targets,
            &mut paths.http2_loopback_targets,
            &mut paths.udp_loopback_guest_to_host_ports,
            &mut paths.udp_loopback_host_to_guest_ports,
            &mut paths.used_tcp_guest_ports,
            &mut paths.used_udp_guest_ports,
        );
        let mut sender = ActiveUdpSocket::new(
            &mut kernel,
            process.kernel_pid,
            sender_family,
            Arc::clone(runtime.resources()),
            runtime.clone(),
            reactor_io_limits(&crate::limits::VmLimits::default()),
        )
        .unwrap();
        if reuse {
            kernel
                .socket_set_datagram_option(
                    EXECUTION_DRIVER_NAME,
                    process.kernel_pid,
                    sender.kernel_socket_id.unwrap(),
                    agentos_vm_kernel::socket_table::DatagramSocketOption::ReusePort,
                    true,
                )
                .unwrap();
        }
        if receiver_family == UdpFamily::Ipv6 && overlap && !reuse {
            let bridge = SharedBridge {
                inner: Arc::new(Mutex::new(LocalVmHost::default())),
                permissions: Arc::new(Mutex::new(BTreeMap::from([(
                    "ephemeral-udp".into(),
                    crate::core::permissions::resolve_profile_permissions_policy(
                        agentos_vm_config::VmDefaultsProfile::AgentOs,
                        Some(serde_json::from_value(json!({"network":"deny"})).unwrap()),
                    ),
                )]))),
                set_vm_permissions_history: Default::default(),
                set_vm_permissions_outcomes: Default::default(),
                emit_lifecycle_outcomes: Default::default(),
            };
            let dns = VmDnsConfig::default();
            sender.set_traffic_class(false, 171).unwrap();
            let receiver = &process.udp_sockets["receiver"];
            for direct in [true, false] {
                let sent = sender
                    .send_to(ActiveUdpSendToRequest {
                        bridge: &bridge,
                        kernel: &mut kernel,
                        kernel_pid: process.kernel_pid,
                        vm_id: "ephemeral-udp",
                        dns: &dns,
                        host: "127.0.0.2",
                        port: receiver_addr.port(),
                        context: &paths,
                        contents: &[0, 255, 128],
                    })
                    .unwrap();
                assert!(matches!(
                    sent,
                    ActiveUdpSendResult::Immediate { written: 3, .. }
                ));
                let sender_port = sender.local_addr().unwrap().port();
                assert_ne!(sender_port, receiver_addr.port());
                let charge = 3 + DATAGRAM_METADATA_BYTES;
                assert_eq!(resources.usage(ResourceClass::UdpBytes).used, charge);
                let event = runtime.tokio_handle().block_on(async {
                    tokio::time::timeout(Duration::from_secs(2), async {
                        if direct {
                            let turn = receiver.acquire_fair_turn().await.unwrap();
                            receiver.consume_kernel_datagram(&mut kernel, process.kernel_pid, turn)
                        } else {
                            receiver
                                .poll(&mut kernel, process.kernel_pid, Duration::ZERO)
                                .await
                        }
                    })
                    .await
                    .expect("bounded charged kernel UDP receive")
                    .unwrap()
                    .unwrap()
                });
                let DatagramEvent::Message {
                    data,
                    remote_addr,
                    metadata,
                    ..
                } = &event
                else {
                    panic!("expected a mapped datagram");
                };
                assert_eq!(data.as_slice(), &[0, 255, 128]);
                let ipv4 = Ipv4Addr::new(127, 0, 0, 2);
                assert_eq!(
                    *remote_addr,
                    SocketAddr::new(IpAddr::V6(ipv4.to_ipv6_mapped()), sender_port)
                );
                let facts = metadata.expect("mapped packet facts");
                assert_eq!(facts.destination, IpAddr::V4(ipv4));
                assert_eq!(facts.ipv4_local_address, Some(ipv4));
                assert_eq!(
                    facts.interface,
                    agentos_vm_kernel::socket_table::GuestNetworkInterface::Loopback
                );
                assert_eq!(facts.traffic_class, 171);
                assert!(!receiver
                    .kernel_readable(&kernel, process.kernel_pid)
                    .unwrap());
                assert_eq!(resources.usage(ResourceClass::UdpBytes).used, charge);
                assert_eq!(resources.usage(ResourceClass::BufferedBytes).used, charge);
                assert_eq!(resources.usage(ResourceClass::UdpDatagrams).used, 1);
                assert_eq!(resources.usage(ResourceClass::Datagrams).used, 1);
                drop(event);
                for resource in [
                    ResourceClass::UdpBytes,
                    ResourceClass::BufferedBytes,
                    ResourceClass::UdpDatagrams,
                    ResourceClass::Datagrams,
                ] {
                    assert_eq!(resources.usage(resource).used, 0);
                }
            }
        } else {
            if overlap && !reuse {
                let error = sender
                    .bind(
                        &mut kernel,
                        process.kernel_pid,
                        Some(sender_host),
                        receiver_addr.port(),
                        &paths,
                    )
                    .unwrap_err();
                assert_eq!(host_service_error_code(&error), "EADDRINUSE");
                assert!(
                    sender.local_addr().is_none(),
                    "failed explicit collision must not commit state"
                );
                assert!(kernel
                    .socket_get(sender.kernel_socket_id.unwrap())
                    .unwrap()
                    .local_address()
                    .is_none());
            }
            let sender_addr = sender
                .bind(
                    &mut kernel,
                    process.kernel_pid,
                    Some(sender_host),
                    if explicit_specific || reuse {
                        receiver_addr.port()
                    } else {
                        0
                    },
                    &paths,
                )
                .unwrap();
            assert_eq!(
                sender_addr.port() != receiver_addr.port(),
                overlap && !reuse,
                "specific IPv6 does not reserve IPv4; wildcard does"
            );
        }
        assert!(
            paths
                .used_udp_guest_ports
                .values()
                .all(|ports| ports.iter().all(|port| *port == receiver_addr.port())),
            "only actual guest bound port enters inventory"
        );
    }
}
