use agentos_driver_tokio::{DriverConfig, TokioDriver};
use agentos_executor_contract::backend::{
    bounded_execution_event_channel, ExecutionEvent, ExecutionEventReceiver, HostCallReply,
    HostServiceError, PayloadLimit,
};
use agentos_executor_contract::host::{HostProcessContext, ProcessHostCapabilitySet};
use agentos_executor_obscura_experiment::ExecutorBrowserHost;
use obscura_net::host::BrowserHost;
use std::future::poll_fn;
use std::path::Path;
use std::sync::Arc;
use std::task::Poll;

fn fixture() -> (ExecutorBrowserHost, ExecutionEventReceiver) {
    let (events, receiver) = bounded_execution_event_channel(
        HostProcessContext {
            generation: 5,
            pid: 7,
        },
        1,
        PayloadLimit::new("test.queueBytes", 4096).unwrap(),
        Arc::new(|| {}),
    )
    .unwrap();
    (
        ExecutorBrowserHost::new(
            ProcessHostCapabilitySet::from_event_submission(events),
            1024,
        )
        .unwrap(),
        receiver,
    )
}

#[test]
fn dispatcher_teardown_settles_a_pending_browser_read() {
    let driver = TokioDriver::process(&DriverConfig::default()).unwrap();
    driver.block_on(async {
        let (host, receiver) = fixture();
        let mut read = host.read_file(Path::new("/fixture.html"), 1024);
        poll_fn(|cx| {
            assert!(read.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(receiver);
        let error = read.await.unwrap_err().to_string();
        assert!(error.contains("ECANCELED"), "{error}");
    });
}

#[test]
fn queue_full_rejects_new_browser_work_without_losing_the_first_reply() {
    let driver = TokioDriver::process(&DriverConfig::default()).unwrap();
    driver.block_on(async {
        let (host, receiver) = fixture();
        let mut first = host.read_file(Path::new("/one.html"), 1024);
        poll_fn(|cx| {
            assert!(first.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        let rejected = host
            .read_file(Path::new("/two.html"), 1024)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            rejected.contains("EAGAIN") && rejected.contains("pendingEventCount"),
            "{rejected}"
        );
        let ExecutionEvent::HostCall { reply, .. } = receiver.try_recv().unwrap().unwrap() else {
            panic!("expected the first host call");
        };
        assert_eq!(reply.identity().generation, 5);
        assert_eq!(reply.identity().pid, 7);
        assert_eq!(reply.identity().call_id, 1);
        reply
            .succeed(HostCallReply::Raw(b"first response".to_vec()))
            .unwrap();
        assert_eq!(first.await.unwrap(), b"first response");
        assert!(receiver.try_recv().unwrap().is_none());
    });
}

#[test]
fn canceled_consumer_cannot_claim_a_late_host_reply() {
    let driver = TokioDriver::process(&DriverConfig::default()).unwrap();
    driver.block_on(async {
        let (host, receiver) = fixture();
        let mut read = host.read_file(Path::new("/fixture.html"), 1024);
        poll_fn(|cx| {
            assert!(read.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        let ExecutionEvent::HostCall { reply, .. } = receiver.try_recv().unwrap().unwrap() else {
            panic!("expected a host call");
        };
        drop(read);
        assert!(!reply.claim().unwrap());
        let error = reply
            .fail(HostServiceError::new("ECANCELED", "fixture process ended"))
            .unwrap_err();
        assert_eq!(error.code, "EALREADY");
    });
}
