#[test]
fn inet_traffic_class_inherits_at_connection_enqueue_not_accept() {
    for domain in [SocketDomain::Inet, SocketDomain::Inet6] {
        for external in [false, true] {
            let table = SocketTable::new();
            let spec = SocketSpec::new(domain, SocketType::Stream);
            let listener = table.allocate(1, spec).unwrap();
            let host = if domain == SocketDomain::Inet6 {
                "::1"
            } else {
                "127.0.0.1"
            };
            table
                .bind_inet(listener.id(), InetSocketAddress::new(host, 41001))
                .unwrap();
            table
                .set_tcp_traffic_class(listener.id(), InetTrafficClassOption::IpTos, 42)
                .unwrap();
            if domain == SocketDomain::Inet6 {
                table
                    .set_tcp_traffic_class(
                        listener.id(),
                        InetTrafficClassOption::Ipv6TrafficClass,
                        42,
                    )
                    .unwrap();
            }
            table.listen(listener.id(), 2).unwrap();
            if external {
                table
                    .enqueue_incoming_tcp_connection(
                        listener.id(),
                        InetSocketAddress::new(host, 41002),
                    )
                    .unwrap();
            } else {
                let client = table.allocate(2, spec).unwrap();
                table
                    .bind_inet(client.id(), InetSocketAddress::new(host, 41002))
                    .unwrap();
                table
                    .connect_to_bound_inet_stream(
                        client.id(),
                        InetSocketAddress::new(host, 41001),
                    )
                    .unwrap();
            }
            table
                .set_tcp_traffic_class(listener.id(), InetTrafficClassOption::IpTos, 84)
                .unwrap();
            if domain == SocketDomain::Inet6 {
                table
                    .set_tcp_traffic_class(
                        listener.id(),
                        InetTrafficClassOption::Ipv6TrafficClass,
                        84,
                    )
                    .unwrap();
            }
            let accepted = table.accept(listener.id()).unwrap();
            assert_eq!(accepted.ip_tos(), 40);
            assert_eq!(table.get(listener.id()).unwrap().ip_tos(), 84);
            if domain == SocketDomain::Inet6 {
                assert_eq!(accepted.ipv6_traffic_class(), 40);
            }
        }
    }
}
