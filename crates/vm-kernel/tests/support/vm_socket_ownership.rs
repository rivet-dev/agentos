#[test]
fn vm_publication_survives_server_exit_and_denies_guest_access() {
    let (mut kernel, process) = kernel_with_process();
    let pid = process.pid();
    let listener = kernel
        .socket_create("wasm", pid, SocketSpec::tcp())
        .unwrap();
    kernel
        .socket_bind_inet(
            "wasm",
            pid,
            listener,
            InetSocketAddress::new("127.0.0.1", 41009),
        )
        .unwrap();
    kernel.socket_listen("wasm", pid, listener, 1).unwrap();
    let client = kernel
        .socket_create("wasm", pid, SocketSpec::tcp())
        .unwrap();
    kernel
        .socket_connect_inet_loopback(
            "wasm",
            pid,
            client,
            InetSocketAddress::new("127.0.0.1", 41009),
        )
        .unwrap();
    let server = kernel.socket_accept("wasm", pid, listener).unwrap();
    kernel.socket_retain_for_vm("wasm", pid, client).unwrap();

    assert_eq!(
        kernel
            .socket_read("wasm", pid, client, 1)
            .unwrap_err()
            .code(),
        "EPERM"
    );
    assert_eq!(
        kernel
            .socket_write("wasm", pid, client, b"stolen")
            .unwrap_err()
            .code(),
        "EPERM"
    );
    assert_eq!(
        kernel
            .socket_shutdown("wasm", pid, client, SocketShutdown::Both)
            .unwrap_err()
            .code(),
        "EPERM"
    );
    assert_eq!(
        kernel.socket_close("wasm", pid, client).unwrap_err().code(),
        "EPERM"
    );
    assert_eq!(
        kernel
            .fd_adopt_socket_transfer("wasm", pid, client, 0)
            .unwrap_err()
            .code(),
        "EPERM"
    );
    assert_eq!(kernel.vm_socket_write(client, b"request").unwrap(), 7);
    assert_eq!(
        kernel.socket_read("wasm", pid, server, 7).unwrap(),
        Some(b"request".to_vec())
    );
    kernel
        .socket_write("wasm", pid, server, b"buffered tail")
        .unwrap();
    kernel.exit_process("wasm", pid, 0).unwrap();
    assert!(kernel.socket_get(server).is_none());
    assert_eq!(
        kernel.vm_socket_read(client, 64).unwrap(),
        Some(b"buffered tail".to_vec())
    );
    assert_eq!(kernel.vm_socket_read(client, 64).unwrap(), None);
    kernel.vm_socket_close(client).unwrap();
    assert!(kernel.socket_get(client).is_none());
}

#[test]
fn vm_publication_disposal_retires_retained_sockets() {
    let (mut kernel, process) = kernel_with_process();
    let pid = process.pid();
    let listener = kernel
        .socket_create("wasm", pid, SocketSpec::tcp())
        .unwrap();
    assert_eq!(
        kernel
            .socket_retain_for_vm("wasm", pid, listener)
            .unwrap_err()
            .code(),
        "EPERM"
    );
    kernel
        .socket_bind_inet(
            "wasm",
            pid,
            listener,
            InetSocketAddress::new("127.0.0.1", 41010),
        )
        .unwrap();
    kernel.socket_listen("wasm", pid, listener, 1).unwrap();
    let client = kernel
        .socket_create("wasm", pid, SocketSpec::tcp())
        .unwrap();
    kernel
        .socket_connect_inet_loopback(
            "wasm",
            pid,
            client,
            InetSocketAddress::new("127.0.0.1", 41010),
        )
        .unwrap();
    kernel.socket_retain_for_vm("wasm", pid, client).unwrap();
    let retained = kernel.sockets.clone();
    kernel.dispose().unwrap();
    assert!(retained.get(client).is_none());
    assert!(retained.get(listener).is_none());
}

#[test]
fn vm_publication_bounds_each_direction_and_restores_writable_after_read() {
    let (mut kernel, process) = kernel_with_process();
    let pid = process.pid();
    let listener = kernel
        .socket_create("wasm", pid, SocketSpec::tcp())
        .unwrap();
    kernel
        .socket_bind_inet(
            "wasm",
            pid,
            listener,
            InetSocketAddress::new("127.0.0.1", 41011),
        )
        .unwrap();
    kernel.socket_listen("wasm", pid, listener, 1).unwrap();
    let client = kernel
        .socket_create("wasm", pid, SocketSpec::tcp())
        .unwrap();
    kernel
        .socket_connect_inet_loopback(
            "wasm",
            pid,
            client,
            InetSocketAddress::new("127.0.0.1", 41011),
        )
        .unwrap();
    let server = kernel.socket_accept("wasm", pid, listener).unwrap();
    kernel.socket_retain_for_vm("wasm", pid, client).unwrap();

    let payload = vec![42; 128 * 1024];
    assert_eq!(kernel.vm_socket_write(client, &payload).unwrap(), 64 * 1024);
    assert_eq!(
        kernel.socket_get(server).unwrap().buffered_read_bytes(),
        64 * 1024
    );
    assert_eq!(
        kernel.vm_socket_write(client, b"x").unwrap_err().code(),
        "EAGAIN"
    );
    assert_eq!(
        kernel.socket_write("wasm", pid, server, &payload).unwrap(),
        64 * 1024
    );
    let mut targets = vec![crate::poll::PollTargetEntry::socket(
        server,
        crate::poll::POLLOUT,
    )];
    assert_eq!(
        kernel
            .poll_targets("wasm", pid, targets.clone(), 0)
            .unwrap()
            .targets[0]
            .revents,
        crate::poll::PollEvents::empty()
    );
    assert_eq!(
        kernel.vm_socket_read(client, 7).unwrap().unwrap(),
        vec![42; 7]
    );
    targets = kernel
        .poll_targets("wasm", pid, targets, 0)
        .unwrap()
        .targets;
    assert!(targets[0].revents.intersects(crate::poll::POLLOUT));
    assert_eq!(
        kernel.socket_write("wasm", pid, server, &payload).unwrap(),
        7
    );
    assert_eq!(
        kernel
            .socket_read("wasm", pid, server, 13)
            .unwrap()
            .unwrap(),
        vec![42; 13]
    );
    assert_eq!(kernel.vm_socket_write(client, &payload).unwrap(), 13);
    assert_eq!(kernel.resource_snapshot().socket_buffered_bytes, 128 * 1024);
    kernel.dispose().unwrap();
    assert_eq!(kernel.resource_snapshot().socket_buffered_bytes, 0);
}

#[test]
fn guest_tcp_stream_applies_the_same_receive_window() {
    let (mut kernel, process) = kernel_with_process();
    let pid = process.pid();
    let listener = kernel
        .socket_create("wasm", pid, SocketSpec::tcp())
        .unwrap();
    kernel
        .socket_bind_inet(
            "wasm",
            pid,
            listener,
            InetSocketAddress::new("127.0.0.1", 41012),
        )
        .unwrap();
    kernel.socket_listen("wasm", pid, listener, 1).unwrap();
    let client = kernel
        .socket_create("wasm", pid, SocketSpec::tcp())
        .unwrap();
    kernel
        .socket_connect_inet_loopback(
            "wasm",
            pid,
            client,
            InetSocketAddress::new("127.0.0.1", 41012),
        )
        .unwrap();
    let server = kernel.socket_accept("wasm", pid, listener).unwrap();

    let payload = vec![19; 128 * 1024];
    assert_eq!(
        kernel.socket_write("wasm", pid, client, &payload).unwrap(),
        64 * 1024
    );
    assert_eq!(
        kernel
            .socket_write("wasm", pid, client, b"x")
            .unwrap_err()
            .code(),
        "EAGAIN"
    );
    assert_eq!(
        kernel
            .socket_read("wasm", pid, server, 17)
            .unwrap()
            .unwrap(),
        vec![19; 17]
    );
    assert_eq!(
        kernel.socket_write("wasm", pid, client, &payload).unwrap(),
        17
    );
    assert_eq!(
        kernel.socket_get(server).unwrap().buffered_read_bytes(),
        64 * 1024
    );
}
