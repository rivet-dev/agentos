use std::time::Duration;

use codex_otel::{
    sanitize_metric_tag_value, AuthEnvTelemetryMetadata, SessionTelemetry, SessionTelemetryMetadata,
};

#[test]
fn metric_tag_sanitizer_is_explicit_no_op() {
    let value = "user/input with spaces and symbols !@#$";

    assert_eq!(sanitize_metric_tag_value(value), value);
}

#[test]
fn session_telemetry_drops_invalid_metric_inputs() {
    let telemetry = SessionTelemetry::new()
        .with_auth_env(AuthEnvTelemetryMetadata {
            auth_mode: Some("api-key".to_string()),
        })
        .with_model("model/name", "slug")
        .with_metrics_service_name("service/name");

    telemetry.counter("", -1, &[("", "bad tag")]);
    telemetry.histogram("", i64::MIN, &[("tag", "")]);
    telemetry.record_duration("", Duration::MAX, &[("tag with space", "value")]);

    assert!(telemetry.shutdown_metrics().is_ok());
    assert!(telemetry.runtime_metrics_summary().is_none());
}

#[test]
fn session_telemetry_metadata_remains_zero_state() {
    assert_eq!(std::mem::size_of::<SessionTelemetry>(), 0);
    assert_eq!(std::mem::size_of::<SessionTelemetryMetadata>(), 0);

    let _metadata = SessionTelemetryMetadata;

    let telemetry = SessionTelemetry::new();
    telemetry.reset_runtime_metrics();

    assert!(telemetry.runtime_metrics_summary().is_none());
}
