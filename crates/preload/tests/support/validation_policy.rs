use super::*;

fn artifact_with(byte: char, size: u64) -> PreloadArtifact {
    PreloadArtifact {
        url: format!("https://packages.gameinc.io/{byte}.aospkg"),
        digest: format!("sha256:{}", byte.to_string().repeat(64)),
        size: Some(size),
        package_format_version: PACKAGE_FORMAT_VERSION,
    }
}
fn empty_state(policy: &PreloadValidationPolicy) -> PreloadCoordinatorState {
    initial_coordinator_state_with_policy(Default::default(), policy).unwrap()
}
fn observation(process: &str, window_id: u64, count: u64, now: i64) -> PreloadRecordUsage {
    let artifact = artifact_with('a', 1);
    PreloadRecordUsage {
        protocol_version: PRELOAD_PROTOCOL_VERSION,
        process_id: process.into(),
        window_id,
        window_started_at_ms: now,
        window_ended_at_ms: now,
        observations: vec![PreloadUsageObservation {
            url: artifact.url,
            digest: artifact.digest,
            size: 1,
            count,
            last_observed_at_ms: now,
        }],
        dropped_observations: 0,
    }
}

#[test]
fn immutable_preload_policy_has_nine_default_preserving_overrides_and_strict_zero_semantics() {
    let defaults = PreloadValidationPolicy::default();
    let partial: PreloadValidationPolicy =
        serde_json::from_str(r#"{"maxUrlBytes":8192,"maxObservationClockSkewMs":0}"#).unwrap();
    assert_eq!(partial.max_state_bytes, defaults.max_state_bytes);
    partial.validate().unwrap();
    assert!(serde_json::from_str::<PreloadValidationPolicy>(r#"{"unknown":1}"#).is_err());
    let mut invalid = defaults.clone();
    invalid.max_state_bytes = 0;
    assert!(invalid
        .validate()
        .unwrap_err()
        .to_string()
        .contains("maxStateBytes"));
    let slot = OnceLock::new();
    assert!(policy::configure_in_slot(&slot, invalid).is_err());
    assert!(slot.get().is_none());
    policy::configure_in_slot(&slot, partial.clone()).unwrap();
    policy::configure_in_slot(&slot, partial.clone()).unwrap();
    assert!(policy::configure_in_slot(&slot, defaults).is_err());
    assert_eq!(slot.get(), Some(&partial));
}

#[test]
fn url_id_format_and_artifact_metadata_use_selected_policy_and_shared_source_validator() {
    let defaults = PreloadValidationPolicy::default();
    let mut raised = defaults.clone();
    raised.max_url_bytes = 8192;
    raised.max_process_id_bytes = 256;
    raised.max_supported_formats = 9;
    raised.max_artifact_bytes = 64 * 1024 * 1024 * 1024 + 1;
    let mut artifact = artifact_with('a', 1);
    artifact.url = format!("https://packages.gameinc.io/{}.aospkg", "a".repeat(4096));
    assert!(validate_artifact_with_policy(&artifact, &defaults).is_err());
    validate_artifact_with_policy(&artifact, &raised).unwrap();
    artifact.url = "file:///tmp/fixture.aospkg".into();
    assert!(validate_artifact_with_policy(&artifact, &raised).is_err());
    artifact = artifact_with('a', 1);
    artifact.digest = "not-a-digest".into();
    assert!(validate_artifact_with_policy(&artifact, &raised).is_err());
    let id = "p".repeat(129);
    assert!(validate_process_id_with_policy(&id, &defaults).is_err());
    validate_process_id_with_policy(&id, &raised).unwrap();
    let action = PreloadGetPlan {
        protocol_version: PRELOAD_PROTOCOL_VERSION,
        process_id: id,
        supported_package_format_versions: vec![1; 9],
        max_entries: None,
        max_bytes: None,
    };
    assert!(validate_get_plan_request(&action, &defaults).is_err());
    validate_get_plan_request(&action, &raised).unwrap();
    let artifact = artifact_with('a', raised.max_artifact_bytes);
    assert!(validate_artifact_with_policy(&artifact, &defaults).is_err());
    validate_artifact_with_policy(&artifact, &raised).unwrap();
    let options = hosted_package_options_with_policy(&raised);
    assert_eq!(options.max_source_url_bytes, 8192);
    let resolver = agentos_client::PackageResolver::new(options).unwrap();
    resolver
        .validate_source(&PackageSource::Url {
            url: format!("https://packages.gameinc.io/{}.aospkg", "b".repeat(4096)),
            expected_digest: Some(format!("sha256:{}", "a".repeat(64))),
        })
        .unwrap();
}

#[test]
fn observation_windows_counts_coalescing_and_score_clamping_keep_owning_failures() {
    let policy = PreloadValidationPolicy {
        max_observation_window_ms: 10,
        max_observation_clock_skew_ms: 0,
        max_usage_count_per_observation: 3,
        max_candidate_score: 3,
        ..Default::default()
    };
    let now = 1000;
    let mut state = empty_state(&policy);
    let mut wide = observation("p", 1, 1, now);
    wide.window_started_at_ms = now - 11;
    assert!(apply_usage_with_policy(&mut state, wide, now, &policy)
        .unwrap_err()
        .to_string()
        .contains("maxObservationWindowMs"));
    let mut future = observation("p", 1, 1, now + 1);
    assert!(
        apply_usage_with_policy(&mut state, future.clone(), now, &policy)
            .unwrap_err()
            .to_string()
            .contains("maxObservationClockSkewMs")
    );
    let mut tolerant = policy.clone();
    tolerant.max_observation_clock_skew_ms = 1;
    apply_usage_with_policy(&mut state, future.clone(), now, &tolerant).unwrap();
    state = empty_state(&policy);
    future = observation("p", 1, 2, now);
    future.observations.push(future.observations[0].clone());
    let pre = state.clone();
    assert!(apply_usage_with_policy(&mut state, future, now, &policy)
        .unwrap_err()
        .to_string()
        .contains("maxUsageCountPerObservation"));
    assert_eq!(state, pre);
    apply_usage_with_policy(&mut state, observation("p", 1, 2, now), now, &policy).unwrap();
    apply_usage_with_policy(
        &mut state,
        observation("p", 2, 2, now + 1),
        now + 1,
        &policy,
    )
    .unwrap();
    assert_eq!(state.candidates[0].score, 3);
    let mut tighter = policy.clone();
    tighter.max_candidate_score = 2;
    assert!(validate_coordinator_state_with_policy(&state, &tighter)
        .unwrap_err()
        .to_string()
        .contains("maxCandidateScore"));
}

#[test]
fn durable_size_policy_runs_before_decode_and_before_any_update_value_exists() {
    use rivetkit::ColumnValue;
    let policy = PreloadValidationPolicy::default();
    let state = empty_state(&policy);
    let encoded = coordinator_store::encode_with_policy(&state, &policy).unwrap();
    let rows = vec![vec![ColumnValue::Text(encoded.clone())]];
    let before = format!("{rows:?}");
    let mut lower = policy.clone();
    lower.max_state_bytes = encoded.len() - 1;
    assert!(coordinator_store::decode_rows_with_policy(&rows, &lower)
        .unwrap_err()
        .to_string()
        .contains("maxStateBytes"));
    assert_eq!(format!("{rows:?}"), before);
    assert!(coordinator_store::encode_with_policy(&state, &lower)
        .unwrap_err()
        .to_string()
        .contains("maxStateBytes"));
    let malformed = vec![vec![ColumnValue::Text("x".repeat(32))]];
    lower.max_state_bytes = 16;
    assert!(
        coordinator_store::decode_rows_with_policy(&malformed, &lower)
            .unwrap_err()
            .to_string()
            .contains("maxStateBytes")
    );
    lower.max_state_bytes = 64;
    assert!(
        coordinator_store::decode_rows_with_policy(&malformed, &lower)
            .unwrap_err()
            .to_string()
            .contains("decode preload")
    );
    assert_eq!(
        coordinator_store::decode_rows_with_policy(&rows, &policy).unwrap(),
        state
    );
}

#[tokio::test]
async fn producer_uses_same_count_clamp_and_retains_its_entry_drop_accounting() {
    let policy = PreloadValidationPolicy {
        max_usage_count_per_observation: 2,
        ..Default::default()
    };
    let service = ProcessPreloadService::new_with_policy(
        PreloadProcessOptions {
            max_observation_entries: 1,
            ..Default::default()
        },
        policy,
    )
    .unwrap();
    let software = InstalledSoftware {
        package_id: "fixture".into(),
        digest: format!("sha256:{}", "a".repeat(64)),
        size_bytes: 1,
        package_name: "fixture".into(),
        version: "1.0.0".into(),
        commands: vec![],
    };
    for _ in 0..3 {
        service
            .observe("https://packages.gameinc.io/a.aospkg", &software)
            .await;
    }
    assert_eq!(
        service.observations.lock().await.entries[&software.digest].count,
        2
    );
    let other = InstalledSoftware {
        digest: format!("sha256:{}", "b".repeat(64)),
        ..software
    };
    service
        .observe("https://packages.gameinc.io/b.aospkg", &other)
        .await;
    assert_eq!(service.observations.lock().await.dropped, 1);
}

#[test]
#[ignore = "bounded20MiB durable fixture; run in explicit expensive validation"]
fn durable_state_above_legacy_size_requires_explicit_raised_policy() {
    let defaults = PreloadValidationPolicy::default();
    let mut state = empty_state(&defaults);
    state.config.max_candidates = 5000;
    state.candidates = (0..5000)
        .map(|index| PreloadCandidate {
            artifact: PreloadArtifact {
                url: format!("https://packages.gameinc.io/{index}/{}", "x".repeat(3700)),
                digest: format!("sha256:{index:064x}"),
                size: Some(1),
                package_format_version: 1,
            },
            score: 1,
            last_observed_at_ms: 1000,
        })
        .collect();
    let mut raised = defaults.clone();
    raised.max_state_bytes = 32 * 1024 * 1024;
    assert!(coordinator_store::encode_with_policy(&state, &defaults)
        .unwrap_err()
        .to_string()
        .contains("maxStateBytes"));
    let encoded = coordinator_store::encode_with_policy(&state, &raised).unwrap();
    assert!(encoded.len() > defaults.max_state_bytes);
    let rows = vec![vec![rivetkit::ColumnValue::Text(encoded)]];
    assert_eq!(
        coordinator_store::decode_rows_with_policy(&rows, &raised).unwrap(),
        state
    );
}

include!("child_source_policy.rs");
