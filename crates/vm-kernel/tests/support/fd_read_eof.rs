#[test]
fn fd_read_eof_normalizes_positive_length_vfs_device_proc_and_anonymous_reads() {
    let (mut kernel, process) = kernel_with_process();
    let pid = process.pid();
    kernel.write_file("/eof-empty", Vec::new()).unwrap();
    kernel.write_file("/eof-data", b"abc".to_vec()).unwrap();
    kernel.write_file("/eof-unlinked", b"abc".to_vec()).unwrap();
    let before = kernel.resource_snapshot().open_fds;
    for path in [
        "/eof-empty",
        "/eof-data",
        "/eof-unlinked",
        "/dev/null",
        "/proc/version",
    ] {
        let fd = kernel.fd_open("wasm", pid, path, O_RDONLY, None).unwrap();
        if path == "/eof-unlinked" {
            kernel.remove_file_for_process("wasm", pid, path).unwrap();
        }
        let mut data = Vec::new();
        let mut eof = false;
        for _ in 0..8 {
            match kernel
                .fd_read_with_timeout_result("wasm", pid, fd, 4096, Some(Duration::ZERO))
                .unwrap()
            {
                Some(chunk) => {
                    assert!(
                        !chunk.is_empty(),
                        "positive-length EOF at {path} must be None"
                    );
                    data.extend(chunk);
                }
                None => {
                    eof = true;
                    break;
                }
            }
        }
        assert!(eof, "finite file {path} must reach EOF");
        if path == "/eof-data" || path == "/eof-unlinked" {
            assert_eq!(data, b"abc");
        }
        if path == "/eof-empty" || path == "/dev/null" {
            assert!(data.is_empty());
        }
        if path == "/proc/version" {
            assert!(!data.is_empty());
        }
        assert_eq!(
            kernel
                .fd_read_with_timeout_result("wasm", pid, fd, 1, Some(Duration::ZERO))
                .unwrap(),
            None
        );
        kernel.fd_close("wasm", pid, fd).unwrap();
    }
    assert_eq!(kernel.resource_snapshot().open_fds, before);
}

#[test]
fn fd_read_eof_zero_length_requests_preserve_data_and_non_eof_result() {
    let (mut kernel, process) = kernel_with_process();
    let pid = process.pid();
    kernel.write_file("/zero-read", b"abc".to_vec()).unwrap();
    for path in ["/zero-read", "/dev/null", "/dev/zero", "/proc/version"] {
        let fd = kernel.fd_open("wasm", pid, path, O_RDONLY, None).unwrap();
        assert_eq!(
            kernel
                .fd_read_with_timeout_result("wasm", pid, fd, 0, Some(Duration::ZERO))
                .unwrap(),
            Some(Vec::new())
        );
        let data = kernel
            .fd_read_with_timeout_result("wasm", pid, fd, 3, Some(Duration::ZERO))
            .unwrap();
        match path {
            "/zero-read" => assert_eq!(data, Some(b"abc".to_vec())),
            "/dev/null" => assert_eq!(data, None),
            "/dev/zero" => assert_eq!(data, Some(vec![0; 3])),
            _ => assert!(data.is_some_and(|bytes| !bytes.is_empty())),
        }
        assert_eq!(
            kernel
                .fd_read_with_timeout_result("wasm", pid, fd, 0, Some(Duration::ZERO))
                .unwrap(),
            Some(Vec::new())
        );
        kernel.fd_close("wasm", pid, fd).unwrap();
    }
}

#[test]
fn fd_read_eof_preserves_pipe_and_stream_socket_readiness_data_and_close() {
    let (mut kernel, process) = kernel_with_process();
    let pid = process.pid();
    let baseline = kernel.resource_snapshot().open_fds;
    for socket in [false, true] {
        let (reader, writer) = if socket {
            kernel
                .fd_socketpair("wasm", pid, SocketType::Stream, false, false)
                .unwrap()
        } else {
            kernel.open_pipe("wasm", pid).unwrap()
        };
        assert_eq!(
            kernel
                .fd_read_with_timeout_result("wasm", pid, reader, 3, Some(Duration::ZERO))
                .unwrap_err()
                .code(),
            "EAGAIN"
        );
        kernel.fd_write("wasm", pid, writer, b"abc").unwrap();
        assert_eq!(
            kernel
                .fd_read_with_timeout_result("wasm", pid, reader, 3, Some(Duration::ZERO))
                .unwrap(),
            Some(b"abc".to_vec())
        );
        kernel.fd_close("wasm", pid, writer).unwrap();
        assert_eq!(
            kernel
                .fd_read_with_timeout_result("wasm", pid, reader, 3, Some(Duration::ZERO))
                .unwrap(),
            None
        );
        kernel.fd_close("wasm", pid, reader).unwrap();
    }
    assert_eq!(kernel.resource_snapshot().open_fds, baseline);
}

#[test]
fn fd_read_eof_does_not_turn_an_empty_datagram_into_stream_eof() {
    let (mut kernel, process) = kernel_with_process();
    let pid = process.pid();
    let (reader, writer) = kernel
        .fd_socketpair("wasm", pid, SocketType::Datagram, false, false)
        .unwrap();
    kernel.fd_write("wasm", pid, writer, b"").unwrap();
    assert_eq!(
        kernel
            .fd_read_with_timeout_result("wasm", pid, reader, 8, Some(Duration::ZERO))
            .unwrap(),
        Some(Vec::new())
    );
    assert_eq!(
        kernel
            .fd_read_with_timeout_result("wasm", pid, reader, 8, Some(Duration::ZERO))
            .unwrap_err()
            .code(),
        "EAGAIN"
    );
    kernel.fd_close("wasm", pid, reader).unwrap();
    kernel.fd_close("wasm", pid, writer).unwrap();
}
