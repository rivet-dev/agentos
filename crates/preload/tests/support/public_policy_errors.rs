use super::*;
use rivet_error::RivetError;

fn public_limit(
    error: anyhow::Error,
    setting: &str,
    observed: u64,
    maximum: u64,
) -> serde_json::Value {
    // Check typed extraction and its serialized projection, including context.
    // Hosted HTTP controls separately qualify the framework's masking boundary.
    let error = error.context("coordinator action failed");
    let public = RivetError::extract(&error);
    let wire = serde_json::to_value(public).unwrap();
    assert_eq!(wire["group"], "user");
    assert_eq!(wire["code"], "limit_exceeded");
    assert_eq!(wire["meta"]["budget"]["setting"], setting);
    assert_eq!(wire["meta"]["budget"]["observed"], observed);
    assert_eq!(wire["meta"]["budget"]["limit"], maximum);
    assert!(wire["message"]
        .as_str()
        .unwrap()
        .contains("--preload-policy-json"));
    let typed = error
        .downcast_ref::<policy::PreloadLimitViolation>()
        .expect("original typed owner survives");
    assert_eq!(typed.setting, setting);
    assert_eq!(typed.observed, observed);
    assert_eq!(typed.maximum, maximum);
    wire
}

#[test]
fn framework_extraction_preserves_typed_preload_policy_projection() {
    let policy = PreloadValidationPolicy::default();
    public_limit(
        policy.check("maxUrlBytes", 4097, 4096).unwrap_err(),
        "preloadPolicy.maxUrlBytes",
        4097,
        4096,
    );
    // Prefixes alone must never be the classification mechanism.
    let prefix = anyhow!("limit_exceeded: not an owning typed preload error");
    let public = RivetError::extract(&prefix);
    assert_eq!(public.group(), "core");
    assert_eq!(public.code(), "internal_error");
}

#[test]
fn actual_state_codec_public_error_keeps_creation_only_remedy_and_checks_before_decode() {
    let policy = PreloadValidationPolicy {
        max_state_bytes: 16,
        ..Default::default()
    };
    let rows = vec![vec![rivetkit::ColumnValue::Text("x".repeat(32))]];
    let wire = public_limit(
        coordinator_store::decode_rows_with_policy(&rows, &policy).unwrap_err(),
        "preloadPolicy.maxStateBytes",
        32,
        16,
    );
    let remedy = wire["meta"]["budget"]["remediation"].as_str().unwrap();
    assert!(remedy.contains("creation-only"));
    assert!(remedy.contains("saved row is unchanged"));
    assert!(matches!(&rows[0][0],rivetkit::ColumnValue::Text(text) if text==&"x".repeat(32)));
}

#[test]
fn actual_clock_guard_uses_typed_public_error_without_hiding_invalid_order() {
    let policy = PreloadValidationPolicy {
        max_observation_clock_skew_ms: 0,
        ..Default::default()
    };
    let mut state = initial_coordinator_state_with_policy(Default::default(), &policy).unwrap();
    let input = PreloadRecordUsage {
        protocol_version: PRELOAD_PROTOCOL_VERSION,
        process_id: "fixture".into(),
        window_id: 1,
        window_started_at_ms: 1001,
        window_ended_at_ms: 1001,
        observations: vec![],
        dropped_observations: 0,
    };
    public_limit(
        apply_usage_with_policy(&mut state, input.clone(), 1000, &policy).unwrap_err(),
        "preloadPolicy.maxObservationClockSkewMs",
        1,
        0,
    );
    let mut invalid = input;
    invalid.window_started_at_ms = 1002;
    let error = apply_usage_with_policy(&mut state, invalid, 1000, &policy).unwrap_err();
    assert!(error
        .downcast_ref::<policy::PreloadLimitViolation>()
        .is_none());
    assert!(error.to_string().contains("starts after"));
}

#[test]
fn existing_schema_selects_saved_state_without_revalidating_stale_creation_input() {
    use std::cell::Cell;
    let defaults = PreloadValidationPolicy::default();
    let mut raised = defaults.clone();
    raised.max_url_bytes = 8192;
    let long = PreloadArtifact {
        url: format!("https://packages.gameinc.io/{}.aospkg", "x".repeat(4096)),
        digest: format!("sha256:{}", "a".repeat(64)),
        size: Some(1),
        package_format_version: 1,
    };
    let initial = PreloadCoordinatorCreateInput {
        baseline: vec![long],
        ..Default::default()
    };
    let mut saved = initial_coordinator_state_with_policy(initial.clone(), &raised).unwrap();
    saved.baseline = normalize_artifacts_with_policy(vec![], &saved.config, &raised).unwrap();
    saved.revision = 2;
    let saved_bytes = coordinator_store::encode_with_policy(&saved, &defaults).unwrap();
    let called = Cell::new(false);
    let selected = coordinator_store::encode_new_state_for_schema(
        1,
        || {
            called.set(true);
            initial_coordinator_state_with_policy(initial.clone(), &defaults)
        },
        &defaults,
    )
    .unwrap();
    assert!(selected.is_none());
    assert!(!called.get());
    let rows = vec![vec![rivetkit::ColumnValue::Text(saved_bytes.clone())]];
    assert_eq!(
        coordinator_store::decode_rows_with_policy(&rows, &defaults).unwrap(),
        saved
    );
    assert_eq!(
        coordinator_store::encode_with_policy(&saved, &defaults).unwrap(),
        saved_bytes
    );
    let error = coordinator_store::encode_new_state_for_schema(
        0,
        || initial_coordinator_state_with_policy(initial, &defaults),
        &defaults,
    )
    .unwrap_err();
    assert!(error
        .downcast_ref::<policy::PreloadLimitViolation>()
        .is_some());
}

#[test]
fn schema_and_saved_row_errors_precede_unused_initializer_and_do_not_recreate_state() {
    let policy = PreloadValidationPolicy::default();
    let error = coordinator_store::encode_new_state_for_schema(
        99,
        || panic!("schema error must precede input"),
        &policy,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("schema version99")
            || error.to_string().contains("schema version 99")
    );
    assert!(coordinator_store::encode_new_state_for_schema(
        1,
        || panic!("existing schema must not initialize"),
        &policy
    )
    .unwrap()
    .is_none());
    assert!(coordinator_store::decode_rows_with_policy(&[], &policy)
        .unwrap_err()
        .to_string()
        .contains("missing"));
    let rows = vec![vec![rivetkit::ColumnValue::Text("{}".into())]];
    assert!(coordinator_store::decode_rows_with_policy(&rows, &policy).is_err());
    let policy = PreloadValidationPolicy {
        max_state_bytes: 1,
        ..Default::default()
    };
    let initial = initial_coordinator_state_with_policy(Default::default(), &policy).unwrap();
    let error =
        coordinator_store::encode_new_state_for_schema(0, || Ok(initial), &policy).unwrap_err();
    assert!(error
        .downcast_ref::<policy::PreloadLimitViolation>()
        .is_some());
}
