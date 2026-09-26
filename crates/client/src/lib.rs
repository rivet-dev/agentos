#![forbid(unsafe_code)]

//! # agentos-client
//!
//! High-level Rust client SDK for the Agent OS sidecar. This is a 1:1 port of the TypeScript
//! `AgentOs` client (`packages/core/src/agent-os.ts`): every public method, option type, return
//! type, event, and error maps across with identical semantics.
//!
//! The client spawns the native `agentos-sidecar` binary and speaks the framed BARE
//! protocol over its stdio (see [`transport`]). It does NOT embed the kernel in-process and does NOT
//! define a new sidecar wire protocol. The generated agentOS language execution schema surface comes
//! from `agentos_sidecar_client::wire`.
//!
//! See the companion design docs in `~/.agents/specs/rust-client-sdk/` (ADR-001, spec, reference,
//! checklist) for the architecture, type-mapping, error taxonomy, and streaming model.

pub mod agent_os;
pub(crate) mod command_line;
pub mod config;
pub mod cron;
pub mod error;
pub mod fs;
pub mod language_execution;
pub mod net;
mod output_replay;
pub mod process;
pub mod sidecar;
pub mod software;
pub mod stream;
pub mod transport;

/// Same-version static actor integration, deliberately excluded from the
/// default public Core SDK. This keeps config projection/transport access out
/// of the actor without adding an unmatched public AgentOs method.
#[cfg(feature = "actor-internals")]
#[doc(hidden)]
pub mod actor_internals {
    /// Worker shutdown owns the entire child, including failed VM leases.
    pub async fn shutdown_package_session(
        session: &crate::AgentOsPackageSession,
    ) -> Result<(), crate::ClientError> {
        session.shutdown_worker().await
    }

    /// Compare only the VM config document, not runtime kind or ConfigureVm
    /// fields. The actor fixes its runtime kind and separately classifies
    /// hosted filesystem/software fields and any fields absent from this projection.
    pub async fn vm_config_equivalent(
        vm: &crate::AgentOs,
        before: &crate::AgentOsConfig,
        after: &crate::AgentOsConfig,
        before_restart_identity: Vec<String>,
        after_restart_identity: Vec<String>,
    ) -> Result<bool, crate::ClientError> {
        vm.vm_config_equivalent(
            before,
            after,
            before_restart_identity,
            after_restart_identity,
        )
        .await
    }
}

/// Same-version sidecar bridge while the resolver still lives in this crate.
/// Removed when package acquisition/cache ownership moves out of Core.
#[cfg(feature = "sidecar-internals")]
#[doc(hidden)]
pub mod sidecar_internals {
    pub fn package_acquisition_timeout_ms(resolver: &crate::PackageResolver) -> u64 {
        resolver.acquisition_timeout_ms()
    }
}

// ---------------------------------------------------------------------------
// Centralized constants (ADR-001 §6 / spec.md §7)
// ---------------------------------------------------------------------------

/// Bounded exited-shell exit-code retention (for `wait_shell` after exit).
pub const CLOSED_SHELL_EXIT_CODE_RETENTION_LIMIT: usize = 2048;

/// Two-phase shell-drain timeout during dispose (milliseconds).
pub const SHELL_DISPOSE_TIMEOUT_MS: u64 = 5_000;

/// VM lifecycle ready timeout during `create` (milliseconds).
pub const VM_READY_TIMEOUT_MS: u64 = 10_000;

/// Maximum scheduled cron jobs per VM.
pub const CRON_JOB_LIMIT: usize = 1024;

// ---------------------------------------------------------------------------
// Public re-exports
// ---------------------------------------------------------------------------

pub use agent_os::{AgentOs, PackageDescriptor, SoftwareInfo};
pub use error::{ClientError, ClientResult, ResourceLimitDetails};
pub use language_execution::{
    CodeEvaluationResult, CodeExecutionResult, ContextDescriptor, ExecutionPtyOptions,
    InlineExecutionOptions, JavaScriptExecutionOptions, JavaScriptModuleFormat,
    LanguageExecutionOptions, LanguageSpawnOptions, ProcessDescriptor, TypeScriptDiagnostic,
};
#[cfg(feature = "sidecar-internals")]
pub use sidecar::configure_shared_sidecar_package_cache;
#[cfg(any(feature = "actor-internals", feature = "sidecar-internals"))]
pub use sidecar::AgentOsPackageSession;
pub use sidecar::{
    AgentOsSidecar, AgentOsSidecarDescription, AgentOsSidecarPlacement, SidecarState,
};
pub use stream::{ByteStream, Subscription};

pub use config::{
    node_modules_mount, AgentOsConfig, AgentOsConfigBuilder, AgentOsLimits, AgentOsPackageLimits,
    AgentOsSidecarConfig, ExecutionLimits, FsPermissionRule, FsPermissions, HostFunction,
    HostFunctionCallback, HostFunctionCollection, HostFunctionCollections, HostFunctionLimits,
    HttpLimits, JsRuntimeLimits, MountConfig, MountPlugin, OverlayMountConfig, PackageRef,
    PatternPermissionRule, PatternPermissions, PermissionMode, Permissions, PluginLimits,
    PythonLimits, ResourceLimits, RootFilesystemConfig, RootFilesystemKind, RootFilesystemMode,
    RootLowerInput, RulePermissions, ScheduleCallback, ScheduleDriver, ScheduleEntry,
    ScheduleHandle, SidecarJsBridgeCall, SidecarJsBridgeCallback, SidecarSqliteCallback,
    SoftwareInput, SoftwareKind, TimerScheduleDriver, TlsLimits, VmGroupConfig,
    VmSqliteCallbackRequest, VmSqliteCallbackResponse, VmSqliteDescriptor, VmSqliteQueryResult,
    VmSqliteStatement, VmSqliteValue, VmUserAccountConfig, VmUserConfig, WasmLimits,
    VM_SQLITE_CALLBACK_NAMESPACE,
};

pub use process::{
    ExecOptions, ExecResult, ProcessExit, ProcessInfo, ProcessOutput, ProcessOutputEvent,
    ProcessOutputReplay, ProcessStatus, ProcessStream, ProcessTreeNode, SpawnHandle, SpawnOptions,
    SpawnStdio, SpawnedProcessInfo, StandaloneWasmBackend, StdinInput, TimingMitigation,
};

pub use net::{HttpRequest, HttpResponse, HttpStreamChunk, HttpStreamHead};

pub use software::{
    configure_process_package_cache, process_package_cache_stats, validate_package_source,
    InstalledSoftware, PackageManifestInfo, PackageResolver, PackageResolverOptions, PackageSource,
    ProcessPackageCacheOptions, ProcessPackageCacheStats, VerifiedPackage,
    DEFAULT_MAX_PACKAGE_BYTES,
};

pub use fs::{
    BatchReadResult, BatchWriteEntry, BatchWriteResult, DirEntry, DirEntryType,
    DynamicMountDescriptor, FileContent, FilesystemEntry, FilesystemEntryEncoding,
    FilesystemSnapshotEntries, FilesystemSnapshotExport, MkdirOptions, MountInfo,
    ReaddirRecursiveOptions, RemoveOptions, RootSnapshotExport, SnapshotExportKind,
    VirtualDirEntry, VirtualFileSystem, VirtualStat,
};

pub use shell::{
    ConnectTerminalOptions, OpenShellOptions, ShellData, ShellExit, ShellHandle, TerminalInfo,
    TerminalOutputEvent, TerminalSnapshot,
};

pub use cron::{
    CronAction, CronActionInfo, CronEvent, CronJobHandle, CronJobInfo, CronJobOptions, CronManager,
    CronOverlap,
};

// `shell` is declared here because its methods live in a sibling module to keep `lib.rs` re-exports
// flat; the module file itself is `shell.rs`.
pub mod shell;
