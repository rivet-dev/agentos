/// Exercises the production envelope, sender and every shared receiver primitive
/// with deterministic interleavings; no V8 timing or saturation is required.
#[test]
fn javascript_terminal_delivery_preserves_enqueue_and_retained_ownership() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    let lifecycle = JavascriptExecutionLifecycle::default();
    assert!(
        lifecycle.begin_retained().is_err(),
        "active/pre-result calls have no consumed terminal"
    );
    lifecycle.exited.store(true, Ordering::Release);
    assert!(
        lifecycle.begin_retained().is_err(),
        "result observation before diagnostic/terminal enqueue is not delivery"
    );
    assert_eq!(lifecycle.take_committed_terminal().unwrap(), None);

    // Normal receipt can precede the producer's successful-send continuation.
    let pending = lifecycle.prepare_terminal(3).unwrap();
    assert_eq!(lifecycle.take_committed_terminal().unwrap(), None);
    assert!(lifecycle.begin_retained().is_err());
    let (sender, raw) = flume::bounded(2);
    let receiver = JavascriptEventReceiver(raw);
    sender
        .try_send(QueuedJavascriptExecutionEvent::Terminal(Arc::clone(
            &pending.0,
        )))
        .unwrap();
    assert!(matches!(
        receiver.try_recv(),
        Ok(JavascriptExecutionEvent::Exited(3))
    ));
    lifecycle.begin_retained().unwrap();
    assert!(
        lifecycle.begin_retained().is_err(),
        "a second submit cannot reuse the prior claim"
    );
    let successor = lifecycle.prepare_terminal(9).unwrap();
    pending.0.commit().unwrap();
    assert_eq!(
        lifecycle.take_committed_terminal().unwrap(),
        None,
        "old continuation must not replace the successor pointer"
    );
    sender
        .try_send(QueuedJavascriptExecutionEvent::Terminal(Arc::clone(
            &successor.0,
        )))
        .unwrap();
    successor.0.commit().unwrap();
    assert_eq!(lifecycle.take_committed_terminal().unwrap(), Some(9));
    assert_eq!(lifecycle.take_committed_terminal().unwrap(), None);
    assert!(matches!(
        receiver.try_recv(),
        Err(flume::TryRecvError::Empty)
    ));
    drop((pending, successor, sender));
    assert!(matches!(
        receiver.try_recv(),
        Err(flume::TryRecvError::Disconnected)
    ));

    // Teardown consumes only terminal ownership; the queued output stays in
    // exactly its original position with its original payload/allocation.
    let lifecycle = JavascriptExecutionLifecycle::default();
    let (sender, raw) = flume::bounded(4);
    let receiver = JavascriptEventReceiver(raw);
    let stdout = vec![b'a', b'b'];
    let stdout_ptr = stdout.as_ptr();
    sender
        .try_send(QueuedJavascriptExecutionEvent::Event(
            JavascriptExecutionEvent::Stdout(stdout),
        ))
        .unwrap();
    let old = lifecycle.prepare_terminal(7).unwrap();
    sender
        .try_send(QueuedJavascriptExecutionEvent::Terminal(Arc::clone(&old.0)))
        .unwrap();
    old.0.commit().unwrap();
    assert!(
        lifecycle.begin_retained().is_err(),
        "committed but unclaimed is still owned"
    );
    assert_eq!(lifecycle.take_committed_terminal().unwrap(), Some(7));
    lifecycle.begin_retained().unwrap();
    let next = lifecycle.prepare_terminal(5).unwrap();
    sender
        .try_send(QueuedJavascriptExecutionEvent::Event(
            JavascriptExecutionEvent::Stderr(vec![b'e']),
        ))
        .unwrap();
    sender
        .try_send(QueuedJavascriptExecutionEvent::Terminal(Arc::clone(
            &next.0,
        )))
        .unwrap();
    next.0.commit().unwrap();
    match receiver.recv().unwrap() {
        JavascriptExecutionEvent::Stdout(bytes) => {
            assert_eq!(bytes, b"ab");
            assert_eq!(
                bytes.as_ptr(),
                stdout_ptr,
                "no output clone or replacement buffer"
            );
        }
        other => panic!("original stdout must remain first: {other:?}"),
    }
    assert!(
        matches!(receiver.recv_timeout(Duration::ZERO), Ok(JavascriptExecutionEvent::Stderr(bytes)) if bytes == b"e")
    );
    assert!(matches!(
        receiver.recv(),
        Ok(JavascriptExecutionEvent::Exited(5))
    ));
    assert_eq!(lifecycle.take_committed_terminal().unwrap(), None);
    assert!(matches!(
        receiver.recv_timeout(Duration::ZERO),
        Err(flume::RecvTimeoutError::Timeout)
    ));
    drop((old, next, sender));
    assert!(matches!(
        receiver.recv(),
        Err(flume::RecvError::Disconnected)
    ));

    let runtime = crate::test_runtime_context();
    runtime.tokio_handle().block_on(async {
        let lifecycle = JavascriptExecutionLifecycle::default();
        let (sender, raw) = flume::bounded(2);
        let receiver = JavascriptEventReceiver(raw);
        let gauge = register_queue(TrackedLimit::JavascriptEventChannel, 2);
        let notify = Notify::new();
        assert!(send_javascript_event_async(&sender, &gauge, &lifecycle, Some(&notify), JavascriptExecutionEvent::Exited(11)).await);
        // A real wake must never precede eligibility, even when teardown has
        // not consumed the ordinary FIFO terminal yet.
        let mut notified = Box::pin(notify.notified());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(notified.as_mut().poll(&mut cx), Poll::Ready(())));
        assert_eq!(lifecycle.take_committed_terminal().unwrap(), Some(11));
        assert!(time::timeout(Duration::ZERO, receiver.recv_async()).await.is_err());
        assert!(send_javascript_event_async(&sender, &gauge, &lifecycle, None, JavascriptExecutionEvent::Stdout(vec![b'x'])).await);
        assert!(matches!(receiver.recv_async().await, Ok(JavascriptExecutionEvent::Stdout(bytes)) if bytes == b"x"));
        drop(sender);
        assert!(matches!(receiver.recv_async().await, Err(flume::RecvError::Disconnected)));

        // A closed receiver cannot produce a committed terminal proof.
        let lifecycle = JavascriptExecutionLifecycle::default();
        let (sender, receiver) = flume::bounded(1);
        drop(receiver);
        let gauge = register_queue(TrackedLimit::JavascriptEventChannel, 1);
        assert!(!send_javascript_event_async(&sender, &gauge, &lifecycle, None, JavascriptExecutionEvent::Exited(4)).await);
        assert_eq!(lifecycle.take_committed_terminal().unwrap(), None);
        assert!(lifecycle.begin_retained().is_err());

        // Cancelling a genuinely blocked send does not create proof or remove
        // the full queue's output. No producer is allowed to spin or drop it.
        let lifecycle = JavascriptExecutionLifecycle::default();
        let (sender, raw) = flume::bounded(1);
        let receiver = JavascriptEventReceiver(raw);
        sender.try_send(QueuedJavascriptExecutionEvent::Event(JavascriptExecutionEvent::Stdout(vec![b'z']))).unwrap();
        let mut send = Box::pin(send_javascript_event_async(&sender, &gauge, &lifecycle, None, JavascriptExecutionEvent::Exited(6)));
        assert!(matches!(send.as_mut().poll(&mut cx), Poll::Pending));
        assert_eq!(lifecycle.take_committed_terminal().unwrap(), None);
        drop(send);
        assert_eq!(lifecycle.take_committed_terminal().unwrap(), None);
        assert!(lifecycle.begin_retained().is_err());
        assert!(matches!(receiver.try_recv(), Ok(JavascriptExecutionEvent::Stdout(bytes)) if bytes == b"z"));
        assert!(matches!(receiver.try_recv(), Err(flume::TryRecvError::Empty)));

        // Conversely, possession of an actual envelope is valid normal
        // delivery even if the sending continuation was cancelled.
        let lifecycle = JavascriptExecutionLifecycle::default();
        let pending = lifecycle.prepare_terminal(12).unwrap();
        let (sender, raw) = flume::bounded(1);
        let receiver = JavascriptEventReceiver(raw);
        sender.try_send(QueuedJavascriptExecutionEvent::Terminal(Arc::clone(&pending.0))).unwrap();
        drop(pending);
        assert_eq!(lifecycle.take_committed_terminal().unwrap(), None);
        assert!(matches!(receiver.recv_async().await, Ok(JavascriptExecutionEvent::Exited(12))));
        lifecycle.begin_retained().unwrap();
        assert_eq!(lifecycle.take_committed_terminal().unwrap(), None);
    });
}
