use super::*;
fn budgets(value: serde_json::Value) -> crate::HostedActorBudgets {
    crate::HostedActorBudgets::normalize(serde_json::from_value(value).unwrap()).unwrap()
}
fn violation(error: &anyhow::Error, setting: &str) {
    let value = error
        .chain()
        .find_map(|e| e.downcast_ref::<agentos_actor_contract::ActionBudgetViolation>())
        .expect("typed budget failure");
    assert_eq!(value.setting, setting);
    assert!(value.to_string().contains("--actor-budgets-json"));
}

#[test]
fn cron_admission_history_and_error_retention_use_the_worker_owner() {
    let default = budgets(serde_json::json!({}));
    let raised = budgets(
        serde_json::json!({"cron":{"maxJobs":1100,"maxHistory":512,"maxErrorBytes":20000,"maxNameBytes":512,"maxExpressionBytes":512,"maxTimezoneBytes":256,"maxArgumentBytes":131072}}),
    );
    violation(
        &check_cron_capacity(1024, false, &default).unwrap_err(),
        "actor.cron.maxJobs",
    );
    check_cron_capacity(1024, false, &raised).unwrap();
    check_cron_capacity(1100, true, &raised).unwrap();
    assert_eq!(
        select_cron_history(None, &raised).unwrap(),
        32,
        "caller default is unchanged"
    );
    violation(
        &select_cron_history(Some(257), &default).unwrap_err(),
        "actor.cron.maxHistory",
    );
    assert_eq!(select_cron_history(Some(257), &raised).unwrap(), 257);
    let disabled = budgets(serde_json::json!({"cron":{"maxHistory":0,"maxErrorBytes":7}}));
    assert_eq!(select_cron_history(Some(0), &disabled).unwrap(), 0);
    violation(
        &select_cron_history(None, &disabled).unwrap_err(),
        "actor.cron.maxHistory",
    );
    assert!(select_cron_history(Some(-1), &raised).is_err());
    let text = "x".repeat(16385);
    assert!(bounded_error(&text, &default).ends_with('…'));
    assert_eq!(bounded_error(&text, &raised), text);
    assert_eq!(
        bounded_error("🦀🦀", &disabled),
        "🦀…",
        "preserve UTF-8 prefix and existing ellipsis semantics"
    );
    violation(
        &validate_nonempty_bytes("cron name", &"n".repeat(257), default.cron.max_name_bytes)
            .unwrap_err(),
        "actor.cron.maxNameBytes",
    );
    validate_nonempty_bytes("cron name", &"n".repeat(257), raised.cron.max_name_bytes).unwrap();
}
