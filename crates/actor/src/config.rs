use crate::budgets::{check_budget, HostedActorBudgets};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use agentos_client::{
    validate_package_source_with_options, AgentOsConfig, AgentOsLimits, FsPermissionRule,
    FsPermissions, MountConfig, MountPlugin, PackageResolverOptions, PackageSource,
    PatternPermissionRule, PatternPermissions, Permissions, RootFilesystemConfig,
    RootFilesystemKind, RootFilesystemMode, RulePermissions, VmSqliteDescriptor, VmUserConfig,
};
use anyhow::{bail, Context, Result};
use futures::{stream, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};

#[cfg(test)]
const MAX_ENVIRONMENT_ENTRIES: usize = 256;
#[cfg(test)]
const MAX_PERMISSION_RULES_PER_DOMAIN: usize = 256;
const CONFIG_PACKAGE_RESOLUTION_CONCURRENCY: usize = 8;
const CONFIG_PACKAGE_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(60);

/// Callback-free configuration accepted by the hosted actor.
///
/// This is deliberately smaller than [`AgentOsConfig`]. The hosted actor never
/// accepts bindings, host mounts, local package paths, a sidecar handle, or a
/// custom scheduler. Later feature revisions extend this closed shape with the
/// serializable hosted filesystem and remote-package descriptors.
#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentOsActorConfigInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<VmUserConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_node_builtins: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_resolution_time: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loopback_exempt_ports: Option<Vec<u16>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<AgentOsLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<HostedFilesystemConfigInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub software: Option<Vec<RemotePackageSourceInput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<PreviewPolicyInput>,
}

/// Complete persisted actor configuration for the fields supported by this
/// revision. Fields whose defaults belong to the sidecar remain optional so
/// the actor does not freeze a second copy of runtime policy.
#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentOsActorConfig {
    pub user: Option<VmUserConfig>,
    pub environment: Option<BTreeMap<String, String>>,
    pub allowed_node_builtins: Option<Vec<String>>,
    pub high_resolution_time: Option<bool>,
    pub loopback_exempt_ports: Vec<u16>,
    pub limits: Option<AgentOsLimits>,
    pub permissions: Option<Permissions>,
    pub filesystem: HostedFilesystemConfig,
    pub software: Vec<RemotePackageSource>,
    pub preview: PreviewPolicy,
}

/// URL-only hosted package input. This is intentionally not a serde wrapper
/// around Core's `PackageSource`, whose trusted `Path` variant must remain
/// impossible to construct through an actor action or creation payload.
#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemotePackageSourceInput {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// Resolution metadata is accepted so a `config.get` snapshot can be sent
    /// back to `config.set`. Core re-resolves the source and never trusts it.
    #[cfg_attr(feature = "contract", ts(rename = "sizeBytes"))]
    #[serde(default, rename = "sizeBytes", skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_id: Option<String>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemotePackageSource {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[cfg_attr(feature = "contract", ts(rename = "sizeBytes"))]
    #[serde(default, rename = "sizeBytes", skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_id: Option<String>,
}

pub const MIN_PREVIEW_TTL_MS: u64 = 1_000;
pub const MAX_PREVIEW_TTL_MS: u64 = 24 * 60 * 60 * 1_000;
pub const MAX_ACTIVE_PREVIEWS: u32 = 128;
pub const DEFAULT_PREVIEW_TTL_MS: u64 = 15 * 60 * 1_000;

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewPolicyInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_ttl_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_ttl_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_active: Option<u32>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewPolicy {
    pub default_ttl_ms: u64,
    pub max_ttl_ms: u64,
    pub max_active: u32,
}

impl Default for PreviewPolicy {
    fn default() -> Self {
        Self {
            default_ttl_ms: DEFAULT_PREVIEW_TTL_MS,
            max_ttl_ms: MAX_PREVIEW_TTL_MS,
            max_active: MAX_ACTIVE_PREVIEWS,
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedFilesystemConfigInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<HostedRootFilesystemInput>,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub mounts: Vec<HostedFilesystemMountInput>,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[cfg_attr(feature = "contract", ts(tag = "type", rename_all = "kebab-case"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HostedRootFilesystemInput {
    Default {},
    ActorSqlite {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
        #[serde(default, rename = "readOnly")]
        read_only: bool,
    },
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedFilesystemMountInput {
    pub path: String,
    pub backend: HostedFilesystemBackendInput,
    #[cfg_attr(feature = "contract", ts(optional, as = "Option<_>"))]
    #[serde(default)]
    pub read_only: bool,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[cfg_attr(feature = "contract", ts(tag = "type", rename_all = "kebab-case"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HostedFilesystemBackendInput {
    ActorSqlite {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
    },
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedFilesystemConfig {
    pub root: HostedRootFilesystem,
    pub mounts: Vec<HostedFilesystemMount>,
}

impl Default for HostedFilesystemConfig {
    fn default() -> Self {
        Self {
            root: HostedRootFilesystem::Default,
            mounts: Vec::new(),
        }
    }
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[cfg_attr(feature = "contract", ts(tag = "type", rename_all = "kebab-case"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HostedRootFilesystem {
    Default,
    ActorSqlite {
        namespace: String,
        #[serde(rename = "readOnly")]
        read_only: bool,
    },
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedFilesystemMount {
    pub path: String,
    pub backend: HostedFilesystemBackend,
    pub read_only: bool,
}

#[cfg_attr(feature = "contract", derive(ts_rs::TS))]
#[cfg_attr(feature = "contract", ts(tag = "type", rename_all = "kebab-case"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HostedFilesystemBackend {
    ActorSqlite { namespace: String },
}

impl AgentOsActorConfig {
    pub fn normalize(input: AgentOsActorConfigInput) -> Result<Self> {
        Self::normalize_with_budgets(input, &HostedActorBudgets::normalize(Default::default())?)
    }

    pub(crate) fn normalize_with_budgets(
        input: AgentOsActorConfigInput,
        budgets: &HostedActorBudgets,
    ) -> Result<Self> {
        validate_environment(input.environment.as_ref(), budgets)?;
        let allowed_node_builtins = input
            .allowed_node_builtins
            .map(|values| {
                agentos_vm_config::normalize_allowed_node_builtins(values, "allowedNodeBuiltins")
            })
            .transpose()?;
        let loopback_exempt_ports = agentos_vm_config::normalize_loopback_exempt_ports(
            input.loopback_exempt_ports.unwrap_or_default(),
            "loopbackExemptPorts",
        )?;
        let filesystem = normalize_filesystem(input.filesystem.unwrap_or_default(), budgets)?;
        let software = normalize_remote_software(input.software.unwrap_or_default(), budgets)?;
        validate_permissions(input.permissions.as_ref(), budgets)?;
        let preview = normalize_preview_policy(input.preview.unwrap_or_default(), budgets)?;
        let config = Self {
            user: input.user,
            environment: input.environment,
            allowed_node_builtins,
            high_resolution_time: input.high_resolution_time,
            loopback_exempt_ports,
            limits: input.limits,
            permissions: input.permissions,
            filesystem,
            software,
            preview,
        };

        // Reuse the Rust Core client's VM serializer and canonical vm-config
        // validation. The actor owns only hosted-boundary bounds above.
        config.to_core_config(None, None, None).validate()?;
        Ok(config)
    }

    pub(crate) fn validate_restored_budgets(&self, budgets: &HostedActorBudgets) -> Result<()> {
        let input = serde_json::from_value(
            serde_json::to_value(self).context("encode restored desired config for validation")?,
        )
        .context("decode restored desired config for validation")?;
        Self::normalize_with_budgets(input, budgets)
            .context("validate restored desired config against immutable worker budgets")?;
        Ok(())
    }

    pub(crate) fn to_core_config(
        &self,
        sidecar_binary_path: Option<String>,
        database: Option<VmSqliteDescriptor>,
        sqlite_callback: Option<agentos_client::SidecarSqliteCallback>,
    ) -> AgentOsConfig {
        let config = AgentOsConfig {
            user: self.user.clone(),
            environment: self.environment.clone(),
            allowed_node_builtins: self.allowed_node_builtins.clone(),
            high_resolution_time: self.high_resolution_time,
            loopback_exempt_ports: self.loopback_exempt_ports.clone(),
            limits: self.limits.clone(),
            permissions: self.permissions.clone(),
            root_filesystem: self.filesystem.root.to_core(),
            mounts: self
                .filesystem
                .mounts
                .iter()
                .map(HostedFilesystemMount::to_core)
                .collect(),
            sidecar_binary_path,
            database,
            sidecar_sqlite_callback: sqlite_callback,
            package_resolver: Some(agentos_preload::hosted_package_options()),
            ..AgentOsConfig::default()
        };
        config
    }

    /// Raw equality outside the actor gateway policy. A difference requires
    /// replacement unless the sidecar comparison explicitly proves equivalence.
    pub(crate) fn core_runtime_eq(&self, other: &Self) -> bool {
        // Compare the complete desired document, except the one actor-owned
        // gateway field that is live-applied. A newly added config field must
        // require replacement until it is explicitly classified otherwise.
        let mut before = self.clone();
        before.preview = other.preview;
        before == *other
    }

    /// Only these explicit fields are covered by the sidecar creation-policy
    /// comparison. Hosted filesystem/software changes and future actor fields
    /// must remain replacement-triggering even if Core's projection omits them.
    pub(crate) fn sidecar_comparison_covers_changes(&self, other: &Self) -> bool {
        let mut before = self.clone();
        before.user = other.user.clone();
        before.environment = other.environment.clone();
        before.allowed_node_builtins = other.allowed_node_builtins.clone();
        before.high_resolution_time = other.high_resolution_time;
        before.loopback_exempt_ports = other.loopback_exempt_ports.clone();
        before.limits = other.limits.clone();
        before.permissions = other.permissions.clone();
        before.preview = other.preview;
        before == *other
    }
}

impl RemotePackageSource {
    pub(crate) fn to_core(&self) -> PackageSource {
        PackageSource::Url {
            url: self.url.clone(),
            expected_digest: self.digest.clone(),
        }
    }

    pub(crate) fn resolved(url: String, installed: &agentos_client::InstalledSoftware) -> Self {
        Self {
            url,
            digest: Some(installed.digest.clone()),
            size: Some(installed.size_bytes),
            package_id: Some(installed.package_id.clone()),
        }
    }
}

pub(crate) fn normalize_remote_source(
    source: RemotePackageSourceInput,
) -> Result<RemotePackageSource> {
    normalize_remote_source_with_options(source, &agentos_preload::hosted_package_options())
}

fn normalize_remote_source_with_options(
    source: RemotePackageSourceInput,
    options: &PackageResolverOptions,
) -> Result<RemotePackageSource> {
    if source.package_id.is_some() && source.package_id != source.digest {
        bail!(
            "invalid_input: software packageId must equal digest when resolution metadata is supplied"
        );
    }
    validate_package_source_with_options(
        &PackageSource::Url {
            url: source.url.clone(),
            expected_digest: source.digest.clone(),
        },
        options,
    )?;
    Ok(RemotePackageSource {
        package_id: source.digest.clone(),
        url: source.url,
        digest: source.digest,
        size: None,
    })
}

fn normalize_remote_software(
    sources: Vec<RemotePackageSourceInput>,
    budgets: &HostedActorBudgets,
) -> Result<Vec<RemotePackageSource>> {
    check_budget(
        "actor.config.maxRemoteSoftware",
        sources.len(),
        budgets.config.max_remote_software,
        "sources",
        "software",
    )?;
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::with_capacity(sources.len());
    for source in sources {
        let source = normalize_remote_source(source)?;
        let identity = (source.url.clone(), source.digest.clone());
        if !seen.insert(identity) {
            continue;
        }
        normalized.push(source);
    }
    Ok(normalized)
}

/// Resolve and pin every URL before a replacement becomes durable. The actor
/// stores only verified content identities while retaining the URL for future
/// acquisition and preload observations.
pub(crate) async fn resolve_remote_software(
    sources: Vec<RemotePackageSource>,
) -> Result<Vec<RemotePackageSource>> {
    let resolution = stream::iter(sources)
        .map(|source| async move {
            let url = source.url.clone();
            let installed = crate::preload::acquire_required_package(source.to_core(), None)
                .await
                .with_context(|| format!("resolve required software URL {url:?}"))?;
            Ok::<_, anyhow::Error>(RemotePackageSource::resolved(url, &installed))
        })
        .buffered(CONFIG_PACKAGE_RESOLUTION_CONCURRENCY)
        .try_collect::<Vec<_>>();
    let resolved = tokio::time::timeout(CONFIG_PACKAGE_RESOLUTION_TIMEOUT, resolution)
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "config_package_resolution_timeout: required software resolution exceeded {}ms; raise CONFIG_PACKAGE_RESOLUTION_TIMEOUT",
                CONFIG_PACKAGE_RESOLUTION_TIMEOUT.as_millis()
            )
        })??;

    let mut package_ids = BTreeSet::new();
    Ok(resolved
        .into_iter()
        .filter(|source| {
            source
                .package_id
                .as_ref()
                .is_some_and(|package_id| package_ids.insert(package_id.clone()))
        })
        .collect())
}

impl HostedRootFilesystem {
    fn to_core(&self) -> RootFilesystemConfig {
        match self {
            Self::Default => RootFilesystemConfig::default(),
            Self::ActorSqlite {
                namespace,
                read_only,
            } => RootFilesystemConfig {
                kind: RootFilesystemKind::Native,
                mode: Some(if *read_only {
                    RootFilesystemMode::ReadOnly
                } else {
                    RootFilesystemMode::Ephemeral
                }),
                native_plugin: Some(actor_sqlite_plugin(namespace)),
                disable_default_base_layer: true,
                lowers: Vec::new(),
            },
        }
    }
}

impl HostedFilesystemMount {
    fn to_core(&self) -> MountConfig {
        let HostedFilesystemBackend::ActorSqlite { namespace } = &self.backend;
        MountConfig::Native {
            path: self.path.clone(),
            plugin: actor_sqlite_plugin(namespace),
            guest_source: Some("actor-sqlite".into()),
            guest_fstype: Some("actor-sqlite".into()),
            read_only: self.read_only,
        }
    }
}

fn actor_sqlite_plugin(namespace: &str) -> MountPlugin {
    MountPlugin {
        id: "chunked_sqlite".into(),
        config: Some(serde_json::json!({ "namespace": namespace })),
    }
}

fn normalize_filesystem(
    input: HostedFilesystemConfigInput,
    budgets: &HostedActorBudgets,
) -> Result<HostedFilesystemConfig> {
    check_budget(
        "actor.config.maxHostedMounts",
        input.mounts.len(),
        budgets.config.max_hosted_mounts,
        "mounts",
        "hosted filesystem",
    )?;

    let root = match input.root.unwrap_or(HostedRootFilesystemInput::Default {}) {
        HostedRootFilesystemInput::Default {} => HostedRootFilesystem::Default,
        HostedRootFilesystemInput::ActorSqlite {
            namespace,
            read_only,
        } => HostedRootFilesystem::ActorSqlite {
            namespace: normalize_namespace(
                namespace.unwrap_or_else(|| "actor-root".into()),
                budgets,
            )?,
            read_only,
        },
    };

    let mut mounts = Vec::with_capacity(input.mounts.len());
    let mut namespaces = BTreeSet::new();
    if let HostedRootFilesystem::ActorSqlite { namespace, .. } = &root {
        namespaces.insert(namespace.clone());
    }
    for (index, mount) in input.mounts.into_iter().enumerate() {
        validate_hosted_mount_path(&mount.path, budgets)?;
        let backend = match mount.backend {
            HostedFilesystemBackendInput::ActorSqlite { namespace } => {
                let namespace = normalize_namespace(
                    namespace.unwrap_or_else(|| format!("actor-mount-{index}")),
                    budgets,
                )?;
                if !namespaces.insert(namespace.clone()) {
                    bail!("actor-sqlite namespace {namespace:?} is used more than once");
                }
                HostedFilesystemBackend::ActorSqlite { namespace }
            }
        };
        if mounts
            .iter()
            .any(|existing: &HostedFilesystemMount| paths_overlap(&existing.path, &mount.path))
        {
            bail!(
                "filesystem mount path {:?} overlaps another configured mount",
                mount.path
            );
        }
        mounts.push(HostedFilesystemMount {
            path: mount.path,
            backend,
            read_only: mount.read_only,
        });
    }

    Ok(HostedFilesystemConfig { root, mounts })
}

fn validate_hosted_mount_path(path: &str, budgets: &HostedActorBudgets) -> Result<()> {
    if path.is_empty() {
        bail!("invalid_input: filesystem mount path must not be empty");
    }
    check_budget(
        "actor.paths.maxPathBytes",
        path.len(),
        budgets.max_path_bytes,
        "bytes",
        "filesystem mount path",
    )?;
    if !path.starts_with('/') || path == "/" || path.ends_with('/') {
        bail!("filesystem mount path must be a normalized, non-root absolute path");
    }
    if path.split('/').any(|part| part == "." || part == "..") {
        bail!("filesystem mount path must not contain traversal segments");
    }
    if ["/proc", "/etc/agentos", "/opt/agentos", "/__agentos"]
        .iter()
        .any(|reserved| path == *reserved || path.starts_with(&format!("{reserved}/")))
    {
        bail!("filesystem mount path {path:?} is reserved by agentOS");
    }
    Ok(())
}

fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn normalize_namespace(namespace: String, budgets: &HostedActorBudgets) -> Result<String> {
    if namespace.is_empty() {
        bail!("invalid_input: actor-sqlite namespace must not be empty");
    }
    check_budget(
        "actor.config.maxFilesystemNamespaceBytes",
        namespace.len(),
        budgets.config.max_filesystem_namespace_bytes,
        "bytes",
        "actor-sqlite namespace",
    )?;
    if !namespace
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("actor-sqlite namespace may contain only ASCII letters, digits, '-', '_', and '.'");
    }
    Ok(namespace)
}

fn validate_environment(
    environment: Option<&BTreeMap<String, String>>,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    let Some(environment) = environment else {
        return Ok(());
    };
    let limits = &budgets.config;
    check_budget(
        "actor.config.maxEnvironmentEntries",
        environment.len(),
        limits.max_environment_entries,
        "entries",
        "configuration environment",
    )?;
    let mut total = 0usize;
    for (key, value) in environment {
        if key.is_empty() || key.contains('=') || key.as_bytes().contains(&0) {
            bail!("invalid_input: environment key must be nonempty without '=' or NUL");
        }
        check_budget(
            "actor.config.maxEnvironmentKeyBytes",
            key.len(),
            limits.max_environment_key_bytes,
            "bytes",
            "environment key",
        )?;
        if value.as_bytes().contains(&0) {
            bail!("limit_exceeded: environment value contains NUL");
        }
        check_budget(
            "actor.config.maxEnvironmentValueBytes",
            value.len(),
            limits.max_environment_value_bytes,
            "bytes",
            "environment value",
        )?;
        total = total
            .checked_add(key.len())
            .and_then(|n| n.checked_add(value.len()))
            .ok_or_else(|| anyhow::anyhow!("limit_exceeded: environment byte count overflow"))?;
    }
    check_budget(
        "actor.config.maxEnvironmentBytes",
        total,
        limits.max_environment_bytes,
        "bytes",
        "configuration environment",
    )
}

fn validate_permissions(
    permissions: Option<&Permissions>,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    let Some(permissions) = permissions else {
        return Ok(());
    };
    if let Some(FsPermissions::Rules(rules)) = permissions.fs.as_ref() {
        validate_fs_permission_rules("permissions.fs", rules, budgets)?;
    }
    for (label, domain) in [
        ("permissions.network", permissions.network.as_ref()),
        (
            "permissions.childProcess",
            permissions.child_process.as_ref(),
        ),
        ("permissions.process", permissions.process.as_ref()),
        ("permissions.env", permissions.env.as_ref()),
        (
            "permissions.hostFunction",
            permissions.host_function.as_ref(),
        ),
    ] {
        if let Some(PatternPermissions::Rules(rules)) = domain {
            validate_pattern_permission_rules(label, rules, budgets)?;
        }
    }
    let encoded = serde_json::to_vec(permissions).context("encode permissions for bounds check")?;
    check_budget(
        "actor.config.maxPermissionConfigBytes",
        encoded.len(),
        budgets.config.max_permission_config_bytes,
        "bytes",
        "permissions",
    )?;
    Ok(())
}

fn validate_fs_permission_rules(
    label: &str,
    permissions: &RulePermissions<FsPermissionRule>,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    validate_permission_rule_count(label, permissions.rules.len(), budgets)?;
    for (index, rule) in permissions.rules.iter().enumerate() {
        validate_permission_values(
            &format!("{label}.rules[{index}].operations"),
            rule.operations.as_deref(),
            budgets,
        )?;
        validate_permission_values(
            &format!("{label}.rules[{index}].paths"),
            rule.paths.as_deref(),
            budgets,
        )?;
    }
    Ok(())
}

fn validate_pattern_permission_rules(
    label: &str,
    permissions: &RulePermissions<PatternPermissionRule>,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    validate_permission_rule_count(label, permissions.rules.len(), budgets)?;
    for (index, rule) in permissions.rules.iter().enumerate() {
        validate_permission_values(
            &format!("{label}.rules[{index}].operations"),
            rule.operations.as_deref(),
            budgets,
        )?;
        validate_permission_values(
            &format!("{label}.rules[{index}].patterns"),
            rule.patterns.as_deref(),
            budgets,
        )?;
    }
    Ok(())
}

fn validate_permission_rule_count(
    label: &str,
    count: usize,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    check_budget(
        "actor.config.maxPermissionRulesPerDomain",
        count,
        budgets.config.max_permission_rules_per_domain,
        "rules",
        label,
    )?;
    Ok(())
}

fn validate_permission_values(
    label: &str,
    values: Option<&[String]>,
    budgets: &HostedActorBudgets,
) -> Result<()> {
    let Some(values) = values else {
        return Ok(());
    };
    check_budget(
        "actor.config.maxPermissionValuesPerRule",
        values.len(),
        budgets.config.max_permission_values_per_rule,
        "values",
        label,
    )?;
    for value in values {
        if value.as_bytes().contains(&0) {
            bail!("limit_exceeded: permission value contains NUL");
        }
        check_budget(
            "actor.config.maxPermissionValueBytes",
            value.len(),
            budgets.config.max_permission_value_bytes,
            "bytes",
            label,
        )?;
    }
    Ok(())
}

fn normalize_preview_policy(
    input: PreviewPolicyInput,
    budgets: &HostedActorBudgets,
) -> Result<PreviewPolicy> {
    let policy = PreviewPolicy {
        default_ttl_ms: input.default_ttl_ms.unwrap_or(DEFAULT_PREVIEW_TTL_MS),
        max_ttl_ms: input
            .max_ttl_ms
            .unwrap_or(budgets.preview.max_ttl_ms.maximum as u64),
        max_active: input
            .max_active
            .unwrap_or(budgets.preview.max_active.maximum as u32),
    };
    if policy.max_ttl_ms < MIN_PREVIEW_TTL_MS {
        bail!("limit_exceeded: preview.maxTtlMs is below its minimum TTL");
    }
    budgets.preview.max_ttl_ms.check(
        "configured preview maxTtlMs",
        usize::try_from(policy.max_ttl_ms)
            .context("preview TTL exceeds platform representation")?,
    )?;
    if !(MIN_PREVIEW_TTL_MS..=policy.max_ttl_ms).contains(&policy.default_ttl_ms) {
        bail!(
            "invalid_input: preview.defaultTtlMs must be between {MIN_PREVIEW_TTL_MS} and preview.maxTtlMs ({})",
            policy.max_ttl_ms
        );
    }
    if policy.max_active == 0 {
        bail!("limit_exceeded: preview.maxActive must be positive");
    }
    budgets
        .preview
        .max_active
        .check("configured preview maxActive", policy.max_active as usize)?;
    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_materializes_defaults_and_canonicalizes_sets() {
        let normalized = AgentOsActorConfig::normalize(AgentOsActorConfigInput {
            allowed_node_builtins: Some(vec!["path".into(), "fs".into(), "path".into()]),
            loopback_exempt_ports: Some(vec![8080, 80, 8080]),
            ..AgentOsActorConfigInput::default()
        })
        .expect("normalize config");

        assert_eq!(
            normalized.allowed_node_builtins,
            Some(vec!["fs".into(), "path".into()])
        );
        assert_eq!(normalized.loopback_exempt_ports, vec![80, 8080]);
        assert_eq!(normalized.high_resolution_time, None);
        assert_eq!(normalized.filesystem, HostedFilesystemConfig::default());
        assert_eq!(normalized.preview, PreviewPolicy::default());
        assert_eq!(
            AgentOsActorConfig::normalize(AgentOsActorConfigInput::default())
                .expect("normalize default config"),
            AgentOsActorConfig::default()
        );
    }

    #[test]
    fn normalization_rejects_oversized_collections() {
        let error = AgentOsActorConfig::normalize(AgentOsActorConfigInput {
            loopback_exempt_ports: Some(vec![
                80;
                agentos_vm_config::MAX_VM_LOOPBACK_EXEMPT_PORTS + 1
            ]),
            ..AgentOsActorConfigInput::default()
        })
        .expect_err("oversized list must fail");
        assert!(error.to_string().contains("loopbackExemptPorts has"));
    }

    #[test]
    fn runtime_comparison_excludes_only_live_preview_policy() {
        let original = AgentOsActorConfig::default();
        let mut changed = original.clone();
        changed.preview.max_active = 4;
        assert!(original.core_runtime_eq(&changed));

        changed.environment = Some(BTreeMap::from([("DEBUG".into(), "1".into())]));
        assert!(!original.core_runtime_eq(&changed));

        let mut changed = original.clone();
        changed.filesystem.root = HostedRootFilesystem::ActorSqlite {
            namespace: "root".into(),
            read_only: false,
        };
        assert!(!original.core_runtime_eq(&changed));
    }

    #[test]
    fn sidecar_comparison_only_covers_explicit_shared_fields() {
        let original = AgentOsActorConfig::default();
        let mut changed = original.clone();
        changed.high_resolution_time = Some(false);
        changed.limits = Some(AgentOsLimits::default());
        changed.preview.max_active = 4;
        assert!(original.sidecar_comparison_covers_changes(&changed));

        changed.filesystem.root = HostedRootFilesystem::ActorSqlite {
            namespace: "different-root".into(),
            read_only: false,
        };
        assert!(!original.sidecar_comparison_covers_changes(&changed));
        changed.filesystem = original.filesystem.clone();
        changed.software.push(RemotePackageSource {
            url: "https://example.com/package.aospkg".into(),
            digest: None,
            size: None,
            package_id: None,
        });
        assert!(!original.sidecar_comparison_covers_changes(&changed));
    }

    #[test]
    fn filesystem_registry_is_closed_and_materializes_namespaces() {
        let normalized = AgentOsActorConfig::normalize(AgentOsActorConfigInput {
            filesystem: Some(HostedFilesystemConfigInput {
                root: Some(HostedRootFilesystemInput::ActorSqlite {
                    namespace: None,
                    read_only: false,
                }),
                mounts: vec![HostedFilesystemMountInput {
                    path: "/workspace".into(),
                    backend: HostedFilesystemBackendInput::ActorSqlite { namespace: None },
                    read_only: false,
                }],
            }),
            ..AgentOsActorConfigInput::default()
        })
        .expect("normalize hosted filesystem");

        assert_eq!(
            normalized.filesystem.root,
            HostedRootFilesystem::ActorSqlite {
                namespace: "actor-root".into(),
                read_only: false,
            }
        );
        assert_eq!(
            normalized.filesystem.mounts[0].backend,
            HostedFilesystemBackend::ActorSqlite {
                namespace: "actor-mount-0".into(),
            }
        );
        assert_eq!(normalized.filesystem.mounts[0].path, "/workspace");
    }

    #[test]
    fn filesystem_registry_rejects_reserved_and_overlapping_mounts() {
        let mount = |path: &str, namespace: &str| HostedFilesystemMountInput {
            path: path.into(),
            backend: HostedFilesystemBackendInput::ActorSqlite {
                namespace: Some(namespace.into()),
            },
            read_only: false,
        };
        for path in ["/", "/proc", "/opt/agentos/packages", "/a/../b"] {
            let error = normalize_filesystem(
                HostedFilesystemConfigInput {
                    root: None,
                    mounts: vec![mount(path, "one")],
                },
                &HostedActorBudgets::normalize(Default::default()).unwrap(),
            )
            .expect_err("unsafe mount must fail");
            assert!(error.to_string().contains("mount path"));
        }

        let error = normalize_filesystem(
            HostedFilesystemConfigInput {
                root: None,
                mounts: vec![mount("/workspace", "one"), mount("/workspace/cache", "two")],
            },
            &HostedActorBudgets::normalize(Default::default()).unwrap(),
        )
        .expect_err("overlap must fail");
        assert!(error.to_string().contains("overlaps"));
    }

    #[test]
    fn environment_and_preview_preserve_explicit_empty_and_false_values() {
        let normalized = AgentOsActorConfig::normalize(AgentOsActorConfigInput {
            environment: Some(BTreeMap::new()),
            high_resolution_time: Some(false),
            preview: Some(PreviewPolicyInput {
                default_ttl_ms: Some(MIN_PREVIEW_TTL_MS),
                max_ttl_ms: Some(MIN_PREVIEW_TTL_MS),
                max_active: Some(1),
            }),
            ..Default::default()
        })
        .expect("normalize explicit values");

        assert_eq!(normalized.environment, Some(BTreeMap::new()));
        assert_eq!(normalized.high_resolution_time, Some(false));
        assert_eq!(normalized.preview.max_active, 1);
        assert_eq!(normalized.preview.max_ttl_ms, MIN_PREVIEW_TTL_MS);
    }

    #[test]
    fn environment_and_permissions_are_bounded() {
        let environment = (0..=MAX_ENVIRONMENT_ENTRIES)
            .map(|index| (format!("KEY_{index}"), String::new()))
            .collect();
        let error = AgentOsActorConfig::normalize(AgentOsActorConfigInput {
            environment: Some(environment),
            ..Default::default()
        })
        .expect_err("oversized environment must fail");
        assert!(error
            .to_string()
            .contains("actor.config.maxEnvironmentEntries"));

        let permissions = Permissions {
            network: Some(PatternPermissions::Rules(RulePermissions {
                default: None,
                rules: (0..=MAX_PERMISSION_RULES_PER_DOMAIN)
                    .map(|_| PatternPermissionRule {
                        mode: agentos_client::PermissionMode::Allow,
                        operations: None,
                        patterns: None,
                    })
                    .collect(),
            })),
            ..Default::default()
        };
        let error = AgentOsActorConfig::normalize(AgentOsActorConfigInput {
            permissions: Some(permissions),
            ..Default::default()
        })
        .expect_err("oversized permission rule set must fail");
        assert!(error
            .to_string()
            .contains("actor.config.maxPermissionRulesPerDomain"));
    }

    #[test]
    fn resolved_software_metadata_round_trips_as_input_but_is_not_trusted() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let input: RemotePackageSourceInput = serde_json::from_value(serde_json::json!({
            "url": "https://example.com/tool.aospkg",
            "digest": digest,
            "sizeBytes": 42,
            "packageId": digest,
        }))
        .expect("deserialize resolved snapshot as input");
        let normalized = normalize_remote_source(input).expect("normalize remote source");
        assert_eq!(normalized.package_id, normalized.digest);
        assert_eq!(normalized.size, None);
        let serialized = serde_json::to_value(RemotePackageSource {
            size: Some(42),
            ..normalized
        })
        .expect("serialize resolved source");
        assert_eq!(serialized["sizeBytes"], 42);
        assert!(serialized.get("size").is_none());
    }
}

#[cfg(test)]
#[path = "../tests/unit/config_worker_budgets.rs"]
mod worker_budget_tests;

#[cfg(test)]
#[path = "../tests/unit/config_remaining_budgets.rs"]
mod remaining_budget_tests;

#[cfg(test)]
#[path = "../tests/unit/preload_source_policy.rs"]
mod preload_source_policy_tests;
