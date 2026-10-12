use super::*;

fn saved_config() -> AgentOsActorConfig {
    let owner = HostedActorBudgets::normalize(
        serde_json::from_value(
            serde_json::json!({"preview":{"maxTtlMs":172800000,"maxActive":256}}),
        )
        .unwrap(),
    )
    .unwrap();
    AgentOsActorConfig::normalize_with_budgets(serde_json::from_value(
        serde_json::json!({"preview":{"defaultTtlMs":1000,"maxTtlMs":86400001,"maxActive":129}})
    ).unwrap(), &owner).unwrap()
}

#[tokio::test]
async fn restored_policy_rejection_enters_the_real_failed_runtime_path_without_activation() {
    let desired = saved_config();
    let bytes = serde_json::to_vec(&desired).unwrap();
    let runtime = RuntimeController::new_with_budgets(
        "restored-policy-control",
        7,
        Arc::new(HostedActorBudgets::normalize(Default::default()).unwrap()),
    );
    for _ in 0..2 {
        let error = runtime.admit_boot_policy(&desired, 7).await.unwrap_err();
        let typed = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<agentos_actor_contract::ActionBudgetViolation>())
            .unwrap();
        assert_eq!(typed.setting, "actor.preview.maxTtlMs");
        let status = runtime.status().await;
        assert_eq!(status.lifecycle, VmLifecycleState::Failed);
        assert_eq!(status.config_state, crate::ConfigApplyState::Failed);
        assert_eq!(status.desired_config_revision, 7);
        assert_eq!(status.generation, 0);
        assert!(status.core.is_none());
        assert!(status.last_boot_at_ms.is_none());
        let issue = status.issues.last().unwrap();
        assert_eq!(issue.code, "runtime_configuration_rejected");
        assert!(issue.message.contains("actor.preview.maxTtlMs"));
        assert!(issue.message.contains("--actor-budgets-json"));
        assert!(runtime.pending_creation.lock().await.is_none());
        assert!(runtime.state.lock().await.vm.is_none());
        assert_eq!(serde_json::to_vec(&desired).unwrap(), bytes);
    }
}

#[tokio::test]
async fn matching_owner_admits_saved_values_without_mutation_or_activation() {
    let desired = saved_config();
    let bytes = serde_json::to_vec(&desired).unwrap();
    let owner = HostedActorBudgets::normalize(
        serde_json::from_value(
            serde_json::json!({"preview":{"maxTtlMs":172800000,"maxActive":256}}),
        )
        .unwrap(),
    )
    .unwrap();
    let runtime =
        RuntimeController::new_with_budgets("restored-policy-matching-control", 7, Arc::new(owner));
    runtime.admit_boot_policy(&desired, 7).await.unwrap();
    assert_eq!(
        runtime.status().await.lifecycle,
        VmLifecycleState::Initializing
    );
    assert!(runtime.pending_creation.lock().await.is_none());
    assert_eq!(serde_json::to_vec(&desired).unwrap(), bytes);
}
