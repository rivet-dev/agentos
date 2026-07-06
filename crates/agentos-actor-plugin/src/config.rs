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
    AgentOsConfig, AgentOsLimits, AgentOsSidecarConfig, MountConfig, MountPlugin, PackageRef,
    Permissions, RootFilesystemConfig, SoftwareInput,
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
    /// Package dirs to project into `/opt/agentos` (secure-exec package
    /// projection). Each `dir` holds an `agentos-package.json` manifest + payload.
    #[serde(default)]
    packages: Vec<PackageJson>,
    /// Guest mount point for the projection (JS sends `OPT_AGENTOS_ROOT`).
    #[serde(default)]
    packages_mount_at: Option<String>,
    /// Agent adapter configs emitted by the JS layer. The client resolves agent
    /// adapters from its own table, so these are accepted (so `deny_unknown_fields`
    /// does not reject the JS envelope) but intentionally not consumed here.
    #[serde(default)]
    #[allow(dead_code)]
    agent_configs: Vec<serde_json::Value>,
    #[serde(default)]
    additional_instructions: Option<String>,
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

/// One `{ path }` entry from the JS `packages` list: the packed `.aospkg`
/// file (normal case) or a transition package dir.
#[derive(serde::Deserialize, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PackageJson {
    #[serde(alias = "packagePath")]
    path: String,
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
                .map(|package| PackageRef {
                    path: package.path.clone(),
                })
                .collect(),
            packages_mount_at: self.packages_mount_at.clone(),
            loopback_exempt_ports: self.loopback_exempt_ports.clone(),
            allowed_node_builtins: self.allowed_node_builtins.clone(),
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

    /// Configured software packages, for the `listSoftware` action. For a packed
    /// `.aospkg` the name/agent/version come from the vbare chunk1 manifest; a
    /// transition dir still reads `<dir>/agentos-package.json` (toolchain input)
    /// plus `<dir>/package.json` for the version. `kind` is `agent` when the
    /// manifest declares an agent block, else `wasm-commands`. A package whose
    /// manifest is unreadable is skipped rather than aborting the whole listing.
    pub(crate) fn list_software(&self) -> Vec<SoftwareInfoDto> {
        self.packages
            .iter()
            .filter_map(|package| read_package_software_info(&package.path))
            .collect()
    }
}

/// Read a projected package into a [`SoftwareInfoDto`] (sans `commands`, which
/// are filled from the live VM in the dispatch arm). Returns `None` if the
/// package manifest is missing or malformed.
fn read_package_software_info(path: &str) -> Option<SoftwareInfoDto> {
    if std::path::Path::new(path).is_file() {
        return read_aospkg_software_info(path);
    }
    read_package_dir_software_info(path)
}

/// Packed `.aospkg`: decode the chunk1 vbare manifest (the runtime manifest —
/// packed packages ship no `agentos-package.json`).
fn read_aospkg_software_info(path: &str) -> Option<SoftwareInfoDto> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let file_len = usize::try_from(file.metadata().ok()?.len()).ok()?;
    let mut header = [0u8; vfs::package_format::AOSPKG_HEADER_LEN];
    file.read_exact(&mut header).ok()?;
    let parsed = vfs::package_format::parse_aospkg_header_from_prefix(&header, file_len).ok()?;
    let mut manifest = vec![0u8; parsed.manifest.len()];
    file.seek(SeekFrom::Start(parsed.manifest.start as u64)).ok()?;
    file.read_exact(&mut manifest).ok()?;
    let manifest = vfs::package_format::versioned::decode_package_manifest(&manifest).ok()?;
    Some(SoftwareInfoDto {
        package: manifest.name,
        kind: if manifest.agent.is_some() {
            "agent"
        } else {
            "wasm-commands"
        }
        .to_owned(),
        version: Some(manifest.version),
        commands: Vec::new(),
    })
}

/// Transition package dir: `agentos-package.json` is the toolchain-input
/// manifest and `package.json` carries the best-effort version.
fn read_package_dir_software_info(dir: &str) -> Option<SoftwareInfoDto> {
    let manifest_path = std::path::Path::new(dir).join("agentos-package.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(manifest_path).ok()?).ok()?;
    let name = manifest.get("name").and_then(|v| v.as_str())?.to_owned();
    let kind = if manifest.get("agent").is_some_and(|v| v.is_object()) {
        "agent"
    } else {
        "wasm-commands"
    }
    .to_owned();
    // Version is best-effort from the package's `package.json`.
    let version = std::fs::read_to_string(std::path::Path::new(dir).join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|pkg| {
            pkg.get("version")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        });
    Some(SoftwareInfoDto {
        package: name,
        kind,
        version,
        commands: Vec::new(),
    })
}
