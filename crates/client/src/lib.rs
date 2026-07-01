#![forbid(unsafe_code)]

//! # agentos-client
//!
//! High-level Rust client SDK for the Agent OS native sidecar. This is a 1:1 port of the TypeScript
//! `AgentOs` client (`packages/core/src/agent-os.ts`): every public method, option type, return
//! type, event, and error maps across with identical semantics.
//!
//! The client spawns the native `agentos-sidecar` binary and speaks the existing framed BARE
//! protocol over its stdio (see [`transport`]). It does NOT embed the kernel in-process and does NOT
//! define a new sidecar wire protocol. The generated Secure Exec schema surface comes from
//! `secure_exec_client::wire`; Agent OS layers ACP/session semantics on top of those generated wire
//! frames through the wrapper client.
//!
//! See the companion design docs in `~/.agents/specs/rust-client-sdk/` (ADR-001, spec, reference,
//! checklist) for the architecture, type-mapping, error taxonomy, and streaming model.

pub mod agent_os;
pub(crate) mod command_line;
pub mod config;
pub mod cron;
pub mod error;
pub mod fs;
pub mod json_rpc;
pub mod net;
pub mod process;
pub mod session;
pub mod sidecar;
pub mod stream;
pub mod transport;

// ---------------------------------------------------------------------------
// Centralized constants (ADR-001 §6 / spec.md §7)
// ---------------------------------------------------------------------------

/// ACP protocol version negotiated on session creation.
pub const ACP_PROTOCOL_VERSION: u64 = 1;

/// Per-request permission timeout (milliseconds).
pub const PERMISSION_TIMEOUT_MS: u64 = 120_000;

/// Bounded closed-session-id set capacity (for `close_session` idempotence).
pub const CLOSED_SESSION_ID_RETENTION_LIMIT: usize = 2048;

/// Two-phase shell-drain timeout during dispose (milliseconds).
pub const SHELL_DISPOSE_TIMEOUT_MS: u64 = 5_000;

/// VM lifecycle ready timeout during `create` (milliseconds).
pub const VM_READY_TIMEOUT_MS: u64 = 10_000;

/// Maximum scheduled cron jobs per VM.
pub const CRON_JOB_LIMIT: usize = 1024;

// ---------------------------------------------------------------------------
// Public re-exports
// ---------------------------------------------------------------------------

pub use agent_os::{AgentOs, PackageDescriptor};
pub use error::{ClientError, ClientResult};
pub use sidecar::{
    AgentOsSidecar, AgentOsSidecarDescription, AgentOsSidecarPlacement, SidecarState,
};
pub use stream::{ByteStream, Subscription};

pub use config::{
    AcpLimits, AgentOsConfig, AgentOsConfigBuilder, AgentOsLimits, AgentOsSidecarConfig,
    FsPermissionRule, FsPermissions, HostTool, HttpLimits, JsRuntimeLimits, MountConfig,
    MountPlugin, OverlayMountConfig, PatternPermissionRule, PatternPermissions, PermissionMode,
    Permissions, PluginLimits, PythonLimits, ResourceLimits, RootFilesystemConfig,
    RootFilesystemKind, RootFilesystemMode, RootLowerInput, RulePermissions, ScheduleCallback,
    ScheduleDriver, ScheduleEntry, ScheduleHandle, SidecarJsBridgeCall, SidecarJsBridgeCallback,
    SoftwareInput, SoftwareKind, TimerScheduleDriver, ToolCallback, ToolKit, ToolLimits,
    WasmLimits,
};

pub use process::{
    ExecOptions, ExecResult, ProcessInfo, ProcessStatus, ProcessTreeNode, SpawnHandle,
    SpawnOptions, SpawnStdio, SpawnedProcessInfo, StdinInput, TimingMitigation,
};

pub use fs::{
    BatchReadResult, BatchWriteEntry, BatchWriteResult, DeleteOptions, DirEntry, DirEntryType,
    FileContent, FilesystemEntry, FilesystemEntryEncoding, FilesystemSnapshotEntries,
    FilesystemSnapshotExport, MkdirOptions, MountFsOptions, ReaddirRecursiveOptions,
    RootSnapshotExport, SnapshotExportKind, VirtualDirEntry, VirtualFileSystem, VirtualStat,
};

pub use shell::{ConnectTerminalOptions, OpenShellOptions, ShellHandle};

pub use session::{
    AgentCapabilities, AgentExitEvent, AgentExitStream, AgentExitSubscription, AgentInfo,
    AgentRegistryEntry, ConfigAllowedValue, CreateSessionOptions, McpServerConfig, PermissionReply,
    PermissionRequest, PromptCapabilities, PromptResult, ResumeSessionOptions, ResumeSessionResult,
    SessionConfigOption, SessionId, SessionInfo, SessionInitData, SessionMode, SessionModeState,
};

pub use json_rpc::{
    is_unknown_session, AcpTimeoutErrorData, JsonRpcError, JsonRpcId, JsonRpcNotification,
    JsonRpcResponse, UnknownSessionErrorData,
};

pub use cron::{
    CronAction, CronEvent, CronJobHandle, CronJobInfo, CronJobOptions, CronManager, CronOverlap,
};

// `shell` is declared here because its methods live in a sibling module to keep `lib.rs` re-exports
// flat; the module file itself is `shell.rs`.
pub mod shell;
