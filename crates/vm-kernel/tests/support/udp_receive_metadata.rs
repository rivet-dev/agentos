#[test]
fn udp_metadata_wrappers_preserve_admission_state_and_reject_foreign_owners() {
    use agentos_vm_kernel::socket_table::{
        DatagramReceiveOptions, GuestNetworkInterface, DATAGRAM_METADATA_BYTES,
    };
    use std::net::{IpAddr, Ipv4Addr};
    let mut kernel = new_kernel("udp-pktinfo-owner");
    let sender = spawn_shell(&mut kernel);
    let receiver = spawn_shell(&mut kernel);
    let tx = kernel
        .socket_create("shell", sender.pid(), SocketSpec::udp())
        .unwrap();
    let rx = kernel
        .socket_create("shell", receiver.pid(), SocketSpec::udp())
        .unwrap();
    let options = kernel
        .socket_datagram_receive_options("shell", receiver.pid(), rx)
        .unwrap();
    assert_eq!(options.snapshot(), 0);
    assert!(kernel
        .socket_set_datagram_receive_option("shell", sender.pid(), rx, 1, true)
        .is_err());
    assert!(kernel
        .socket_set_datagram_receive_option("foreign", receiver.pid(), rx, 1, true)
        .is_err());
    assert!(kernel
        .socket_datagram_receive_options("shell", sender.pid(), rx)
        .is_err());
    assert_eq!(options.snapshot(), 0, "rejected updates are atomic");
    kernel
        .socket_bind_inet(
            "shell",
            sender.pid(),
            tx,
            InetSocketAddress::new("127.0.0.1", 51001),
        )
        .unwrap();
    kernel
        .socket_bind_inet(
            "shell",
            receiver.pid(),
            rx,
            InetSocketAddress::new("0.0.0.0", 51002),
        )
        .unwrap();
    for enabled_at_admission in [false, true] {
        kernel
            .socket_set_datagram_receive_option(
                "shell",
                receiver.pid(),
                rx,
                DatagramReceiveOptions::PACKET_INFO_V4,
                enabled_at_admission,
            )
            .unwrap();
        assert_eq!(
            options.snapshot() & DatagramReceiveOptions::PACKET_INFO_V4 != 0,
            enabled_at_admission,
            "same canonical owner"
        );
        kernel
            .socket_send_to_inet_loopback_with_traffic_class(
                "shell",
                sender.pid(),
                tx,
                InetSocketAddress::new("127.0.0.2", 51002),
                b"abc",
                Some(171),
            )
            .unwrap();
        assert_eq!(
            kernel.resource_snapshot().socket_buffered_bytes,
            3 + DATAGRAM_METADATA_BYTES
        );
        // Toggle off/on after enqueue: packet facts must retain admission availability.
        kernel
            .socket_set_datagram_receive_option("shell", receiver.pid(), rx, 1, false)
            .unwrap();
        kernel
            .socket_set_datagram_receive_option("shell", receiver.pid(), rx, 1, true)
            .unwrap();
        let packet = kernel
            .socket_recv_datagram("shell", receiver.pid(), rx, 3)
            .unwrap()
            .unwrap();
        assert_eq!(packet.payload(), b"abc");
        let metadata = packet.metadata();
        assert_eq!(
            metadata.destination,
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2))
        );
        assert_eq!(metadata.interface, GuestNetworkInterface::Loopback);
        assert_eq!(metadata.traffic_class, 171);
        assert_eq!(
            metadata.ipv4_local_address,
            if enabled_at_admission {
                Some(Ipv4Addr::new(127, 0, 0, 2))
            } else {
                None
            }
        );
        assert_eq!(kernel.resource_snapshot().socket_buffered_bytes, 0);
    }
    assert!(kernel
        .socket_set_datagram_receive_option("shell", receiver.pid(), rx, 3, true)
        .is_err());
    assert_eq!(options.snapshot(), 1);
}

#[test]
fn udp_metadata_queue_budget_rejects_before_mutation() {
    use agentos_vm_kernel::socket_table::DATAGRAM_METADATA_BYTES;
    let mut config = KernelVmConfig::new("udp-metadata-byte-budget");
    config.permissions = Permissions::allow_all();
    config.resources.max_socket_buffered_bytes = Some(DATAGRAM_METADATA_BYTES + 3);
    let mut kernel = KernelVm::new(MemoryFileSystem::new(), config);
    kernel
        .register_driver(CommandDriver::new("shell", ["sh"]))
        .unwrap();
    let sender = spawn_shell(&mut kernel);
    let receiver = spawn_shell(&mut kernel);
    let tx = kernel
        .socket_create("shell", sender.pid(), SocketSpec::udp())
        .unwrap();
    let rx = kernel
        .socket_create("shell", receiver.pid(), SocketSpec::udp())
        .unwrap();
    kernel
        .socket_bind_inet(
            "shell",
            sender.pid(),
            tx,
            InetSocketAddress::new("127.0.0.1", 51003),
        )
        .unwrap();
    kernel
        .socket_bind_inet(
            "shell",
            receiver.pid(),
            rx,
            InetSocketAddress::new("127.0.0.1", 51004),
        )
        .unwrap();
    kernel
        .socket_send_to_inet_loopback(
            "shell",
            sender.pid(),
            tx,
            InetSocketAddress::new("127.0.0.1", 51004),
            b"abc",
        )
        .unwrap();
    assert!(kernel
        .socket_send_to_inet_loopback(
            "shell",
            sender.pid(),
            tx,
            InetSocketAddress::new("127.0.0.1", 51004),
            b""
        )
        .is_err());
    assert_eq!(kernel.socket_get(rx).unwrap().queued_datagrams(), 1);
    assert_eq!(
        kernel.resource_snapshot().socket_buffered_bytes,
        DATAGRAM_METADATA_BYTES + 3
    );
    kernel
        .socket_recv_datagram("shell", receiver.pid(), rx, 3)
        .unwrap();
    assert_eq!(kernel.resource_snapshot().socket_buffered_bytes, 0);
}

#[test]
fn udp_mapped_metadata_retains_packet_facts_for_either_packet_info_option() {
    use agentos_vm_kernel::socket_table::{DatagramReceiveOptions, SocketDomain, SocketType};
    use std::net::{IpAddr, Ipv4Addr};
    let mut kernel = new_kernel("udp-mapped-packet-info");
    let process = spawn_shell(&mut kernel);
    let tx = kernel
        .socket_create("shell", process.pid(), SocketSpec::udp())
        .unwrap();
    let rx = kernel
        .socket_create(
            "shell",
            process.pid(),
            SocketSpec::new(SocketDomain::Inet6, SocketType::Datagram),
        )
        .unwrap();
    kernel
        .socket_bind_inet(
            "shell",
            process.pid(),
            tx,
            InetSocketAddress::new("127.0.0.1", 51005),
        )
        .unwrap();
    kernel
        .socket_bind_inet(
            "shell",
            process.pid(),
            rx,
            InetSocketAddress::new("::ffff:127.0.0.2", 51006),
        )
        .unwrap();
    for mask in 0..=15 {
        for bit in [1, 2, 4, 8] {
            kernel
                .socket_set_datagram_receive_option(
                    "shell",
                    process.pid(),
                    rx,
                    bit,
                    mask & bit != 0,
                )
                .unwrap();
        }
        kernel
            .socket_send_to_inet_loopback(
                "shell",
                process.pid(),
                tx,
                InetSocketAddress::new("127.0.0.2", 51006),
                b"mapped",
            )
            .unwrap();
        // Enabling both options after admission must not invent earlier packet facts.
        for bit in [1, 4] {
            kernel
                .socket_set_datagram_receive_option("shell", process.pid(), rx, bit, true)
                .unwrap();
        }
        let packet = kernel
            .socket_recv_datagram("shell", process.pid(), rx, 6)
            .unwrap()
            .unwrap();
        assert_eq!(packet.payload(), b"mapped");
        let destination = Ipv4Addr::new(127, 0, 0, 2);
        assert_eq!(packet.metadata().destination, IpAddr::V4(destination));
        let retained = mask
            & (DatagramReceiveOptions::PACKET_INFO_V4 | DatagramReceiveOptions::PACKET_INFO_V6)
            != 0;
        assert_eq!(
            packet.metadata().ipv4_local_address,
            retained.then_some(destination),
            "admission mask {mask}"
        );
        assert_eq!(kernel.resource_snapshot().socket_buffered_bytes, 0);
    }
}

#[test]
fn udp_ipv4_loopback_reaches_ipv6_wildcard_receiver() {
    use agentos_vm_kernel::socket_table::{SocketDomain, SocketType};
    let mut kernel = new_kernel("udp-dual-stack-wildcard");
    let process = spawn_shell(&mut kernel);
    let tx = kernel
        .socket_create("shell", process.pid(), SocketSpec::udp())
        .unwrap();
    let rx = kernel
        .socket_create(
            "shell",
            process.pid(),
            SocketSpec::new(SocketDomain::Inet6, SocketType::Datagram),
        )
        .unwrap();
    kernel
        .socket_bind_inet(
            "shell",
            process.pid(),
            tx,
            InetSocketAddress::new("127.0.0.1", 51007),
        )
        .unwrap();
    kernel
        .socket_bind_inet(
            "shell",
            process.pid(),
            rx,
            InetSocketAddress::new("::", 51008),
        )
        .unwrap();
    kernel
        .socket_send_to_inet_loopback(
            "shell",
            process.pid(),
            tx,
            InetSocketAddress::new("127.0.0.2", 51008),
            b"dual",
        )
        .unwrap();
    let packet = kernel
        .socket_recv_datagram("shell", process.pid(), rx, 4)
        .unwrap()
        .unwrap();
    assert_eq!(packet.payload(), b"dual");
    assert_eq!(
        packet.source_address(),
        Some(&InetSocketAddress::new("127.0.0.1", 51007))
    );
    assert_eq!(packet.metadata().destination.to_string(), "127.0.0.2");
    assert_eq!(kernel.resource_snapshot().socket_buffered_bytes, 0);
}

fn bind_udp_dual_stack_control(
    kernel: &mut KernelVm<MemoryFileSystem>,
    process: &KernelProcessHandle,
    domain: agentos_vm_kernel::socket_table::SocketDomain,
    host: &str,
    port: u16,
    reuse: bool,
) -> agentos_vm_kernel::socket_table::SocketId {
    let socket = kernel
        .socket_create(
            "shell",
            process.pid(),
            SocketSpec::new(
                domain,
                agentos_vm_kernel::socket_table::SocketType::Datagram,
            ),
        )
        .unwrap();
    if reuse {
        kernel
            .socket_set_datagram_option(
                "shell",
                process.pid(),
                socket,
                DatagramSocketOption::ReusePort,
                true,
            )
            .unwrap();
    }
    kernel
        .socket_bind_inet(
            "shell",
            process.pid(),
            socket,
            InetSocketAddress::new(host, port),
        )
        .unwrap();
    socket
}

#[test]
fn udp_dual_stack_fallback_preserves_ipv4_exact_and_wildcard_priority() {
    use agentos_vm_kernel::socket_table::SocketDomain;
    let mut kernel = new_kernel("udp-dual-stack-priority");
    let process = spawn_shell(&mut kernel);
    let tx = bind_udp_dual_stack_control(
        &mut kernel,
        &process,
        SocketDomain::Inet,
        "127.0.0.1",
        51201,
        false,
    );
    let mapped_tx = bind_udp_dual_stack_control(
        &mut kernel,
        &process,
        SocketDomain::Inet6,
        "::",
        51213,
        false,
    );
    // Bind IPv6 first so priority cannot depend on the lowest socket ID.
    let v6 = bind_udp_dual_stack_control(
        &mut kernel,
        &process,
        SocketDomain::Inet6,
        "::",
        51202,
        true,
    );
    let v4 = bind_udp_dual_stack_control(
        &mut kernel,
        &process,
        SocketDomain::Inet,
        "0.0.0.0",
        51202,
        true,
    );
    let exact = bind_udp_dual_stack_control(
        &mut kernel,
        &process,
        SocketDomain::Inet,
        "127.0.0.2",
        51202,
        true,
    );
    for (sender, winner, remaining, destination) in [
        (tx, exact, vec![v4, v6], "127.0.0.2"),
        (tx, v4, vec![v6], "127.0.0.2"),
        (mapped_tx, v6, vec![], "::ffff:127.0.0.2"),
    ] {
        kernel
            .socket_send_to_inet_loopback(
                "shell",
                process.pid(),
                sender,
                InetSocketAddress::new(destination, 51202),
                b"priority",
            )
            .unwrap();
        let packet = kernel
            .socket_recv_datagram("shell", process.pid(), winner, 8)
            .unwrap()
            .unwrap();
        assert_eq!(packet.payload(), b"priority");
        assert_eq!(packet.metadata().destination.to_string(), "127.0.0.2");
        for other in remaining {
            assert_eq!(
                kernel
                    .socket_recv_datagram("shell", process.pid(), other, 8)
                    .unwrap_err()
                    .code(),
                "EAGAIN"
            );
        }
        assert_eq!(kernel.resource_snapshot().socket_buffered_bytes, 0);
        kernel.socket_close("shell", process.pid(), winner).unwrap();
    }
    let error = kernel
        .socket_send_to_inet_loopback(
            "shell",
            process.pid(),
            tx,
            InetSocketAddress::new("127.0.0.2", 51202),
            b"closed",
        )
        .unwrap_err();
    assert_eq!(error.code(), "EPERM");
}

#[test]
fn udp_dual_stack_fallback_does_not_match_ipv6_specific_or_remote_addresses() {
    use agentos_vm_kernel::socket_table::SocketDomain;
    let mut kernel = new_kernel("udp-dual-stack-negative-address");
    let process = spawn_shell(&mut kernel);
    let tx = bind_udp_dual_stack_control(
        &mut kernel,
        &process,
        SocketDomain::Inet,
        "127.0.0.1",
        51203,
        false,
    );
    let specific = bind_udp_dual_stack_control(
        &mut kernel,
        &process,
        SocketDomain::Inet6,
        "::1",
        51204,
        false,
    );
    let wildcard = bind_udp_dual_stack_control(
        &mut kernel,
        &process,
        SocketDomain::Inet6,
        "::",
        51205,
        false,
    );
    for (host, port) in [
        ("127.0.0.2", 51204),
        ("127.0.0.2", 51206),
        ("192.0.2.1", 51205),
    ] {
        let error = kernel
            .socket_send_to_inet_loopback(
                "shell",
                process.pid(),
                tx,
                InetSocketAddress::new(host, port),
                b"denied",
            )
            .unwrap_err();
        assert_eq!(error.code(), "EPERM");
    }
    for rx in [specific, wildcard] {
        assert_eq!(
            kernel
                .socket_recv_datagram("shell", process.pid(), rx, 8)
                .unwrap_err()
                .code(),
            "EAGAIN"
        );
    }
    assert_eq!(kernel.resource_snapshot().socket_buffered_bytes, 0);
}

#[test]
fn udp_dual_stack_fallback_keeps_driver_socket_and_network_permissions() {
    use agentos_vm_kernel::socket_table::SocketDomain;
    let mut kernel = new_kernel("udp-dual-stack-policy");
    let sender = spawn_shell(&mut kernel);
    let other = spawn_shell(&mut kernel);
    let tx = bind_udp_dual_stack_control(
        &mut kernel,
        &sender,
        SocketDomain::Inet,
        "127.0.0.1",
        51207,
        false,
    );
    let rx =
        bind_udp_dual_stack_control(&mut kernel, &other, SocketDomain::Inet6, "::", 51208, false);
    let target = InetSocketAddress::new("127.0.0.2", 51208);
    let foreign = kernel
        .socket_send_to_inet_loopback("foreign", sender.pid(), tx, target.clone(), b"denied")
        .unwrap_err();
    assert_eq!(foreign.code(), "EPERM");
    let not_owner = kernel
        .socket_send_to_inet_loopback("shell", other.pid(), tx, target.clone(), b"denied")
        .unwrap_err();
    assert_eq!(not_owner.code(), "EPERM");
    kernel.set_permissions(Permissions::default());
    let policy = kernel
        .socket_send_to_inet_loopback("shell", sender.pid(), tx, target, b"denied")
        .unwrap_err();
    assert_eq!(policy.code(), "EACCES");
    assert_eq!(
        kernel
            .socket_recv_datagram("shell", other.pid(), rx, 8)
            .unwrap_err()
            .code(),
        "EAGAIN"
    );
    assert_eq!(kernel.resource_snapshot().socket_buffered_bytes, 0);
}

#[test]
fn udp_dual_stack_fallback_never_routes_to_another_vm() {
    use agentos_vm_kernel::socket_table::SocketDomain;
    let mut first = new_kernel("udp-dual-stack-first");
    let mut second = new_kernel("udp-dual-stack-second");
    let sender = spawn_shell(&mut first);
    let receiver = spawn_shell(&mut second);
    let tx = bind_udp_dual_stack_control(
        &mut first,
        &sender,
        SocketDomain::Inet,
        "127.0.0.1",
        51209,
        false,
    );
    let rx = bind_udp_dual_stack_control(
        &mut second,
        &receiver,
        SocketDomain::Inet6,
        "::",
        51210,
        false,
    );
    let error = first
        .socket_send_to_inet_loopback(
            "shell",
            sender.pid(),
            tx,
            InetSocketAddress::new("127.0.0.2", 51210),
            b"denied",
        )
        .unwrap_err();
    assert_eq!(error.code(), "EPERM");
    assert_eq!(
        second
            .socket_recv_datagram("shell", receiver.pid(), rx, 8)
            .unwrap_err()
            .code(),
        "EAGAIN"
    );
    assert_eq!(first.resource_snapshot().socket_buffered_bytes, 0);
    assert_eq!(second.resource_snapshot().socket_buffered_bytes, 0);
}

// Default dual-stack IPv6 wildcards share the IPv4 port space.
#[test]
fn udp_dual_stack_wildcard_bind_conflicts_without_reuse() {
    use agentos_vm_kernel::socket_table::{SocketDomain, SocketType};
    for ipv6_first in [true, false] {
        let mut kernel = new_kernel("udp-dual-stack-bind-conflict");
        let process = spawn_shell(&mut kernel);
        let first_domain = if ipv6_first {
            SocketDomain::Inet6
        } else {
            SocketDomain::Inet
        };
        let second_domain = if ipv6_first {
            SocketDomain::Inet
        } else {
            SocketDomain::Inet6
        };
        let first_host = if ipv6_first { "::" } else { "127.0.0.1" };
        let second_host = if ipv6_first { "127.0.0.1" } else { "::" };
        let first = kernel
            .socket_create(
                "shell",
                process.pid(),
                SocketSpec::new(first_domain, SocketType::Datagram),
            )
            .unwrap();
        let second = kernel
            .socket_create(
                "shell",
                process.pid(),
                SocketSpec::new(second_domain, SocketType::Datagram),
            )
            .unwrap();
        kernel
            .socket_bind_inet(
                "shell",
                process.pid(),
                first,
                InetSocketAddress::new(first_host, 51211),
            )
            .unwrap();
        let error = kernel
            .socket_bind_inet(
                "shell",
                process.pid(),
                second,
                InetSocketAddress::new(second_host, 51211),
            )
            .unwrap_err();
        assert_eq!(error.code(), "EADDRINUSE");
    }
}
