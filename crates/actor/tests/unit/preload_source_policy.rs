use super::*;

#[test]
fn hosted_remote_source_normalization_preserves_identity_discards_size_and_uses_source_policy() {
    let digest = format!("sha256:{}", "a".repeat(64));
    let source = RemotePackageSourceInput {
        url: format!("https://packages.gameinc.io/{}.aospkg", "a".repeat(4096)),
        digest: Some(digest.clone()),
        size: Some(7),
        package_id: Some(digest.clone()),
    };
    let defaults = PackageResolverOptions::default();
    assert!(normalize_remote_source_with_options(source.clone(), &defaults).is_err());
    let raised = PackageResolverOptions {
        max_source_url_bytes: 8192,
        ..defaults
    };
    let normalized = normalize_remote_source_with_options(source.clone(), &raised).unwrap();
    assert_eq!(normalized.url, source.url);
    assert_eq!(normalized.digest, Some(digest.clone()));
    assert_eq!(normalized.package_id, Some(digest));
    // Caller-provided size is unverified metadata; acquisition owns verification.
    assert_eq!(normalized.size, None);
    let mut invalid = source.clone();
    invalid.url = "file:///tmp/fixture.aospkg".into();
    assert!(normalize_remote_source_with_options(invalid, &raised).is_err());
    let mut invalid = source.clone();
    invalid.digest = Some("not-a-digest".into());
    invalid.package_id = invalid.digest.clone();
    assert!(normalize_remote_source_with_options(invalid, &raised).is_err());
    let mut invalid = source;
    invalid.package_id = Some(format!("sha256:{}", "b".repeat(64)));
    assert!(normalize_remote_source_with_options(invalid, &raised)
        .unwrap_err()
        .to_string()
        .contains("packageId must equal digest"));
}

#[test]
#[ignore = "startup policy freezes process-global state; run exact control alone"]
fn hosted_core_config_projects_the_actual_startup_source_url_policy() {
    agentos_preload::configure_preload_validation_policy(
        serde_json::from_value(serde_json::json!({"maxUrlBytes":8192})).unwrap(),
    )
    .unwrap();
    let config = AgentOsActorConfig::normalize(Default::default()).unwrap();
    let core = config.to_core_config(None, None, None);
    let options = core
        .package_resolver
        .expect("hosted Core acquisition must retain startup policy");
    assert_eq!(options.max_source_url_bytes, 8192);
    assert_eq!(
        options.allow_insecure_local_http,
        agentos_preload::hosted_package_options().allow_insecure_local_http
    );
}
