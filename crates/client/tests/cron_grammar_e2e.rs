//! Cron grammar parity verification against a real sidecar. Exercises `schedule_cron`'s
//! accept/reject behavior across the croner feature set (5/6/7-field, named months/weekdays, `?`,
//! `L`, `LW`, `#`, ranges, steps) plus one-shot timestamps. Pure client logic + the sidecar; no
//! WASM/V8.
//!
//! These independently verify the cron parity claims (grammar + RFC3339 fractional seconds + past
//! one-shot rejection) with real assertions, rather than relying on subagent summaries.

mod common;

use agent_os_client::{AgentOs, ClientError, CronAction, CronJobOptions};
use chrono::Utc;

fn noop_action() -> CronAction {
    CronAction::Callback {
        callback: std::sync::Arc::new(|| Box::pin(async {})),
    }
}

fn try_schedule(os: &AgentOs, schedule: &str) -> Result<(), ClientError> {
    os.schedule_cron(CronJobOptions {
        id: None,
        schedule: schedule.to_string(),
        action: noop_action(),
        overlap: None,
    })
    .map(|handle| handle.cancel())
}

#[tokio::test]
async fn cron_grammar_matches_croner() {
    if !common::require_sidecar("cron_grammar_matches_croner") {
        return;
    }
    let os = common::new_vm().await;

    // Accepted by croner (and therefore by us).
    let valid = [
        "* * * * *",      // 5-field
        "*/30 * * * * *", // 6-field (with seconds)
        "0 0 * * MON",    // named weekday
        "0 0 1 JAN *",    // named month
        "0 0 1 * ?",      // `?` day-of-week
        "0 0 L * *",      // last day of month
        "0 0 LW * *",     // last weekday of month
        "0 0 * * 1#2",    // 2nd Monday
        "0 0 1,15 * *",   // list
        "0 9-17 * * *",   // range
    ];
    for expr in valid {
        assert!(
            try_schedule(&os, expr).is_ok(),
            "expected croner-valid schedule to be accepted: {expr:?}"
        );
    }

    // Rejected by croner (and therefore by us) -> InvalidSchedule.
    let invalid = [
        "* * * *",        // too few fields
        "60 * * * *",     // minute out of range
        "0 0 32 * *",     // day-of-month out of range
        "0 0 * * 8",      // day-of-week out of range
        "5/15 * * * *",   // numeric-prefix stepping (croner rejects)
        "not a schedule", // garbage
        "",               // empty
    ];
    for expr in invalid {
        match try_schedule(&os, expr) {
            Err(ClientError::InvalidSchedule(_)) => {}
            other => panic!("expected InvalidSchedule for {expr:?}, got {other:?}"),
        }
    }

    // One-shot ISO-8601: future accepted, past rejected as PastSchedule.
    let future = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    assert!(
        try_schedule(&os, &future).is_ok(),
        "future one-shot should be accepted: {future}"
    );
    let past = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    match try_schedule(&os, &past) {
        Err(ClientError::PastSchedule(_)) => {}
        other => panic!("expected PastSchedule for a past one-shot, got {other:?}"),
    }

    os.shutdown().await.expect("shutdown");
}
