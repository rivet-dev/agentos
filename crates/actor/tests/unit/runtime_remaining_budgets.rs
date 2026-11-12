use super::*;
#[tokio::test]
async fn actual_actor_runtime_evicts_oldest_issues_and_accepts_capacity_above_old_default() {
    let records = Arc::new(std::sync::Mutex::new(Vec::new()));
    let _capture = tracing::subscriber::set_default(Capture(Arc::clone(&records)));
    for (capacity, count) in [(2usize, 3usize), (40, 33)] {
        let owner = Arc::new(
            HostedActorBudgets::normalize(
                serde_json::from_value(
                    serde_json::json!({"retention":{"maxRuntimeIssues":capacity}}),
                )
                .unwrap(),
            )
            .unwrap(),
        );
        let actor = crate::AgentOsActor::with_budgets(
            RuntimeController::new("retention-control", 1),
            owner,
        );
        for n in 0..count {
            actor
                .runtime
                .fail_boot("synthetic", anyhow!("issue-{n}"))
                .await
                .expect_err("synthetic failure remains a failure");
        }
        let status = actor.runtime.status().await;
        assert_eq!(status.issues.len(), count.min(capacity));
        assert_eq!(
            status.issues.first().unwrap().message,
            format!("issue-{}", count.saturating_sub(capacity))
        );
        assert_eq!(
            status.issues.last().unwrap().message,
            format!("issue-{}", count - 1)
        );
    }
    assert!(
        records.lock().unwrap().iter().any(|event| event
            .contains("actor.retention.maxRuntimeIssues")
            && event.contains("evicting oldest runtime issue")
            && event.contains("limit=2")
            && event.contains("observed=3")),
        "actual eviction diagnostic names capacity and attempted occupancy"
    );
}

#[derive(Clone)]
struct Capture(Arc<std::sync::Mutex<Vec<String>>>);
impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Fields(String);
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                use std::fmt::Write;
                write!(&mut self.0, "{}={value:?};", field.name()).unwrap();
            }
        }
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}
