use agentos_client::{AgentOsLimits, ExecutionLimits, TlsLimits};
use serde_json::json;

#[test]
fn tls_and_execution_limits_round_trip_with_typescript_wire_names() {
    let limits = AgentOsLimits {
        tls: Some(TlsLimits {
            max_buffered_bytes: Some(2048),
        }),
        execution: Some(ExecutionLimits {
            completed_ttl_ms: Some(60_000),
            max_completed_executions: Some(128),
            live_execution_warning_threshold: Some(32),
        }),
        ..Default::default()
    };
    let expected = json!({
        "tls": { "maxBufferedBytes": 2048 },
        "execution": {
            "completedTtlMs": 60_000,
            "maxCompletedExecutions": 128,
            "liveExecutionWarningThreshold": 32
        }
    });
    assert_eq!(serde_json::to_value(&limits).unwrap(), expected);
    assert_eq!(
        serde_json::from_value::<AgentOsLimits>(expected).unwrap(),
        limits
    );
}

#[test]
fn omitted_limit_fields_preserve_sidecar_defaults() {
    assert_eq!(
        serde_json::to_value(AgentOsLimits::default()).unwrap(),
        json!({})
    );
    let empty: AgentOsLimits = serde_json::from_value(json!({"tls": {}, "execution": {}})).unwrap();
    assert_eq!(empty.tls, Some(TlsLimits::default()));
    assert_eq!(empty.execution, Some(ExecutionLimits::default()));
    assert_eq!(
        serde_json::to_value(empty).unwrap(),
        json!({"tls": {}, "execution": {}})
    );
}

#[test]
fn tls_and_execution_limits_reject_unknown_fields_and_noninteger_values() {
    for invalid in [
        json!({"tls": {"max_buffered_bytes": 1024}}),
        json!({"execution": {"completed_ttl_ms": 1024}}),
        json!({"execution": {"unknown": 1}}),
        json!({"tls": {"maxBufferedBytes": -1}}),
        json!({"execution": {"completedTtlMs": 1.5}}),
    ] {
        assert!(
            serde_json::from_value::<AgentOsLimits>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn reactor_limits_preserve_optional_wire_shape_and_shared_contract() {
    use agentos_client::ReactorLimits;
    let explicit = AgentOsLimits {
        reactor: Some(ReactorLimits {
            operation_deadline_ms: Some(500),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&explicit).unwrap(),
        json!({"reactor": {"operationDeadlineMs": 500}})
    );
    assert_eq!(
        serde_json::from_value::<AgentOsLimits>(json!({"reactor": {}}))
            .unwrap()
            .reactor,
        Some(ReactorLimits::default())
    );
    assert_eq!(
        serde_json::to_value(AgentOsLimits::default()).unwrap(),
        json!({})
    );
}

#[test]
fn reactor_limits_share_numeric_range_and_unknown_field_validation() {
    use agentos_vm_config::{CreateVmConfig, VmLimitsConfig};
    let boundary: agentos_vm_config::VmLimitsConfig =
        serde_json::from_value(json!({"reactor":{"maxTasks":9_007_199_254_740_991_u64}})).unwrap();
    assert!(CreateVmConfig {
        limits: Some(boundary),
        ..Default::default()
    }
    .validate(16 * 1024 * 1024)
    .is_ok());
    for field in [
        "maxCapabilities",
        "maxReadyHandles",
        "maxTasks",
        "workQuantum",
        "byteQuantum",
        "maxHandleCommands",
        "maxHandleCommandBytes",
        "maxBridgeCalls",
        "maxBridgeRequestBytes",
        "maxBridgeResponseBytes",
        "maxAsyncCompletions",
        "maxAsyncCompletionBytes",
        "maxBlockingJobs",
        "maxBlockingBytes",
        "perHandleOperationQuantum",
        "acceptQuantum",
        "datagramQuantum",
        "completionQuantum",
        "signalQuantum",
        "shutdownDeadlineMs",
        "operationDeadlineMs",
    ] {
        let value = json!({"reactor": {field: 37}});
        let public: AgentOsLimits = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(public).unwrap(), value);
        for invalid in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!(9_007_199_254_740_992_u64),
            json!(u64::MAX),
        ] {
            let value = json!({"reactor": {field: invalid}});
            let valid = serde_json::from_value::<AgentOsLimits>(value.clone())
                .ok()
                .and_then(|limits| serde_json::to_value(limits).ok())
                .and_then(|value| serde_json::from_value::<VmLimitsConfig>(value).ok())
                .is_some_and(|limits| {
                    CreateVmConfig {
                        limits: Some(limits),
                        ..Default::default()
                    }
                    .validate(16 * 1024 * 1024)
                    .is_ok()
                });
            assert!(!valid, "{value}");
        }
    }
    for value in [
        json!({"reactor":{"unknown":1}}),
        json!({"reactor":{"operation_deadline_ms":1}}),
    ] {
        assert!(serde_json::from_value::<AgentOsLimits>(value).is_err());
    }
}

#[cfg(feature = "contract")]
#[test]
fn reactor_contract_uses_canonical_optional_config() {
    use ts_rs::TS;
    assert!(AgentOsLimits::decl().contains("reactor?: ReactorLimitsConfig"));
    assert_eq!(
        agentos_client::ReactorLimits::decl(),
        agentos_vm_config::ReactorLimitsConfig::decl()
    );
}

#[test]
fn reactor_public_json_override_is_recognized() {
    let value = json!({"reactor": {"operationDeadlineMs": 500}});
    let limits: AgentOsLimits =
        serde_json::from_value(value.clone()).expect("public reactor override must be accepted");
    assert_eq!(serde_json::to_value(limits).unwrap(), value);
}
