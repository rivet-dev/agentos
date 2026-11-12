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
fn actual_language_options_use_shared_execution_and_language_budgets() {
    let default = budgets(serde_json::json!({}));
    let raised = budgets(
        serde_json::json!({"execution":{"maxArguments":1100,"maxArgumentBytes":20000,"maxEnvironmentEntries":1100,"maxEnvironmentBytes":524288,"maxStdinBytes":524288,"maxTerminalDimension":5000},"language":{"maxTimeoutMs":310000,"maxSourceBytes":786432,"maxContextIdBytes":512,"maxJsonBytes":524288,"maxPackages":300,"maxIndexUrls":20},"paths":{"maxPathBytes":8192}}),
    );
    let options = ActorLanguageExecutionOptions {
        args: vec![String::new(); 1025],
        ..Default::default()
    };
    violation(
        &options.validate(&default).unwrap_err(),
        "actor.execution.maxArguments",
    );
    options.validate(&raised).unwrap();
    validate_arguments(&[String::new()], &default).unwrap();
    let source = "x".repeat(512 * 1024 + 1);
    violation(
        &validate_source("source", &source, &default).unwrap_err(),
        "actor.language.maxSourceBytes",
    );
    validate_source("source", &source, &raised).unwrap();
    let long_path = format!("/{}", "a".repeat(4096));
    violation(
        &validate_path("cwd", &long_path, &default).unwrap_err(),
        "actor.paths.maxPathBytes",
    );
    validate_path("cwd", &long_path, &raised).unwrap();
    let pty = ActorExecutionPtyOptions {
        cols: Some(4097),
        rows: Some(24),
    };
    violation(
        &pty.validate(&default).unwrap_err(),
        "actor.execution.maxTerminalDimension",
    );
    pty.validate(&raised).unwrap();
    let timeout = ActorLanguageExecutionOptions {
        timeout_ms: Some(300001),
        ..Default::default()
    };
    violation(
        &timeout.validate(&default).unwrap_err(),
        "actor.language.maxTimeoutMs",
    );
    timeout.validate(&raised).unwrap();
    assert_eq!(timeout.into_core(&raised).timeout_ms, Some(300001));
    assert_eq!(
        ActorLanguageExecutionOptions::default()
            .into_core(&raised)
            .timeout_ms,
        Some(310000)
    );
    assert_eq!(
        ActorLanguageSpawnOptions::default()
            .into_core(&raised)
            .timeout_ms,
        None,
        "spawn omission must not become a process lifetime limit"
    );
    assert!(validate_timeout(Some(0), &raised).is_err());
}

#[test]
fn language_context_json_and_result_checks_use_owner() {
    let default = budgets(serde_json::json!({}));
    let raised = budgets(
        serde_json::json!({"language":{"maxContextIdBytes":512,"maxJsonBytes":524288,"maxResultBytes":1048576}}),
    );
    let id = "c".repeat(257);
    violation(
        &validate_context_id(&id, &default).unwrap_err(),
        "actor.language.maxContextIdBytes",
    );
    validate_context_id(&id, &raised).unwrap();
    let json = "j".repeat(256 * 1024);
    violation(
        &validate_json_value("inputs", &json, &default).unwrap_err(),
        "actor.language.maxJsonBytes",
    );
    validate_json_value("inputs", &json, &raised).unwrap();
    let output = "o".repeat(768 * 1024);
    violation(
        &validate_cbor_result("language result", &output, &default).unwrap_err(),
        "actor.language.maxResultBytes",
    );
    validate_cbor_result("language result", &output, &raised).unwrap();
}
