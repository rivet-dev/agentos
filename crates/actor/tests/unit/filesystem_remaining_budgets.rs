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
fn actual_collection_and_directory_guards_accept_raised_finite_budgets() {
    let default = budgets(serde_json::json!({}));
    let raised = budgets(
        serde_json::json!({"filesystem":{"maxBatchPaths":256,"maxDirectoryEntries":5000,"maxDirectoryResultBytes":1048576,"maxRecursiveDepth":128}}),
    );
    let paths = vec!["/a".into(); 129];
    violation(
        &validate_paths(&paths, &default).unwrap_err(),
        "actor.filesystem.maxBatchPaths",
    );
    validate_paths(&paths, &raised).unwrap();
    violation(
        &validate_directory_output(4097, 0, &default).unwrap_err(),
        "actor.filesystem.maxDirectoryEntries",
    );
    validate_directory_output(4097, 524289, &raised).unwrap();
    violation(
        &validate_directory_output(1, 524289, &default).unwrap_err(),
        "actor.filesystem.maxDirectoryResultBytes",
    );
    violation(
        &validate_recursive_depth(Some(65), &default).unwrap_err(),
        "actor.filesystem.maxRecursiveDepth",
    );
    validate_recursive_depth(Some(65), &raised).unwrap();
    validate_recursive_depth(None, &raised).unwrap();
    validate_recursive_depth(Some(0), &raised).unwrap();
    violation(
        &validate_recursive_depth(Some(129), &raised).unwrap_err(),
        "actor.filesystem.maxRecursiveDepth",
    );
}
