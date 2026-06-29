//! Plugin-side `config_json` deserializer — ported from the deleted r6
//! `rivetkit-napi/src/agent_os.rs` `AgentOsConfigJson` (spec §6.6/§7: the
//! config schema is agentos-owned and lives plugin-side; r6 treats
//! `config_json` as an opaque passthrough string).
//!
//! `config_json` is a JSON-encoded subset of [`AgentOsConfig`]. Fields that
//! cannot be represented in JSON (`schedule_driver`, `MountConfig::driver`, the
//! `sidecar_js_bridge_callback`) are intentionally absent; passing them must
//! fail loud, enforced by `deny_unknown_fields`.

use agentos_client::{
    AgentOsConfig, AgentOsLimits, AgentOsSidecarConfig, MountConfig, MountPlugin, Permissions,
    RootFilesystemConfig, SoftwareInput,
};
use anyhow::{Context, Result};

/// Serializable mirror of [`AgentOsConfig`]. `deny_unknown_fields` enforces
/// fail-loud behavior when callers pass fields outside this allow-list
/// (including non-serializable fields like `schedule_driver`).
///
/// Keep this struct in sync with
/// `packages/agentos/src/config.ts::nativeAgentOsOptionsSchema` and
/// `packages/agentos/src/actor.ts::buildConfigJson`; TS preflight validation
/// should reject the same native-boundary fields before this serde guard runs.
#[derive(serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct AgentOsConfigJson {
    #[serde(default)]
    software: Vec<SoftwareInput>,
    /// `{ packageDir }` boot packages (the current TS `buildConfigJson` shape).
    #[serde(default)]
    packages: Vec<PackageDirJson>,
    /// Guest mount root for the package projection (`/opt/agentos`).
    #[serde(default)]
    packages_mount_at: Option<String>,
    /// Agent ACP adapter configs derived from package manifests. Accepted for
    /// schema parity with the TS `buildConfigJson`; adapter registration is not
    /// yet ported to the plugin, so a non-empty list logs a warning at
    /// bring-up.
    #[serde(default)]
    agent_configs: Vec<serde_json::Value>,
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
struct PackageDirJson {
    dir: String,
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

/// Reply DTO for the `listMounts` action: one configured mount, flattened so the
/// UI gets `path` / `kind` (the native plugin id, e.g. `host_dir`, `s3`,
/// `google_drive`, `sandbox_agent`) / `config` (provider-specific detail) /
/// `readOnly`. This echoes the actor's declarative mount config — the kernel
/// has no runtime mount table to enumerate.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MountInfoDto {
    pub path: String,
    pub kind: String,
    pub config: Option<serde_json::Value>,
    pub read_only: bool,
}

/// Reply DTO for the `listSoftware` action: one configured software package.
/// `kind` is the kebab-case [`SoftwareKind`] tag (`wasm-commands` / `agent` /
/// `tool`). Reflects the requested `software` bundle (the default `common`
/// bundle is already expanded into the envelope TS-side in `buildConfigJson`).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SoftwareInfoDto {
    pub package: String,
    pub kind: String,
    pub version: Option<String>,
    /// Command names this package ships (wasm-commands packages only; empty for
    /// agent/tool). Filled from the live VM in the `listSoftware` dispatch arm,
    /// not derivable from config alone. See `AgentOs::provided_commands`.
    pub commands: Vec<String>,
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
        if !self.agent_configs.is_empty() {
            tracing::warn!(
                count = self.agent_configs.len(),
                "agentConfigs are not yet applied by the actor plugin; package \
                 agents will not be registered for sessions"
            );
        }
        AgentOsConfig {
            software: self.software.clone(),
            packages: self
                .packages
                .iter()
                .map(|package| package.dir.clone())
                .collect(),
            packages_mount_at: self.packages_mount_at.clone(),
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

    /// Configured mounts, flattened for the `listMounts` action. `kind` is the
    /// native plugin id and `config` its provider-specific detail.
    pub(crate) fn list_mounts(&self) -> Vec<MountInfoDto> {
        self.mounts
            .iter()
            .map(|mount| MountInfoDto {
                path: mount.path.clone(),
                kind: mount.plugin.id.clone(),
                config: mount.plugin.config.clone(),
                read_only: mount.read_only,
            })
            .collect()
    }

    /// Configured software packages, for the `listSoftware` action. `kind` is the
    /// kebab-case [`SoftwareKind`] serde tag (`wasm-commands` / `agent` / `tool`).
    pub(crate) fn list_software(&self) -> Vec<SoftwareInfoDto> {
        self.software
            .iter()
            .map(|software| SoftwareInfoDto {
                package: software.package.clone(),
                kind: serde_json::to_value(software.kind)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "wasm-commands".to_owned()),
                version: software.version.clone(),
                // Filled from the live VM in the dispatch arm (config alone does
                // not carry the packages' command binaries).
                commands: Vec::new(),
            })
            .collect()
    }
}
