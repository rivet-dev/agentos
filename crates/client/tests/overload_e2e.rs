//! Overload e2e against a real `agentos-sidecar`: a request spike far beyond the sidecar's
//! in-flight limit waits in the sidecar request queue, fails only with typed retryable errors, and
//! never stops another VM on the same sidecar from being disposed.

mod common;

use agentos_client::ClientError;

const SPIKE: usize = 5_000;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn request_spike_fails_only_with_typed_retryable_errors_and_dispose_still_succeeds() {
    if !common::require_sidecar(
        "request_spike_fails_only_with_typed_retryable_errors_and_dispose_still_succeeds",
    ) {
        return;
    }
    let busy = common::new_vm_with_sidecar_pool("overload-spike").await;
    let target = common::new_vm_with_sidecar_pool("overload-spike").await;

    let paths = (0..SPIKE)
        .map(|index| format!("/tmp/spike-{index}"))
        .collect::<Vec<_>>();
    let spike = futures::future::join_all(paths.iter().map(|path| busy.exists(path)));
    let (results, disposal) = tokio::join!(spike, target.shutdown());

    disposal.expect("dispose succeeds while a spike saturates the sidecar");
    let mut succeeded = 0;
    let mut rejected_paths = Vec::new();
    for (path, result) in paths.iter().zip(results) {
        match result {
            Ok(false) => succeeded += 1,
            Ok(true) => panic!("spike paths do not exist"),
            Err(error) => match error.downcast_ref::<ClientError>() {
                Some(ClientError::ResourceLimit { details, .. }) => {
                    rejected_paths.push(path);
                    assert_eq!(details.retryable, Some(true));
                    assert_eq!(details.errno.as_deref(), Some("EAGAIN"));
                    assert!(details
                        .configuration_path
                        .as_deref()
                        .is_some_and(|path| path.starts_with("runtime.protocol.")));
                }
                _ => panic!("spike request failed with an untyped error: {error}"),
            },
        }
    }
    // A burst larger than the bounded ingress and request queues has no
    // guaranteed first-attempt success ratio: that depends on scheduling.
    // Require actual progress, then prove every typed rejection is recoverable
    // after the burst drains, without changing any queue limit.
    assert!(succeeded > 0, "the burst must make progress");
    for path in rejected_paths {
        assert!(!busy
            .exists(path)
            .await
            .expect("retry after overload drains"));
        succeeded += 1;
    }
    assert_eq!(succeeded, SPIKE, "every request must eventually succeed");

    busy.shutdown().await.expect("dispose the busy VM");
}
