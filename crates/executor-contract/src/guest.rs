use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Owned compatibility host-call envelope used by legacy synchronous guest
/// adapters. Native adapters should prefer typed [`crate::host::HostOperation`]
/// values, but both forms remain engine independent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRpcRequest {
    pub id: u64,
    pub method: String,
    pub args: Vec<Value>,
    pub raw_bytes_args: HashMap<usize, Vec<u8>>,
}

/// Per-execution guest identity and operating-system projection supplied by
/// the sidecar. Concrete executors translate these owned values into their
/// guest-specific bootstrap representation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuestRuntimeConfig {
    #[serde(default)]
    pub bridge_limits: GuestBridgeLimits,
    pub virtual_pid: Option<u64>,
    pub virtual_ppid: Option<u64>,
    pub virtual_uid: Option<u64>,
    pub virtual_gid: Option<u64>,
    pub virtual_exec_path: Option<String>,
    pub os_cpu_count: Option<u64>,
    pub os_totalmem: Option<u64>,
    pub os_freemem: Option<u64>,
    pub os_homedir: Option<String>,
    pub os_hostname: Option<String>,
    pub os_tmpdir: Option<String>,
    pub os_type: Option<String>,
    pub os_release: Option<String>,
    pub os_version: Option<String>,
    pub os_machine: Option<String>,
    pub os_shell: Option<String>,
    pub os_user: Option<String>,
    pub high_resolution_time: bool,
    /// Forward stdout/stderr writes to the owning kernel descriptor table.
    /// Standalone executors keep their local output event contract.
    #[serde(default)]
    pub kernel_stdio: bool,
    /// Optional code evaluated by V8-family adapters when creating a reusable
    /// snapshot. Non-V8 executors preserve but otherwise ignore this field.
    pub snapshot_userland_code: Option<String>,
    /// Trusted sidecar script evaluated before the entrypoint is linked.
    pub prelude_script: Option<String>,
}

/// Effective trusted per-execution bridge retention policy. Shared defaults also
/// apply to standalone executors; VM overrides are supplied by the composition root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct GuestBridgeLimits {
    pub max_chunk_size_line_bytes: usize,
    pub max_pending_response_bytes: usize,
    pub max_readline_history_entries: usize,
}

impl Default for GuestBridgeLimits {
    fn default() -> Self {
        Self {
            max_chunk_size_line_bytes: 16 * 1024 + 32,
            max_pending_response_bytes: 256 * 1024,
            max_readline_history_entries: 1024,
        }
    }
}
