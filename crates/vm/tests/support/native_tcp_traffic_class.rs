#[test]
fn native_tcp_options_are_installed_before_connect() {
    let runtime = agentos_driver_tokio::TokioDriver::process(&Default::default()).unwrap();
    let handle = runtime.handle();
    let _entered = handle.tokio_handle().enter();
    for address in ["127.0.0.1:1", "[::1]:1"] {
        let address: SocketAddr = address.parse().unwrap();
        let socket = configured_native_tcp_socket(address, 42, 42).unwrap();
        let options = SockRef::from(&socket);
        assert_eq!(options.tos_v4().unwrap(), 40);
        if address.is_ipv6() {
            assert_eq!(options.tclass_v6().unwrap(), 40);
        }
        assert_eq!(
            socket.local_addr().unwrap().port(),
            0,
            "configuring options must not bind or connect"
        );
    }
}
