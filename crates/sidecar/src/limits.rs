//! Typed, operator-tunable VM-scoped runtime limits.
//!
//! `VmLimits` is the single home for runtime bounds that operators may tune through
//! `CreateVmRequest.metadata`. Every field is a concrete value (not `Option`): the `Default`
//! impls own the numbers and they are byte-identical to the historical hardcoded constants, so
//! behavior is unchanged unless an operator overrides a key. Parsing follows the proven
//! `parse_resource_limits` precedent: start from `VmLimits::default()`, override only keys that
//! are present in the metadata map, and fail loudly on unparseable or invalid values.
//!
//! Key namespace:
//! - Kernel `ResourceLimits` fields keep their existing `resource.*` metadata keys (parsed by
//!   `crate::vm::parse_resource_limits`).
//! - Every other group uses `limits.<group>.<field>` snake_case keys, for example
//!   `limits.http.max_fetch_response_bytes` or `limits.wasm.max_module_file_bytes`.

use std::collections::BTreeMap;

use agent_os_kernel::resource_accounting::ResourceLimits;

use crate::protocol::DEFAULT_MAX_FRAME_BYTES;
use crate::state::SidecarError;

/// Default cap on `vm.fetch()` buffered response bodies. Historically aliased to the wire frame
/// cap; decoupled here but still validated to stay within the negotiated frame budget.
pub const DEFAULT_MAX_FETCH_RESPONSE_BYTES: usize = DEFAULT_MAX_FRAME_BYTES;

pub const DEFAULT_TOOL_TIMEOUT_MS: u64 = 30_000;
pub const MAX_TOOL_TIMEOUT_MS: u64 = 300_000;
pub const MAX_REGISTERED_TOOLKITS: usize = 64;
pub const MAX_REGISTERED_TOOLS_PER_VM: usize = 256;
pub const MAX_TOOLS_PER_TOOLKIT: usize = 64;
pub const MAX_TOOL_SCHEMA_BYTES: usize = 16 * 1024;
pub const MAX_TOOL_EXAMPLES_PER_TOOL: usize = 16;
pub const MAX_TOOL_EXAMPLE_INPUT_BYTES: usize = 4 * 1024;

pub const MAX_PERSISTED_MANIFEST_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_PERSISTED_MANIFEST_FILE_BYTES: u64 = 1024 * 1024 * 1024;

pub const DEFAULT_ACP_MAX_READ_LINE_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_ACP_STDOUT_BUFFER_BYTE_LIMIT: usize = 1024 * 1024;

pub const DEFAULT_JS_CAPTURED_OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_JS_STDIN_BUFFER_LIMIT_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_JS_EVENT_PAYLOAD_LIMIT_BYTES: usize = 1024 * 1024;
pub const DEFAULT_V8_IPC_MAX_FRAME_BYTES: u32 = 64 * 1024 * 1024;

pub const DEFAULT_PYTHON_OUTPUT_BUFFER_MAX_BYTES: usize = 1024 * 1024;
pub const DEFAULT_PYTHON_EXECUTION_TIMEOUT_MS: u64 = 5 * 60 * 1000;
pub const DEFAULT_PYTHON_VFS_RPC_TIMEOUT_MS: u64 = 30 * 1000;

pub const DEFAULT_WASM_MAX_MODULE_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_WASM_CAPTURED_OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_WASM_SYNC_READ_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// All operator-tunable VM-scoped limits. Fields are concrete values; the `Default` impls own the
/// numbers and equal today's hardcoded constants, so unset operator config leaves behavior
/// unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmLimits {
    /// Kernel resource limits (existing type, existing `resource.*` keys).
    pub resources: ResourceLimits,
    pub http: HttpLimits,
    pub tools: ToolLimits,
    pub plugins: PluginLimits,
    pub acp: AcpLimits,
    pub js_runtime: JsRuntimeLimits,
    pub python: PythonLimits,
    pub wasm: WasmLimits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpLimits {
    /// Cap on `vm.fetch()` buffered response bodies. Must be `<=` the sidecar wire frame cap.
    pub max_fetch_response_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolLimits {
    pub default_tool_timeout_ms: u64,
    pub max_tool_timeout_ms: u64,
    pub max_registered_toolkits: usize,
    pub max_registered_tools_per_vm: usize,
    pub max_tools_per_toolkit: usize,
    pub max_tool_schema_bytes: usize,
    pub max_tool_examples_per_tool: usize,
    pub max_tool_example_input_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginLimits {
    pub max_persisted_manifest_bytes: usize,
    pub max_persisted_manifest_file_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpLimits {
    /// Maximum length of a single ACP adapter stdout line. Threaded into `AcpClientOptions`.
    pub max_read_line_bytes: usize,
    /// Pre-session ACP adapter stdout buffer cap.
    pub stdout_buffer_byte_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsRuntimeLimits {
    /// `None` keeps the V8 engine default heap. Maps to the existing `AGENT_OS_V8_HEAP_LIMIT_MB`
    /// per-execution env knob.
    pub v8_heap_limit_mb: Option<u32>,
    pub captured_output_limit_bytes: usize,
    pub stdin_buffer_limit_bytes: usize,
    pub event_payload_limit_bytes: usize,
    /// V8 IPC codec frame cap. Must feed both codec sides (`crates/execution/src/v8_ipc.rs` and
    /// `crates/v8-runtime/src/ipc_binary.rs`).
    pub v8_ipc_max_frame_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonLimits {
    pub output_buffer_max_bytes: usize,
    pub execution_timeout_ms: u64,
    pub vfs_rpc_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmLimits {
    pub max_module_file_bytes: u64,
    pub captured_output_limit_bytes: usize,
    /// WASM sync read cap. Also templated into the JS runner shim, so it must flow from one field.
    pub sync_read_limit_bytes: usize,
}

impl Default for VmLimits {
    fn default() -> Self {
        Self {
            resources: ResourceLimits::default(),
            http: HttpLimits::default(),
            tools: ToolLimits::default(),
            plugins: PluginLimits::default(),
            acp: AcpLimits::default(),
            js_runtime: JsRuntimeLimits::default(),
            python: PythonLimits::default(),
            wasm: WasmLimits::default(),
        }
    }
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            max_fetch_response_bytes: DEFAULT_MAX_FETCH_RESPONSE_BYTES,
        }
    }
}

impl Default for ToolLimits {
    fn default() -> Self {
        Self {
            default_tool_timeout_ms: DEFAULT_TOOL_TIMEOUT_MS,
            max_tool_timeout_ms: MAX_TOOL_TIMEOUT_MS,
            max_registered_toolkits: MAX_REGISTERED_TOOLKITS,
            max_registered_tools_per_vm: MAX_REGISTERED_TOOLS_PER_VM,
            max_tools_per_toolkit: MAX_TOOLS_PER_TOOLKIT,
            max_tool_schema_bytes: MAX_TOOL_SCHEMA_BYTES,
            max_tool_examples_per_tool: MAX_TOOL_EXAMPLES_PER_TOOL,
            max_tool_example_input_bytes: MAX_TOOL_EXAMPLE_INPUT_BYTES,
        }
    }
}

impl Default for PluginLimits {
    fn default() -> Self {
        Self {
            max_persisted_manifest_bytes: MAX_PERSISTED_MANIFEST_BYTES,
            max_persisted_manifest_file_bytes: MAX_PERSISTED_MANIFEST_FILE_BYTES,
        }
    }
}

impl Default for AcpLimits {
    fn default() -> Self {
        Self {
            max_read_line_bytes: DEFAULT_ACP_MAX_READ_LINE_BYTES,
            stdout_buffer_byte_limit: DEFAULT_ACP_STDOUT_BUFFER_BYTE_LIMIT,
        }
    }
}

impl Default for JsRuntimeLimits {
    fn default() -> Self {
        Self {
            v8_heap_limit_mb: None,
            captured_output_limit_bytes: DEFAULT_JS_CAPTURED_OUTPUT_LIMIT_BYTES,
            stdin_buffer_limit_bytes: DEFAULT_JS_STDIN_BUFFER_LIMIT_BYTES,
            event_payload_limit_bytes: DEFAULT_JS_EVENT_PAYLOAD_LIMIT_BYTES,
            v8_ipc_max_frame_bytes: DEFAULT_V8_IPC_MAX_FRAME_BYTES,
        }
    }
}

impl Default for PythonLimits {
    fn default() -> Self {
        Self {
            output_buffer_max_bytes: DEFAULT_PYTHON_OUTPUT_BUFFER_MAX_BYTES,
            execution_timeout_ms: DEFAULT_PYTHON_EXECUTION_TIMEOUT_MS,
            vfs_rpc_timeout_ms: DEFAULT_PYTHON_VFS_RPC_TIMEOUT_MS,
        }
    }
}

impl Default for WasmLimits {
    fn default() -> Self {
        Self {
            max_module_file_bytes: DEFAULT_WASM_MAX_MODULE_FILE_BYTES,
            captured_output_limit_bytes: DEFAULT_WASM_CAPTURED_OUTPUT_LIMIT_BYTES,
            sync_read_limit_bytes: DEFAULT_WASM_SYNC_READ_LIMIT_BYTES,
        }
    }
}

/// Parse the full set of VM-scoped limits from `CreateVmRequest.metadata`.
///
/// `resources` is parsed by `crate::vm::parse_resource_limits`. Every other group reads
/// `limits.<group>.<field>` keys, overriding only keys that are present, then runs cross-field
/// validation. `sidecar_max_frame_bytes` is the negotiated wire frame cap; HTTP fetch bodies must
/// fit within it.
pub fn parse_vm_limits(
    metadata: &BTreeMap<String, String>,
    resources: ResourceLimits,
    sidecar_max_frame_bytes: usize,
) -> Result<VmLimits, SidecarError> {
    let mut limits = VmLimits {
        resources,
        ..VmLimits::default()
    };

    // HTTP.
    if let Some(value) = metadata.get("limits.http.max_fetch_response_bytes") {
        limits.http.max_fetch_response_bytes =
            parse_usize("limits.http.max_fetch_response_bytes", value)?;
    }

    // Tools.
    if let Some(value) = metadata.get("limits.tools.default_tool_timeout_ms") {
        limits.tools.default_tool_timeout_ms =
            parse_u64("limits.tools.default_tool_timeout_ms", value)?;
    }
    if let Some(value) = metadata.get("limits.tools.max_tool_timeout_ms") {
        limits.tools.max_tool_timeout_ms = parse_u64("limits.tools.max_tool_timeout_ms", value)?;
    }
    if let Some(value) = metadata.get("limits.tools.max_registered_toolkits") {
        limits.tools.max_registered_toolkits =
            parse_usize("limits.tools.max_registered_toolkits", value)?;
    }
    if let Some(value) = metadata.get("limits.tools.max_registered_tools_per_vm") {
        limits.tools.max_registered_tools_per_vm =
            parse_usize("limits.tools.max_registered_tools_per_vm", value)?;
    }
    if let Some(value) = metadata.get("limits.tools.max_tools_per_toolkit") {
        limits.tools.max_tools_per_toolkit =
            parse_usize("limits.tools.max_tools_per_toolkit", value)?;
    }
    if let Some(value) = metadata.get("limits.tools.max_tool_schema_bytes") {
        limits.tools.max_tool_schema_bytes =
            parse_usize("limits.tools.max_tool_schema_bytes", value)?;
    }
    if let Some(value) = metadata.get("limits.tools.max_tool_examples_per_tool") {
        limits.tools.max_tool_examples_per_tool =
            parse_usize("limits.tools.max_tool_examples_per_tool", value)?;
    }
    if let Some(value) = metadata.get("limits.tools.max_tool_example_input_bytes") {
        limits.tools.max_tool_example_input_bytes =
            parse_usize("limits.tools.max_tool_example_input_bytes", value)?;
    }

    // Plugins.
    if let Some(value) = metadata.get("limits.plugins.max_persisted_manifest_bytes") {
        limits.plugins.max_persisted_manifest_bytes =
            parse_usize("limits.plugins.max_persisted_manifest_bytes", value)?;
    }
    if let Some(value) = metadata.get("limits.plugins.max_persisted_manifest_file_bytes") {
        limits.plugins.max_persisted_manifest_file_bytes =
            parse_u64("limits.plugins.max_persisted_manifest_file_bytes", value)?;
    }

    // ACP.
    if let Some(value) = metadata.get("limits.acp.max_read_line_bytes") {
        limits.acp.max_read_line_bytes = parse_usize("limits.acp.max_read_line_bytes", value)?;
    }
    if let Some(value) = metadata.get("limits.acp.stdout_buffer_byte_limit") {
        limits.acp.stdout_buffer_byte_limit =
            parse_usize("limits.acp.stdout_buffer_byte_limit", value)?;
    }

    // JS runtime.
    if let Some(value) = metadata.get("limits.js_runtime.v8_heap_limit_mb") {
        limits.js_runtime.v8_heap_limit_mb =
            Some(parse_u32("limits.js_runtime.v8_heap_limit_mb", value)?);
    }
    if let Some(value) = metadata.get("limits.js_runtime.captured_output_limit_bytes") {
        limits.js_runtime.captured_output_limit_bytes =
            parse_usize("limits.js_runtime.captured_output_limit_bytes", value)?;
    }
    if let Some(value) = metadata.get("limits.js_runtime.stdin_buffer_limit_bytes") {
        limits.js_runtime.stdin_buffer_limit_bytes =
            parse_usize("limits.js_runtime.stdin_buffer_limit_bytes", value)?;
    }
    if let Some(value) = metadata.get("limits.js_runtime.event_payload_limit_bytes") {
        limits.js_runtime.event_payload_limit_bytes =
            parse_usize("limits.js_runtime.event_payload_limit_bytes", value)?;
    }
    if let Some(value) = metadata.get("limits.js_runtime.v8_ipc_max_frame_bytes") {
        limits.js_runtime.v8_ipc_max_frame_bytes =
            parse_u32("limits.js_runtime.v8_ipc_max_frame_bytes", value)?;
    }

    // Python.
    if let Some(value) = metadata.get("limits.python.output_buffer_max_bytes") {
        limits.python.output_buffer_max_bytes =
            parse_usize("limits.python.output_buffer_max_bytes", value)?;
    }
    if let Some(value) = metadata.get("limits.python.execution_timeout_ms") {
        limits.python.execution_timeout_ms =
            parse_u64("limits.python.execution_timeout_ms", value)?;
    }
    if let Some(value) = metadata.get("limits.python.vfs_rpc_timeout_ms") {
        limits.python.vfs_rpc_timeout_ms = parse_u64("limits.python.vfs_rpc_timeout_ms", value)?;
    }

    // WASM.
    if let Some(value) = metadata.get("limits.wasm.max_module_file_bytes") {
        limits.wasm.max_module_file_bytes =
            parse_u64("limits.wasm.max_module_file_bytes", value)?;
    }
    if let Some(value) = metadata.get("limits.wasm.captured_output_limit_bytes") {
        limits.wasm.captured_output_limit_bytes =
            parse_usize("limits.wasm.captured_output_limit_bytes", value)?;
    }
    if let Some(value) = metadata.get("limits.wasm.sync_read_limit_bytes") {
        limits.wasm.sync_read_limit_bytes =
            parse_usize("limits.wasm.sync_read_limit_bytes", value)?;
    }

    validate_vm_limits(&limits, sidecar_max_frame_bytes)?;

    Ok(limits)
}

/// Cross-field validation. Fail-by-default: reject any configuration that would deadlock or
/// violate the wire frame budget with an explicit, actionable message.
fn validate_vm_limits(
    limits: &VmLimits,
    sidecar_max_frame_bytes: usize,
) -> Result<(), SidecarError> {
    if limits.http.max_fetch_response_bytes == 0 {
        return Err(SidecarError::InvalidState(
            "limits.http.max_fetch_response_bytes must be greater than zero".to_string(),
        ));
    }
    if limits.http.max_fetch_response_bytes > sidecar_max_frame_bytes {
        return Err(SidecarError::InvalidState(format!(
            "limits.http.max_fetch_response_bytes ({}) must be <= the sidecar wire frame cap ({})",
            limits.http.max_fetch_response_bytes, sidecar_max_frame_bytes
        )));
    }

    if limits.tools.default_tool_timeout_ms > limits.tools.max_tool_timeout_ms {
        return Err(SidecarError::InvalidState(format!(
            "limits.tools.default_tool_timeout_ms ({}) must be <= limits.tools.max_tool_timeout_ms ({})",
            limits.tools.default_tool_timeout_ms, limits.tools.max_tool_timeout_ms
        )));
    }

    let nonzero_usize: [(&str, usize); 13] = [
        (
            "limits.tools.max_registered_toolkits",
            limits.tools.max_registered_toolkits,
        ),
        (
            "limits.tools.max_registered_tools_per_vm",
            limits.tools.max_registered_tools_per_vm,
        ),
        (
            "limits.tools.max_tools_per_toolkit",
            limits.tools.max_tools_per_toolkit,
        ),
        (
            "limits.tools.max_tool_schema_bytes",
            limits.tools.max_tool_schema_bytes,
        ),
        (
            "limits.tools.max_tool_example_input_bytes",
            limits.tools.max_tool_example_input_bytes,
        ),
        (
            "limits.plugins.max_persisted_manifest_bytes",
            limits.plugins.max_persisted_manifest_bytes,
        ),
        ("limits.acp.max_read_line_bytes", limits.acp.max_read_line_bytes),
        (
            "limits.acp.stdout_buffer_byte_limit",
            limits.acp.stdout_buffer_byte_limit,
        ),
        (
            "limits.js_runtime.captured_output_limit_bytes",
            limits.js_runtime.captured_output_limit_bytes,
        ),
        (
            "limits.js_runtime.stdin_buffer_limit_bytes",
            limits.js_runtime.stdin_buffer_limit_bytes,
        ),
        (
            "limits.js_runtime.event_payload_limit_bytes",
            limits.js_runtime.event_payload_limit_bytes,
        ),
        (
            "limits.python.output_buffer_max_bytes",
            limits.python.output_buffer_max_bytes,
        ),
        (
            "limits.wasm.captured_output_limit_bytes",
            limits.wasm.captured_output_limit_bytes,
        ),
    ];
    for (key, value) in nonzero_usize {
        if value == 0 {
            return Err(SidecarError::InvalidState(format!(
                "{key} must be greater than zero"
            )));
        }
    }

    if limits.wasm.sync_read_limit_bytes == 0 {
        return Err(SidecarError::InvalidState(
            "limits.wasm.sync_read_limit_bytes must be greater than zero".to_string(),
        ));
    }
    if limits.wasm.max_module_file_bytes == 0 {
        return Err(SidecarError::InvalidState(
            "limits.wasm.max_module_file_bytes must be greater than zero".to_string(),
        ));
    }
    if limits.js_runtime.v8_ipc_max_frame_bytes == 0 {
        return Err(SidecarError::InvalidState(
            "limits.js_runtime.v8_ipc_max_frame_bytes must be greater than zero".to_string(),
        ));
    }
    if limits.python.execution_timeout_ms == 0 {
        return Err(SidecarError::InvalidState(
            "limits.python.execution_timeout_ms must be greater than zero".to_string(),
        ));
    }
    if limits.python.vfs_rpc_timeout_ms == 0 {
        return Err(SidecarError::InvalidState(
            "limits.python.vfs_rpc_timeout_ms must be greater than zero".to_string(),
        ));
    }
    if let Some(0) = limits.js_runtime.v8_heap_limit_mb {
        return Err(SidecarError::InvalidState(
            "limits.js_runtime.v8_heap_limit_mb must be greater than zero".to_string(),
        ));
    }

    Ok(())
}

fn parse_usize(key: &str, value: &str) -> Result<usize, SidecarError> {
    value
        .parse::<usize>()
        .map_err(|error| SidecarError::InvalidState(format!("invalid limit {key}={value}: {error}")))
}

fn parse_u64(key: &str, value: &str) -> Result<u64, SidecarError> {
    value
        .parse::<u64>()
        .map_err(|error| SidecarError::InvalidState(format!("invalid limit {key}={value}: {error}")))
}

fn parse_u32(key: &str, value: &str) -> Result<u32, SidecarError> {
    value
        .parse::<u32>()
        .map_err(|error| SidecarError::InvalidState(format!("invalid limit {key}={value}: {error}")))
}
