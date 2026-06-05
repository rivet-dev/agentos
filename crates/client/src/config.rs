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

/// A host-side tool execute callback. Receives the validated JSON input, returns a JSON result or an
/// error string. Stays host-side (never crosses to the guest); the guest invokes it by name via the
/// sidecar tool-invocation callback channel.
pub type ToolCallback = Arc<
    dyn Fn(serde_json::Value) -> futures::future::BoxFuture<'static, Result<serde_json::Value, String>>
        + Send
        + Sync,
>;

/// A single host tool within a [`ToolKit`].
#[derive(Clone)]
pub struct HostTool {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool input (forwarded to the sidecar `register_toolkit` definition).
    pub input_schema: serde_json::Value,
    pub timeout_ms: Option<u64>,
    /// Host-side implementation, invoked when the guest calls `<toolkit>:<tool>`.
    pub execute: ToolCallback,
}

/// A registered tool kit (in-process; tool implementations stay host-side). Tools are exposed to the
/// guest as `<toolkit>:<tool>` and dispatched back to [`HostTool::execute`] via the sidecar
/// tool-invocation callback channel.
#[derive(Clone)]
pub struct ToolKit {
    pub name: String,
    pub description: String,
    pub tools: Vec<HostTool>,
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

/// The callback fired by a [`ScheduleDriver`] when a schedule entry triggers.
///
/// Mirrors the TS `ScheduleEntry.callback: () => void | Promise<void>`. The cron manager passes a
/// closure that runs one job execution; the driver awaits it (and, for the default driver, reschedules
/// the next cron fire afterwards).
pub type ScheduleCallback =
    Arc<dyn Fn() -> futures::future::BoxFuture<'static, ()> + Send + Sync>;

/// A schedule entry handed to a [`ScheduleDriver`]. Mirrors TS `ScheduleEntry`
/// (`cron/schedule-driver.ts`).
#[derive(Clone)]
pub struct ScheduleEntry {
    /// Unique ID for this job.
    pub id: String,
    /// 5/6/7-field cron expression OR an ISO-8601 one-shot timestamp.
    pub schedule: String,
    /// Called when the schedule fires.
    pub callback: ScheduleCallback,
}

/// Driver-owned scheduling abstraction. Mirrors the TS `ScheduleDriver` interface
/// (`cron/schedule-driver.ts`) exactly: the driver parses the schedule, arms the timer, reschedules
/// cron entries after each fire, and tears everything down on [`ScheduleDriver::dispose`]. This is the
/// documented extension point: a custom driver (deterministic virtual-time test driver, fire-immediately
/// driver, etc.) fully controls timing.
pub trait ScheduleDriver: Send + Sync {
    /// Schedule a callback to fire on a cron expression or at a specific time. Returns a cancellation
    /// handle.
    fn schedule(&self, entry: ScheduleEntry) -> ScheduleHandle;

    /// Cancel a previously scheduled entry.
    fn cancel(&self, handle: &ScheduleHandle);

    /// Tear down all scheduled work.
    fn dispose(&self);
}

/// Handle to a scheduled entry. Mirrors TS `ScheduleHandle { id }`. Identifies the entry to cancel via
/// [`ScheduleDriver::cancel`].
#[derive(Clone)]
pub struct ScheduleHandle {
    pub id: String,
}

/// Default schedule driver backed by `tokio` timers and the system clock.
///
/// Mirrors the TS `TimerScheduleDriver`: for cron expressions it computes the next fire time and arms
/// a single timer, rescheduling after each fire; for one-shot timestamps it fires once and removes the
/// entry. Driver-held timer tasks are tracked so [`ScheduleDriver::cancel`] / [`ScheduleDriver::dispose`]
/// can abort them.
#[derive(Default)]
pub struct TimerScheduleDriver {
    timers: Arc<scc::HashMap<String, tokio_util::sync::CancellationToken>>,
}

impl TimerScheduleDriver {
    pub fn new() -> Self {
        Self {
            timers: Arc::new(scc::HashMap::new()),
        }
    }

    /// Arm the next fire for `entry`. For a one-shot or an exhausted cron the entry is dropped. For a
    /// recurring cron the timer reschedules itself after firing the callback. `cancel` is the per-entry
    /// cancellation token shared with the registry slot.
    fn schedule_next(
        timers: Arc<scc::HashMap<String, tokio_util::sync::CancellationToken>>,
        entry: ScheduleEntry,
        cancel: tokio_util::sync::CancellationToken,
    ) {
        let now = chrono::Utc::now();
        let parsed = match crate::cron::parse_schedule(&entry.schedule) {
            Ok(parsed) => parsed,
            Err(_) => {
                let _ = timers.remove(&entry.id);
                return;
            }
        };
        let is_cron = parsed.is_cron();
        let next = match crate::cron::resolve_next_run(&parsed, now) {
            Some(next) => next,
            None => {
                // No upcoming run (one-shot in the past, or exhausted cron).
                let _ = timers.remove(&entry.id);
                return;
            }
        };

        let delay = (next - now)
            .to_std()
            .unwrap_or(std::time::Duration::ZERO);

        tokio::spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => {
                    return;
                }
                _ = tokio::time::sleep(delay) => {}
            }
            if cancel.is_cancelled() {
                return;
            }
            // The driver is fire-and-forget; errors are the caller's responsibility.
            (entry.callback)().await;

            if is_cron && timers.contains(&entry.id) {
                Self::schedule_next(Arc::clone(&timers), entry, cancel);
            } else {
                let _ = timers.remove(&entry.id);
            }
        });
    }
}

impl ScheduleDriver for TimerScheduleDriver {
    fn schedule(&self, entry: ScheduleEntry) -> ScheduleHandle {
        let id = entry.id.clone();
        let cancel = tokio_util::sync::CancellationToken::new();
        // Replace any existing timer for this id, cancelling it first.
        if let Some((_, old)) = self.timers.remove(&id) {
            old.cancel();
        }
        let _ = self.timers.insert(id.clone(), cancel.clone());

        Self::schedule_next(Arc::clone(&self.timers), entry, cancel);

        ScheduleHandle { id }
    }

    fn cancel(&self, handle: &ScheduleHandle) {
        if let Some((_, cancel)) = self.timers.remove(&handle.id) {
            cancel.cancel();
        }
    }

    fn dispose(&self) {
        self.timers.scan(|_, cancel| cancel.cancel());
        self.timers.clear();
    }
}

/// Metadata helpers reused when building sidecar requests.
pub(crate) fn empty_metadata() -> BTreeMap<String, String> {
    BTreeMap::new()
}
