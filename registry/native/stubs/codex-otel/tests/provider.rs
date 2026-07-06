use std::collections::HashMap;
use std::path::PathBuf;

use codex_otel::config::{OtelExporter, OtelHttpProtocol, OtelSettings, OtelTlsConfig};
use codex_otel::OtelProvider;

#[test]
fn configured_exporters_remain_disabled() {
    let mut headers = HashMap::new();
    headers.insert("authorization".to_string(), "bearer test-token".to_string());

    let tls = Some(OtelTlsConfig {
        ca_certificate: Some(PathBuf::from("/certs/ca.pem")),
        client_certificate: Some(PathBuf::from("/certs/client.pem")),
        client_private_key: Some(PathBuf::from("/certs/client.key")),
    });

    let settings = OtelSettings {
        environment: "test".to_string(),
        service_name: "codex-otel-stub".to_string(),
        service_version: "0.0.0".to_string(),
        codex_home: PathBuf::from("/codex-home"),
        exporter: OtelExporter::OtlpHttp {
            endpoint: "https://otel.example.invalid/v1/traces".to_string(),
            headers: headers.clone(),
            protocol: OtelHttpProtocol::Json,
            tls: tls.clone(),
        },
        trace_exporter: OtelExporter::OtlpGrpc {
            endpoint: "https://otel.example.invalid:4317".to_string(),
            headers: headers.clone(),
            tls: tls.clone(),
        },
        metrics_exporter: OtelExporter::Statsig,
        runtime_metrics: true,
    };

    let provider = OtelProvider::from(&settings).expect("stub provider should not fail");

    assert!(provider.is_none());
}
