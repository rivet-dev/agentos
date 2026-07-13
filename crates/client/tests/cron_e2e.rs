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

#[tokio::test]
async fn cron_callback_fires_and_registry_round_trips() {
    if !common::require_sidecar("cron_callback_fires_and_registry_round_trips") {
        return;
    }
    let os = common::new_vm().await;

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
        match tokio::time::timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Ok(CronEvent::Fire { job_id, .. })) if job_id == "oneshot-test" => saw_fire = true,
            Ok(Ok(CronEvent::Complete { job_id, .. })) if job_id == "oneshot-test" => {
                saw_complete = true
            }
            Ok(Ok(_)) => {}
            Ok(Err(_)) | Err(_) => break,
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
                callback: Arc::new(|| Box::pin(async {})),
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
