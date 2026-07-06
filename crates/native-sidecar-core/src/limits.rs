//! Typed, operator-tunable VM-scoped runtime limits.
//!
//! `VmLimits` is the single home for runtime bounds that operators may tune through the typed
//! create-VM JSON config. Every field is a concrete value (not `Option`): the `Default` impls own
//! the numbers and they are byte-identical to the historical hardcoded constants, so behavior is
//! unchanged unless an operator overrides a config field.

use agentos_kernel::resource_accounting::ResourceLimits;
use agentos_vm_config::{ResourceLimitsConfig, VmLimitsConfig};

use crate::SidecarCoreError;

/// Default cap on `vm.fetch()` buffered response bodies. Historically aliased to the wire frame
/// cap; decoupled here but still validated to stay within the negotiated frame budget.
pub const DEFAULT_MAX_FETCH_RESPONSE_BYTES: usize = 1024 * 1024;

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
pub const DEFAULT_V8_HEAP_LIMIT_MB: u32 = 128;
pub const DEFAULT_V8_CPU_TIME_LIMIT_MS: u32 = 30_000;
pub const DEFAULT_V8_WALL_CLOCK_LIMIT_MS: u32 = 0;
pub const DEFAULT_NODE_IMPORT_CACHE_MATERIALIZE_TIMEOUT_MS: u64 = 30_000;

pub const DEFAULT_PYTHON_OUTPUT_BUFFER_MAX_BYTES: usize = 1024 * 1024;
pub const DEFAULT_PYTHON_EXECUTION_TIMEOUT_MS: u64 = 5 * 60 * 1000;
/// `0` keeps the Pyodide runner's V8 old-space at the engine default.
pub const DEFAULT_PYTHON_MAX_OLD_SPACE_MB: usize = 0;
pub const DEFAULT_PYTHON_VFS_RPC_TIMEOUT_MS: u64 = 30 * 1000;

pub const DEFAULT_WASM_MAX_MODULE_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_WASM_CAPTURED_OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_WASM_SYNC_READ_LIMIT_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_WASM_PREWARM_TIMEOUT_MS: u64 = 30_000;
pub const DEFAULT_WASM_RUNNER_HEAP_LIMIT_MB: u32 = 2048;

/// All operator-tunable VM-scoped limits. Fields are concrete values; the `Default` impls own the
/// numbers and equal today's hardcoded constants, so unset operator config leaves behavior
/// unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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

pub fn virtual_os_cpu_count(resource_limits: &ResourceLimits) -> usize {
    resource_limits.virtual_cpu_count.unwrap_or(1).max(1)
}

pub fn virtual_os_totalmem_bytes(resource_limits: &ResourceLimits) -> u64 {
    resource_limits
        .max_wasm_memory_bytes
        .unwrap_or(1024 * 1024 * 1024)
}

pub fn virtual_os_freemem_bytes(resource_limits: &ResourceLimits) -> u64 {
    resource_limits
        .max_wasm_memory_bytes
        .unwrap_or(512 * 1024 * 1024)
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
    /// `None` keeps the V8 engine default heap. Carried as the typed
    /// `JavascriptExecutionLimits.v8_heap_limit_mb` on the execution request
    /// (no longer the `AGENTOS_V8_HEAP_LIMIT_MB` env knob).
    pub v8_heap_limit_mb: Option<u32>,
    /// Sync-RPC blocking-wait ceiling in ms. `None` keeps the engine default.
    pub sync_rpc_wait_timeout_ms: Option<u64>,
    /// Active JavaScript CPU-time budget in ms. `0` disables the CPU watchdog.
    pub cpu_time_limit_ms: u32,
    /// JavaScript wall-clock backstop in ms. `0` disables the wall-clock watchdog.
    pub wall_clock_limit_ms: u32,
    /// Timeout for materializing the per-VM Node import cache.
    pub import_cache_materialize_timeout_ms: u64,
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
    /// Pyodide V8 old-space cap in MB (`0` keeps the V8 default).
    pub max_old_space_mb: usize,
    pub vfs_rpc_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmLimits {
    pub max_module_file_bytes: u64,
    pub captured_output_limit_bytes: usize,
    /// WASM sync read cap. Also templated into the JS runner shim, so it must flow from one field.
    pub sync_read_limit_bytes: usize,
    /// Best-effort warmup/compile-cache timeout.
    pub prewarm_timeout_ms: u64,
    /// V8 heap cap for the trusted JS runner isolate that hosts WASI/WASM.
    pub runner_heap_limit_mb: u32,
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
            // Workers-style 128 MiB heap cap by default. Operators can raise or
            // clear this through trusted VM config when a VM needs more room.
            v8_heap_limit_mb: Some(DEFAULT_V8_HEAP_LIMIT_MB),
            sync_rpc_wait_timeout_ms: None,
            cpu_time_limit_ms: DEFAULT_V8_CPU_TIME_LIMIT_MS,
            wall_clock_limit_ms: DEFAULT_V8_WALL_CLOCK_LIMIT_MS,
            import_cache_materialize_timeout_ms: DEFAULT_NODE_IMPORT_CACHE_MATERIALIZE_TIMEOUT_MS,
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
            max_old_space_mb: DEFAULT_PYTHON_MAX_OLD_SPACE_MB,
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
            prewarm_timeout_ms: DEFAULT_WASM_PREWARM_TIMEOUT_MS,
            runner_heap_limit_mb: DEFAULT_WASM_RUNNER_HEAP_LIMIT_MB,
        }
    }
}

pub fn vm_limits_from_config(
    config: Option<&VmLimitsConfig>,
    sidecar_max_frame_bytes: usize,
) -> Result<VmLimits, SidecarCoreError> {
    let mut limits = VmLimits::default();
    let Some(config) = config else {
        validate_vm_limits(&limits, sidecar_max_frame_bytes)?;
        return Ok(limits);
    };

    if let Some(resources) = config.resources.as_ref() {
        apply_resource_limits_config(&mut limits.resources, resources)?;
    }
    if let Some(http) = config.http.as_ref() {
        set_usize(
            &mut limits.http.max_fetch_response_bytes,
            http.max_fetch_response_bytes,
            "limits.http.maxFetchResponseBytes",
        )?;
    }
    if let Some(tools) = config.tools.as_ref() {
        set_u64(
            &mut limits.tools.default_tool_timeout_ms,
            tools.default_tool_timeout_ms,
            "limits.tools.defaultToolTimeoutMs",
        )?;
        set_u64(
            &mut limits.tools.max_tool_timeout_ms,
            tools.max_tool_timeout_ms,
            "limits.tools.maxToolTimeoutMs",
        )?;
        set_usize(
            &mut limits.tools.max_registered_toolkits,
            tools.max_registered_toolkits,
            "limits.tools.maxRegisteredToolkits",
        )?;
        set_usize(
            &mut limits.tools.max_registered_tools_per_vm,
            tools.max_registered_tools_per_vm,
            "limits.tools.maxRegisteredToolsPerVm",
        )?;
        set_usize(
            &mut limits.tools.max_tools_per_toolkit,
            tools.max_tools_per_toolkit,
            "limits.tools.maxToolsPerToolkit",
        )?;
        set_usize(
            &mut limits.tools.max_tool_schema_bytes,
            tools.max_tool_schema_bytes,
            "limits.tools.maxToolSchemaBytes",
        )?;
        set_usize(
            &mut limits.tools.max_tool_examples_per_tool,
            tools.max_tool_examples_per_tool,
            "limits.tools.maxToolExamplesPerTool",
        )?;
        set_usize(
            &mut limits.tools.max_tool_example_input_bytes,
            tools.max_tool_example_input_bytes,
            "limits.tools.maxToolExampleInputBytes",
        )?;
    }
    if let Some(plugins) = config.plugins.as_ref() {
        set_usize(
            &mut limits.plugins.max_persisted_manifest_bytes,
            plugins.max_persisted_manifest_bytes,
            "limits.plugins.maxPersistedManifestBytes",
        )?;
        set_u64(
            &mut limits.plugins.max_persisted_manifest_file_bytes,
            plugins.max_persisted_manifest_file_bytes,
            "limits.plugins.maxPersistedManifestFileBytes",
        )?;
    }
    if let Some(acp) = config.acp.as_ref() {
        set_usize(
            &mut limits.acp.max_read_line_bytes,
            acp.max_read_line_bytes,
            "limits.acp.maxReadLineBytes",
        )?;
        set_usize(
            &mut limits.acp.stdout_buffer_byte_limit,
            acp.stdout_buffer_byte_limit,
            "limits.acp.stdoutBufferByteLimit",
        )?;
    }
    if let Some(js_runtime) = config.js_runtime.as_ref() {
        if let Some(value) = js_runtime.v8_heap_limit_mb {
            limits.js_runtime.v8_heap_limit_mb = Some(
                u32::try_from(value)
                    .map_err(|_| integer_too_large("limits.jsRuntime.v8HeapLimitMb", value))?,
            );
        }
        if let Some(value) = js_runtime.cpu_time_limit_ms {
            limits.js_runtime.cpu_time_limit_ms = u32::try_from(value)
                .map_err(|_| integer_too_large("limits.jsRuntime.cpuTimeLimitMs", value))?;
        }
        if let Some(value) = js_runtime.wall_clock_limit_ms {
            limits.js_runtime.wall_clock_limit_ms = u32::try_from(value)
                .map_err(|_| integer_too_large("limits.jsRuntime.wallClockLimitMs", value))?;
        }
        set_u64(
            &mut limits.js_runtime.import_cache_materialize_timeout_ms,
            js_runtime.import_cache_materialize_timeout_ms,
            "limits.jsRuntime.importCacheMaterializeTimeoutMs",
        )?;
        set_usize(
            &mut limits.js_runtime.captured_output_limit_bytes,
            js_runtime.captured_output_limit_bytes,
            "limits.jsRuntime.capturedOutputLimitBytes",
        )?;
        set_usize(
            &mut limits.js_runtime.stdin_buffer_limit_bytes,
            js_runtime.stdin_buffer_limit_bytes,
            "limits.jsRuntime.stdinBufferLimitBytes",
        )?;
        set_usize(
            &mut limits.js_runtime.event_payload_limit_bytes,
            js_runtime.event_payload_limit_bytes,
            "limits.jsRuntime.eventPayloadLimitBytes",
        )?;
        if let Some(value) = js_runtime.v8_ipc_max_frame_bytes {
            limits.js_runtime.v8_ipc_max_frame_bytes = u32::try_from(value)
                .map_err(|_| integer_too_large("limits.jsRuntime.v8IpcMaxFrameBytes", value))?;
        }
        if let Some(value) = js_runtime.sync_rpc_wait_timeout_ms {
            limits.js_runtime.sync_rpc_wait_timeout_ms = Some(value);
        }
    }
    if let Some(python) = config.python.as_ref() {
        set_usize(
            &mut limits.python.output_buffer_max_bytes,
            python.output_buffer_max_bytes,
            "limits.python.outputBufferMaxBytes",
        )?;
        set_u64(
            &mut limits.python.execution_timeout_ms,
            python.execution_timeout_ms,
            "limits.python.executionTimeoutMs",
        )?;
        set_usize(
            &mut limits.python.max_old_space_mb,
            python.max_old_space_mb,
            "limits.python.maxOldSpaceMb",
        )?;
        set_u64(
            &mut limits.python.vfs_rpc_timeout_ms,
            python.vfs_rpc_timeout_ms,
            "limits.python.vfsRpcTimeoutMs",
        )?;
    }
    if let Some(wasm) = config.wasm.as_ref() {
        set_u64(
            &mut limits.wasm.max_module_file_bytes,
            wasm.max_module_file_bytes,
            "limits.wasm.maxModuleFileBytes",
        )?;
        set_usize(
            &mut limits.wasm.captured_output_limit_bytes,
            wasm.captured_output_limit_bytes,
            "limits.wasm.capturedOutputLimitBytes",
        )?;
        set_usize(
            &mut limits.wasm.sync_read_limit_bytes,
            wasm.sync_read_limit_bytes,
            "limits.wasm.syncReadLimitBytes",
        )?;
        set_u64(
            &mut limits.wasm.prewarm_timeout_ms,
            wasm.prewarm_timeout_ms,
            "limits.wasm.prewarmTimeoutMs",
        )?;
        if let Some(value) = wasm.runner_heap_limit_mb {
            limits.wasm.runner_heap_limit_mb = u32::try_from(value)
                .map_err(|_| integer_too_large("limits.wasm.runnerHeapLimitMb", value))?;
        }
    }

    validate_vm_limits(&limits, sidecar_max_frame_bytes)?;
    Ok(limits)
}

fn apply_resource_limits_config(
    limits: &mut ResourceLimits,
    config: &ResourceLimitsConfig,
) -> Result<(), SidecarCoreError> {
    set_optional_usize(
        &mut limits.virtual_cpu_count,
        config.cpu_count,
        "limits.resources.cpuCount",
    )?;
    set_optional_usize(
        &mut limits.max_processes,
        config.max_processes,
        "limits.resources.maxProcesses",
    )?;
    set_optional_usize(
        &mut limits.max_open_fds,
        config.max_open_fds,
        "limits.resources.maxOpenFds",
    )?;
    set_optional_usize(
        &mut limits.max_pipes,
        config.max_pipes,
        "limits.resources.maxPipes",
    )?;
    set_optional_usize(
        &mut limits.max_ptys,
        config.max_ptys,
        "limits.resources.maxPtys",
    )?;
    set_optional_usize(
        &mut limits.max_sockets,
        config.max_sockets,
        "limits.resources.maxSockets",
    )?;
    set_optional_usize(
        &mut limits.max_connections,
        config.max_connections,
        "limits.resources.maxConnections",
    )?;
    set_optional_usize(
        &mut limits.max_socket_buffered_bytes,
        config.max_socket_buffered_bytes,
        "limits.resources.maxSocketBufferedBytes",
    )?;
    set_optional_usize(
        &mut limits.max_socket_datagram_queue_len,
        config.max_socket_datagram_queue_len,
        "limits.resources.maxSocketDatagramQueueLen",
    )?;
    set_optional_u64(
        &mut limits.max_filesystem_bytes,
        config.max_filesystem_bytes,
    );
    set_optional_usize(
        &mut limits.max_inode_count,
        config.max_inode_count,
        "limits.resources.maxInodeCount",
    )?;
    set_optional_u64(
        &mut limits.max_blocking_read_ms,
        config.max_blocking_read_ms,
    );
    set_optional_usize(
        &mut limits.max_pread_bytes,
        config.max_pread_bytes,
        "limits.resources.maxPreadBytes",
    )?;
    set_optional_usize(
        &mut limits.max_fd_write_bytes,
        config.max_fd_write_bytes,
        "limits.resources.maxFdWriteBytes",
    )?;
    set_optional_usize(
        &mut limits.max_process_argv_bytes,
        config.max_process_argv_bytes,
        "limits.resources.maxProcessArgvBytes",
    )?;
    set_optional_usize(
        &mut limits.max_process_env_bytes,
        config.max_process_env_bytes,
        "limits.resources.maxProcessEnvBytes",
    )?;
    set_optional_usize(
        &mut limits.max_readdir_entries,
        config.max_readdir_entries,
        "limits.resources.maxReaddirEntries",
    )?;
    set_optional_usize(
        &mut limits.max_recursive_fs_depth,
        config.max_recursive_fs_depth,
        "limits.resources.maxRecursiveFsDepth",
    )?;
    set_optional_usize(
        &mut limits.max_recursive_fs_entries,
        config.max_recursive_fs_entries,
        "limits.resources.maxRecursiveFsEntries",
    )?;
    set_optional_u64(&mut limits.max_wasm_fuel, config.max_wasm_fuel);
    set_optional_u64(
        &mut limits.max_wasm_memory_bytes,
        config.max_wasm_memory_bytes,
    );
    set_optional_usize(
        &mut limits.max_wasm_stack_bytes,
        config.max_wasm_stack_bytes,
        "limits.resources.maxWasmStackBytes",
    )?;
    Ok(())
}

fn set_usize(target: &mut usize, value: Option<u64>, key: &str) -> Result<(), SidecarCoreError> {
    if let Some(value) = value {
        *target = usize::try_from(value).map_err(|_| integer_too_large(key, value))?;
    }
    Ok(())
}

fn set_u64(target: &mut u64, value: Option<u64>, _key: &str) -> Result<(), SidecarCoreError> {
    if let Some(value) = value {
        *target = value;
    }
    Ok(())
}

fn set_optional_usize(
    target: &mut Option<usize>,
    value: Option<u64>,
    key: &str,
) -> Result<(), SidecarCoreError> {
    if let Some(value) = value {
        *target = Some(usize::try_from(value).map_err(|_| integer_too_large(key, value))?);
    }
    Ok(())
}

fn set_optional_u64(target: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        *target = Some(value);
    }
}

fn integer_too_large(key: &str, value: u64) -> SidecarCoreError {
    SidecarCoreError::new(format!("{key} value {value} does not fit this platform"))
}

/// Cross-field validation. Fail-by-default: reject any configuration that would deadlock or
/// violate the wire frame budget with an explicit, actionable message.
pub fn validate_vm_limits(
    limits: &VmLimits,
    sidecar_max_frame_bytes: usize,
) -> Result<(), SidecarCoreError> {
    if limits.http.max_fetch_response_bytes == 0 {
        return Err(SidecarCoreError::new(
            "limits.http.max_fetch_response_bytes must be greater than zero".to_string(),
        ));
    }
    if limits.http.max_fetch_response_bytes > sidecar_max_frame_bytes {
        return Err(SidecarCoreError::new(format!(
            "limits.http.max_fetch_response_bytes ({}) must be <= the sidecar wire frame cap ({})",
            limits.http.max_fetch_response_bytes, sidecar_max_frame_bytes
        )));
    }

    if limits.tools.default_tool_timeout_ms > limits.tools.max_tool_timeout_ms {
        return Err(SidecarCoreError::new(format!(
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
        (
            "limits.acp.max_read_line_bytes",
            limits.acp.max_read_line_bytes,
        ),
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
            return Err(SidecarCoreError::new(format!(
                "{key} must be greater than zero"
            )));
        }
    }

    if limits.wasm.sync_read_limit_bytes == 0 {
        return Err(SidecarCoreError::new(
            "limits.wasm.sync_read_limit_bytes must be greater than zero".to_string(),
        ));
    }
    if limits.wasm.prewarm_timeout_ms == 0 {
        return Err(SidecarCoreError::new(
            "limits.wasm.prewarm_timeout_ms must be greater than zero".to_string(),
        ));
    }
    if limits.wasm.runner_heap_limit_mb == 0 {
        return Err(SidecarCoreError::new(
            "limits.wasm.runner_heap_limit_mb must be greater than zero".to_string(),
        ));
    }
    if limits.wasm.max_module_file_bytes == 0 {
        return Err(SidecarCoreError::new(
            "limits.wasm.max_module_file_bytes must be greater than zero".to_string(),
        ));
    }
    if limits.js_runtime.v8_ipc_max_frame_bytes == 0 {
        return Err(SidecarCoreError::new(
            "limits.js_runtime.v8_ipc_max_frame_bytes must be greater than zero".to_string(),
        ));
    }
    if limits.python.execution_timeout_ms == 0 {
        return Err(SidecarCoreError::new(
            "limits.python.execution_timeout_ms must be greater than zero".to_string(),
        ));
    }
    if limits.python.vfs_rpc_timeout_ms == 0 {
        return Err(SidecarCoreError::new(
            "limits.python.vfs_rpc_timeout_ms must be greater than zero".to_string(),
        ));
    }
    if let Some(0) = limits.js_runtime.v8_heap_limit_mb {
        return Err(SidecarCoreError::new(
            "limits.js_runtime.v8_heap_limit_mb must be greater than zero".to_string(),
        ));
    }
    if limits.js_runtime.import_cache_materialize_timeout_ms == 0 {
        return Err(SidecarCoreError::new(
            "limits.js_runtime.import_cache_materialize_timeout_ms must be greater than zero"
                .to_string(),
        ));
    }

    Ok(())
}
