use codex_otel::config::OtelExporter;
use codex_otel::metrics::{MetricsClient, MetricsConfig, MetricsError};
use codex_otel::{start_global_timer, SessionTelemetry};

fn assert_exporter_disabled(result: Result<codex_otel::Timer, MetricsError>) {
    assert!(matches!(result, Err(MetricsError::ExporterDisabled)));
}

#[test]
fn metrics_client_start_timer_reports_disabled_exporter() {
    let config = MetricsConfig::otlp(
        "test",
        "codex-otel-stub",
        "0.0.0",
        OtelExporter::None,
    );
    let client = MetricsClient::new(config).expect("stub metrics client should initialize");

    assert_exporter_disabled(client.start_timer("duration", &[]));
}

#[test]
fn session_telemetry_start_timer_reports_disabled_exporter() {
    assert_exporter_disabled(SessionTelemetry::new().start_timer("duration", &[]));
}

#[test]
fn global_timer_reports_disabled_exporter() {
    assert_exporter_disabled(start_global_timer("duration", &[]));
}
