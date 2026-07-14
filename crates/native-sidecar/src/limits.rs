//! Native compatibility exports for shared VM-scoped runtime limits.

pub use agentos_native_sidecar_core::limits::{
    validate_vm_limits, AcpLimits, HttpLimits, JsRuntimeLimits, PluginLimits, PythonLimits,
    ToolLimits, VmLimits, WasmLimits, DEFAULT_ACP_MAX_READ_LINE_BYTES,
    DEFAULT_ACP_STDOUT_BUFFER_BYTE_LIMIT, DEFAULT_JS_CAPTURED_OUTPUT_LIMIT_BYTES,
    DEFAULT_MAX_FETCH_RESPONSE_BYTES, DEFAULT_PYTHON_EXECUTION_TIMEOUT_MS,
    DEFAULT_PYTHON_MAX_OLD_SPACE_MB, DEFAULT_PYTHON_OUTPUT_BUFFER_MAX_BYTES,
    DEFAULT_PYTHON_VFS_RPC_TIMEOUT_MS, DEFAULT_TOOL_TIMEOUT_MS, DEFAULT_V8_HEAP_LIMIT_MB,
    DEFAULT_WASM_CAPTURED_OUTPUT_LIMIT_BYTES, DEFAULT_WASM_MAX_MODULE_FILE_BYTES,
    DEFAULT_WASM_SYNC_READ_LIMIT_BYTES, MAX_PERSISTED_MANIFEST_BYTES,
    MAX_PERSISTED_MANIFEST_FILE_BYTES, MAX_REGISTERED_TOOLKITS, MAX_REGISTERED_TOOLS_PER_VM,
    MAX_TOOLS_PER_TOOLKIT, MAX_TOOL_EXAMPLES_PER_TOOL, MAX_TOOL_EXAMPLE_INPUT_BYTES,
    MAX_TOOL_SCHEMA_BYTES, MAX_TOOL_TIMEOUT_MS,
};
use agentos_vm_config::VmLimitsConfig;

use crate::state::SidecarError;

pub fn vm_limits_from_config(
    config: Option<&VmLimitsConfig>,
    sidecar_max_frame_bytes: usize,
) -> Result<VmLimits, SidecarError> {
    agentos_native_sidecar_core::limits::vm_limits_from_config(config, sidecar_max_frame_bytes)
        .map_err(|error| SidecarError::InvalidState(error.to_string()))
}
