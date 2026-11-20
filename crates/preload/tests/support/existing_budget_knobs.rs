use super::*;

#[test]
fn process_knobs_above_old_ceiling_remain_finite_and_checked() {
    let options = PreloadProcessOptions {
        startup_deadline_ms: 300001,
        plan_read_timeout_ms: 60001,
        max_plan_entries: 513,
        max_plan_bytes: 64 * 1024 * 1024 * 1024 + 1,
        warm_concurrency: 513,
        max_observation_entries: 129,
        flush_interval_ms: 86400001,
        action_timeout_ms: 60001,
    };
    options.validate().unwrap();
    assert!(PreloadProcessOptions {
        flush_interval_ms: u64::MAX,
        ..Default::default()
    }
    .validate()
    .is_err());
}

#[test]
fn coordinator_existing_knobs_can_raise_their_own_budgets() {
    let config = PreloadCoordinatorConfig::normalize(PreloadCoordinatorConfigInput {
        max_plan_entries: Some(513),
        max_plan_bytes: Some(64 * 1024 * 1024 * 1024 + 1),
        max_candidates: Some(4097),
        max_tracked_processes: Some(2049),
        max_batch_observations: Some(129),
        max_concurrent_actions: Some(513),
        plan_ttl_ms: Some(86400001),
    })
    .unwrap();
    assert_eq!(config.max_plan_entries, 513);
    assert!(
        PreloadCoordinatorConfig::normalize(PreloadCoordinatorConfigInput {
            plan_ttl_ms: Some(u64::MAX),
            ..Default::default()
        })
        .is_err()
    );
}

#[test]
fn valid_unexpired_plan_is_not_rejected_by_a_second_fixed_lifetime() {
    let now = now_ms().unwrap();
    let service = ProcessPreloadService::new(Default::default()).unwrap();
    let plan = PreloadPlan {
        protocol_version: PRELOAD_PROTOCOL_VERSION,
        revision: 1,
        generated_at_ms: now,
        expires_at_ms: now + 86400001,
        artifacts: vec![],
    };
    assert!(service.validate_plan(plan).unwrap().is_empty());
}
