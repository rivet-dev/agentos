use agentos_actor::{configure_process_actor_budgets, HostedActorBudgetOptions};

#[test]
fn immutable_process_owner() {
    if let Ok(case) = std::env::var("AGENTOS_BUDGET_OWNER_CASE") {
        let options: HostedActorBudgetOptions =
            serde_json::from_str(r#"{"transport":{"maxMessageBytes":4194304}}"#).unwrap();
        if case == "configure-first" {
            configure_process_actor_budgets(options.clone()).unwrap();
            let config = agentos_actor::hosted_actor_config();
            assert_eq!(config.max_incoming_message_size, 4 * 1024 * 1024);
            configure_process_actor_budgets(options).unwrap();
            assert!(configure_process_actor_budgets(Default::default())
                .unwrap_err()
                .to_string()
                .contains("already frozen"));
        } else {
            assert_eq!(case, "consume-first");
            let _registry = agentos_actor::registry();
            let error = configure_process_actor_budgets(options).unwrap_err();
            assert!(error
                .to_string()
                .contains("before registry()/Actor::create"));
        }
        return;
    }
    for case in ["configure-first", "consume-first"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "immutable_process_owner", "--nocapture"])
            .env("AGENTOS_BUDGET_OWNER_CASE", case)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "isolated owner scenario {case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
