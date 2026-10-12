#[test]
fn udp_receive_metadata_has_one_byte_charge_across_peek_and_final_drop() {
    let charge = 3 + DATAGRAM_METADATA_BYTES;
    let resources = ResourceLedger::root(
        "udp-metadata-charge",
        [
            (
                ResourceClass::BufferedBytes,
                ResourceLimit::new(charge, "test.bufferedBytes"),
            ),
            (
                ResourceClass::UdpBytes,
                ResourceLimit::new(charge, "test.udpBytes"),
            ),
            (
                ResourceClass::Datagrams,
                ResourceLimit::new(1, "test.datagrams"),
            ),
            (
                ResourceClass::UdpDatagrams,
                ResourceLimit::new(1, "test.udpDatagrams"),
            ),
        ],
    );
    let (data, bytes, datagrams, udp_bytes, udp_datagrams) =
        reserve_udp_receive_buffer(&resources, 3).unwrap();
    assert_eq!(resources.usage(ResourceClass::BufferedBytes).used, charge);
    assert_eq!(resources.usage(ResourceClass::UdpBytes).used, charge);
    assert!(reserve_udp_receive_buffer(&resources, 0).is_err());
    let packet = DatagramEvent::Message {
        data,
        remote_addr: "127.0.0.1:1".parse().unwrap(),
        metadata: Some(agentos_vm_kernel::socket_table::DatagramMetadata {
            destination: "127.0.0.2".parse().unwrap(),
            ipv4_local_address: Some(Ipv4Addr::new(127, 0, 0, 2)),
            interface: agentos_vm_kernel::socket_table::GuestNetworkInterface::Loopback,
            traffic_class: 171,
        }),
        _byte_reservation: SharedReservation::new(bytes),
        _datagram_reservation: SharedReservation::new(datagrams),
        _udp_byte_reservation: SharedReservation::new(udp_bytes),
        _udp_datagram_reservation: SharedReservation::new(udp_datagrams),
    };
    let peeked = packet.clone();
    assert_eq!(
        resources.usage(ResourceClass::UdpBytes).used,
        charge,
        "peek must not reserve again"
    );
    drop(packet);
    assert_eq!(
        resources.usage(ResourceClass::UdpBytes).used,
        charge,
        "peek retains ownership"
    );
    drop(peeked);
    assert!(
        resources.is_zero(),
        "final owner releases metadata and payload together"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn native_udp_metadata_projects_interfaces_and_records_admission_availability() {
    let process_runtime =
        agentos_driver_tokio::TokioDriver::process(&agentos_driver_tokio::DriverConfig::default())
            .unwrap();
    let handle = process_runtime.handle();
    for (ipv6, mapped) in [(false, false), (true, false), (true, true)] {
        let receiver = UdpSocket::bind(if ipv6 { "[::]:0" } else { "0.0.0.0:0" }).unwrap();
        let port = receiver.local_addr().unwrap().port();
        receiver.set_nonblocking(true).unwrap();
        let mut capture = NativeUdpMetadataCapture::configure(&receiver, ipv6).unwrap();
        assert_eq!(
            capture.supported & (RECEIVE_PKTINFO_V4 | RECEIVE_TOS_V4),
            RECEIVE_PKTINFO_V4 | RECEIVE_TOS_V4
        );
        if ipv6 {
            assert_eq!(
                capture.supported & (RECEIVE_PKTINFO_V6 | RECEIVE_TCLASS_V6),
                RECEIVE_PKTINFO_V6 | RECEIVE_TCLASS_V6
            );
        }
        let sender = UdpSocket::bind(if ipv6 && !mapped {
            "[::1]:0"
        } else {
            "127.0.0.1:0"
        })
        .unwrap();
        if ipv6 && !mapped {
            SockRef::from(&sender).set_tclass_v6(171).unwrap();
        } else {
            SockRef::from(&sender).set_tos_v4(171).unwrap();
        }
        let destination = if ipv6 && !mapped {
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port)
        } else {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)), port)
        };
        let socket = {
            let _entered = handle.tokio_handle().enter();
            tokio::net::UdpSocket::from_std(receiver).unwrap()
        };
        handle.tokio_handle().block_on(async {
            let options: &[u8] = if mapped {
                &[
                    0,
                    RECEIVE_PKTINFO_V4,
                    RECEIVE_TOS_V4,
                    RECEIVE_PKTINFO_V6,
                    RECEIVE_TCLASS_V6,
                    15,
                ]
            } else {
                &[0, 15]
            };
            for &options in options {
                sender.send_to(&[0, 255, 128], destination).unwrap();
                let (length, _, metadata) = tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        socket.readable().await.unwrap();
                        let mut data = [0u8; 3];
                        match capture.receive(&socket, &mut data, options) {
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                continue
                            }
                            result => {
                                assert_eq!(data, [0, 255, 128]);
                                break result.unwrap();
                            }
                        }
                    }
                })
                .await
                .unwrap();
                assert_eq!(length, 3);
                let metadata = metadata.unwrap();
                assert_eq!(metadata.destination, destination.ip());
                assert_eq!(
                    metadata.interface,
                    agentos_vm_kernel::socket_table::GuestNetworkInterface::Loopback
                );
                assert_eq!(metadata.traffic_class, 171);
                if !ipv6 || mapped {
                    assert_eq!(
                        metadata.ipv4_local_address,
                        if options & (RECEIVE_PKTINFO_V4 | RECEIVE_PKTINFO_V6) == 0 {
                            None
                        } else {
                            Some(Ipv4Addr::new(127, 0, 0, 2))
                        }
                    );
                }
            }
        });
    }
}

#[test]
fn udp_payload_capacity_keeps_ceiling_and_accounts_metadata_only_in_queue_limits() {
    let limits = reactor_io_limits(&crate::limits::VmLimits::default());
    let unlimited = ResourceLedger::root("udp-unlimited", []);
    assert_eq!(
        udp_receive_capacity(&unlimited, limits).unwrap(),
        UDP_MAX_DATAGRAM_BYTES
    );
    for queue_bytes in [
        DATAGRAM_METADATA_BYTES - 1,
        DATAGRAM_METADATA_BYTES,
        DATAGRAM_METADATA_BYTES + 3,
        UDP_MAX_DATAGRAM_BYTES + DATAGRAM_METADATA_BYTES,
    ] {
        let resources = ResourceLedger::root(
            "udp-exact-byte-budget",
            [
                (
                    ResourceClass::BufferedBytes,
                    ResourceLimit::new(queue_bytes, "test.bufferedBytes"),
                ),
                (
                    ResourceClass::UdpBytes,
                    ResourceLimit::new(queue_bytes, "test.udpBytes"),
                ),
            ],
        );
        if queue_bytes < DATAGRAM_METADATA_BYTES {
            assert!(udp_receive_capacity(&resources, limits).is_err());
        } else {
            assert_eq!(
                udp_receive_capacity(&resources, limits).unwrap(),
                (queue_bytes - DATAGRAM_METADATA_BYTES).min(UDP_MAX_DATAGRAM_BYTES)
            );
            if queue_bytes == DATAGRAM_METADATA_BYTES {
                let zero = reserve_udp_receive_buffer(&resources, 0).unwrap();
                assert_eq!(
                    resources.usage(ResourceClass::UdpBytes).used,
                    DATAGRAM_METADATA_BYTES
                );
                drop(zero);
                assert!(resources.is_zero());
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn failed_udp_metadata_owner_admission_preserves_state_and_transferred_options() {
    use agentos_vm_kernel::{command_registry::CommandDriver, kernel::KernelVmConfig};
    use agentos_vm_kernel::{mount_table::MountTable, vfs::MemoryFileSystem};
    let process_runtime = agentos_driver_tokio::TokioDriver::process(&Default::default()).unwrap();
    let resources = Arc::new(ResourceLedger::child(
        "udp-task-admission-red",
        [(ResourceClass::Tasks, ResourceLimit::new(0, "test.maxTasks"))],
        Arc::clone(process_runtime.handle().resources()),
    ));
    let runtime = process_runtime
        .handle()
        .scoped_for_vm(Arc::clone(&resources), 71001);
    let mut config = KernelVmConfig::new("udp-metadata-admission");
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
    let mut socket = ActiveUdpSocket::new(
        &mut kernel,
        process.pid(),
        UdpFamily::Ipv4,
        Arc::clone(&resources),
        runtime.clone(),
        reactor_io_limits(&crate::limits::VmLimits::default()),
    )
    .unwrap();
    let mut alias = socket.clone_for_fd_transfer().unwrap();
    alias
        .set_receive_metadata_option(&mut kernel, process.pid(), RECEIVE_PKTINFO_V4, true)
        .unwrap();
    assert!(socket.receive_metadata_option(RECEIVE_PKTINFO_V4).unwrap());
    assert!(
        socket.local_addr().is_none(),
        "receive option must not bind"
    );
    assert!(socket.connection_snapshot().native_commands.is_none());
    drop(alias);
    let context = SocketPathContext {
        sandbox_root: Default::default(),
        unix_abstract_namespace: [0; 32],
        unix_socket_host_dir: Default::default(),
        unix_bound_addresses: Default::default(),
        host_net_transfer_descriptions: Default::default(),
        mounts: Vec::new(),
        listen_policy: Default::default(),
        loopback_exempt_ports: Default::default(),
        tcp_loopback_guest_to_host_ports: Default::default(),
        http_loopback_targets: Default::default(),
        http2_loopback_targets: Default::default(),
        udp_loopback_guest_to_host_ports: Default::default(),
        udp_loopback_host_to_guest_ports: Default::default(),
        used_tcp_guest_ports: Default::default(),
        used_udp_guest_ports: Default::default(),
    };
    socket
        .bind(
            &mut kernel,
            process.pid(),
            Some("127.0.0.1"),
            51005,
            &context,
        )
        .unwrap();
    let before = socket.connection_snapshot();
    assert!(before.receive_metadata_supported.is_none());
    let error = {
        let _entered = runtime.tokio_handle().enter();
        socket.ensure_native_owner().unwrap_err()
    };
    assert!(
        error.to_string().contains("test.maxTasks"),
        "must fail actual bounded task admission: {error}"
    );
    let after = socket.connection_snapshot();
    assert!(after.receive_metadata_supported.is_none());
    assert!(after.native_commands.is_none());
    assert!(after.native_local_addr.is_none());
    assert_eq!(after.guest_local_addr, before.guest_local_addr);
    assert_eq!(
        after.receive_metadata_options.snapshot(),
        before.receive_metadata_options.snapshot()
    );
    assert!(socket.receive_metadata_option(RECEIVE_PKTINFO_V4).unwrap());
    socket.close(&mut kernel, process.pid());
    assert!(
        resources.is_zero(),
        "failed owner admission must roll back all permits"
    );
}

#[cfg(not(target_os = "linux"))]
#[test]
fn native_udp_without_metadata_support_preserves_ordinary_datagrams() {
    let runtime =
        agentos_driver_tokio::TokioDriver::process(&agentos_driver_tokio::DriverConfig::default())
            .unwrap();
    let handle = runtime.handle();
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    let destination = receiver.local_addr().unwrap();
    receiver.set_nonblocking(true).unwrap();
    let mut capture = NativeUdpMetadataCapture::configure(&receiver, false).unwrap();
    assert_eq!(capture.supported, 0);
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    let source = sender.local_addr().unwrap();
    let socket = {
        let _entered = handle.tokio_handle().enter();
        tokio::net::UdpSocket::from_std(receiver).unwrap()
    };
    handle.tokio_handle().block_on(async {
        for payload in [b"".as_slice(), &[0, 255, 128]] {
            sender.send_to(payload, destination).unwrap();
            let mut data = [0u8; 3];
            let (length, remote, metadata) = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    socket.readable().await.unwrap();
                    match capture.receive(&socket, &mut data, 0) {
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                        result => break result.unwrap(),
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(&data[..length], payload);
            assert_eq!(remote, source);
            assert!(metadata.is_none());
        }
    });
}
