use super::*;

#[test]
fn preload_json_is_validated_before_hosted_startup_without_freezing_process_state() {
    let options = parse_actor_options(
        [
            r#"--preload-policy-json={"maxUrlBytes":8192,"maxObservationClockSkewMs":0}"#
                .to_owned(),
            "--preload-max-plan-entries=513".to_owned(),
        ]
        .into_iter(),
    )
    .unwrap();
    assert_eq!(options.preload_policy.max_url_bytes, 8192);
    assert_eq!(options.preload_policy.max_observation_clock_skew_ms, 0);
    assert_eq!(options.preload_policy.max_state_bytes, 16 * 1024 * 1024);
    assert_eq!(options.preload.max_plan_entries, 513);
    for json in [
        r#"{"maxStateBytes":0}"#,
        r#"{"unknown":1}"#,
        r#"{"maxUrlBytes":-1}"#,
        r#"{"maxObservationWindowMs":18446744073709551615}"#,
    ] {
        let error = parse_actor_options([format!("--preload-policy-json={json}")].into_iter())
            .err()
            .unwrap();
        assert!(error.contains("--preload-policy-json"), "{error}");
    }
    assert_eq!(
        parse_actor_options(std::iter::empty())
            .unwrap()
            .preload_policy,
        agentos_actor::PreloadValidationPolicy::default()
    );
}
