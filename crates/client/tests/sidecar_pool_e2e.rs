//! Shared-sidecar pooling e2e against a real sidecar. Verifies that two VMs created with the default
//! (shared "default" pool) config reuse a single sidecar process, report a shared `active_vm_count`,
//! stay isolated, and release independently. No V8/WASM required.

mod common;

use agent_os_client::fs::FileContent;

#[tokio::test]
async fn shared_sidecar_pooling_reuses_one_process() {
    if !common::sidecar_available() {
        eprintln!("skipping shared_sidecar_pooling_reuses_one_process: sidecar not built");
        return;
    }

    // Default config => shared "default" pool, so both VMs land on one sidecar process.
    let a = common::new_vm().await;
    let b = common::new_vm().await;

    let desc_a = a.sidecar().describe();
    let desc_b = b.sidecar().describe();
    assert_eq!(
        desc_a.sidecar_id, desc_b.sidecar_id,
        "both VMs should share the same pooled sidecar"
    );
    assert_eq!(
        desc_a.active_vm_count, 2,
        "the shared sidecar should report 2 active VMs"
    );

    // Sharing a process must not break VM isolation.
    a.write_file("/tmp/who", FileContent::Text("A".to_string()))
        .await
        .expect("write A");
    b.write_file("/tmp/who", FileContent::Text("B".to_string()))
        .await
        .expect("write B");
    assert_eq!(a.read_file("/tmp/who").await.expect("read A"), b"A");
    assert_eq!(b.read_file("/tmp/who").await.expect("read B"), b"B");

    // Releasing one VM leaves the sibling working and drops the shared count to 1.
    a.shutdown().await.expect("shutdown A");
    assert_eq!(
        a.sidecar().describe().active_vm_count,
        1,
        "active_vm_count should drop to 1 after one VM releases"
    );
    assert_eq!(b.read_file("/tmp/who").await.expect("B still live"), b"B");

    b.shutdown().await.expect("shutdown B");
}
