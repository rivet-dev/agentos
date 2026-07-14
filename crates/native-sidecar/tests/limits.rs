//! Tests for typed create-VM limits config defaults, overrides, and validation.

use agentos_native_sidecar::limits::{vm_limits_from_config, VmLimits};
use agentos_native_sidecar_core::{
    process_exited_event_with_result, CaptureChunkOutcome, CapturedOutputState,
    MAX_PROCESS_ID_BYTES,
};
use agentos_sidecar_protocol::protocol::{GuestRuntimeKind, OwnershipScope, StreamChannel};
use agentos_sidecar_protocol::wire::{event_frame_from_compat, ProtocolFrame, WireFrameCodec};
use agentos_vm_config::{
    HttpLimitsConfig, JsRuntimeLimitsConfig, PythonLimitsConfig, ResourceLimitsConfig,
    ToolLimitsConfig, VmLimitsConfig, WasmLimitsConfig,
};
use serde_json::json;

// Must match the production sidecar wire frame cap (wire::DEFAULT_MAX_FRAME_BYTES),
// which is what vm_limits_from_config is called with at runtime (lib.rs/state.rs).
const SIDECAR_FRAME_CAP: usize = agentos_sidecar_protocol::wire::DEFAULT_MAX_FRAME_BYTES;

#[test]
fn defaults_match_struct_default() {
    let parsed =
        vm_limits_from_config(None, SIDECAR_FRAME_CAP).expect("empty config parses to defaults");
    assert_eq!(parsed, VmLimits::default());
    assert_eq!(
        parsed.js_runtime.v8_heap_limit_mb,
        Some(128),
        "JavaScript heap must be bounded by default"
    );
    assert_eq!(
        parsed.resources.max_wasm_memory_bytes,
        Some(128 * 1024 * 1024),
        "WASM memory must be bounded by default"
    );
}

#[test]
fn overrides_only_present_keys() {
    let config = VmLimitsConfig {
        tools: Some(ToolLimitsConfig {
            max_tool_schema_bytes: Some(4096),
            ..Default::default()
        }),
        wasm: Some(WasmLimitsConfig {
            max_module_file_bytes: Some(1_048_576),
            ..Default::default()
        }),
        js_runtime: Some(JsRuntimeLimitsConfig {
            v8_heap_limit_mb: Some(256),
            ..Default::default()
        }),
        python: Some(PythonLimitsConfig {
            execution_timeout_ms: Some(1000),
            ..Default::default()
        }),
        http: Some(HttpLimitsConfig {
            max_fetch_response_bytes: Some(65_536),
        }),
        ..Default::default()
    };
    let parsed = vm_limits_from_config(Some(&config), SIDECAR_FRAME_CAP).expect("valid overrides");

    assert_eq!(parsed.tools.max_tool_schema_bytes, 4096);
    assert_eq!(parsed.wasm.max_module_file_bytes, 1_048_576);
    assert_eq!(parsed.js_runtime.v8_heap_limit_mb, Some(256));
    assert_eq!(parsed.python.execution_timeout_ms, 1000);
    assert_eq!(parsed.http.max_fetch_response_bytes, 65536);

    // Unspecified fields keep defaults.
    let defaults = VmLimits::default();
    assert_eq!(
        parsed.tools.max_registered_toolkits,
        defaults.tools.max_registered_toolkits
    );
    assert_eq!(
        parsed.wasm.sync_read_limit_bytes,
        defaults.wasm.sync_read_limit_bytes
    );
}

#[test]
fn resources_subset_threads_through() {
    let config = VmLimitsConfig {
        resources: Some(ResourceLimitsConfig {
            max_processes: Some(8),
            max_captured_output_bytes: Some(4096),
            ..Default::default()
        }),
        ..Default::default()
    };
    let parsed =
        vm_limits_from_config(Some(&config), SIDECAR_FRAME_CAP).expect("resources thread through");
    assert_eq!(parsed.resources.max_processes, Some(8));
    assert_eq!(parsed.max_captured_output_bytes, 4096);
}

#[test]
fn rejects_zero_aggregate_capture_budget() {
    let config = VmLimitsConfig {
        resources: Some(ResourceLimitsConfig {
            max_captured_output_bytes: Some(0),
            ..Default::default()
        }),
        ..Default::default()
    };
    let error = vm_limits_from_config(Some(&config), SIDECAR_FRAME_CAP)
        .expect_err("zero aggregate capture budget must be rejected");
    assert!(error.to_string().contains("maxCapturedOutputBytes"));
}

#[test]
fn rejects_unparseable_value() {
    let error = serde_json::from_value::<VmLimitsConfig>(json!({
        "tools": { "maxToolSchemaBytes": "not-a-number" }
    }))
    .expect_err("unparseable value rejected");
    assert!(error.to_string().contains("invalid type"));
}

#[test]
fn rejects_fetch_body_exceeding_frame_cap() {
    let config = VmLimitsConfig {
        http: Some(HttpLimitsConfig {
            max_fetch_response_bytes: Some((SIDECAR_FRAME_CAP + 1) as u64),
        }),
        ..Default::default()
    };
    let error = vm_limits_from_config(Some(&config), SIDECAR_FRAME_CAP)
        .expect_err("oversized fetch body rejected");
    assert!(error.to_string().contains("wire frame cap"));
}

#[test]
fn rejects_default_timeout_above_max() {
    let config = VmLimitsConfig {
        tools: Some(ToolLimitsConfig {
            default_tool_timeout_ms: Some(60_000),
            max_tool_timeout_ms: Some(30_000),
            ..Default::default()
        }),
        ..Default::default()
    };
    let error =
        vm_limits_from_config(Some(&config), SIDECAR_FRAME_CAP).expect_err("default above max");
    assert!(error.to_string().contains("max_tool_timeout_ms"));
}

#[test]
fn rejects_zero_buffer_cap() {
    let config = VmLimitsConfig {
        js_runtime: Some(JsRuntimeLimitsConfig {
            captured_output_limit_bytes: Some(0),
            ..Default::default()
        }),
        ..Default::default()
    };
    let error =
        vm_limits_from_config(Some(&config), SIDECAR_FRAME_CAP).expect_err("zero buffer cap");
    assert!(error.to_string().contains("captured_output_limit_bytes"));
}

#[test]
fn rejects_capture_limits_that_cannot_fit_both_streams_in_terminal_frame() {
    let limits = agentos_native_sidecar_core::VmLimits::default();
    let frame_cap = limits
        .js_runtime
        .captured_output_limit_bytes
        .saturating_mul(2)
        .saturating_add(4095);
    let error = agentos_native_sidecar_core::validate_vm_limits(&limits, frame_cap)
        .expect_err("combined captured streams must fit the terminal frame");
    assert!(error
        .to_string()
        .contains("limits.jsRuntime.capturedOutputLimitBytes"));
    assert!(error.to_string().contains("sidecar wire frame cap"));
}

#[test]
fn maximum_capture_terminal_with_maximum_process_id_fits_validated_frame_cap() {
    let frame_cap = 16 * 1024;
    let max_stream_bytes =
        (frame_cap - agentos_native_sidecar_core::CAPTURE_TERMINAL_FRAME_OVERHEAD_BYTES) / 2;
    let mut limits = agentos_native_sidecar_core::VmLimits::default();
    limits.http.max_fetch_response_bytes = frame_cap;
    limits.js_runtime.captured_output_limit_bytes = max_stream_bytes;
    limits.python.output_buffer_max_bytes = max_stream_bytes;
    limits.wasm.captured_output_limit_bytes = max_stream_bytes;
    agentos_native_sidecar_core::validate_vm_limits(&limits, frame_cap)
        .expect("maximum capture limits should validate");

    let process_id = "p".repeat(MAX_PROCESS_ID_BYTES);
    let mut capture = CapturedOutputState::for_runtime(
        &limits,
        GuestRuntimeKind::JavaScript,
        agentos_native_sidecar_core::CapturedOutputBudget::for_vm(&limits),
    );
    assert_eq!(
        capture.record_chunk(
            &process_id,
            StreamChannel::Stdout,
            &vec![b'o'; max_stream_bytes],
        ),
        CaptureChunkOutcome::Forward
    );
    assert_eq!(
        capture.record_chunk(
            &process_id,
            StreamChannel::Stderr,
            &vec![b'e'; max_stream_bytes],
        ),
        CaptureChunkOutcome::Forward
    );
    assert_eq!(
        capture.record_chunk(&process_id, StreamChannel::Stdout, b"!"),
        CaptureChunkOutcome::LimitExceeded
    );

    let event = process_exited_event_with_result(
        OwnershipScope::vm("connection", "session", "vm"),
        &process_id,
        137,
        Some(capture.into_result()),
    );
    let generated = event_frame_from_compat(event).expect("convert terminal event");
    WireFrameCodec::new(frame_cap)
        .encode_message(&ProtocolFrame::EventFrame(generated))
        .expect("validated worst-case capture terminal must fit the wire frame cap");
}
