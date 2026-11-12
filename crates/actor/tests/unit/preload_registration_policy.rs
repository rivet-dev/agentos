use super::*;

#[test]
fn both_production_registrants_select_the_frozen_transport_owner() {
    let defaults = HostedActorBudgets::normalize(Default::default()).unwrap();
    let raised = HostedActorBudgets::normalize(
        serde_json::from_value(serde_json::json!({"transport":{"maxMessageBytes":8*1024*1024}}))
            .unwrap(),
    )
    .unwrap();
    for budgets in [&defaults, &raised] {
        let actor = hosted_actor_config_with_budgets(budgets);
        let coordinator = preload_coordinator_config_with_budgets(budgets);
        assert_eq!(actor.max_incoming_message_size, budgets.max_message_bytes);
        assert_eq!(actor.max_outgoing_message_size, budgets.max_message_bytes);
        assert_eq!(
            coordinator.max_incoming_message_size,
            budgets.max_message_bytes
        );
        assert_eq!(
            coordinator.max_outgoing_message_size,
            budgets.max_message_bytes
        );
        assert!(coordinator.has_database);
        assert_eq!(
            coordinator.action_timeout,
            ActorConfig::default().action_timeout
        );
        assert_eq!(actor.action_timeout, Duration::from_secs(6 * 60));
    }
    assert_eq!(defaults.max_message_bytes, ACTOR_MESSAGE_SIZE_LIMIT);
}
