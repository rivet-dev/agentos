//! Closed hosted-actor configuration and deterministic Core projection.

use std::collections::{BTreeMap, BTreeSet};

use agentos_client::{
    validate_package_source, AgentOsConfig, AgentOsLimits, FsPermissionRule, FsPermissions,
    MountConfig, MountPlugin, PackageResolverOptions, PackageSource, PatternPermissionRule,
    PatternPermissions, Permissions, RootFilesystemConfig, RootFilesystemKind, RootFilesystemMode,
    RulePermissions, VmSqliteDescriptor, VmUserConfig,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

const MAX_ENVIRONMENT_ENTRIES: usize = 256;
const MAX_ENVIRONMENT_KEY_BYTES: usize = 256;
const MAX_ENVIRONMENT_VALUE_BYTES: usize = 8 * 1024;
const MAX_ENVIRONMENT_BYTES: usize = 64 * 1024;
const MAX_PERMISSION_RULES_PER_DOMAIN: usize = 256;
const MAX_PERMISSION_VALUES_PER_RULE: usize = 128;
const MAX_PERMISSION_VALUE_BYTES: usize = 4 * 1024;
const MAX_PERMISSION_CONFIG_BYTES: usize = 256 * 1024;
const MAX_HOSTED_MOUNTS: usize = 32;
const MAX_FILESYSTEM_PATH_BYTES: usize = 4 * 1024;
const MAX_FILESYSTEM_NAMESPACE_BYTES: usize = 128;
pub const MAX_REMOTE_SOFTWARE: usize = 128;

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
    Durable {
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
    Durable {
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
    Durable {
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
    Durable { namespace: String },
}

impl AgentOsActorConfig {
    pub fn normalize(input: AgentOsActorConfigInput) -> Result<Self> {
        validate_environment(input.environment.as_ref())?;
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
        let filesystem = normalize_filesystem(input.filesystem.unwrap_or_default())?;
        let software = normalize_remote_software(input.software.unwrap_or_default())?;
        validate_permissions(input.permissions.as_ref())?;
        let preview = normalize_preview_policy(input.preview.unwrap_or_default())?;
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

    pub fn to_core_config(
        &self,
        sidecar_binary_path: Option<String>,
        database: Option<VmSqliteDescriptor>,
        sqlite_callback: Option<agentos_client::SidecarSqliteCallback>,
    ) -> AgentOsConfig {
        let mut config = AgentOsConfig::default();
        config.user = self.user.clone();
        config.environment = self.environment.clone();
        config.allowed_node_builtins = self.allowed_node_builtins.clone();
        config.high_resolution_time = self.high_resolution_time;
        config.loopback_exempt_ports = self.loopback_exempt_ports.clone();
        config.limits = self.limits.clone();
        config.permissions = self.permissions.clone();
        config.root_filesystem = self.filesystem.root.to_core();
        config.mounts = self
            .filesystem
            .mounts
            .iter()
            .map(HostedFilesystemMount::to_core)
            .collect();
        config.sidecar_binary_path = sidecar_binary_path;
        config.database = database;
        config.sidecar_sqlite_callback = sqlite_callback;
        if std::env::var("AGENTOS_ALLOW_INSECURE_LOCAL_PACKAGE_HTTP").as_deref() == Ok("1") {
            config.package_resolver = Some(PackageResolverOptions {
                allow_insecure_local_http: true,
                ..PackageResolverOptions::default()
            });
        }
        config
    }

    /// Conservative fallback used only while no sidecar VM is available to
    /// classify the complete runtime projection. Preview policy is actor-owned
    /// and live; all other desired-state changes require replacement here.
    pub fn runtime_intent_differs(&self, other: &Self) -> bool {
        let mut before = self.clone();
        before.preview = other.preview;
        before != *other
    }

    /// Actor-only runtime identity forwarded into the sidecar classifier.
    /// Core's VM projection covers every other runtime-affecting field. Preview
    /// policy is intentionally absent because it is applied live by the actor.
    pub fn restart_identity(&self) -> Vec<String> {
        self.software
            .iter()
            .map(|source| {
                source.package_id.clone().unwrap_or_else(|| {
                    format!("{}#{}", source.url, source.digest.as_deref().unwrap_or(""))
                })
            })
            .collect()
    }
}

impl RemotePackageSource {
    pub fn to_core(&self) -> PackageSource {
        PackageSource::Url {
            url: self.url.clone(),
            expected_digest: self.digest.clone(),
        }
    }

    pub fn resolved(url: String, installed: &agentos_client::InstalledSoftware) -> Self {
        Self {
            url,
            digest: Some(installed.digest.clone()),
            size: Some(installed.size_bytes),
            package_id: Some(installed.package_id.clone()),
        }
    }
}

pub fn normalize_remote_source(source: RemotePackageSourceInput) -> Result<RemotePackageSource> {
    if source.package_id.is_some() && source.package_id != source.digest {
        bail!(
            "invalid_input: software packageId must equal digest when resolution metadata is supplied"
        );
    }
    validate_package_source(&PackageSource::Url {
        url: source.url.clone(),
        expected_digest: source.digest.clone(),
    })?;
    Ok(RemotePackageSource {
        package_id: source.digest.clone(),
        url: source.url,
        digest: source.digest,
        size: None,
    })
}

fn normalize_remote_software(
    sources: Vec<RemotePackageSourceInput>,
) -> Result<Vec<RemotePackageSource>> {
    if sources.len() > MAX_REMOTE_SOFTWARE {
        bail!(
            "software exceeds limit of {MAX_REMOTE_SOFTWARE}; reduce the package source collection"
        );
    }
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

impl HostedRootFilesystem {
    fn to_core(&self) -> RootFilesystemConfig {
        match self {
            Self::Default => RootFilesystemConfig::default(),
            Self::Durable {
                namespace,
                read_only,
            } => RootFilesystemConfig {
                kind: RootFilesystemKind::Native,
                mode: Some(if *read_only {
                    RootFilesystemMode::ReadOnly
                } else {
                    RootFilesystemMode::Ephemeral
                }),
                native_plugin: Some(durable_sqlite_plugin(namespace)),
                disable_default_base_layer: true,
                lowers: Vec::new(),
            },
        }
    }
}

impl HostedFilesystemMount {
    fn to_core(&self) -> MountConfig {
        let HostedFilesystemBackend::Durable { namespace } = &self.backend;
        MountConfig::Native {
            path: self.path.clone(),
            plugin: durable_sqlite_plugin(namespace),
            guest_source: Some("durable".into()),
            guest_fstype: Some("durable".into()),
            read_only: self.read_only,
        }
    }
}

fn durable_sqlite_plugin(namespace: &str) -> MountPlugin {
    MountPlugin {
        id: "chunked_sqlite".into(),
        config: Some(serde_json::json!({ "namespace": namespace })),
    }
}

fn normalize_filesystem(input: HostedFilesystemConfigInput) -> Result<HostedFilesystemConfig> {
    if input.mounts.len() > MAX_HOSTED_MOUNTS {
        bail!(
            "filesystem.mounts exceeds limit of {MAX_HOSTED_MOUNTS}; reduce the mount collection"
        );
    }

    let root = match input.root.unwrap_or(HostedRootFilesystemInput::Default {}) {
        HostedRootFilesystemInput::Default {} => HostedRootFilesystem::Default,
        HostedRootFilesystemInput::Durable {
            namespace,
            read_only,
        } => HostedRootFilesystem::Durable {
            namespace: normalize_namespace(namespace.unwrap_or_else(|| "root".into()))?,
            read_only,
        },
    };

    let mut mounts = Vec::with_capacity(input.mounts.len());
    let mut namespaces = BTreeSet::new();
    if let HostedRootFilesystem::Durable { namespace, .. } = &root {
        namespaces.insert(namespace.clone());
    }
    for (index, mount) in input.mounts.into_iter().enumerate() {
        validate_hosted_mount_path(&mount.path)?;
        let backend = match mount.backend {
            HostedFilesystemBackendInput::Durable { namespace } => {
                let namespace =
                    normalize_namespace(namespace.unwrap_or_else(|| format!("mount-{index}")))?;
                if !namespaces.insert(namespace.clone()) {
                    bail!("durable filesystem namespace {namespace:?} is used more than once");
                }
                HostedFilesystemBackend::Durable { namespace }
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

fn validate_hosted_mount_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > MAX_FILESYSTEM_PATH_BYTES {
        bail!("filesystem mount path must contain 1..={MAX_FILESYSTEM_PATH_BYTES} bytes");
    }
    if !path.starts_with('/')
        || path == "/"
        || path.ends_with('/')
        || path.contains("//")
        || path.contains('\0')
    {
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

fn normalize_namespace(namespace: String) -> Result<String> {
    if namespace.is_empty() || namespace.len() > MAX_FILESYSTEM_NAMESPACE_BYTES {
        bail!(
            "durable filesystem namespace must contain 1..={MAX_FILESYSTEM_NAMESPACE_BYTES} bytes"
        );
    }
    if !namespace
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("durable filesystem namespace may contain only ASCII letters, digits, '-', '_', and '.'");
    }
    Ok(namespace)
}

fn validate_environment(environment: Option<&BTreeMap<String, String>>) -> Result<()> {
    let Some(environment) = environment else {
        return Ok(());
    };
    if environment.len() > MAX_ENVIRONMENT_ENTRIES {
        bail!(
            "limit_exceeded: environment has {} entries; maximum is {MAX_ENVIRONMENT_ENTRIES}; reduce the environment or raise MAX_ENVIRONMENT_ENTRIES",
            environment.len()
        );
    }
    let mut total_bytes = 0usize;
    for (key, value) in environment {
        if key.is_empty()
            || key.len() > MAX_ENVIRONMENT_KEY_BYTES
            || key.contains('=')
            || key.as_bytes().contains(&0)
        {
            bail!(
                "invalid_input: environment key {key:?} must contain 1..={MAX_ENVIRONMENT_KEY_BYTES} bytes without '=' or NUL"
            );
        }
        if value.len() > MAX_ENVIRONMENT_VALUE_BYTES || value.as_bytes().contains(&0) {
            bail!(
                "limit_exceeded: environment value for {key:?} exceeds {MAX_ENVIRONMENT_VALUE_BYTES} bytes or contains NUL; reduce it or raise MAX_ENVIRONMENT_VALUE_BYTES"
            );
        }
        total_bytes = total_bytes
            .checked_add(key.len())
            .and_then(|total| total.checked_add(value.len()))
            .ok_or_else(|| anyhow::anyhow!("limit_exceeded: environment byte count overflow"))?;
    }
    if total_bytes > MAX_ENVIRONMENT_BYTES {
        bail!(
            "limit_exceeded: environment contains {total_bytes} bytes; maximum is {MAX_ENVIRONMENT_BYTES}; reduce it or raise MAX_ENVIRONMENT_BYTES"
        );
    }
    if environment.len() * 100 / MAX_ENVIRONMENT_ENTRIES >= 80
        || total_bytes * 100 / MAX_ENVIRONMENT_BYTES >= 80
    {
        tracing::warn!(
            limit = "actor_config_environment",
            observed_entries = environment.len(),
            capacity_entries = MAX_ENVIRONMENT_ENTRIES,
            observed_bytes = total_bytes,
            capacity_bytes = MAX_ENVIRONMENT_BYTES,
            configuration_path = "MAX_ENVIRONMENT_ENTRIES/MAX_ENVIRONMENT_BYTES",
            "actor configuration environment approaching capacity"
        );
    }
    Ok(())
}

fn validate_permissions(permissions: Option<&Permissions>) -> Result<()> {
    let Some(permissions) = permissions else {
        return Ok(());
    };
    if let Some(FsPermissions::Rules(rules)) = permissions.fs.as_ref() {
        validate_fs_permission_rules("permissions.fs", rules)?;
    }
    for (label, domain) in [
        ("permissions.network", permissions.network.as_ref()),
        (
            "permissions.childProcess",
            permissions.child_process.as_ref(),
        ),
        ("permissions.process", permissions.process.as_ref()),
        ("permissions.env", permissions.env.as_ref()),
        ("permissions.binding", permissions.binding.as_ref()),
    ] {
        if let Some(PatternPermissions::Rules(rules)) = domain {
            validate_pattern_permission_rules(label, rules)?;
        }
    }
    let encoded = serde_json::to_vec(permissions).context("encode permissions for bounds check")?;
    if encoded.len() > MAX_PERMISSION_CONFIG_BYTES {
        bail!(
            "limit_exceeded: permissions contains {} bytes; maximum is {MAX_PERMISSION_CONFIG_BYTES}; reduce it or raise MAX_PERMISSION_CONFIG_BYTES",
            encoded.len()
        );
    }
    if encoded.len() * 100 / MAX_PERMISSION_CONFIG_BYTES >= 80 {
        tracing::warn!(
            limit = "actor_config_permissions_bytes",
            observed = encoded.len(),
            capacity = MAX_PERMISSION_CONFIG_BYTES,
            configuration_path = "MAX_PERMISSION_CONFIG_BYTES",
            "actor configuration permissions approaching capacity"
        );
    }
    Ok(())
}

fn validate_fs_permission_rules(
    label: &str,
    permissions: &RulePermissions<FsPermissionRule>,
) -> Result<()> {
    validate_permission_rule_count(label, permissions.rules.len())?;
    for (index, rule) in permissions.rules.iter().enumerate() {
        validate_permission_values(
            &format!("{label}.rules[{index}].operations"),
            rule.operations.as_deref(),
        )?;
        validate_permission_values(
            &format!("{label}.rules[{index}].paths"),
            rule.paths.as_deref(),
        )?;
    }
    Ok(())
}

fn validate_pattern_permission_rules(
    label: &str,
    permissions: &RulePermissions<PatternPermissionRule>,
) -> Result<()> {
    validate_permission_rule_count(label, permissions.rules.len())?;
    for (index, rule) in permissions.rules.iter().enumerate() {
        validate_permission_values(
            &format!("{label}.rules[{index}].operations"),
            rule.operations.as_deref(),
        )?;
        validate_permission_values(
            &format!("{label}.rules[{index}].patterns"),
            rule.patterns.as_deref(),
        )?;
    }
    Ok(())
}

fn validate_permission_rule_count(label: &str, count: usize) -> Result<()> {
    if count > MAX_PERMISSION_RULES_PER_DOMAIN {
        bail!(
            "limit_exceeded: {label} has {count} rules; maximum is {MAX_PERMISSION_RULES_PER_DOMAIN}; reduce it or raise MAX_PERMISSION_RULES_PER_DOMAIN"
        );
    }
    Ok(())
}

fn validate_permission_values(label: &str, values: Option<&[String]>) -> Result<()> {
    let Some(values) = values else {
        return Ok(());
    };
    if values.len() > MAX_PERMISSION_VALUES_PER_RULE {
        bail!(
            "limit_exceeded: {label} has {} values; maximum is {MAX_PERMISSION_VALUES_PER_RULE}; reduce it or raise MAX_PERMISSION_VALUES_PER_RULE",
            values.len()
        );
    }
    if let Some(value) = values
        .iter()
        .find(|value| value.len() > MAX_PERMISSION_VALUE_BYTES || value.as_bytes().contains(&0))
    {
        bail!(
            "limit_exceeded: {label} value contains {} bytes or NUL; maximum is {MAX_PERMISSION_VALUE_BYTES}; reduce it or raise MAX_PERMISSION_VALUE_BYTES",
            value.len()
        );
    }
    Ok(())
}

fn normalize_preview_policy(input: PreviewPolicyInput) -> Result<PreviewPolicy> {
    let policy = PreviewPolicy {
        default_ttl_ms: input.default_ttl_ms.unwrap_or(DEFAULT_PREVIEW_TTL_MS),
        max_ttl_ms: input.max_ttl_ms.unwrap_or(MAX_PREVIEW_TTL_MS),
        max_active: input.max_active.unwrap_or(MAX_ACTIVE_PREVIEWS),
    };
    if !(MIN_PREVIEW_TTL_MS..=MAX_PREVIEW_TTL_MS).contains(&policy.max_ttl_ms) {
        bail!(
            "limit_exceeded: preview.maxTtlMs must be between {MIN_PREVIEW_TTL_MS} and {MAX_PREVIEW_TTL_MS}; lower it or raise MAX_PREVIEW_TTL_MS"
        );
    }
    if !(MIN_PREVIEW_TTL_MS..=policy.max_ttl_ms).contains(&policy.default_ttl_ms) {
        bail!(
            "invalid_input: preview.defaultTtlMs must be between {MIN_PREVIEW_TTL_MS} and preview.maxTtlMs ({})",
            policy.max_ttl_ms
        );
    }
    if policy.max_active == 0 || policy.max_active > MAX_ACTIVE_PREVIEWS {
        bail!(
            "limit_exceeded: preview.maxActive must be between 1 and {MAX_ACTIVE_PREVIEWS}; lower it or raise MAX_ACTIVE_PREVIEWS"
        );
    }
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
    fn unavailable_sidecar_fallback_excludes_only_live_preview_policy() {
        let original = AgentOsActorConfig::default();
        let mut changed = original.clone();
        changed.preview.max_active = 4;
        assert!(!original.runtime_intent_differs(&changed));

        changed.environment = Some(BTreeMap::from([("DEBUG".into(), "1".into())]));
        assert!(original.runtime_intent_differs(&changed));

        let mut changed = original.clone();
        changed.filesystem.root = HostedRootFilesystem::Durable {
            namespace: "root".into(),
            read_only: false,
        };
        assert!(original.runtime_intent_differs(&changed));
    }

    #[test]
    fn actor_restart_identity_contains_only_software() {
        let original = AgentOsActorConfig::default();
        let mut changed = original.clone();
        changed.high_resolution_time = Some(false);
        changed.limits = Some(AgentOsLimits::default());
        changed.preview.max_active = 4;
        assert_eq!(original.restart_identity(), changed.restart_identity());

        changed.filesystem.root = HostedRootFilesystem::Durable {
            namespace: "different-root".into(),
            read_only: false,
        };
        assert_eq!(original.restart_identity(), changed.restart_identity());
        changed.filesystem = original.filesystem.clone();
        changed.software.push(RemotePackageSource {
            url: "https://example.com/package.aospkg".into(),
            digest: None,
            size: None,
            package_id: None,
        });
        assert_ne!(original.restart_identity(), changed.restart_identity());
    }

    #[test]
    fn filesystem_registry_is_closed_and_materializes_namespaces() {
        let normalized = AgentOsActorConfig::normalize(AgentOsActorConfigInput {
            filesystem: Some(HostedFilesystemConfigInput {
                root: Some(HostedRootFilesystemInput::Durable {
                    namespace: None,
                    read_only: false,
                }),
                mounts: vec![HostedFilesystemMountInput {
                    path: "/workspace".into(),
                    backend: HostedFilesystemBackendInput::Durable { namespace: None },
                    read_only: false,
                }],
            }),
            ..AgentOsActorConfigInput::default()
        })
        .expect("normalize hosted filesystem");

        assert_eq!(
            normalized.filesystem.root,
            HostedRootFilesystem::Durable {
                namespace: "root".into(),
                read_only: false,
            }
        );
        assert_eq!(
            normalized.filesystem.mounts[0].backend,
            HostedFilesystemBackend::Durable {
                namespace: "mount-0".into(),
            }
        );
        assert_eq!(normalized.filesystem.mounts[0].path, "/workspace");
    }

    #[test]
    fn filesystem_registry_rejects_reserved_and_overlapping_mounts() {
        let mount = |path: &str, namespace: &str| HostedFilesystemMountInput {
            path: path.into(),
            backend: HostedFilesystemBackendInput::Durable {
                namespace: Some(namespace.into()),
            },
            read_only: false,
        };
        for path in [
            "/",
            "/proc",
            "/opt/agentos/packages",
            "/a/../b",
            "//proc",
            "/opt//agentos",
            "/etc//agentos/config",
            "/workspace//cache",
            "/workspace\0suffix",
        ] {
            let error = normalize_filesystem(HostedFilesystemConfigInput {
                root: None,
                mounts: vec![mount(path, "one")],
            })
            .expect_err("unsafe mount must fail");
            assert!(error.to_string().contains("mount path"));
        }

        let error = normalize_filesystem(HostedFilesystemConfigInput {
            root: None,
            mounts: vec![mount("/workspace", "one"), mount("/workspace/cache", "two")],
        })
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
        assert!(error.to_string().contains("environment has"));

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
        assert!(error.to_string().contains("maximum is 256"));
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
