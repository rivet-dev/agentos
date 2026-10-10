//! Lifecycle e2e against a real `agentos-native-sidecar`: independent VMs, post-shutdown isolation, and
//! idempotent shutdown, and cancellation of execution waits.

mod common;

use agentos_client::fs::FileContent;

#[tokio::test]
async fn lifecycle_independent_vms_and_idempotent_shutdown() {
    if !common::require_sidecar("lifecycle_independent_vms_and_idempotent_shutdown") {
        return;
    }

    // Two independent VMs (each its own sidecar child + handshake).
    let a = common::new_vm().await;
    let b = common::new_vm().await;

    a.write_file("/tmp/who.txt", FileContent::Text("A".to_string()))
        .await
        .expect("write A");
    b.write_file("/tmp/who.txt", FileContent::Text("B".to_string()))
        .await
        .expect("write B");

    assert_eq!(a.read_file("/tmp/who.txt").await.expect("read A"), b"A");
    assert_eq!(b.read_file("/tmp/who.txt").await.expect("read B"), b"B");

    // Shutting down A must not affect B.
    a.shutdown().await.expect("shutdown A");
    assert_eq!(
        b.read_file("/tmp/who.txt").await.expect("B still live"),
        b"B"
    );

    // Shutdown is idempotent.
    b.shutdown().await.expect("shutdown B");
    b.shutdown().await.expect("shutdown B again (idempotent)");

    shutdown_rejects_pending_execution_without_affecting_shared_sibling().await;
}

async fn shutdown_rejects_pending_execution_without_affecting_shared_sibling() {
    use agentos_client::error::ClientError;
    use std::time::Duration;

    if !common::require_sidecar(
        "shutdown_rejects_pending_execution_without_affecting_shared_sibling",
    ) {
        return;
    }
    let a = common::new_vm_with_sidecar_pool("lifecycle-execution-disposal").await;
    let b = agentos_client::AgentOs::create(agentos_client::config::AgentOsConfig {
        max_pending_execution_waits: Some(1),
        sidecar: Some(agentos_client::config::AgentOsSidecarConfig::Shared {
            pool: Some("lifecycle-execution-disposal".into()),
        }),
        ..Default::default()
    })
    .await
    .expect("create bounded sibling VM");
    let execution = tokio::spawn({
        let a = a.clone();
        async move {
            a.execute_javascript(
                "import fs from 'node:fs'; fs.writeFileSync('/tmp/execution-started', 'yes'); await new Promise(() => {});",
                Default::default(),
            ).await
        }
    });
    let started = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if a.read_file("/tmp/execution-started").await.is_ok() {
                break;
            }
            assert!(
                !execution.is_finished(),
                "execution ended before reaching the completion wait"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let shutdown = tokio::spawn({
        let a = a.clone();
        async move { a.shutdown().await }
    });
    let outcome = tokio::time::timeout(Duration::from_secs(1), execution).await;
    let cleanup = shutdown.await.expect("shutdown task");
    let sibling = b.execute_javascript("42", Default::default()).await;
    let process = b
        .spawn_javascript("await new Promise(() => {});", Default::default())
        .await
        .expect("spawn language process");
    let mut waiting = Box::pin(b.wait_process(process.pid));
    assert!(
        tokio::time::timeout(Duration::from_millis(1), waiting.as_mut())
            .await
            .is_err()
    );
    let over_capacity = b.execute_javascript("42", Default::default()).await;
    let shutdown = b.shutdown();
    let (wait_result, shutdown_result) = tokio::join!(waiting, shutdown);
    shutdown_result.expect("shutdown sibling");
    assert!(
        matches!(wait_result, Err(ClientError::VmDisposed)),
        "background wait result: {wait_result:?}"
    );
    assert!(
        matches!(
            over_capacity,
            Err(ClientError::ExecutionWaitLimit { limit: 1 })
        ),
        "background wait must count toward SDK capacity: {over_capacity:?}"
    );
    started.expect("guest reached its completion wait");
    assert!(matches!(
        outcome
            .expect("execution wait must settle promptly")
            .expect("execution task"),
        Err(ClientError::VmDisposed)
    ));
    cleanup.expect("native VM teardown");
    assert!(sibling.is_ok(), "sibling VM remains usable: {sibling:?}");
    assert!(matches!(
        a.execute_javascript("42", Default::default()).await,
        Err(ClientError::VmDisposed)
    ));
}
