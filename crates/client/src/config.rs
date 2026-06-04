//! Configuration types: `AgentOsConfig` (= TS `AgentOsOptions`), the permissions tree, root
//! filesystem config, mount config, and the schedule-driver abstraction.
//!
//! Ported from `packages/core/src/agent-os.ts` (`AgentOsOptions`), `runtime.ts` (`Permissions`),
//! `layers.ts` / `overlay-filesystem.ts` (root/overlay), and `cron/` (schedule driver).
//!
//! Non-serializable parameters (`MountConfig::Plain.driver`, `CronAction::Callback`) are in-process
//! only and become `Arc<dyn ...>` trait objects; they cannot cross the wire and are gated exactly as
//! the actor layer gates them.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::fs::VirtualFileSystem;

/// Resolved client options (= TS `AgentOsOptions`). All fields optional with documented defaults.
#[derive(Default)]
pub struct AgentOsConfig {
    /// Software packages to install (flattened). Default `[]`.
    pub software: Vec<SoftwareInput>,
    /// Loopback ports exempt from the default outbound-to-host block.
    pub loopback_exempt_ports: Vec<u16>,
    /// Allowed Node.js builtins. Default: the hardened native-bridge set.
    pub allowed_node_builtins: Option<Vec<String>>,
    /// Working directory used for guest module resolution. Default: host cwd.
    pub module_access_cwd: Option<String>,
    /// Root filesystem configuration. Default: overlay + bundled base snapshot.
    pub root_filesystem: RootFilesystemConfig,
    /// Additional mounts.
    pub mounts: Vec<MountConfig>,
    /// Extra OS instructions appended to agent sessions.
    pub additional_instructions: Option<String>,
    /// Schedule driver used by the cron manager. Default: [`TimerScheduleDriver`].
    pub schedule_driver: Option<Arc<dyn ScheduleDriver>>,
    /// Tool kits to register.
    pub tool_kits: Vec<ToolKit>,
    /// Permission policy. Default: allow-all.
    pub permissions: Option<Permissions>,
    /// Sidecar placement/config. Default: shared `default` pool.
    pub sidecar: Option<AgentOsSidecarConfig>,
}

/// Builder for [`AgentOsConfig`].
#[derive(Default)]
pub struct AgentOsConfigBuilder {
    config: AgentOsConfig,
}

impl AgentOsConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn software(mut self, software: Vec<SoftwareInput>) -> Self {
        self.config.software = software;
        self
    }

    pub fn loopback_exempt_ports(mut self, ports: Vec<u16>) -> Self {
        self.config.loopback_exempt_ports = ports;
        self
    }

    pub fn allowed_node_builtins(mut self, builtins: Vec<String>) -> Self {
        self.config.allowed_node_builtins = Some(builtins);
        self
    }

    pub fn module_access_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.config.module_access_cwd = Some(cwd.into());
        self
    }

    pub fn root_filesystem(mut self, root: RootFilesystemConfig) -> Self {
        self.config.root_filesystem = root;
        self
    }

    pub fn mounts(mut self, mounts: Vec<MountConfig>) -> Self {
        self.config.mounts = mounts;
        self
    }

    pub fn additional_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.config.additional_instructions = Some(instructions.into());
        self
    }

    pub fn schedule_driver(mut self, driver: Arc<dyn ScheduleDriver>) -> Self {
        self.config.schedule_driver = Some(driver);
        self
    }

    pub fn tool_kits(mut self, tool_kits: Vec<ToolKit>) -> Self {
        self.config.tool_kits = tool_kits;
        self
    }

    pub fn permissions(mut self, permissions: Permissions) -> Self {
        self.config.permissions = Some(permissions);
        self
    }

    pub fn sidecar(mut self, sidecar: AgentOsSidecarConfig) -> Self {
        self.config.sidecar = Some(sidecar);
        self
    }

    pub fn build(self) -> AgentOsConfig {
        self.config
    }
}

/// A flattened software package input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareInput {
    pub package: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// A registered tool kit (in-process; tool implementations stay host-side).
#[derive(Clone)]
pub struct ToolKit {
    pub name: String,
    pub description: String,
    // TODO(parity: model tool definitions + host invocation callbacks).
}

// ---------------------------------------------------------------------------
// Permissions tree (runtime.ts)
// ---------------------------------------------------------------------------

/// Top-level permission policy. All domains optional (`allowAll` when omitted).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fs: Option<FsPermissions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<PatternPermissions>,
    #[serde(default, rename = "childProcess", skip_serializing_if = "Option::is_none")]
    pub child_process: Option<PatternPermissions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<PatternPermissions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<PatternPermissions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<PatternPermissions>,
}

/// `"allow"` or `"deny"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    Allow,
    Deny,
}

/// `PermissionMode | RulePermissions<FsPermissionRule>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FsPermissions {
    Mode(PermissionMode),
    Rules(RulePermissions<FsPermissionRule>),
}

/// `PermissionMode | RulePermissions<PatternPermissionRule>` (network/childProcess/process/env/tool).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PatternPermissions {
    Mode(PermissionMode),
    Rules(RulePermissions<PatternPermissionRule>),
}

/// `{ default?: PermissionMode; rules: T[] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulePermissions<T> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<PermissionMode>,
    pub rules: Vec<T>,
}

/// `{ mode; operations?; paths? }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsPermissionRule {
    pub mode: PermissionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
}

/// `{ mode; operations?; patterns? }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternPermissionRule {
    pub mode: PermissionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patterns: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Root filesystem (layers.ts / overlay-filesystem.ts)
// ---------------------------------------------------------------------------

/// Root filesystem configuration. Default: overlay + bundled base snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootFilesystemConfig {
    #[serde(default, rename = "type")]
    pub kind: RootFilesystemKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RootFilesystemMode>,
    #[serde(default, rename = "disableDefaultBaseLayer")]
    pub disable_default_base_layer: bool,
    #[serde(default)]
    pub lowers: Vec<RootLowerInput>,
}

impl Default for RootFilesystemConfig {
    fn default() -> Self {
        Self {
            kind: RootFilesystemKind::Overlay,
            mode: None,
            disable_default_base_layer: false,
            lowers: Vec::new(),
        }
    }
}

/// The root filesystem kind. Currently only `overlay`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RootFilesystemKind {
    #[default]
    Overlay,
}

/// Root filesystem mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RootFilesystemMode {
    Ephemeral,
    ReadOnly,
}

/// A lower (immutable) snapshot layer input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RootLowerInput {
    /// The bundled base filesystem snapshot.
    BundledBaseFilesystem,
    /// A snapshot export (`{ kind: "snapshot-export", source }`).
    #[serde(untagged)]
    SnapshotExport(crate::fs::RootSnapshotExport),
}

// ---------------------------------------------------------------------------
// Mounts
// ---------------------------------------------------------------------------

/// A filesystem mount. `Plain.driver` is an in-process trait object and cannot cross the wire.
pub enum MountConfig {
    /// Plain mount over an in-process [`VirtualFileSystem`] driver.
    Plain {
        path: String,
        driver: Arc<dyn VirtualFileSystem>,
        read_only: bool,
    },
    /// Native plugin mount (`{ id; config? }`).
    Native {
        path: String,
        plugin: MountPlugin,
        read_only: bool,
    },
    /// Overlay mount (`{ type: "overlay"; store; mode?; lowers }`).
    Overlay {
        path: String,
        filesystem: OverlayMountConfig,
    },
}

/// A native mount plugin descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountPlugin {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// Overlay mount filesystem config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayMountConfig {
    #[serde(rename = "type")]
    pub kind: String,
    pub store: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RootFilesystemMode>,
    pub lowers: Vec<RootLowerInput>,
}

// ---------------------------------------------------------------------------
// Sidecar config
// ---------------------------------------------------------------------------

/// How the client obtains its sidecar handle.
pub enum AgentOsSidecarConfig {
    /// Use (or create) a shared pooled sidecar (`pool` default `"default"`).
    Shared { pool: Option<String> },
    /// Use an explicit sidecar handle.
    Explicit {
        handle: Arc<crate::sidecar::AgentOsSidecar>,
    },
}

// ---------------------------------------------------------------------------
// Schedule driver
// ---------------------------------------------------------------------------

/// Abstraction over wall-clock scheduling, allowing tests to inject a deterministic clock.
///
/// Ported from the TS `ScheduleDriver` interface used by the cron manager.
pub trait ScheduleDriver: Send + Sync {
    /// The current wall-clock time.
    fn now(&self) -> chrono::DateTime<chrono::Utc>;

    /// Schedule `callback` to run after `delay`. Returns a cancellation handle.
    ///
    /// TODO(parity: define the returned timer handle type once cron dispatch lands).
    fn schedule(
        &self,
        delay: std::time::Duration,
        callback: Box<dyn FnOnce() + Send + Sync>,
    ) -> ScheduleHandle;
}

/// Opaque handle to a scheduled timer; dropping it cancels the pending callback.
pub struct ScheduleHandle {
    pub(crate) abort: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl ScheduleHandle {
    pub fn cancel(mut self) {
        if let Some(abort) = self.abort.take() {
            abort();
        }
    }
}

impl Drop for ScheduleHandle {
    fn drop(&mut self) {
        if let Some(abort) = self.abort.take() {
            abort();
        }
    }
}

/// Default schedule driver backed by `tokio` timers and the system clock.
#[derive(Default)]
pub struct TimerScheduleDriver;

impl TimerScheduleDriver {
    pub fn new() -> Self {
        Self
    }
}

impl ScheduleDriver for TimerScheduleDriver {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }

    fn schedule(
        &self,
        _delay: std::time::Duration,
        _callback: Box<dyn FnOnce() + Send + Sync>,
    ) -> ScheduleHandle {
        todo!("parity: spawn a tokio timer task that runs callback after delay")
    }
}

/// Metadata helpers reused when building sidecar requests.
pub(crate) fn empty_metadata() -> BTreeMap<String, String> {
    BTreeMap::new()
}
