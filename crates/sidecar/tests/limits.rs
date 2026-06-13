//! Tests for `parse_vm_limits`: defaults, per-key overrides, and cross-field validation.

use std::collections::BTreeMap;

use agent_os_kernel::resource_accounting::ResourceLimits;
use agent_os_sidecar::limits::{parse_vm_limits, VmLimits};

const SIDECAR_FRAME_CAP: usize = 1024 * 1024;

fn metadata(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

#[test]
fn defaults_match_struct_default() {
    let parsed = parse_vm_limits(
        &BTreeMap::new(),
        ResourceLimits::default(),
        SIDECAR_FRAME_CAP,
    )
    .expect("empty metadata parses to defaults");
    assert_eq!(parsed, VmLimits::default());
}

#[test]
fn overrides_only_present_keys() {
    let md = metadata(&[
        ("limits.tools.max_tool_schema_bytes", "4096"),
        ("limits.wasm.max_module_file_bytes", "1048576"),
        ("limits.js_runtime.v8_heap_limit_mb", "256"),
        ("limits.python.execution_timeout_ms", "1000"),
        ("limits.http.max_fetch_response_bytes", "65536"),
    ]);
    let parsed =
        parse_vm_limits(&md, ResourceLimits::default(), SIDECAR_FRAME_CAP).expect("valid overrides");

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
    let mut resources = ResourceLimits::default();
    resources.max_processes = Some(8);
    let parsed = parse_vm_limits(&BTreeMap::new(), resources.clone(), SIDECAR_FRAME_CAP)
        .expect("resources thread through");
    assert_eq!(parsed.resources.max_processes, Some(8));
}

#[test]
fn rejects_unparseable_value() {
    let md = metadata(&[("limits.tools.max_tool_schema_bytes", "not-a-number")]);
    let error = parse_vm_limits(&md, ResourceLimits::default(), SIDECAR_FRAME_CAP)
        .expect_err("unparseable value rejected");
    assert!(error.to_string().contains("limits.tools.max_tool_schema_bytes"));
}

#[test]
fn rejects_fetch_body_exceeding_frame_cap() {
    let md = metadata(&[(
        "limits.http.max_fetch_response_bytes",
        &(SIDECAR_FRAME_CAP + 1).to_string(),
    )]);
    let error = parse_vm_limits(&md, ResourceLimits::default(), SIDECAR_FRAME_CAP)
        .expect_err("oversized fetch body rejected");
    assert!(error.to_string().contains("wire frame cap"));
}

#[test]
fn rejects_default_timeout_above_max() {
    let md = metadata(&[
        ("limits.tools.default_tool_timeout_ms", "60000"),
        ("limits.tools.max_tool_timeout_ms", "30000"),
    ]);
    let error = parse_vm_limits(&md, ResourceLimits::default(), SIDECAR_FRAME_CAP)
        .expect_err("default above max rejected");
    assert!(error.to_string().contains("max_tool_timeout_ms"));
}

#[test]
fn rejects_zero_buffer_cap() {
    let md = metadata(&[("limits.js_runtime.captured_output_limit_bytes", "0")]);
    let error = parse_vm_limits(&md, ResourceLimits::default(), SIDECAR_FRAME_CAP)
        .expect_err("zero buffer cap rejected");
    assert!(error
        .to_string()
        .contains("captured_output_limit_bytes"));
}
