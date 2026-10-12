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
fn preview_normalizer_raises_worker_defaults_without_rewriting_explicit_saved_values() {
    let default = budgets(serde_json::json!({}));
    let raised = budgets(serde_json::json!({"preview":{"maxTtlMs":172800000,"maxActive":256}}));
    let input = PreviewPolicyInput {
        max_ttl_ms: Some(86400001),
        max_active: Some(129),
        ..Default::default()
    };
    violation(
        &normalize_preview_policy(input.clone(), &default).unwrap_err(),
        "actor.preview.maxTtlMs",
    );
    let active_only = PreviewPolicyInput {
        max_active: Some(129),
        ..Default::default()
    };
    violation(
        &normalize_preview_policy(active_only.clone(), &default).unwrap_err(),
        "actor.preview.maxActive",
    );
    assert_eq!(
        normalize_preview_policy(active_only, &raised)
            .unwrap()
            .max_active,
        129
    );
    let actual = normalize_preview_policy(input, &raised).unwrap();
    assert_eq!(actual.max_ttl_ms, 86400001);
    assert_eq!(actual.max_active, 129);
    let selected = normalize_preview_policy(Default::default(), &raised).unwrap();
    assert_eq!(selected.max_active, 256);
    assert_eq!(selected.max_ttl_ms, 172800000);
    let desired = AgentOsActorConfig::normalize_with_budgets(
        AgentOsActorConfigInput {
            preview: Some(PreviewPolicyInput {
                default_ttl_ms: Some(1000),
                max_ttl_ms: Some(2000),
                max_active: Some(1),
            }),
            ..Default::default()
        },
        &raised,
    )
    .unwrap();
    let before = serde_json::to_vec(&desired).unwrap();
    desired.validate_restored_budgets(&raised).unwrap();
    assert_eq!(serde_json::to_vec(&desired).unwrap(), before);
}

#[test]
fn remaining_startup_groups_reject_only_real_invalid_representations_and_keep_zero_history() {
    for value in [
        serde_json::json!({"retention":{"maxRuntimeIssues":0}}),
        serde_json::json!({"filesystem":{"maxRecursiveDepth":4294967296u64}}),
        serde_json::json!({"network":{"maxStreamChunkBytes":4294967296u64}}),
        serde_json::json!({"preview":{"maxActive":4294967296u64}}),
        serde_json::json!({"preview":{"maxTtlMs":999}}),
        serde_json::json!({"network":{"maxStreamLifetimeMs":u64::MAX}}),
    ] {
        assert!(
            crate::HostedActorBudgets::normalize(serde_json::from_value(value).unwrap()).is_err()
        );
    }
    assert_eq!(
        budgets(serde_json::json!({"filesystem":{"maxRecursiveDepth":0}}))
            .filesystem
            .max_recursive_depth
            .maximum,
        0
    );
    assert_eq!(
        budgets(serde_json::json!({"cron":{"maxHistory":0}}))
            .cron
            .max_history
            .maximum,
        0
    );
    assert!(serde_json::from_value::<crate::HostedActorBudgetOptions>(
        serde_json::json!({"retention":{"unknown":1}})
    )
    .is_err());
}
