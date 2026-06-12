use agent_os_kernel::pty::{
    LineDisciplineConfig, PartialTermios, PartialTermiosControlChars, PtyManager, MAX_CANON,
    MAX_PTY_BUFFER_BYTES, SIGINT,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn wait_for(predicate: impl Fn() -> bool, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(predicate(), "condition should become true before timeout");
}

#[test]
fn raw_mode_delivers_bytes_and_applies_icrnl_translation() {
    let manager = PtyManager::new();
    let pty = manager.create_pty();
    manager
        .set_discipline(
            pty.master.description.id(),
            LineDisciplineConfig {
                canonical: Some(false),
                echo: Some(false),
                isig: Some(false),
            },
        )
        .expect("set raw mode");

    manager
        .write(pty.master.description.id(), b"hello\rworld")
        .expect("write master");
    let data = manager
        .read(pty.slave.description.id(), 64)
        .expect("read slave")
        .expect("slave should receive data");

    assert_eq!(String::from_utf8(data).expect("valid utf8"), "hello\nworld");
}

#[test]
fn raw_mode_pending_short_read_buffers_remaining_bytes() {
    let manager = PtyManager::new();
    let pty = manager.create_pty();
    manager
        .set_discipline(
            pty.master.description.id(),
            LineDisciplineConfig {
                canonical: Some(false),
                echo: Some(false),
                isig: Some(false),
            },
        )
        .expect("set raw mode");

    let reader = {
        let manager = manager.clone();
        let slave_id = pty.slave.description.id();
        std::thread::spawn(move || {
            manager
                .read_with_timeout(slave_id, 1, Some(Duration::from_secs(1)))
                .expect("pending short read")
                .expect("first byte should be delivered")
        })
    };

    manager
        .write(pty.master.description.id(), b"hello")
        .expect("write raw input");

    let first = reader.join().expect("reader thread should finish");
    assert_eq!(first, b"h");

    let remaining = manager
        .read(pty.slave.description.id(), 64)
        .expect("read remaining bytes")
        .expect("remaining bytes should stay buffered");
    assert_eq!(remaining, b"ello");
}

#[test]
fn split_delivery_with_second_queued_reader_leaves_no_stale_waiters() {
    let manager = PtyManager::new();
    let pty = manager.create_pty();
    manager
        .set_discipline(
            pty.master.description.id(),
            LineDisciplineConfig {
                canonical: Some(false),
                echo: Some(false),
                isig: Some(false),
            },
        )
        .expect("set raw mode");

    let slave_id = pty.slave.description.id();

    // Reader A asks for one byte and must be first in the waiter queue.
    let reader_a = {
        let manager = manager.clone();
        std::thread::spawn(move || {
            manager
                .read_with_timeout(slave_id, 1, Some(Duration::from_secs(5)))
                .expect("first read should succeed")
                .expect("first read should deliver data")
        })
    };
    wait_for(
        || manager.pending_read_waiter_count() == 1,
        Duration::from_secs(1),
    );

    // Reader B queues behind A and will pick up the buffered tail.
    let reader_b = {
        let manager = manager.clone();
        std::thread::spawn(move || {
            manager
                .read_with_timeout(slave_id, 64, Some(Duration::from_secs(5)))
                .expect("second read should succeed")
                .expect("second read should deliver data")
        })
    };
    wait_for(
        || manager.pending_read_waiter_count() == 2,
        Duration::from_secs(1),
    );

    // The split delivery hands "h" to reader A and buffers "ello", which
    // reader B drains directly from the input buffer.
    manager
        .write(pty.master.description.id(), b"hello")
        .expect("write raw input");

    assert_eq!(reader_a.join().expect("reader A should finish"), b"h");
    assert_eq!(reader_b.join().expect("reader B should finish"), b"ello");

    // Reader B returned via the direct buffer-drain path, so its waiter
    // entry and queue id must be gone.
    assert_eq!(manager.pending_read_waiter_count(), 0);
    assert_eq!(manager.queued_read_waiter_count(), 0);

    // A stale waiter would swallow this write and the read would time out.
    manager
        .write(pty.master.description.id(), b"world")
        .expect("write after split delivery");
    let follow_up = manager
        .read_with_timeout(slave_id, 64, Some(Duration::from_secs(1)))
        .expect("follow-up read should succeed")
        .expect("follow-up read should deliver data");
    assert_eq!(follow_up, b"world");
}

#[test]
fn split_output_delivery_with_second_queued_reader_leaves_no_stale_waiters() {
    let manager = PtyManager::new();
    let pty = manager.create_pty();
    manager
        .set_discipline(
            pty.master.description.id(),
            LineDisciplineConfig {
                canonical: Some(false),
                echo: Some(false),
                isig: Some(false),
            },
        )
        .expect("set raw mode");

    let master_id = pty.master.description.id();

    // Reader A asks for one byte and must be first in the waiter queue.
    let reader_a = {
        let manager = manager.clone();
        std::thread::spawn(move || {
            manager
                .read_with_timeout(master_id, 1, Some(Duration::from_secs(5)))
                .expect("first read should succeed")
                .expect("first read should deliver data")
        })
    };
    wait_for(
        || manager.pending_read_waiter_count() == 1,
        Duration::from_secs(1),
    );

    // Reader B queues behind A and will pick up the buffered tail.
    let reader_b = {
        let manager = manager.clone();
        std::thread::spawn(move || {
            manager
                .read_with_timeout(master_id, 64, Some(Duration::from_secs(5)))
                .expect("second read should succeed")
                .expect("second read should deliver data")
        })
    };
    wait_for(
        || manager.pending_read_waiter_count() == 2,
        Duration::from_secs(1),
    );

    // The split delivery hands "h" to reader A and buffers "ello", which
    // reader B drains directly from the output buffer.
    manager
        .write(pty.slave.description.id(), b"hello")
        .expect("write slave output");

    assert_eq!(reader_a.join().expect("reader A should finish"), b"h");
    assert_eq!(reader_b.join().expect("reader B should finish"), b"ello");

    // Reader B returned via the direct buffer-drain path, so its waiter
    // entry and queue id must be gone.
    assert_eq!(manager.pending_read_waiter_count(), 0);
    assert_eq!(manager.queued_read_waiter_count(), 0);

    // A stale waiter would swallow this write and the read would time out.
    manager
        .write(pty.slave.description.id(), b"world")
        .expect("write after split delivery");
    let follow_up = manager
        .read_with_timeout(master_id, 64, Some(Duration::from_secs(1)))
        .expect("follow-up read should succeed")
        .expect("follow-up read should deliver data");
    assert_eq!(follow_up, b"world");
}

#[test]
fn canonical_mode_buffers_until_newline_and_honors_backspace() {
    let manager = PtyManager::new();
    let pty = manager.create_pty();

    manager
        .write(pty.master.description.id(), b"echo helo\x7flo\n")
        .expect("write canonical input");

    let line = manager
        .read(pty.slave.description.id(), 64)
        .expect("read canonical line")
        .expect("line should be available");
    assert_eq!(String::from_utf8(line).expect("valid utf8"), "echo hello\n");

    let echo = manager
        .read(pty.master.description.id(), 64)
        .expect("read echo")
        .expect("echo should be available");
    assert_eq!(
        String::from_utf8(echo).expect("valid utf8"),
        "echo helo\x08 \x08lo\r\n"
    );
}

#[test]
fn control_characters_signal_the_foreground_process_group() {
    let signals = Arc::new(Mutex::new(Vec::new()));
    let signal_log = Arc::clone(&signals);
    let manager = PtyManager::with_signal_handler(Arc::new(move |pgid, signal| {
        signal_log
            .lock()
            .expect("signal log lock poisoned")
            .push((pgid, signal));
    }));
    let pty = manager.create_pty();

    manager
        .set_foreground_pgid(pty.master.description.id(), 42)
        .expect("set foreground pgid");
    manager
        .write(pty.master.description.id(), [0x03])
        .expect("write intr char");

    assert_eq!(
        *signals.lock().expect("signal log lock poisoned"),
        vec![(42, SIGINT)]
    );
}

#[test]
fn peer_close_returns_hangup_instead_of_blocking() {
    let manager = PtyManager::new();
    let pty = manager.create_pty();

    manager.close(pty.master.description.id());
    let result = manager
        .read(pty.slave.description.id(), 16)
        .expect("read after hangup");

    assert_eq!(result, None);
}

#[test]
fn oversized_raw_write_fails_atomically() {
    let manager = PtyManager::new();
    let pty = manager.create_pty();
    manager
        .set_discipline(
            pty.master.description.id(),
            LineDisciplineConfig {
                canonical: Some(false),
                echo: Some(false),
                isig: Some(false),
            },
        )
        .expect("set raw mode");

    let error = manager
        .write(
            pty.master.description.id(),
            vec![b'x'; MAX_PTY_BUFFER_BYTES + 1],
        )
        .expect_err("oversized write should fail");
    assert_eq!(error.code(), "EAGAIN");

    manager
        .write(pty.master.description.id(), vec![b'a'; MAX_CANON.min(8)])
        .expect("subsequent small write should still succeed");
    let data = manager
        .read(pty.slave.description.id(), 16)
        .expect("read after failed write")
        .expect("data should be buffered");
    assert_eq!(data, vec![b'a'; MAX_CANON.min(8)]);
}

#[test]
fn canonical_echo_backpressure_does_not_mutate_pending_line() {
    let manager = PtyManager::new();
    let pty = manager.create_pty();

    manager
        .write(pty.slave.description.id(), vec![b'x'; MAX_PTY_BUFFER_BYTES])
        .expect("fill master output buffer");

    let error = manager
        .write(pty.master.description.id(), b"a")
        .expect_err("echo backpressure should reject the input byte");
    assert_eq!(error.code(), "EAGAIN");

    let drained = manager
        .read(pty.master.description.id(), MAX_PTY_BUFFER_BYTES)
        .expect("read full echo buffer")
        .expect("echo buffer should have data");
    assert_eq!(drained.len(), MAX_PTY_BUFFER_BYTES);

    manager
        .write(pty.master.description.id(), b"\n")
        .expect("newline should succeed after draining echo buffer");
    let line = manager
        .read(pty.slave.description.id(), 16)
        .expect("read canonical line")
        .expect("line should be delivered");

    assert_eq!(line, b"\n");
}

#[test]
fn many_pending_reads_are_cleaned_up_when_peer_closes() {
    let manager = PtyManager::new();
    let pty = manager.create_pty();
    let reader_count = 64;
    let mut readers = Vec::new();

    for _ in 0..reader_count {
        let manager = manager.clone();
        let slave_id = pty.slave.description.id();
        readers.push(std::thread::spawn(move || {
            manager
                .read_with_timeout(slave_id, 1, Some(Duration::from_secs(5)))
                .expect("read should finish on peer close")
        }));
    }

    wait_for(
        || manager.pending_read_waiter_count() == reader_count,
        Duration::from_secs(1),
    );

    manager.close(pty.master.description.id());

    for reader in readers {
        assert_eq!(reader.join().expect("reader thread should finish"), None);
    }
    assert_eq!(manager.pending_read_waiter_count(), 0);
    assert_eq!(manager.queued_read_waiter_count(), 0);
}

#[test]
fn many_timed_out_reads_are_removed_from_waiter_queues() {
    let manager = PtyManager::new();
    let pty = manager.create_pty();
    let reader_count = 64;
    let mut readers = Vec::new();

    for _ in 0..reader_count {
        let manager = manager.clone();
        let slave_id = pty.slave.description.id();
        readers.push(std::thread::spawn(move || {
            manager
                .read_with_timeout(slave_id, 1, Some(Duration::from_millis(25)))
                .expect_err("read should time out")
                .code()
        }));
    }

    for reader in readers {
        assert_eq!(
            reader.join().expect("reader thread should finish"),
            "EAGAIN"
        );
    }
    assert_eq!(manager.pending_read_waiter_count(), 0);
    assert_eq!(manager.queued_read_waiter_count(), 0);
}

#[test]
fn set_discipline_only_updates_requested_fields() {
    let manager = PtyManager::new();
    let pty = manager.create_pty();

    manager
        .set_discipline(
            pty.master.description.id(),
            LineDisciplineConfig {
                canonical: Some(false),
                echo: Some(false),
                isig: Some(false),
            },
        )
        .expect("set initial raw mode");
    manager
        .set_discipline(
            pty.master.description.id(),
            LineDisciplineConfig {
                echo: Some(true),
                ..LineDisciplineConfig::default()
            },
        )
        .expect("enable echo only");

    let termios = manager
        .get_termios(pty.master.description.id())
        .expect("read merged termios");
    assert!(!termios.icanon);
    assert!(termios.echo);
    assert!(!termios.isig);
}

#[test]
fn set_termios_only_updates_requested_fields() {
    let manager = PtyManager::new();
    let pty = manager.create_pty();

    manager
        .set_termios(
            pty.master.description.id(),
            PartialTermios {
                echo: Some(false),
                cc: Some(PartialTermiosControlChars {
                    verase: Some(0x08),
                    ..PartialTermiosControlChars::default()
                }),
                ..PartialTermios::default()
            },
        )
        .expect("merge termios update");

    let termios = manager
        .get_termios(pty.master.description.id())
        .expect("read merged termios");
    assert!(termios.icrnl);
    assert!(termios.icanon);
    assert!(!termios.echo);
    assert_eq!(termios.cc.verase, 0x08);
    assert_eq!(termios.cc.vintr, 0x03);
}
