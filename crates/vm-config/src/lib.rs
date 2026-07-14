use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Canonical Rust-side VM config. Unknown fields must stay rejected here and by
/// TypeScript thin-client option validation. Update both when an explicit wire
/// option changes the generated VM config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
#[derive(Default)]
pub struct CreateVmConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "Record<string, string>")]
    #[ts(optional)]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(
        default,
        rename = "rootFilesystem",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub root_filesystem: Option<RootFilesystemConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub permissions: Option<PermissionsPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limits: Option<VmLimitsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dns: Option<VmDnsConfig>,
    #[serde(
        default,
        rename = "nativeRoot",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub native_root: Option<NativeRootFilesystemConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub listen: Option<VmListenPolicyConfig>,
    #[serde(
        default,
        rename = "loopbackExemptPorts",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub loopback_exempt_ports: Option<Vec<u16>>,
    #[serde(default, rename = "jsRuntime", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub js_runtime: Option<JsRuntimeConfig>,
    /// VM-scoped instructions applied to every ACP session. Session-specific
    /// instructions are combined with these by the ACP sidecar adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub agent_additional_instructions: Option<String>,
}

impl CreateVmConfig {
    pub fn validate(&self, max_frame_bytes: usize) -> Result<(), VmConfigError> {
        if let Some(cwd) = self.cwd.as_deref() {
            validate_guest_path("cwd", cwd)?;
        }
        if let Some(root_filesystem) = &self.root_filesystem {
            root_filesystem.validate()?;
        }
        if let Some(native_root) = &self.native_root {
            native_root.validate()?;
        }
        if self.native_root.is_some()
            && self
                .root_filesystem
                .as_ref()
                .is_some_and(|root| !root.effective_bootstrap_entries().is_empty())
        {
            return Err(VmConfigError::new(
                "nativeRoot does not support rootFilesystem.bootstrapEntries",
            ));
        }
        if let Some(dns) = &self.dns {
            dns.validate()?;
        }
        if let Some(listen) = &self.listen {
            listen.validate()?;
        }
        if let Some(limits) = &self.limits {
            limits.validate(max_frame_bytes)?;
        }
        if let Some(js_runtime) = &self.js_runtime {
            js_runtime.validate()?;
        }
        Ok(())
    }
}

/// Guest JavaScript host-environment configuration.
///
/// Selects which globals/builtins/module-resolution surface guest JS sees,
/// modeled on esbuild's `platform`. Omitting this preserves full Node.js
/// emulation (`platform = node`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct JsRuntimeConfig {
    /// Which host environment to emulate for guest JS. Default `node`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub platform: Option<JsRuntimePlatform>,
    /// How bare import specifiers resolve. Independent of `platform`.
    /// Default `node`.
    #[serde(
        default,
        rename = "moduleResolution",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub module_resolution: Option<JsModuleResolution>,
    /// Node builtin-module allow-list. Only valid when `platform = node`.
    /// `None` => engine default allow-list. `Some([])` => deny all builtins.
    /// `Some([..])` => exactly those.
    #[serde(
        default,
        rename = "allowedBuiltins",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub allowed_builtins: Option<Vec<String>>,
    /// Opt in to a high-resolution monotonic guest clock. Default false keeps
    /// the security-oriented 1ms timer resolution.
    #[serde(
        default,
        rename = "highResolutionTime",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub high_resolution_time: Option<bool>,
}

impl JsRuntimeConfig {
    fn validate(&self) -> Result<(), VmConfigError> {
        if let Some(allowed) = &self.allowed_builtins {
            if self.platform.unwrap_or_default() != JsRuntimePlatform::Node {
                return Err(VmConfigError::new(
                    "jsRuntime.allowedBuiltins is only valid when jsRuntime.platform is \"node\"",
                ));
            }
            for name in allowed {
                if !is_known_node_builtin(name) {
                    return Err(VmConfigError::new(format!(
                        "jsRuntime.allowedBuiltins contains unknown builtin {name:?}"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
#[derive(Default)]
pub enum JsRuntimePlatform {
    /// Full Node.js host surface (process/Buffer/require, `node:*`, npm
    /// resolution, virtual Node identity). Default.
    #[default]
    Node,
    /// Web-platform globals (fetch/URL/WebCrypto/...), no Node surface.
    Browser,
    /// Universal primitives only (console, timers, queueMicrotask) — no web
    /// platform, no Node surface.
    Neutral,
    /// Language-only: ECMAScript spec globals + WebAssembly. Nothing host-provided.
    Bare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
#[derive(Default)]
pub enum JsModuleResolution {
    /// node_modules ancestor-walk + exports/imports/conditions + realpath. Default.
    #[default]
    Node,
    /// Relative/absolute ESM from the VFS only; bare specifiers do not resolve.
    Relative,
    /// No resolution: any import/require (even relative) fails.
    None,
}

/// Canonical set of recognized Node builtin module names (without the `node:`
/// prefix), kept in sync with `normalize_builtin_specifier` in
/// `crates/execution/src/javascript.rs`. Used to validate
/// `jsRuntime.allowedBuiltins` entries.
const KNOWN_NODE_BUILTINS: &[&str] = &[
    "assert",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "dns/promises",
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "path",
    "path/posix",
    "path/win32",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "sqlite",
    "stream",
    "stream/consumers",
    "stream/promises",
    "stream/web",
    "string_decoder",
    "sys",
    "timers",
    "timers/promises",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "util/types",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

fn is_known_node_builtin(name: &str) -> bool {
    let bare = name.strip_prefix("node:").unwrap_or(name);
    KNOWN_NODE_BUILTINS.contains(&bare)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct RootFilesystemConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mode: Option<RootFilesystemMode>,
    #[serde(
        default,
        rename = "disableDefaultBaseLayer",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub disable_default_base_layer: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lowers: Option<Vec<RootFilesystemLowerDescriptor>>,
    #[serde(
        default,
        rename = "bootstrapEntries",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub bootstrap_entries: Option<Vec<RootFilesystemEntry>>,
}

impl RootFilesystemConfig {
    pub fn effective_mode(&self) -> RootFilesystemMode {
        self.mode.unwrap_or_default()
    }

    pub fn effective_disable_default_base_layer(&self) -> bool {
        self.disable_default_base_layer.unwrap_or(false)
    }

    pub fn effective_lowers(&self) -> &[RootFilesystemLowerDescriptor] {
        self.lowers.as_deref().unwrap_or_default()
    }

    pub fn effective_bootstrap_entries(&self) -> &[RootFilesystemEntry] {
        self.bootstrap_entries.as_deref().unwrap_or_default()
    }

    fn validate(&self) -> Result<(), VmConfigError> {
        for lower in self.effective_lowers() {
            if let RootFilesystemLowerDescriptor::Snapshot { entries } = lower {
                for entry in entries {
                    entry.validate()?;
                }
            }
        }
        for entry in self.effective_bootstrap_entries() {
            entry.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
#[derive(Default)]
pub enum RootFilesystemMode {
    #[default]
    Ephemeral,
    ReadOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub enum RootFilesystemLowerDescriptor {
    Snapshot {
        #[serde(default)]
        entries: Vec<RootFilesystemEntry>,
    },
    BundledBaseFilesystem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct RootFilesystemEntry {
    pub path: String,
    pub kind: RootFilesystemEntryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub uid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub gid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub encoding: Option<RootFilesystemEntryEncoding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub target: Option<String>,
    #[serde(default)]
    pub executable: bool,
}

impl RootFilesystemEntry {
    fn validate(&self) -> Result<(), VmConfigError> {
        validate_guest_path("root filesystem entry path", &self.path)?;
        match self.kind {
            RootFilesystemEntryKind::File => {
                if self.target.is_some() {
                    return Err(VmConfigError::new(format!(
                        "file entry {} must not include target",
                        self.path
                    )));
                }
            }
            RootFilesystemEntryKind::Directory => {
                if self.content.is_some() || self.encoding.is_some() || self.target.is_some() {
                    return Err(VmConfigError::new(format!(
                        "directory entry {} must not include content, encoding, or target",
                        self.path
                    )));
                }
            }
            RootFilesystemEntryKind::Symlink => {
                if self.target.as_deref().unwrap_or("").is_empty() {
                    return Err(VmConfigError::new(format!(
                        "symlink entry {} requires target",
                        self.path
                    )));
                }
                if self.content.is_some() || self.encoding.is_some() {
                    return Err(VmConfigError::new(format!(
                        "symlink entry {} must not include content or encoding",
                        self.path
                    )));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub enum RootFilesystemEntryKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub enum RootFilesystemEntryEncoding {
    Utf8,
    Base64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct NativeRootFilesystemConfig {
    pub plugin: MountPluginDescriptor,
    #[serde(default, rename = "readOnly", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub read_only: Option<bool>,
}

impl NativeRootFilesystemConfig {
    pub fn effective_read_only(&self) -> bool {
        self.read_only.unwrap_or(false)
    }

    fn validate(&self) -> Result<(), VmConfigError> {
        if self.plugin.id.trim().is_empty() {
            return Err(VmConfigError::new("nativeRoot.plugin.id is required"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct MountPluginDescriptor {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    #[ts(type = "import(\"@rivet-dev/agentos-runtime-core/descriptors\").MountConfigJsonValue")]
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub enum PermissionMode {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub enum FsPermissionScope {
    Mode(PermissionMode),
    Rules(FsPermissionRuleSet),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub enum PatternPermissionScope {
    Mode(PermissionMode),
    Rules(PatternPermissionRuleSet),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct FsPermissionRuleSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default: Option<PermissionMode>,
    #[serde(default)]
    pub rules: Vec<FsPermissionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct PatternPermissionRuleSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default: Option<PermissionMode>,
    #[serde(default)]
    pub rules: Vec<PatternPermissionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct FsPermissionRule {
    pub mode: PermissionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub operations: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct PatternPermissionRule {
    pub mode: PermissionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub operations: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub patterns: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct PermissionsPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fs: Option<FsPermissionScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub network: Option<PatternPermissionScope>,
    #[serde(
        default,
        rename = "childProcess",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub child_process: Option<PatternPermissionScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub process: Option<PatternPermissionScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub env: Option<PatternPermissionScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub binding: Option<PatternPermissionScope>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct VmLimitsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resources: Option<ResourceLimitsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub http: Option<HttpLimitsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tools: Option<ToolLimitsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugins: Option<PluginLimitsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub acp: Option<AcpLimitsConfig>,
    #[serde(default, rename = "jsRuntime", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub js_runtime: Option<JsRuntimeLimitsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub python: Option<PythonLimitsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub wasm: Option<WasmLimitsConfig>,
}

impl VmLimitsConfig {
    fn validate(&self, max_frame_bytes: usize) -> Result<(), VmConfigError> {
        if let Some(http) = &self.http {
            if let Some(max_fetch_response_bytes) = http.max_fetch_response_bytes {
                if max_fetch_response_bytes == 0 {
                    return Err(VmConfigError::new(
                        "limits.http.maxFetchResponseBytes must be greater than zero",
                    ));
                }
                if max_fetch_response_bytes as usize > max_frame_bytes {
                    return Err(VmConfigError::new(format!(
                        "limits.http.maxFetchResponseBytes ({max_fetch_response_bytes}) must be <= the sidecar wire frame cap ({max_frame_bytes})"
                    )));
                }
            }
        }
        if let Some(tools) = &self.tools {
            if let (Some(default), Some(max)) =
                (tools.default_tool_timeout_ms, tools.max_tool_timeout_ms)
            {
                if default > max {
                    return Err(VmConfigError::new(
                        "limits.tools.defaultToolTimeoutMs must be <= limits.tools.maxToolTimeoutMs",
                    ));
                }
            }
        }
        Ok(())
    }
}

macro_rules! limits_struct {
    ($name:ident { $($field:ident),* $(,)? }) => {
        #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        #[ts(export, export_to = "../../../packages/core/src/generated/")]
        pub struct $name {
            $(
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                #[ts(type = "number")]
                pub $field: Option<u64>,
            )*
        }
    };
}

limits_struct!(ResourceLimitsConfig {
    cpu_count,
    max_processes,
    max_captured_output_bytes,
    max_open_fds,
    max_pipes,
    max_ptys,
    max_sockets,
    max_connections,
    max_socket_buffered_bytes,
    max_socket_datagram_queue_len,
    max_filesystem_bytes,
    max_inode_count,
    max_blocking_read_ms,
    max_pread_bytes,
    max_fd_write_bytes,
    max_process_argv_bytes,
    max_process_env_bytes,
    max_readdir_entries,
    max_recursive_fs_depth,
    max_recursive_fs_entries,
    max_wasm_fuel,
    max_wasm_memory_bytes,
    max_wasm_stack_bytes,
});

limits_struct!(HttpLimitsConfig {
    max_fetch_response_bytes,
});

limits_struct!(ToolLimitsConfig {
    default_tool_timeout_ms,
    max_tool_timeout_ms,
    max_registered_toolkits,
    max_registered_tools_per_vm,
    max_tools_per_toolkit,
    max_tool_schema_bytes,
    max_tool_examples_per_tool,
    max_tool_example_input_bytes,
});

limits_struct!(PluginLimitsConfig {
    max_persisted_manifest_bytes,
    max_persisted_manifest_file_bytes,
});

limits_struct!(AcpLimitsConfig {
    max_read_line_bytes,
    stdout_buffer_byte_limit,
});

limits_struct!(JsRuntimeLimitsConfig {
    v8_heap_limit_mb,
    sync_rpc_wait_timeout_ms,
    cpu_time_limit_ms,
    wall_clock_limit_ms,
    import_cache_materialize_timeout_ms,
    captured_output_limit_bytes,
});

limits_struct!(PythonLimitsConfig {
    output_buffer_max_bytes,
    execution_timeout_ms,
    max_old_space_mb,
    vfs_rpc_timeout_ms,
});

limits_struct!(WasmLimitsConfig {
    max_module_file_bytes,
    captured_output_limit_bytes,
    sync_read_limit_bytes,
    prewarm_timeout_ms,
    runner_heap_limit_mb,
});

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct VmDnsConfig {
    #[serde(default, rename = "nameServers", skip_serializing_if = "Vec::is_empty")]
    pub name_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, Vec<String>>,
}

impl VmDnsConfig {
    fn validate(&self) -> Result<(), VmConfigError> {
        for entry in &self.name_servers {
            if entry.trim().is_empty() {
                return Err(VmConfigError::new(
                    "dns.nameServers entries must not be empty",
                ));
            }
        }
        for (host, addresses) in &self.overrides {
            if host.trim().is_empty() {
                return Err(VmConfigError::new("dns.overrides keys must not be empty"));
            }
            if addresses.is_empty() {
                return Err(VmConfigError::new(format!(
                    "dns.overrides.{host} must contain at least one address"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/core/src/generated/")]
pub struct VmListenPolicyConfig {
    #[serde(default, rename = "portMin", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub port_min: Option<u16>,
    #[serde(default, rename = "portMax", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub port_max: Option<u16>,
    #[serde(
        default,
        rename = "allowPrivileged",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub allow_privileged: Option<bool>,
}

impl VmListenPolicyConfig {
    fn validate(&self) -> Result<(), VmConfigError> {
        if self.port_min == Some(0) {
            return Err(VmConfigError::new(
                "listen.portMin must be between 1 and 65535",
            ));
        }
        if self.port_max == Some(0) {
            return Err(VmConfigError::new(
                "listen.portMax must be between 1 and 65535",
            ));
        }
        if let (Some(min), Some(max)) = (self.port_min, self.port_max) {
            if min > max {
                return Err(VmConfigError::new(
                    "listen.portMin must be <= listen.portMax",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmConfigError {
    message: String,
}

impl VmConfigError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for VmConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for VmConfigError {}

fn validate_guest_path(label: &str, path: &str) -> Result<(), VmConfigError> {
    if !path.starts_with('/') {
        return Err(VmConfigError::new(format!("{label} must be absolute")));
    }
    if path.split('/').any(|part| part == "..") {
        return Err(VmConfigError::new(format!("{label} must not contain '..'")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips() {
        let config = CreateVmConfig::default();
        let json = serde_json::to_string(&config).expect("serialize config");
        let decoded: CreateVmConfig = serde_json::from_str(&json).expect("decode config");
        assert_eq!(decoded, config);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let error =
            serde_json::from_str::<CreateVmConfig>(r#"{"rootFilesystem":{},"surprise":true}"#)
                .expect_err("unknown fields should fail");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn validate_rejects_fetch_limit_above_frame_cap() {
        let config = CreateVmConfig {
            limits: Some(VmLimitsConfig {
                http: Some(HttpLimitsConfig {
                    max_fetch_response_bytes: Some(2048),
                }),
                ..VmLimitsConfig::default()
            }),
            ..CreateVmConfig::default()
        };
        assert!(config.validate(1024).is_err());
    }

    fn js_runtime_config(value: serde_json::Value) -> Result<CreateVmConfig, serde_json::Error> {
        serde_json::from_value(serde_json::json!({ "jsRuntime": value }))
    }

    #[test]
    fn js_runtime_defaults_to_node() {
        let config: CreateVmConfig =
            serde_json::from_value(serde_json::json!({ "jsRuntime": {} })).expect("decode");
        let js = config.js_runtime.expect("jsRuntime present");
        assert!(js.platform.is_none());
        assert!(js.module_resolution.is_none());
        assert!(js.allowed_builtins.is_none());
        assert!(js.high_resolution_time.is_none());
    }

    #[test]
    fn js_runtime_high_resolution_time_defaults_off_and_round_trips() {
        let defaulted = js_runtime_config(serde_json::json!({})).unwrap();
        assert!(defaulted.js_runtime.unwrap().high_resolution_time.is_none());

        let enabled = js_runtime_config(serde_json::json!({
            "highResolutionTime": true,
        }))
        .unwrap();
        assert_eq!(
            enabled.js_runtime.as_ref().unwrap().high_resolution_time,
            Some(true)
        );
        let json = serde_json::to_string(&enabled).expect("serialize");
        assert!(json.contains("highResolutionTime"));
        let decoded: CreateVmConfig = serde_json::from_str(&json).expect("re-decode");
        assert_eq!(decoded, enabled);
    }

    #[test]
    fn js_runtime_all_platform_resolution_combos_round_trip() {
        for platform in ["node", "browser", "neutral", "bare"] {
            for resolution in ["node", "relative", "none"] {
                let config = js_runtime_config(serde_json::json!({
                    "platform": platform,
                    "moduleResolution": resolution,
                }))
                .unwrap_or_else(|err| panic!("decode {platform}/{resolution}: {err}"));
                let json = serde_json::to_string(&config).expect("serialize");
                let decoded: CreateVmConfig = serde_json::from_str(&json).expect("re-decode");
                assert_eq!(decoded, config);
                assert!(config.validate(usize::MAX).is_ok());
            }
        }
    }

    #[test]
    fn js_runtime_allowed_builtins_tri_state() {
        // None => omitted.
        let none = js_runtime_config(serde_json::json!({ "platform": "node" })).unwrap();
        assert!(none.js_runtime.unwrap().allowed_builtins.is_none());
        // Some([]) => deny all (representable, distinct from None).
        let empty = js_runtime_config(serde_json::json!({ "allowedBuiltins": [] })).unwrap();
        assert_eq!(empty.js_runtime.unwrap().allowed_builtins, Some(Vec::new()));
        // Some([..]) => explicit.
        let some = js_runtime_config(serde_json::json!({ "allowedBuiltins": ["path", "node:fs"] }))
            .unwrap();
        assert_eq!(
            some.js_runtime.unwrap().allowed_builtins,
            Some(vec!["path".to_owned(), "node:fs".to_owned()])
        );
    }

    #[test]
    fn js_runtime_preserves_omission_and_explicit_default_overrides() {
        let omitted = js_runtime_config(serde_json::json!({
            "allowedBuiltins": ["path"]
        }))
        .unwrap();
        let omitted_json = serde_json::to_value(&omitted).expect("serialize omitted defaults");
        assert_eq!(
            omitted_json.get("jsRuntime"),
            Some(&serde_json::json!({ "allowedBuiltins": ["path"] }))
        );

        let explicit = js_runtime_config(serde_json::json!({
            "platform": "node",
            "moduleResolution": "node"
        }))
        .unwrap();
        let explicit_json = serde_json::to_value(&explicit).expect("serialize explicit defaults");
        assert_eq!(
            explicit_json.get("jsRuntime"),
            Some(&serde_json::json!({
                "platform": "node",
                "moduleResolution": "node"
            }))
        );
    }

    #[test]
    fn root_filesystem_preserves_omission_and_explicit_default_overrides() {
        let omitted: CreateVmConfig = serde_json::from_value(serde_json::json!({
            "rootFilesystem": {}
        }))
        .expect("decode omitted root defaults");
        let root = omitted
            .root_filesystem
            .as_ref()
            .expect("root filesystem present");
        assert_eq!(root.effective_mode(), RootFilesystemMode::Ephemeral);
        assert!(!root.effective_disable_default_base_layer());
        assert!(root.effective_lowers().is_empty());
        assert!(root.effective_bootstrap_entries().is_empty());
        assert_eq!(
            serde_json::to_value(&omitted).expect("serialize omitted root defaults"),
            serde_json::json!({ "rootFilesystem": {} })
        );

        let explicit: CreateVmConfig = serde_json::from_value(serde_json::json!({
            "rootFilesystem": {
                "mode": "ephemeral",
                "disableDefaultBaseLayer": false,
                "lowers": [],
                "bootstrapEntries": []
            }
        }))
        .expect("decode explicit root defaults");
        assert_eq!(
            serde_json::to_value(&explicit).expect("serialize explicit root defaults"),
            serde_json::json!({
                "rootFilesystem": {
                    "mode": "ephemeral",
                    "disableDefaultBaseLayer": false,
                    "lowers": [],
                    "bootstrapEntries": []
                }
            })
        );
    }

    #[test]
    fn js_runtime_rejects_allowed_builtins_under_non_node_platform() {
        for platform in ["browser", "neutral", "bare"] {
            let config = js_runtime_config(serde_json::json!({
                "platform": platform,
                "allowedBuiltins": ["path"],
            }))
            .unwrap();
            let error = config
                .validate(usize::MAX)
                .expect_err("allowedBuiltins under non-node must reject");
            assert!(error.to_string().contains("allowedBuiltins"));
        }
    }

    #[test]
    fn js_runtime_rejects_unknown_builtin_names() {
        let config = js_runtime_config(serde_json::json!({
            "platform": "node",
            "allowedBuiltins": ["path", "totally_not_a_builtin"],
        }))
        .unwrap();
        let error = config
            .validate(usize::MAX)
            .expect_err("unknown builtin must reject");
        assert!(error.to_string().contains("unknown builtin"));
    }

    #[test]
    fn js_runtime_accepts_empty_allow_list_under_node() {
        let config =
            js_runtime_config(serde_json::json!({ "platform": "node", "allowedBuiltins": [] }))
                .unwrap();
        assert!(config.validate(usize::MAX).is_ok());
    }

    #[test]
    fn js_runtime_rejects_unknown_fields() {
        let error = js_runtime_config(serde_json::json!({ "surprise": true }))
            .expect_err("unknown jsRuntime field should fail");
        assert!(error.to_string().contains("unknown field"));
    }
}
