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
