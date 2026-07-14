//! Cron e2e against a real `agentos-sidecar`. The sidecar owns schedule and run
//! state; the client only arms its returned alarm and routes the callback.
//!
//! Covers: a near-future one-shot callback actually fires and emits Fire/Complete events, and the
//! schedule/list/cancel registry surface for a recurring job.

mod common;

use std::sync::Arc;
use std::time::Duration;

use agentos_client::{CronAction, CronEvent, CronJobOptions};
use chrono::Utc;
use futures::StreamExt;

#[tokio::test]
async fn cron_callback_fires_and_registry_round_trips() {
    if !common::require_sidecar("cron_callback_fires_and_registry_round_trips") {
        return;
    }
    // Each `tokio::test` owns a runtime, so keep this test's transport in its own pool.
    let os = common::new_vm_with_sidecar_pool("cron-e2e-success").await;

    // Subscribe to cron events before scheduling so the Fire/Complete cannot be missed.
    let mut events = os.cron_events();

    // One-shot ~1s in the future, with an explicit offset so the timestamp is unambiguous.
    let notify = Arc::new(tokio::sync::Notify::new());
    let notify_cb = notify.clone();
    let when = (Utc::now() + chrono::Duration::seconds(1)).to_rfc3339();

    let handle = os
        .schedule_cron(CronJobOptions {
            id: Some("oneshot-test".to_string()),
            schedule: when,
            action: CronAction::Callback {
                callback: Arc::new(move || {
                    let notify = notify_cb.clone();
                    Box::pin(async move {
                        notify.notify_one();
                        Ok(())
                    })
                }),
            },
            overlap: None,
        })
        .await
        .expect("schedule one-shot");
    assert_eq!(handle.id, "oneshot-test");

    // The callback must actually run.
    tokio::time::timeout(Duration::from_secs(8), notify.notified())
        .await
        .expect("cron callback should fire within 8s");

    // And the manager must have emitted a Fire event for this job (then a Complete).
    let mut saw_fire = false;
    let mut saw_complete = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline && !(saw_fire && saw_complete) {
        match tokio::time::timeout(Duration::from_millis(500), events.next()).await {
            Ok(Some(Ok(CronEvent::Fire { job_id, .. }))) if job_id == "oneshot-test" => {
                saw_fire = true
            }
            Ok(Some(Ok(CronEvent::Complete { job_id, .. }))) if job_id == "oneshot-test" => {
                saw_complete = true
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(_))) | Ok(None) | Err(_) => break,
        }
    }
    assert!(saw_fire, "expected a cron:fire event for the one-shot");
    assert!(
        saw_complete,
        "expected a cron:complete event for the one-shot"
    );
    os.cancel_cron_job(&handle.id)
        .await
        .expect("remove completed one-shot");

    // Registry surface: schedule a recurring job (won't fire during the test), see it listed, cancel
    // it, and confirm it's gone.
    let recurring = os
        .schedule_cron(CronJobOptions {
            id: Some("daily-test".to_string()),
            schedule: "0 0 * * *".to_string(),
            action: CronAction::Callback {
                callback: Arc::new(|| Box::pin(async { Ok(()) })),
            },
            overlap: None,
        })
        .await
        .expect("schedule recurring");
    assert!(
        os.list_cron_jobs()
            .await
            .expect("list cron jobs")
            .iter()
            .any(|j| j.id == "daily-test"),
        "recurring job should be listed"
    );
    os.cancel_cron_job(&recurring.id)
        .await
        .expect("cancel recurring job");
    assert!(
        !os.list_cron_jobs()
            .await
            .expect("list cron jobs after cancel")
            .iter()
            .any(|j| j.id == "daily-test"),
        "cancelled job should be gone"
    );

    // A durable host stores sidecar state opaquely. Cancelling the live copy
    // makes the same scheduler empty so this test can prove import restores it.
    let durable = os
        .schedule_cron(CronJobOptions {
            id: Some("durable-state-test".to_string()),
            schedule: "0 0 * * *".to_string(),
            action: CronAction::Exec {
                command: "true".to_string(),
                args: Vec::new(),
            },
            overlap: None,
        })
        .await
        .expect("schedule durable job");
    let state = os.export_cron_state().await.expect("export cron state");
    os.cancel_cron_job(&durable.id)
        .await
        .expect("clear live cron state");
    os.import_cron_state(state)
        .await
        .expect("restore cron state");
    assert!(
        os.list_cron_jobs()
            .await
            .expect("list restored cron jobs")
            .iter()
            .any(|job| job.id == durable.id),
        "opaque sidecar state should restore the durable job"
    );
    os.cancel_cron_job(&durable.id)
        .await
        .expect("cancel restored job");

    os.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn failed_cron_callback_is_recorded_as_error() {
    if !common::require_sidecar("failed_cron_callback_is_recorded_as_error") {
        return;
    }
    // Each `tokio::test` owns a runtime, so keep this test's transport in its own pool.
    let os = common::new_vm_with_sidecar_pool("cron-e2e-failure").await;
    let mut events = os.cron_events();
    let job_id = "failed-callback-test";
    let when = (Utc::now() + chrono::Duration::seconds(1)).to_rfc3339();
    let handle = os
        .schedule_cron(CronJobOptions {
            id: Some(job_id.to_string()),
            schedule: when,
            action: CronAction::Callback {
                callback: Arc::new(|| {
                    Box::pin(async { Err(String::from("rust callback failed")) })
                }),
            },
            overlap: None,
        })
        .await
        .expect("schedule failing callback");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    let mut saw_fire = false;
    let mut saw_error = false;
    while tokio::time::Instant::now() < deadline && !saw_error {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, events.next()).await {
            Ok(Some(Ok(CronEvent::Fire { job_id: id, .. }))) if id == job_id => {
                saw_fire = true;
            }
            Ok(Some(Ok(CronEvent::Error {
                job_id: id, error, ..
            }))) if id == job_id => {
                assert!(saw_fire, "cron:error must follow cron:fire");
                assert_eq!(error, "rust callback failed");
                saw_error = true;
            }
            Ok(Some(Ok(CronEvent::Complete { job_id: id, .. }))) if id == job_id => {
                panic!("failed callback was recorded as cron:complete");
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(error))) => panic!("cron event stream failed: {error}"),
            Ok(None) => panic!("cron event stream closed"),
            Err(_) => break,
        }
    }
    assert!(saw_fire, "expected cron:fire for the failing callback");
    assert!(saw_error, "expected cron:error for the failing callback");

    let job = os
        .list_cron_jobs()
        .await
        .expect("list failed callback job")
        .into_iter()
        .find(|job| job.id == job_id)
        .expect("failed callback job remains listed");
    assert_eq!(job.run_count, 1);
    assert!(!job.running);

    handle.cancel().await.expect("cancel failed callback job");
    os.shutdown().await.expect("shutdown");
}
