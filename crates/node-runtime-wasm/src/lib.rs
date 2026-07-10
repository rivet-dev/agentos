//! Build orchestration and checked manifests for `node-runtime.wasm`.
//!
//! The module is a WASI reactor instantiated by the existing native V8 isolate.
//! Only [`POSIX_IMPORT_MODULE`] carries host authority. Node-API and engine
//! imports are closure-private adapters to values in that same isolate; they
//! must never acquire filesystem, process, network, clock, or entropy powers.

/// Import module used by the isolate-local Node-API wire ABI.
pub const NAPI_IMPORT_MODULE: &str = "agentos_napi_v1";

/// Import module used by isolate-local V8 extensions not expressible in Node-API.
pub const ENGINE_IMPORT_MODULE: &str = "agentos_node_engine_v1";

/// The sole function-import module allowed to reach the AgentOS VM kernel.
pub const POSIX_IMPORT_MODULE: &str = "agentos_posix_v1";

/// Only the small R0 reentrancy probe uses this individual import.
pub const NESTED_PROBE_IMPORT: &str = "call_js";

/// Required public lifecycle surface of the persistent Node reactor.
pub const REQUIRED_REACTOR_EXPORTS: &[&str] = &[
    "_initialize",
    "__indirect_function_table",
    "agentos_node_runtime_alloc",
    "agentos_node_runtime_free",
    "agentos_node_runtime_allocated_bytes",
    "agentos_node_runtime_allocation_count",
    "agentos_node_runtime_create",
    "agentos_node_runtime_bootstrap",
    "agentos_node_runtime_run",
    "agentos_node_runtime_interrupt",
    "agentos_node_runtime_quiescence",
    "agentos_node_runtime_teardown",
    "agentos_node_runtime_last_error",
];

/// Typed status results returned by reactor lifecycle operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum ReactorStatus {
    Ok = 0,
    InvalidArgument = -1,
    InvalidState = -2,
    EngineFailure = -3,
    ScriptFailure = -4,
    Interrupted = -5,
    ResourceLimit = -6,
}
