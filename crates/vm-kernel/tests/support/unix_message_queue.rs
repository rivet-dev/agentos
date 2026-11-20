fn unix_message_pair(
    byte_limit: usize,
    message_limit: usize,
) -> (SocketTable, Arc<ResourceLedger>, SocketId, SocketId) {
    let table = SocketTable::new();
    let ledger = test_resource_ledger(byte_limit, message_limit);
    table.set_resource_ledger(Arc::clone(&ledger)).unwrap();
    let writer = table.allocate(1, SocketSpec::unix_datagram()).unwrap().id();
    let reader = table.allocate(2, SocketSpec::unix_datagram()).unwrap().id();
    table.connect_pair(writer, reader).unwrap();
    (table, ledger, writer, reader)
}

fn assert_unix_queue_usage(ledger: &ResourceLedger, bytes: usize, messages: usize) {
    assert_eq!(usage(ledger, ResourceClass::BufferedBytes).used, bytes);
    assert_eq!(usage(ledger, ResourceClass::Datagrams).used, messages);
    assert_eq!(usage(ledger, ResourceClass::UdpBytes).used, 0);
    assert_eq!(usage(ledger, ResourceClass::UdpDatagrams).used, 0);
}

#[test]
fn unix_message_queue_matches_linux_empty_order_boundaries_and_truncation() {
    for send_message in [false, true] {
        for recv_message in [false, true] {
            for size in [2, 8] {
                let (table, ledger, writer, reader) = unix_message_pair(1024, 4);
                for payload in [b"".as_slice(), b"abc", b"defgh"] {
                    let count = if send_message {
                        table.send_message(writer, payload, Vec::new()).unwrap()
                    } else {
                        table.write(writer, payload).unwrap()
                    };
                    assert_eq!(count, payload.len());
                }
                assert_unix_queue_usage(&ledger, 3 * DATAGRAM_METADATA_BYTES + 8, 3);
                for payload in [b"".as_slice(), b"abc", b"defgh"] {
                    let expected = payload[..payload.len().min(size)].to_vec();
                    if recv_message {
                        let actual = table.recv_message(reader, size, false).unwrap().unwrap();
                        assert_eq!(actual.payload, expected);
                        assert_eq!(actual.full_length, payload.len());
                        assert_eq!(actual.truncated, payload.len() > size);
                    } else {
                        assert_eq!(table.read(reader, size).unwrap(), Some(expected));
                    }
                }
                assert_eq!(table.read(reader, size).unwrap_err().code(), "EAGAIN");
                assert_unix_queue_usage(&ledger, 0, 0);
                table.remove(writer).unwrap();
                table.remove(reader).unwrap();
                assert_eq!(table.snapshot().sockets, 0);
            }
        }
    }
}

#[test]
fn unix_message_queue_empty_records_have_bounded_metadata_and_rollback() {
    let (table, ledger, writer, reader) = unix_message_pair(4 * DATAGRAM_METADATA_BYTES, 2);
    table.write(writer, b"").unwrap();
    table.send_message(writer, b"", Vec::new()).unwrap();
    assert_unix_queue_usage(&ledger, 2 * DATAGRAM_METADATA_BYTES, 2);
    for payload in [b"".as_slice(), b"x"] {
        let error = table.write(writer, payload).unwrap_err();
        assert_eq!(error.code(), "EAGAIN");
        assert!(error.to_string().contains("test.maxDatagrams"));
        assert_unix_queue_usage(&ledger, 2 * DATAGRAM_METADATA_BYTES, 2);
    }
    assert_eq!(table.read(reader, 8).unwrap(), Some(Vec::new()));
    assert_eq!(table.read(reader, 8).unwrap(), Some(Vec::new()));
    assert_unix_queue_usage(&ledger, 0, 0);
    table.write(writer, b"x").unwrap();
    assert_unix_queue_usage(&ledger, DATAGRAM_METADATA_BYTES + 1, 1);
    assert_eq!(table.read(reader, 8).unwrap(), Some(b"x".to_vec()));
    assert_unix_queue_usage(&ledger, 0, 0);
}

#[test]
fn unix_message_queue_metadata_byte_limit_rejects_before_publication() {
    let (table, ledger, writer, reader) = unix_message_pair(DATAGRAM_METADATA_BYTES - 1, 2);
    let error = table.write(writer, b"").unwrap_err();
    assert_eq!(error.code(), "EAGAIN");
    assert!(error.to_string().contains("test.maxBufferedBytes"));
    assert_eq!(table.read(reader, 8).unwrap_err().code(), "EAGAIN");
    assert_eq!(table.snapshot().datagram_queue_len, 0);
    assert_unix_queue_usage(&ledger, 0, 0);
}

#[test]
fn unix_message_queue_peek_and_zero_length_read_preserve_receive_consumes() {
    let (table, ledger, writer, reader) = unix_message_pair(1024, 2);
    for payload in [b"".as_slice(), b"abc"] {
        table.write(writer, payload).unwrap();
        let bytes = DATAGRAM_METADATA_BYTES + payload.len();
        assert_eq!(table.read(reader, 0).unwrap(), Some(Vec::new()));
        let peeked = table.recv_message(reader, 0, true).unwrap().unwrap();
        assert_eq!(peeked.full_length, payload.len());
        assert_eq!(peeked.truncated, !payload.is_empty());
        assert_unix_queue_usage(&ledger, bytes, 1);
        let consumed = table.recv_message(reader, 0, false).unwrap().unwrap();
        assert!(consumed.payload.is_empty());
        assert_eq!(consumed.full_length, payload.len());
        assert_eq!(table.read(reader, 8).unwrap_err().code(), "EAGAIN");
        assert_unix_queue_usage(&ledger, 0, 0);
    }
}

#[test]
fn unix_message_queue_shutdown_close_and_snapshot_ownership_release_metadata() {
    for shutdown in [false, true] {
        let (table, ledger, writer, reader) = unix_message_pair(1024, 2);
        table.write(writer, b"").unwrap();
        table.send_message(writer, b"abc", Vec::new()).unwrap();
        assert_unix_queue_usage(&ledger, 2 * DATAGRAM_METADATA_BYTES + 3, 2);
        if shutdown {
            table.shutdown(reader, SocketShutdown::Read).unwrap();
        } else {
            // A returned socket snapshot still owns its cloned message records.
            let removed = table.remove(reader).unwrap();
            assert_eq!(usage(&ledger, ResourceClass::Datagrams).used, 2);
            drop(removed);
        }
        assert_unix_queue_usage(&ledger, 0, 0);
        table.remove(writer).unwrap();
        if shutdown {
            table.remove(reader).unwrap();
        }
        assert_eq!(table.snapshot().sockets, 0);
    }
}

#[test]
fn unix_message_queue_stream_controls_keep_byte_and_eof_semantics() {
    let table = SocketTable::new();
    let ledger = test_resource_ledger(5, 1);
    table.set_resource_ledger(Arc::clone(&ledger)).unwrap();
    let writer = table.allocate(1, SocketSpec::unix_stream()).unwrap().id();
    let reader = table.allocate(2, SocketSpec::unix_stream()).unwrap().id();
    table.connect_pair(writer, reader).unwrap();
    table.write(writer, b"").unwrap();
    table.send_message(writer, b"", Vec::new()).unwrap();
    assert_eq!(table.read(reader, 8).unwrap_err().code(), "EAGAIN");
    assert_unix_queue_usage(&ledger, 0, 0);
    table.write(writer, b"ab").unwrap();
    table.send_message(writer, b"cde", Vec::new()).unwrap();
    assert_unix_queue_usage(&ledger, 5, 0);
    assert_eq!(table.read(reader, 8).unwrap(), Some(b"abcde".to_vec()));
    assert_unix_queue_usage(&ledger, 0, 0);
    table.remove(writer).unwrap();
    assert_eq!(table.read(reader, 8).unwrap(), None);
    table.remove(reader).unwrap();
}

#[test]
fn unix_message_queue_charged_truncation_releases_tail_and_queued_metadata() {
    let (table, ledger, writer, reader) = unix_message_pair(1024, 2);
    table.write(writer, b"abc").unwrap();
    let read = table.read_charged(reader, 2).unwrap().unwrap();
    assert_eq!(read.bytes, b"ab");
    assert_eq!(read.reservation.as_ref().unwrap().amount(), 2);
    assert_unix_queue_usage(&ledger, 2, 0);
    assert_eq!(table.snapshot().datagram_queue_len, 0);
    drop(read);
    assert_unix_queue_usage(&ledger, 0, 0);
}

#[test]
fn unix_message_queue_first_empty_message_drives_readiness_without_idle_polling() {
    use std::sync::atomic::AtomicUsize;
    let (table, _ledger, writer, reader) = unix_message_pair(1024, 2);
    let signals = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&signals);
    table.set_readiness_sink(Some(move |event: SocketReadiness| {
        assert_eq!(event.socket_id, reader);
        assert_eq!(event.kind, SocketReadinessKind::Data);
        observed.fetch_add(1, Ordering::SeqCst);
    }));
    table.write(writer, b"").unwrap();
    table.send_message(writer, b"", Vec::new()).unwrap();
    assert_eq!(signals.load(Ordering::SeqCst), 1);
    assert!(table.poll(reader, POLLIN).unwrap().intersects(POLLIN));
    assert_eq!(table.read(reader, 8).unwrap(), Some(Vec::new()));
    assert!(table.poll(reader, POLLIN).unwrap().intersects(POLLIN));
    assert_eq!(table.read(reader, 8).unwrap(), Some(Vec::new()));
    assert!(!table.poll(reader, POLLIN).unwrap().intersects(POLLIN));
}
