use super::*;
fn budgets(value: serde_json::Value) -> crate::HostedActorBudgets {
    crate::HostedActorBudgets::normalize(serde_json::from_value(value).unwrap()).unwrap()
}
fn violation(error: &anyhow::Error, setting: &str) {
    let error = error
        .chain()
        .find_map(|e| e.downcast_ref::<agentos_actor_contract::ActionBudgetViolation>())
        .expect("typed owning worker budget");
    assert_eq!(error.setting, setting);
    assert!(error.to_string().contains("--actor-budgets-json"));
}

#[test]
fn filesystem_paths_and_exclude_entries_use_worker_path_owner() {
    let default = budgets(serde_json::json!({}));
    let raised = budgets(serde_json::json!({"paths":{"maxPathBytes":8192}}));
    let path = format!("/{}", "a".repeat(4096));
    violation(
        &validate_path(&path, &default).unwrap_err(),
        "actor.paths.maxPathBytes",
    );
    validate_path(&path, &raised).unwrap();
    validate_string("exclude entry", &path, &raised).unwrap();
    assert!(validate_path("", &raised).is_err());
}
