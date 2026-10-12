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
fn configuration_environment_uses_worker_owner_and_preserves_lexical_rules() {
    let default = budgets(serde_json::json!({}));
    let raised = budgets(
        serde_json::json!({"config":{"maxEnvironmentEntries":300,"maxEnvironmentKeyBytes":512,"maxEnvironmentValueBytes":16384,"maxEnvironmentBytes":131072}}),
    );
    let environments = [
        (0..257)
            .map(|n| (format!("K{n}"), "v".into()))
            .collect::<BTreeMap<_, _>>(),
        BTreeMap::from([("k".repeat(257), "v".into())]),
        BTreeMap::from([("K".into(), "v".repeat(8193))]),
        (0..16)
            .map(|n| (format!("K{n}"), "v".repeat(4096)))
            .collect(),
    ];
    let settings = [
        "actor.config.maxEnvironmentEntries",
        "actor.config.maxEnvironmentKeyBytes",
        "actor.config.maxEnvironmentValueBytes",
        "actor.config.maxEnvironmentBytes",
    ];
    for (environment, setting) in environments.into_iter().zip(settings) {
        let input = AgentOsActorConfigInput {
            environment: Some(environment),
            ..Default::default()
        };
        violation(
            &AgentOsActorConfig::normalize_with_budgets(input.clone(), &default).unwrap_err(),
            setting,
        );
        AgentOsActorConfig::normalize_with_budgets(input, &raised)
            .expect("actual configuration normalizer admits raised environment");
    }
    for key in ["", "BAD=KEY", "BAD\0KEY"] {
        assert!(
            validate_environment(Some(&BTreeMap::from([(key.into(), "v".into())])), &raised)
                .is_err()
        );
    }
}

#[test]
fn permission_and_hosted_mount_normalizers_use_the_same_owner() {
    let default = budgets(serde_json::json!({}));
    let raised = budgets(
        serde_json::json!({"config":{"maxPermissionRulesPerDomain":300,"maxPermissionValuesPerRule":256,"maxPermissionValueBytes":8192,"maxPermissionConfigBytes":524288,"maxHostedMounts":40,"maxFilesystemNamespaceBytes":256,"maxRemoteSoftware":140},"paths":{"maxPathBytes":8192}}),
    );
    let permissions = Permissions {
        network: Some(PatternPermissions::Rules(RulePermissions {
            default: None,
            rules: (0..257)
                .map(|_| PatternPermissionRule {
                    mode: agentos_client::PermissionMode::Allow,
                    operations: None,
                    patterns: None,
                })
                .collect(),
        })),
        ..Default::default()
    };
    violation(
        &validate_permissions(Some(&permissions), &default).unwrap_err(),
        "actor.config.maxPermissionRulesPerDomain",
    );
    validate_permissions(Some(&permissions), &raised).unwrap();
    let many = vec!["*".into(); 129];
    violation(
        &validate_permission_values("patterns", Some(&many), &default).unwrap_err(),
        "actor.config.maxPermissionValuesPerRule",
    );
    validate_permission_values("patterns", Some(&many), &raised).unwrap();
    let long = vec!["p".repeat(4097)];
    violation(
        &validate_permission_values("patterns", Some(&long), &default).unwrap_err(),
        "actor.config.maxPermissionValueBytes",
    );
    validate_permission_values("patterns", Some(&long), &raised).unwrap();
    let large_permissions = Permissions {
        network: Some(PatternPermissions::Rules(RulePermissions {
            default: None,
            rules: vec![PatternPermissionRule {
                mode: agentos_client::PermissionMode::Allow,
                operations: None,
                patterns: Some(vec!["p".repeat(4096); 70]),
            }],
        })),
        ..Default::default()
    };
    violation(
        &validate_permissions(Some(&large_permissions), &default).unwrap_err(),
        "actor.config.maxPermissionConfigBytes",
    );
    validate_permissions(Some(&large_permissions), &raised).unwrap();
    let mounts = HostedFilesystemConfigInput {
        mounts: (0..33)
            .map(|n| HostedFilesystemMountInput {
                path: format!("/data{n}"),
                backend: HostedFilesystemBackendInput::ActorSqlite { namespace: None },
                read_only: false,
            })
            .collect(),
        ..Default::default()
    };
    violation(
        &normalize_filesystem(mounts.clone(), &default).unwrap_err(),
        "actor.config.maxHostedMounts",
    );
    assert_eq!(
        normalize_filesystem(mounts, &raised).unwrap().mounts.len(),
        33
    );
    let namespace = "n".repeat(129);
    violation(
        &normalize_namespace(namespace.clone(), &default).unwrap_err(),
        "actor.config.maxFilesystemNamespaceBytes",
    );
    assert_eq!(
        normalize_namespace(namespace.clone(), &raised).unwrap(),
        namespace
    );
    let path = format!("/{}", "a".repeat(4096));
    violation(
        &validate_hosted_mount_path(&path, &default).unwrap_err(),
        "actor.paths.maxPathBytes",
    );
    validate_hosted_mount_path(&path, &raised).unwrap();
    for path in ["/", "/proc/x", "/data/../other"] {
        assert!(validate_hosted_mount_path(path, &raised).is_err());
    }
    let sources = (0..129)
        .map(|n| RemotePackageSourceInput {
            url: format!("https://example.com/p{n}.aospkg"),
            digest: None,
            size: None,
            package_id: None,
        })
        .collect::<Vec<_>>();
    violation(
        &normalize_remote_software(sources.clone(), &default).unwrap_err(),
        "actor.config.maxRemoteSoftware",
    );
    assert_eq!(
        normalize_remote_software(sources, &raised).unwrap().len(),
        129
    );
}

#[test]
fn tighter_restore_validation_keeps_saved_desired_document_exactly_unchanged() {
    let raised = budgets(serde_json::json!({"config":{"maxEnvironmentEntries":300}}));
    let mut desired = AgentOsActorConfig::normalize_with_budgets(
        AgentOsActorConfigInput {
            environment: Some((0..257).map(|n| (format!("K{n}"), "v".into())).collect()),
            ..Default::default()
        },
        &raised,
    )
    .unwrap();
    desired.software = vec![RemotePackageSource {
        url: "https://example.com/pinned.aospkg".into(),
        digest: Some(format!("sha256:{}", "a".repeat(64))),
        size: Some(42),
        package_id: Some(format!("sha256:{}", "a".repeat(64))),
    }];
    desired.preview = PreviewPolicy {
        default_ttl_ms: 1000,
        max_ttl_ms: 2000,
        max_active: 1,
    };
    let original = serde_json::to_vec(&desired).unwrap();
    desired.validate_restored_budgets(&raised).unwrap();
    violation(
        &desired
            .validate_restored_budgets(&budgets(serde_json::json!({})))
            .unwrap_err(),
        "actor.config.maxEnvironmentEntries",
    );
    assert_eq!(
        serde_json::to_vec(&desired).unwrap(),
        original,
        "validation never normalizes back into durable desired state"
    );
    assert_eq!(
        AgentOsActorConfig::normalize(Default::default()).unwrap(),
        AgentOsActorConfig::normalize_with_budgets(
            Default::default(),
            &budgets(serde_json::json!({}))
        )
        .unwrap()
    );
}

#[test]
fn new_startup_groups_reject_zero_unknown_and_real_representation_overflows() {
    for value in [
        serde_json::json!({"config":{"maxEnvironmentEntries":0}}),
        serde_json::json!({"paths":{"maxPathBytes":0}}),
        serde_json::json!({"execution":{"maxTerminalDimension":65536}}),
        serde_json::json!({"language":{"maxTimeoutMs":9007199254740992u64}}),
        serde_json::json!({"process":{"maxWaitMs":9007199254740992u64}}),
    ] {
        let options = serde_json::from_value(value).unwrap();
        assert!(crate::HostedActorBudgets::normalize(options).is_err());
    }
    assert!(serde_json::from_value::<crate::HostedActorBudgetOptions>(
        serde_json::json!({"config":{"notASetting":1}})
    )
    .is_err());
    let options:crate::HostedActorBudgetOptions=serde_json::from_value(serde_json::json!({"execution":{"maxArguments":1100},"config":{"maxEnvironmentEntries":300}})).unwrap();
    assert_eq!(
        serde_json::from_value::<crate::HostedActorBudgetOptions>(
            serde_json::to_value(&options).unwrap()
        )
        .unwrap(),
        options
    );
}
