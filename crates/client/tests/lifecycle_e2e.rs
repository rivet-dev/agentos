//! Lifecycle e2e against a real `agentos-sidecar`: independent VMs, post-shutdown isolation, and
//! idempotent shutdown. No V8/WASM required.

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
}
