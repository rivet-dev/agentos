use crate::socket_table::{InetTrafficClassOption, SocketDomain};

#[test]
fn tcp_traffic_class_kernel_wrappers_ipv4_local() {
    tcp_traffic_class_wrapper_control(SocketDomain::Inet, false);
}
#[test]
fn tcp_traffic_class_kernel_wrappers_ipv4_external() {
    tcp_traffic_class_wrapper_control(SocketDomain::Inet, true);
}
#[test]
fn tcp_traffic_class_kernel_wrappers_ipv6_local() {
    tcp_traffic_class_wrapper_control(SocketDomain::Inet6, false);
}
#[test]
fn tcp_traffic_class_kernel_wrappers_ipv6_external() {
    tcp_traffic_class_wrapper_control(SocketDomain::Inet6, true);
}
fn tcp_traffic_class_wrapper_control(domain: SocketDomain, external: bool) {
    let mut config = KernelVmConfig::new("tcp-option-ownership");
    config.permissions = Permissions::allow_all();
    let mut kernel = KernelVm::new(MountTable::new(MemoryFileSystem::new()), config);
    kernel
        .register_driver(CommandDriver::new("owner", ["sh"]))
        .unwrap();
    kernel
        .register_driver(CommandDriver::new("other", ["other-sh"]))
        .unwrap();
    let first = kernel
        .create_virtual_process("owner", "owner", "sh", vec![], Default::default())
        .unwrap();
    let second = kernel
        .create_virtual_process("owner", "owner", "sh", vec![], Default::default())
        .unwrap();
    let pid = first.pid();
    let peer = second.pid();
    let spec = SocketSpec::new(domain, SocketType::Stream);
    let host = if domain == SocketDomain::Inet6 {
        "::1"
    } else {
        "127.0.0.1"
    };
    let listener = kernel.socket_create("owner", pid, spec).unwrap();
    for (driver, owner) in [("other", pid), ("owner", peer)] {
        let error = kernel
            .socket_set_tcp_traffic_class(
                driver,
                owner,
                listener,
                InetTrafficClassOption::IpTos,
                42,
            )
            .unwrap_err();
        assert_eq!(error.code(), "EPERM");
        assert_eq!(kernel.socket_get(listener).unwrap().ip_tos(), 0);
    }
    kernel
        .socket_bind_inet("owner", pid, listener, InetSocketAddress::new(host, 41001))
        .unwrap();
    kernel
        .socket_set_tcp_traffic_class("owner", pid, listener, InetTrafficClassOption::IpTos, 42)
        .unwrap();
    if domain == SocketDomain::Inet6 {
        kernel
            .socket_set_tcp_traffic_class(
                "owner",
                pid,
                listener,
                InetTrafficClassOption::Ipv6TrafficClass,
                42,
            )
            .unwrap();
    } else {
        assert_eq!(
            kernel
                .socket_set_tcp_traffic_class(
                    "owner",
                    pid,
                    listener,
                    InetTrafficClassOption::Ipv6TrafficClass,
                    42
                )
                .unwrap_err()
                .code(),
            "ENOPROTOOPT"
        );
    }
    kernel.socket_listen("owner", pid, listener, 2).unwrap();
    let client = if external {
        kernel
            .socket_queue_incoming_tcp_connection(
                "owner",
                pid,
                listener,
                InetSocketAddress::new(host, 41002),
            )
            .unwrap();
        None
    } else {
        let client = kernel.socket_create("owner", peer, spec).unwrap();
        kernel
            .socket_bind_inet("owner", peer, client, InetSocketAddress::new(host, 41002))
            .unwrap();
        kernel
            .socket_connect_inet_loopback(
                "owner",
                peer,
                client,
                InetSocketAddress::new(host, 41001),
            )
            .unwrap();
        Some(client)
    };
    kernel
        .socket_set_tcp_traffic_class("owner", pid, listener, InetTrafficClassOption::IpTos, 84)
        .unwrap();
    if domain == SocketDomain::Inet6 {
        kernel
            .socket_set_tcp_traffic_class(
                "owner",
                pid,
                listener,
                InetTrafficClassOption::Ipv6TrafficClass,
                84,
            )
            .unwrap();
    }
    let accepted = kernel.socket_accept("owner", pid, listener).unwrap();
    assert_eq!(kernel.socket_get(accepted).unwrap().ip_tos(), 40);
    if domain == SocketDomain::Inet6 {
        assert_eq!(
            kernel.socket_get(accepted).unwrap().ipv6_traffic_class(),
            40
        );
    }
    let transfer = kernel
        .fd_adopt_socket_transfer("owner", pid, accepted, 0)
        .unwrap();
    kernel
        .fd_install_transfer_at("owner", pid, 10, 0, &transfer)
        .unwrap();
    kernel
        .fd_install_transfer_at("owner", peer, 11, 0, &transfer)
        .unwrap();
    drop(transfer);
    kernel
        .socket_set_tcp_traffic_class("owner", peer, accepted, InetTrafficClassOption::IpTos, 126)
        .unwrap();
    assert_eq!(kernel.socket_get(accepted).unwrap().ip_tos(), 124);
    assert_eq!(
        kernel
            .socket_set_tcp_traffic_class("other", peer, accepted, InetTrafficClassOption::IpTos, 0)
            .unwrap_err()
            .code(),
        "EPERM"
    );
    kernel.fd_close("owner", pid, 10).unwrap();
    kernel
        .socket_set_tcp_traffic_class("owner", peer, accepted, InetTrafficClassOption::IpTos, 168)
        .unwrap();
    assert_eq!(kernel.socket_get(accepted).unwrap().ip_tos(), 168);
    kernel.fd_close("owner", peer, 11).unwrap();
    assert!(kernel.socket_get(accepted).is_none());
    if let Some(client) = client {
        kernel.socket_close("owner", peer, client).unwrap();
    }
    kernel.socket_close("owner", pid, listener).unwrap();
    kernel.dispose().unwrap();
}
