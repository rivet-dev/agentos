//! Shared sidecars must not inherit the lifetime of an unrelated caller runtime.
mod common;

#[test]
fn shared_pool_survives_another_runtime_shutdown() {
    if !common::require_sidecar("shared_pool_survives_another_runtime_shutdown") {
        return;
    }
    let runtime = || {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("test runtime")
    };
    let first_runtime = runtime();
    let second_runtime = runtime();
    let pool = format!("runtime-lifecycle-{}", uuid::Uuid::new_v4());
    let first = first_runtime.block_on(common::new_vm_with_sidecar_pool(pool.clone()));
    let second = second_runtime.block_on(common::new_vm_with_sidecar_pool(pool));
    first_runtime
        .block_on(first.shutdown())
        .expect("shutdown first VM");
    drop(first_runtime);

    let result = second_runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            second
                .write_file("/tmp/still-live", "second runtime")
                .await?;
            second.read_file("/tmp/still-live").await
        })
        .await
    });
    // Dispose the transport even on failure so this regression cannot orphan a child.
    second_runtime
        .block_on(second.sidecar().dispose())
        .expect("dispose second sidecar");
    assert_eq!(
        result
            .expect("surviving runtime must continue receiving responses")
            .expect("filesystem request"),
        b"second runtime"
    );
}
