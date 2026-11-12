#[test]
fn blocking_capacity_wait_retries_on_dequeue_and_stops_on_shutdown() {
    for shutdown in [false, true] {
        let runtime = TokioDriver::build(DriverConfig {
            worker_threads: 1,
            blocking_worker_threads: 1,
            max_queued_blocking_jobs: 1,
            max_blocking_jobs: 4,
            max_blocking_job_bytes: 16,
            ..DriverConfig::default()
        })
        .unwrap();
        let handle = runtime.handle();
        let (started, worker_started) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        handle
            .blocking()
            .submit(2, move || {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(2))
                    .expect("release test worker");
            })
            .unwrap();
        worker_started.recv_timeout(Duration::from_secs(1)).unwrap();
        handle
            .blocking()
            .submit(3, || {})
            .expect("hold one test queue slot");
        runtime.block_on(async {
            let waiting = handle.run_blocking_when_available(4, || 42);
            tokio::pin!(waiting);
            assert!(
                std::future::poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx).is_pending()))
                    .await
            );
            assert_eq!(
                handle.resources().usage(ResourceClass::ExecutorSlots).used,
                2
            );
            assert_eq!(handle.blocking().reserved_bytes(), 5);
            if shutdown {
                handle.close_admission();
                let result = tokio::time::timeout(Duration::from_secs(1), waiting)
                    .await
                    .unwrap();
                assert_eq!(result, Err(BlockingJobError::ShuttingDown));
                release.send(()).unwrap();
            } else {
                release.send(()).unwrap();
                let result = tokio::time::timeout(Duration::from_secs(1), waiting)
                    .await
                    .unwrap();
                assert_eq!(result, Ok(42));
                assert_eq!(
                    handle.resources().usage(ResourceClass::ExecutorSlots).used,
                    0
                );
                assert_eq!(handle.blocking().reserved_bytes(), 0);
            }
        });
    }
}

#[test]
fn blocking_capacity_wait_retries_on_ledger_byte_release() {
    let runtime = TokioDriver::build(DriverConfig {
        worker_threads: 1,
        blocking_worker_threads: 1,
        max_queued_blocking_jobs: 1,
        max_blocking_jobs: 4,
        max_blocking_job_bytes: 16,
        ..DriverConfig::default()
    })
    .unwrap();
    let handle = runtime.handle();
    let (started, worker_started) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    handle
        .blocking()
        .submit(16, move || {
            started.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(2)).unwrap();
        })
        .unwrap();
    worker_started.recv_timeout(Duration::from_secs(1)).unwrap();
    runtime.block_on(async {
        let waiting = handle.run_blocking_when_available(1, || 42);
        tokio::pin!(waiting);
        assert!(
            std::future::poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx).is_pending())).await
        );
        assert_eq!(
            handle.resources().usage(ResourceClass::ExecutorSlots).used,
            1
        );
        assert_eq!(handle.blocking().reserved_bytes(), 16);
        release.send(()).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), waiting)
                .await
                .unwrap(),
            Ok(42)
        );
        assert_eq!(
            handle.resources().usage(ResourceClass::ExecutorSlots).used,
            0
        );
        assert_eq!(handle.blocking().reserved_bytes(), 0);
    });
}
