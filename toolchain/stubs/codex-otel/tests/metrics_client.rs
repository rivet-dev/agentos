use std::collections::HashMap;
use std::time::Duration;

use codex_otel::config::{OtelExporter, OtelHttpProtocol};
use codex_otel::metrics::{global, MetricsClient, MetricsConfig};

fn config() -> MetricsConfig {
    MetricsConfig::otlp("env", "service", "version", OtelExporter::None)
}

fn configured_exporter_config() -> MetricsConfig {
    let mut headers = HashMap::new();
    headers.insert(
        "authorization".to_string(),
        "Bearer secret-token".to_string(),
    );

    MetricsConfig::otlp(
        "prod",
        "secret-service",
        "version",
        OtelExporter::OtlpHttp {
            endpoint: "https://telemetry.example.invalid/v1/metrics?token=secret".to_string(),
            headers,
            protocol: OtelHttpProtocol::Json,
            tls: None,
        },
    )
}

#[test]
fn metrics_client_remains_inert() {
    assert_eq!(std::mem::size_of::<MetricsClient>(), 0);
    assert!(global().is_none());

    let client = MetricsClient::new(config()).expect("stub metrics client should initialize");

    assert!(client.counter("", -1, &[("", "bad tag")]).is_ok());
    assert!(client.histogram("", i64::MIN, &[("tag", "")]).is_ok());
    assert!(client
        .record_duration("", Duration::MAX, &[("tag with space", "value")])
        .is_ok());
    assert!(client.shutdown().is_ok());
    assert!(global().is_none());
}

#[test]
fn configured_exporter_is_ignored_by_metrics_client() {
    assert!(global().is_none());

    let client =
        MetricsClient::new(configured_exporter_config()).expect("configured exporter is inert");

    assert_eq!(std::mem::size_of_val(&client), 0);
    assert!(client
        .counter("requests", 1, &[("route", "/secret")])
        .is_ok());
    assert!(client.shutdown().is_ok());
    assert!(global().is_none());
}

#[test]
fn metrics_config_tag_and_runtime_options_are_no_ops() {
    let config = config()
        .with_export_interval(Duration::MAX)
        .with_runtime_reader()
        .with_tag("", "secret-token")
        .expect("stub tag validation should be disabled");

    assert_eq!(config.environment, "env");
    assert_eq!(config.service_name, "service");
    assert_eq!(config.service_version, "version");
    assert!(global().is_none());
}
