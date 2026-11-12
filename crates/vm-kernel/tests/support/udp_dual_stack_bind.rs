fn new_dual_stack_bind_socket(
    kernel: &mut KernelVm<MemoryFileSystem>,
    process: &KernelProcessHandle,
    ipv6: bool,
    reuse_port: bool,
) -> agentos_vm_kernel::socket_table::SocketId {
    use agentos_vm_kernel::socket_table::{SocketDomain, SocketType};
    let socket = kernel
        .socket_create(
            "shell",
            process.pid(),
            SocketSpec::new(
                if ipv6 {
                    SocketDomain::Inet6
                } else {
                    SocketDomain::Inet
                },
                SocketType::Datagram,
            ),
        )
        .unwrap();
    if reuse_port {
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
    socket
}

#[test]
fn udp_dual_stack_bind_conflicts_in_both_orders_without_mutating_rejected_owner() {
    use agentos_vm_kernel::socket_table::SocketState;
    for ipv6_first in [false, true] {
        for ipv4_host in ["127.0.0.1", "0.0.0.0"] {
            let mut kernel = new_kernel("udp-dual-stack-bind-conflict");
            let process = spawn_shell(&mut kernel);
            let first = new_dual_stack_bind_socket(&mut kernel, &process, ipv6_first, false);
            let second = new_dual_stack_bind_socket(&mut kernel, &process, !ipv6_first, false);
            let first_address =
                InetSocketAddress::new(if ipv6_first { "::" } else { ipv4_host }, 51301);
            let second_address =
                InetSocketAddress::new(if ipv6_first { ipv4_host } else { "::" }, 51301);
            kernel
                .socket_bind_inet("shell", process.pid(), first, first_address.clone())
                .unwrap();
            let error = kernel
                .socket_bind_inet("shell", process.pid(), second, second_address.clone())
                .unwrap_err();
            assert_eq!(
                error.code(),
                "EADDRINUSE",
                "IPv6 first={ipv6_first}, IPv4={ipv4_host}"
            );
            assert_eq!(
                kernel.socket_get(first).unwrap().local_address(),
                Some(&first_address)
            );
            let rejected = kernel.socket_get(second).unwrap();
            assert_eq!(rejected.state(), SocketState::Created);
            assert_eq!(rejected.local_address(), None);
            kernel.socket_close("shell", process.pid(), first).unwrap();
            kernel
                .socket_bind_inet("shell", process.pid(), second, second_address.clone())
                .unwrap();
            assert_eq!(
                kernel.socket_get(second).unwrap().local_address(),
                Some(&second_address)
            );
        }
    }
}

#[test]
fn udp_dual_stack_bind_reuse_port_requires_both_owners() {
    for ipv6_first in [false, true] {
        for (first_reuses, second_reuses) in [(false, true), (true, false), (true, true)] {
            let mut kernel = new_kernel("udp-dual-stack-bind-reuse");
            let process = spawn_shell(&mut kernel);
            let first = new_dual_stack_bind_socket(&mut kernel, &process, ipv6_first, first_reuses);
            let second =
                new_dual_stack_bind_socket(&mut kernel, &process, !ipv6_first, second_reuses);
            let first_address =
                InetSocketAddress::new(if ipv6_first { "::" } else { "127.0.0.1" }, 51302);
            let second_address =
                InetSocketAddress::new(if ipv6_first { "127.0.0.1" } else { "::" }, 51302);
            kernel
                .socket_bind_inet("shell", process.pid(), first, first_address)
                .unwrap();
            let result =
                kernel.socket_bind_inet("shell", process.pid(), second, second_address.clone());
            if first_reuses && second_reuses {
                result.unwrap();
                assert_eq!(
                    kernel.socket_get(second).unwrap().local_address(),
                    Some(&second_address)
                );
            } else {
                assert_eq!(result.unwrap_err().code(), "EADDRINUSE");
                assert_eq!(kernel.socket_get(second).unwrap().local_address(), None);
            }
            assert_eq!(kernel.socket_get(first).unwrap().reuse_port(), first_reuses);
            assert_eq!(
                kernel.socket_get(second).unwrap().reuse_port(),
                second_reuses
            );
        }
    }
}

#[test]
fn udp_dual_stack_bind_keeps_specific_ipv6_and_different_ports_independent() {
    for ipv6_first in [false, true] {
        for (ipv6_host, ipv6_port, ipv4_port) in [("::1", 51303, 51303), ("::", 51304, 51305)] {
            let mut kernel = new_kernel("udp-dual-stack-bind-independent");
            let process = spawn_shell(&mut kernel);
            let first = new_dual_stack_bind_socket(&mut kernel, &process, ipv6_first, false);
            let second = new_dual_stack_bind_socket(&mut kernel, &process, !ipv6_first, false);
            let ipv6_address = InetSocketAddress::new(ipv6_host, ipv6_port);
            let ipv4_address = InetSocketAddress::new("127.0.0.1", ipv4_port);
            let (first_address, second_address) = if ipv6_first {
                (ipv6_address, ipv4_address)
            } else {
                (ipv4_address, ipv6_address)
            };
            kernel
                .socket_bind_inet("shell", process.pid(), first, first_address.clone())
                .unwrap();
            kernel
                .socket_bind_inet("shell", process.pid(), second, second_address.clone())
                .unwrap();
            assert_eq!(
                kernel.socket_get(first).unwrap().local_address(),
                Some(&first_address)
            );
            assert_eq!(
                kernel.socket_get(second).unwrap().local_address(),
                Some(&second_address)
            );
        }
    }
}

#[test]
fn udp_dual_stack_bind_ephemeral_assignment_skips_conflicting_other_family() {
    for ipv6_first in [false, true] {
        let mut kernel = new_kernel("udp-dual-stack-bind-ephemeral");
        let process = spawn_shell(&mut kernel);
        let first = new_dual_stack_bind_socket(&mut kernel, &process, ipv6_first, false);
        let second = new_dual_stack_bind_socket(&mut kernel, &process, !ipv6_first, false);
        kernel
            .socket_bind_inet(
                "shell",
                process.pid(),
                first,
                InetSocketAddress::new(if ipv6_first { "::" } else { "127.0.0.1" }, 49152),
            )
            .unwrap();
        kernel
            .socket_bind_inet(
                "shell",
                process.pid(),
                second,
                InetSocketAddress::new(if ipv6_first { "127.0.0.1" } else { "::" }, 0),
            )
            .unwrap();
        let assigned = kernel.socket_get(second).unwrap();
        assert_ne!(assigned.local_address().unwrap().port(), 49152);
        assert_eq!(
            kernel
                .socket_get(first)
                .unwrap()
                .local_address()
                .unwrap()
                .port(),
            49152
        );
    }
}
