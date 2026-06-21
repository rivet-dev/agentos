//! Plugin-side `config_json` deserializer — ported from the deleted r6
//! `rivetkit-napi/src/agent_os.rs` `AgentOsConfigJson` (spec §6.6/§7: the
//! config schema is agent-os-owned and lives plugin-side; r6 treats
//! `config_json` as an opaque passthrough string).
//!
//! `config_json` is a JSON-encoded subset of [`AgentOsConfig`]. Fields that
//! cannot be represented in JSON (`schedule_driver`, `MountConfig::driver`, the
//! `sidecar_js_bridge_callback`) are intentionally absent; passing them must
//! fail loud, enforced by `deny_unknown_fields`.

use agent_os_client::{
    AgentOsConfig, AgentOsLimits, AgentOsSidecarConfig, MountConfig, MountPlugin, Permissions,
    RootFilesystemConfig, SoftwareInput,
};
use anyhow::{Context, Result};

/// Serializable mirror of [`AgentOsConfig`]. `deny_unknown_fields` enforces
/// fail-loud behavior when callers pass fields outside this allow-list
/// (including non-serializable fields like `schedule_driver`).
#[derive(serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct AgentOsConfigJson {
    #[serde(default)]
    software: Vec<SoftwareInput>,
    #[serde(default)]
    additional_instructions: Option<String>,
    #[serde(default)]
    module_access_cwd: Option<String>,
    #[serde(default)]
    loopback_exempt_ports: Vec<u16>,
    #[serde(default)]
    allowed_node_builtins: Option<Vec<String>>,
    #[serde(default)]
    permissions: Option<Permissions>,
    #[serde(default)]
    mounts: Vec<NativeMountJson>,
    #[serde(default)]
    root_filesystem: Option<RootFilesystemConfig>,
    #[serde(default)]
    limits: Option<AgentOsLimits>,
    #[serde(default)]
    sidecar: Option<SidecarJson>,
}

#[derive(serde::Deserialize, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NativeMountJson {
    path: String,
    plugin: MountPlugin,
    #[serde(default)]
    read_only: bool,
}

#[derive(serde::Deserialize, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SidecarJson {
    #[serde(default)]
    pool: Option<String>,
}

impl AgentOsConfigJson {
    /// Parse a `config_json` envelope. An empty/whitespace string is treated as
    /// the default config (the client supplied no overrides).
    pub(crate) fn parse(config_json: &str) -> Result<Self> {
        if config_json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(config_json).context("agent-os config JSON parse error")
    }

    /// Build a fresh [`AgentOsConfig`] (non-`Clone`, so rebuilt per bring-up).
    ///
    /// `fallback_pool` is the per-plugin-runtime sidecar pool used when the
    /// client did not configure one explicitly. Per spec §7 the plugin never
    /// uses the global `"default"` pool: a unique-per-runtime pool gives one
    /// sidecar process per plugin runtime, shared across the actors it hosts and
    /// isolated from other dlopen loads.
    pub(crate) fn to_agent_os_config(&self, fallback_pool: &str) -> AgentOsConfig {
        let sidecar = match &self.sidecar {
            // Client-configured pool is trusted; honor it verbatim.
            Some(sidecar) => AgentOsSidecarConfig::Shared {
                pool: sidecar.pool.clone(),
            },
            // No client config → isolate this plugin runtime on its own pool.
            None => AgentOsSidecarConfig::Shared {
                pool: Some(fallback_pool.to_owned()),
            },
        };
        AgentOsConfig {
            software: self.software.clone(),
            loopback_exempt_ports: self.loopback_exempt_ports.clone(),
            allowed_node_builtins: self.allowed_node_builtins.clone(),
            module_access_cwd: self.module_access_cwd.clone(),
            additional_instructions: self.additional_instructions.clone(),
            permissions: self.permissions.clone(),
            mounts: self
                .mounts
                .iter()
                .map(|mount| MountConfig::Native {
                    path: mount.path.clone(),
                    plugin: mount.plugin.clone(),
                    read_only: mount.read_only,
                })
                .collect(),
            root_filesystem: self.root_filesystem.clone().unwrap_or_default(),
            limits: self.limits.clone(),
            sidecar: Some(sidecar),
            ..AgentOsConfig::default()
        }
    }
}
