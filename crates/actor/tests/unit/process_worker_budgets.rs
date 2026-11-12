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
fn process_options_share_execution_policy_but_preserve_cwd_and_timeout_defaults() {
    let default = budgets(serde_json::json!({}));
    let raised = budgets(
        serde_json::json!({"execution":{"maxArguments":1100,"maxArgumentBytes":20000,"maxCommandBytes":20000,"maxStdinBytes":524288,"maxTerminalDimension":5000},"process":{"maxWaitMs":310000}}),
    );
    let args = vec![String::new(); 1025];
    violation(
        &validate_arguments(&args, &default).unwrap_err(),
        "actor.execution.maxArguments",
    );
    validate_arguments(&args, &raised).unwrap();
    let command = "c".repeat(16385);
    violation(
        &validate_command(&command, &default).unwrap_err(),
        "actor.execution.maxCommandBytes",
    );
    validate_command(&command, &raised).unwrap();
    let options = ActorExecOptions {
        cwd: Some(format!("/{}", "a".repeat(4096))),
        ..Default::default()
    };
    options
        .validate(&default)
        .expect("process cwd retains its existing16KiB budget, not language4KiB");
    assert_eq!(options.into_core(&default).timeout, Some(300000.0));
    let options = ActorExecOptions {
        timeout_ms: Some(300001),
        ..Default::default()
    };
    violation(
        &options.validate(&default).unwrap_err(),
        "actor.process.maxWaitMs",
    );
    options.validate(&raised).unwrap();
    assert_eq!(options.into_core(&raised).timeout, Some(300001.0));
    assert_eq!(
        ActorExecOptions::default().into_core(&raised).timeout,
        Some(310000.0)
    );
    assert!(ActorExecOptions {
        timeout_ms: Some(0),
        ..Default::default()
    }
    .validate(&raised)
    .is_err());
    violation(
        &validate_dimensions(Some(4097), Some(24), &default).unwrap_err(),
        "actor.execution.maxTerminalDimension",
    );
    validate_dimensions(Some(4097), Some(24), &raised).unwrap();
}

#[test]
fn process_result_converter_uses_captured_output_budget() {
    let default = budgets(serde_json::json!({}));
    let raised = budgets(serde_json::json!({"process":{"maxExecOutputBytes":1048576}}));
    let result = agentos_client::ExecResult {
        exit_code: 0,
        exit_signal: None,
        stdout: "o".repeat(768 * 1024 + 1),
        stderr: String::new(),
    };
    violation(
        &actor_exec_result(result.clone(), &default).unwrap_err(),
        "actor.process.maxExecOutputBytes",
    );
    let converted = actor_exec_result(result.clone(), &raised).unwrap();
    assert_eq!(converted.stdout, result.stdout);
    assert_eq!(converted.status.exit_code, 0);
}
