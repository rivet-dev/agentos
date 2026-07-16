//! Native compatibility exports for shared VM-scoped runtime limits.

pub use agentos_native_sidecar_core::limits::{
    validate_vm_limits, AcpLimits, BindingLimits, HttpLimits, JsRuntimeLimits, PluginLimits,
    ProcessLimits, PythonLimits, VmLimits, WasmLimits, DEFAULT_ACP_MAX_READ_LINE_BYTES,
    DEFAULT_ACP_STDOUT_BUFFER_BYTE_LIMIT, DEFAULT_BINDING_TIMEOUT_MS,
    DEFAULT_JS_CAPTURED_OUTPUT_LIMIT_BYTES, DEFAULT_JS_EVENT_PAYLOAD_LIMIT_BYTES,
    DEFAULT_JS_STDIN_BUFFER_LIMIT_BYTES, DEFAULT_MAX_FETCH_RESPONSE_BYTES,
    DEFAULT_PROCESS_PENDING_EVENT_BYTES, DEFAULT_PROCESS_PENDING_EVENT_COUNT,
    DEFAULT_PROCESS_PENDING_STDIN_BYTES, DEFAULT_PYTHON_EXECUTION_TIMEOUT_MS,
    DEFAULT_PYTHON_MAX_OLD_SPACE_MB, DEFAULT_PYTHON_OUTPUT_BUFFER_MAX_BYTES,
    DEFAULT_PYTHON_VFS_RPC_TIMEOUT_MS, DEFAULT_V8_HEAP_LIMIT_MB, DEFAULT_V8_IPC_MAX_FRAME_BYTES,
    DEFAULT_WASM_CAPTURED_OUTPUT_LIMIT_BYTES, DEFAULT_WASM_MAX_MODULE_FILE_BYTES,
    DEFAULT_WASM_SYNC_READ_LIMIT_BYTES, MAX_BINDINGS_PER_COLLECTION,
    MAX_BINDING_EXAMPLE_INPUT_BYTES, MAX_BINDING_SCHEMA_BYTES, MAX_BINDING_TIMEOUT_MS,
    MAX_EXAMPLES_PER_BINDING, MAX_PERSISTED_MANIFEST_BYTES, MAX_PERSISTED_MANIFEST_FILE_BYTES,
    MAX_REGISTERED_BINDINGS_PER_VM, MAX_REGISTERED_BINDING_COLLECTIONS,
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
