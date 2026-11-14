//! Overload e2e against a real `agentos-native-sidecar`: a request spike far beyond the sidecar's
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
    for result in results {
        match result {
            Ok(false) => succeeded += 1,
            Ok(true) => panic!("spike paths do not exist"),
            Err(error) => match error.downcast_ref::<ClientError>() {
                Some(ClientError::ResourceLimit { details, .. }) => {
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
    assert!(
        succeeded >= SPIKE / 2,
        "most of the spike must be served, got {succeeded}/{SPIKE}"
    );

    busy.shutdown().await.expect("dispose the busy VM");
}
