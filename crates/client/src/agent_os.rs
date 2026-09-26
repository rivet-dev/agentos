//! The `AgentOs` struct (all fields from ADR-001 §3), the `create` builder, and the `shutdown`
//! (dispose) teardown.
//!
//! `AgentOs` is `Arc`-cloneable; all interior state lives behind concurrent maps / atomics /
//! channels so `&self` methods never need an outer lock. Module files add only `impl AgentOs` blocks
//! and never introduce new struct fields.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use scc::HashMap as SccHashMap;
use serde::Deserialize;
use serde_json::{Map, Value};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use agentos_sidecar_client::{wire, TransportError};
use agentos_vm_config as vm_config;

use crate::config::{
    resolve_host_functions, AgentOsConfig, AgentOsLimits, MountConfig, ResolvedHostFunction,
    ResolvedHostFunctions, RootFilesystemConfig, RootFilesystemKind,
    RootFilesystemMode as ConfigRootFilesystemMode, RootLowerInput, SidecarJsBridgeCall,
    SidecarJsBridgeCallback, SidecarSqliteCallback, TimerScheduleDriver,
};
use crate::cron::CronManager;
use crate::error::ClientError;
use crate::process::SYNTHETIC_PID_BASE;
use crate::sidecar::{AgentOsSidecar, AgentOsSidecarPlacement, AgentOsSidecarVmLease};
use crate::transport::{SidecarProcess, WireSidecarCallback};

use once_cell::sync::OnceCell;

// ---------------------------------------------------------------------------
// Registry entries
// ---------------------------------------------------------------------------

/// An SDK-spawned process (TS `_processes` value). Keyed by user-facing pid.
pub(crate) struct ProcessEntry {
    pub command: String,
    pub args: Vec<String>,
    #[allow(dead_code)]
    pub stdout_tx: broadcast::Sender<Vec<u8>>,
    #[allow(dead_code)]
    pub stderr_tx: broadcast::Sender<Vec<u8>>,
    pub output_tx: broadcast::Sender<crate::process::ProcessOutput>,
    /// A failed observation is distinct from a confirmed guest exit.
    pub exit_tx: watch::Sender<crate::process::ProcessOutcome>,
    /// The sidecar-side process id used on the wire.
    pub process_id: String,
    /// The kernel pid returned by the `Execute` response, seeded once the spawn lands. The TS native
    /// path builds `displayPidByKernelPid` from this so `all_processes`/`process_tree` report the
    /// public spawn pid (the map key) for the spawned root, not the raw kernel pid.
    pub kernel_pid: watch::Sender<Option<u32>>,
    /// Handles for the per-process output-callback tasks seeded at spawn (`on_stdout`/`on_stderr`).
    /// The entry retains its own `stdout_tx`/`stderr_tx` clones for late subscribers, so these tasks
    /// never observe the broadcast `Closed`; `shutdown` aborts them when draining the registry.
    pub output_tasks: Vec<JoinHandle<()>>,
    /// Whether the sidecar owns a bounded output replay for this process.
    pub retain_output: bool,
    /// First-class language executions use the semantic execution replay route.
    pub execution_id: Option<String>,
    pub execution_generation: Option<u64>,
    /// Epoch milliseconds captured when `spawn` registered this process (TS `Date.now()`).
    pub started_at: i64,
}

/// A PTY-backed shell (TS `_shells` value). Keyed by synthetic `shell-N` id.
///
/// `data_tx` carries stdout and stderr in their original wire order for terminal renderers.
/// `stderr_tx` is an optional channel-specific diagnostic tap backing the `on_stderr` option and
/// `on_shell_stderr`; terminal consumers must not render both streams or stderr would be duplicated.
pub(crate) struct ShellEntry {
    pub pid: u32,
    pub data_tx: broadcast::Sender<Vec<u8>>,
    pub stderr_tx: broadcast::Sender<Vec<u8>>,
    pub event_tx: broadcast::Sender<crate::shell::TerminalOutputEvent>,
    /// The sidecar-side process id used on the wire.
    pub process_id: String,
    /// Spawn-readiness gate. Pending until the background `Execute` request is
    /// acked. TS `openShell` is fully synchronous so `writeShell` always addresses a live spawn; the
    /// Rust wire spawn is async, so `write_shell`/`close_shell` await this gate before issuing their
    /// wire request to preserve the deterministic ordering and avoid dropping early input.
    pub spawned_tx: watch::Sender<Option<Result<(), ClientError>>>,
    /// Exit-code channel backing `wait_shell` (TS `ShellHandle.wait`). Seeded `None`; the background
    /// event loop publishes a confirmed exit code or a typed observation failure.
    pub exit_tx: watch::Sender<Option<Result<i32, ClientError>>>,
}

#[derive(Clone)]
pub(crate) struct ClosedShellEntry {
    pub shell_id: String,
    pub process_id: String,
    pub pid: u32,
    pub result: Result<i32, ClientError>,
}

/// A connected terminal process and its output fan-out task.
pub(crate) struct TerminalEntry {
    pub exit_task: JoinHandle<()>,
}

// ---------------------------------------------------------------------------
// AgentOs
// ---------------------------------------------------------------------------

/// A self-contained agentOS package to link into a running VM via
/// [`AgentOs::link_software`]. `path` is normally the packed `.aospkg` file;
/// a directory is accepted only for local transition fixtures. The descriptor
/// is forwarded to the sidecar, which owns the `/opt/agentos` projection and
/// reads package metadata from the packed vbare manifest.
#[derive(Debug, Clone)]
pub struct PackageDescriptor {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SoftwareInfo {
    #[serde(rename = "packageName")]
    pub package_name: String,
    pub commands: Vec<String>,
}

/// The high-level client. Cheaply cloneable via `Arc`.
#[derive(Clone)]
pub struct AgentOs {
    inner: Arc<AgentOsInner>,
}

pub(crate) struct AgentOsInner {
    // Transport / connection / VM handle.
    pub(crate) transport: Arc<SidecarProcess>,
    pub(crate) connection_id: String,
    pub(crate) session_id: String,
    pub(crate) vm_id: String,
    /// Projected command names and guest entrypoints reported by the sidecar.
    pub(crate) projected_commands: parking_lot::Mutex<BTreeMap<String, String>>,
    /// Admits one client-originated package or mount mutation without queuing
    /// unbounded waiters. Concurrent callers receive a retryable error.
    pub(crate) vm_configuration_operation: tokio::sync::Mutex<()>,
    pub(crate) installed_software:
        parking_lot::Mutex<BTreeMap<String, crate::software::InstalledSoftware>>,

    // Process registries.
    pub(crate) process_registry_lock: parking_lot::Mutex<()>,
    /// Slots reserved by asynchronous language spawns that have reached client
    /// admission but have not published their [`ProcessEntry`] yet.
    pub(crate) pending_process_registrations: AtomicUsize,
    pub(crate) processes: SccHashMap<u32, ProcessEntry>,
    /// Wire `process_id` allocator for `exec` (the kernel-process view). Distinct from the
    /// spawn synthetic-pid space so an `exec` call never perturbs the observable `spawn` pid sequence
    /// (TS `nextSyntheticPid` is advanced only by `spawn`, never by `exec`).
    pub(crate) process_counter: AtomicU64,
    /// Synthetic display-pid allocator for `spawn` (TS `nextSyntheticPid`, seeded at
    /// [`crate::process::SYNTHETIC_PID_BASE`]). The first spawned process gets `SYNTHETIC_PID_BASE`.
    pub(crate) synthetic_pid_counter: AtomicU64,
    pub(crate) observed_process_time_lock: parking_lot::Mutex<()>,
    /// First-observed start time (epoch ms) per `"<process_id>:<kernel_pid>"`, mirroring TS
    /// `observedProcessStartTimes`. A process keeps the timestamp first seen in `all_processes` across
    /// later calls instead of advancing on every snapshot.
    pub(crate) observed_process_start_times: SccHashMap<String, f64>,
    /// First-observed exit time (epoch ms) per SDK-spawned wire `process_id`, mirroring TS
    /// `tracked.exitTime` (set once when the process is first seen exited).
    pub(crate) observed_process_exit_times: SccHashMap<String, f64>,

    // Shell registries.
    pub(crate) shells: SccHashMap<String, ShellEntry>,
    pub(crate) shell_counter: AtomicU64,
    pub(crate) pending_shell_exits: SccHashMap<u64, JoinHandle<()>>,
    /// Bounded ordered map (cap [`crate::CLOSED_SHELL_EXIT_CODE_RETENTION_LIMIT`]) of exited shells'
    /// exit codes, so `wait_shell` issued after the shell already exited (entry dropped from
    /// `shells`) still resolves with the recorded code — mirrors the TS `_closedShellIds` retention.
    pub(crate) closed_shells: parking_lot::Mutex<VecDeque<ClosedShellEntry>>,
    pub(crate) terminals: SccHashMap<String, TerminalEntry>,
    pub(crate) terminal_count: AtomicUsize,
    pub(crate) terminal_lifecycle_lock: tokio::sync::Mutex<()>,

    // Cron.
    pub(crate) cron: Arc<CronManager>,

    // Config / lifecycle.
    pub(crate) config: Arc<AgentOsConfig>,
    pub(crate) sidecar: Arc<AgentOsSidecar>,
    pub(crate) sidecar_lease: parking_lot::Mutex<Option<AgentOsSidecarVmLease>>,
    pub(crate) dynamic_mounts: parking_lot::Mutex<Vec<wire::MountDescriptor>>,
    pub(crate) disposed: AtomicBool,
    shutdown_result: tokio::sync::Mutex<Option<Result<(), ClientError>>>,
    vm_disposed: AtomicBool,
}

/// Owns VM creation across caller cancellation. Dropping `AgentOs::create` may
/// stop waiting, but it must not cancel a request after the sidecar may have
/// allocated a VM with no returned identity available to the caller.
struct AgentOsCreationTask {
    task: Option<tokio::task::JoinHandle<Result<AgentOs, ClientError>>>,
}

impl Drop for AgentOsCreationTask {
    fn drop(&mut self) {
        let Some(task) = self.task.take() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::error!(
                "VM creation waiter was dropped outside a Tokio runtime; process shutdown must reap the sidecar"
            );
            return;
        };
        runtime.spawn(async move {
            match task.await {
                Ok(Ok(vm)) => {
                    if let Err(error) = vm.shutdown().await {
                        tracing::error!(%error, "failed to dispose VM created after its caller was cancelled");
                    }
                }
                Ok(Err(error)) => {
                    tracing::debug!(%error, "cancelled VM creation finished with an error");
                }
                Err(error) => {
                    tracing::error!(%error, "cancelled VM creation task failed");
                }
            }
        });
    }
}

impl AgentOs {
    /// The sole public VM entry point. Processes software, spawns/authenticates the sidecar, creates
    /// the VM, waits for ready (10s), configures it, takes a lease, and constructs the cron manager
    /// (default [`crate::config::TimerScheduleDriver`]).
    pub async fn create(options: AgentOsConfig) -> Result<AgentOs, ClientError> {
        let mut creation = AgentOsCreationTask {
            task: Some(tokio::spawn(Self::create_owned(options))),
        };
        let result = creation
            .task
            .as_mut()
            .expect("creation task is present")
            .await;
        // No cancellation point exists between observing the joined result and
        // disarming the guard, so a successful VM has exactly one owner.
        creation.task.take();
        match result {
            Ok(result) => result,
            Err(error) => Err(ClientError::Sidecar(format!(
                "VM creation task failed: {error}"
            ))),
        }
    }

    async fn create_owned(options: AgentOsConfig) -> Result<AgentOs, ClientError> {
        let config = Arc::new(options);

        // 1. Resolve the sidecar handle (shared "default" pool unless configured otherwise) and
        //    establish/reuse its shared process + authenticated connection. A shared sidecar hosts
        //    multiple VMs in one process, each opening its own session + VM below.
        let sidecar = match &config.sidecar {
            Some(crate::config::AgentOsSidecarConfig::Explicit { handle }) => handle.clone(),
            Some(crate::config::AgentOsSidecarConfig::Shared { pool }) => {
                AgentOs::get_shared_sidecar(pool.clone(), config.sidecar_binary_path.clone())
                    .await?
            }
            None => AgentOs::get_shared_sidecar(None, config.sidecar_binary_path.clone()).await?,
        };
        let mut lease = Some(sidecar.acquire_vm_lease().await?);
        let mut created_vm = None;
        let mut created_session_key = None;
        let result = async {
            let (transport, connection_id, _) = sidecar.ensure_connection().await?;

            // 2. Open a session for this VM (connection scope) on the shared connection.
            let session = match transport
                .request_wire(
                    wire_connection_ownership(&connection_id),
                    wire::RequestPayload::OpenSessionRequest(wire::OpenSessionRequest {
                        placement: sidecar_wire_placement(&sidecar),
                        metadata: HashMap::new(),
                    }),
                )
                .await?
            {
                wire::ResponsePayload::SessionOpenedResponse(opened) => opened,
                wire::ResponsePayload::RejectedResponse(rejected) => {
                    return Err(rejected_to_error(rejected));
                }
                wire::ResponsePayload::AuthenticatedResponse(_)
                | wire::ResponsePayload::VmCreatedResponse(_)
                | wire::ResponsePayload::VmDisposedResponse(_)
                | wire::ResponsePayload::VmConfigComparedResponse(_)
                | wire::ResponsePayload::RootFilesystemBootstrappedResponse(_)
                | wire::ResponsePayload::VmConfiguredResponse(_)
                | wire::ResponsePayload::HostCallbacksRegisteredResponse(_)
                | wire::ResponsePayload::LayerCreatedResponse(_)
                | wire::ResponsePayload::LayerSealedResponse(_)
                | wire::ResponsePayload::SnapshotImportedResponse(_)
                | wire::ResponsePayload::SnapshotExportedResponse(_)
                | wire::ResponsePayload::OverlayCreatedResponse(_)
                | wire::ResponsePayload::GuestFilesystemResultResponse(_)
                | wire::ResponsePayload::RootFilesystemSnapshotResponse(_)
                | wire::ResponsePayload::ProcessStartedResponse(_)
                | wire::ResponsePayload::StdinWrittenResponse(_)
                | wire::ResponsePayload::PtyResizedResponse(_)
                | wire::ResponsePayload::StdinClosedResponse(_)
                | wire::ResponsePayload::ProcessKilledResponse(_)
                | wire::ResponsePayload::ProcessSnapshotResponse(_)
                | wire::ResponsePayload::ListenerSnapshotResponse(_)
                | wire::ResponsePayload::BoundUdpSnapshotResponse(_)
                | wire::ResponsePayload::SignalStateResponse(_)
                | wire::ResponsePayload::ZombieTimerCountResponse(_)
                | wire::ResponsePayload::FilesystemResultResponse(_)
                | wire::ResponsePayload::PermissionDecisionResponse(_)
                | wire::ResponsePayload::PersistenceStateResponse(_)
                | wire::ResponsePayload::PersistenceFlushedResponse(_)
                | wire::ResponsePayload::VmFetchResponse(_)
                | wire::ResponsePayload::ExtEnvelope(_)
                | wire::ResponsePayload::GuestKernelResultResponse(_)
                | wire::ResponsePayload::ResourceSnapshotResponse(_)
                | wire::ResponsePayload::PackageLinkedResponse(_)
                | wire::ResponsePayload::PackageUnlinkedResponse(_)
                | wire::ResponsePayload::PackageAcquiredResponse(_)
                | wire::ResponsePayload::PackageInstalledResponse(_)
                | wire::ResponsePayload::PackageCacheStatsResponse(_)
                | wire::ResponsePayload::ProvidedCommandsResponse(_)
                | wire::ResponsePayload::ListMountsResponse(_)
                | wire::ResponsePayload::ExecutionAcceptedResponse(_)
                | wire::ResponsePayload::ExecutionCompletedResponse(_)
                | wire::ResponsePayload::ExecutionEvaluationResponse(_)
                | wire::ResponsePayload::TypeScriptCheckResponse(_)
                | wire::ResponsePayload::ExecutionDescriptorResponse(_)
                | wire::ResponsePayload::ExecutionListResponse(_)
                | wire::ResponsePayload::ExecutionDeletedResponse(_)
                | wire::ResponsePayload::ExecutionIoResponse(_)
                | wire::ResponsePayload::ExecutionOutputPageResponse(_)
                | wire::ResponsePayload::ProcessOutputPageResponse(_) => {
                    return Err(ClientError::Sidecar(
                        "unexpected open_session response".to_string(),
                    ));
                }
            };
            let session_id = session.session_id;
            created_session_key = Some(sidecar_session_key(&connection_id, &session_id));

            // 3. Subscribe to events BEFORE CreateVm so the `ready` lifecycle event cannot be missed.
            let mut events = transport.subscribe_wire_events();
            let create_vm_config = serialize_create_vm_config_for_sidecar(&config)?;
            if let Some(callback) = config.sidecar_js_bridge_callback.clone() {
                let _ = session_js_bridge_callbacks()
                    .insert(sidecar_session_key(&connection_id, &session_id), callback);
                transport.register_wire_callback("js_bridge_call", js_bridge_call_callback());
            }
            if let Some(callback) = config.sidecar_sqlite_callback.clone() {
                let _ = session_sqlite_callbacks()
                    .insert(sidecar_session_key(&connection_id, &session_id), callback);
                transport.register_wire_callback("ext", sqlite_callback_callback());
            }

            // 4. Create the VM (session scope).
            let vm = match transport
                .request_wire(
                    wire_session_ownership(&connection_id, &session_id),
                    wire::RequestPayload::CreateVmRequest(wire::CreateVmRequest {
                        runtime: wire::GuestRuntimeKind::JavaScript,
                        config: serde_json::to_string(&create_vm_config).map_err(|error| {
                            ClientError::Sidecar(format!(
                                "failed to serialize create VM config: {error}"
                            ))
                        })?,
                    }),
                )
                .await?
            {
                wire::ResponsePayload::VmCreatedResponse(created) => created,
                wire::ResponsePayload::RejectedResponse(rejected) => {
                    return Err(rejected_to_error(rejected));
                }
                wire::ResponsePayload::AuthenticatedResponse(_)
                | wire::ResponsePayload::SessionOpenedResponse(_)
                | wire::ResponsePayload::VmDisposedResponse(_)
                | wire::ResponsePayload::VmConfigComparedResponse(_)
                | wire::ResponsePayload::RootFilesystemBootstrappedResponse(_)
                | wire::ResponsePayload::VmConfiguredResponse(_)
                | wire::ResponsePayload::HostCallbacksRegisteredResponse(_)
                | wire::ResponsePayload::LayerCreatedResponse(_)
                | wire::ResponsePayload::LayerSealedResponse(_)
                | wire::ResponsePayload::SnapshotImportedResponse(_)
                | wire::ResponsePayload::SnapshotExportedResponse(_)
                | wire::ResponsePayload::OverlayCreatedResponse(_)
                | wire::ResponsePayload::GuestFilesystemResultResponse(_)
                | wire::ResponsePayload::RootFilesystemSnapshotResponse(_)
                | wire::ResponsePayload::ProcessStartedResponse(_)
                | wire::ResponsePayload::StdinWrittenResponse(_)
                | wire::ResponsePayload::PtyResizedResponse(_)
                | wire::ResponsePayload::StdinClosedResponse(_)
                | wire::ResponsePayload::ProcessKilledResponse(_)
                | wire::ResponsePayload::ProcessSnapshotResponse(_)
                | wire::ResponsePayload::ListenerSnapshotResponse(_)
                | wire::ResponsePayload::BoundUdpSnapshotResponse(_)
                | wire::ResponsePayload::SignalStateResponse(_)
                | wire::ResponsePayload::ZombieTimerCountResponse(_)
                | wire::ResponsePayload::FilesystemResultResponse(_)
                | wire::ResponsePayload::PermissionDecisionResponse(_)
                | wire::ResponsePayload::PersistenceStateResponse(_)
                | wire::ResponsePayload::PersistenceFlushedResponse(_)
                | wire::ResponsePayload::VmFetchResponse(_)
                | wire::ResponsePayload::ExtEnvelope(_)
                | wire::ResponsePayload::GuestKernelResultResponse(_)
                | wire::ResponsePayload::ResourceSnapshotResponse(_)
                | wire::ResponsePayload::PackageLinkedResponse(_)
                | wire::ResponsePayload::PackageUnlinkedResponse(_)
                | wire::ResponsePayload::PackageAcquiredResponse(_)
                | wire::ResponsePayload::PackageInstalledResponse(_)
                | wire::ResponsePayload::PackageCacheStatsResponse(_)
                | wire::ResponsePayload::ProvidedCommandsResponse(_)
                | wire::ResponsePayload::ListMountsResponse(_)
                | wire::ResponsePayload::ExecutionAcceptedResponse(_)
                | wire::ResponsePayload::ExecutionCompletedResponse(_)
                | wire::ResponsePayload::ExecutionEvaluationResponse(_)
                | wire::ResponsePayload::TypeScriptCheckResponse(_)
                | wire::ResponsePayload::ExecutionDescriptorResponse(_)
                | wire::ResponsePayload::ExecutionListResponse(_)
                | wire::ResponsePayload::ExecutionDeletedResponse(_)
                | wire::ResponsePayload::ExecutionIoResponse(_)
                | wire::ResponsePayload::ExecutionOutputPageResponse(_)
                | wire::ResponsePayload::ProcessOutputPageResponse(_) => {
                    return Err(ClientError::Sidecar(
                        "unexpected create_vm response".to_string(),
                    ));
                }
            };
            let vm_id = vm.vm_id;
            created_vm = Some((
                transport.clone(),
                connection_id.clone(),
                session_id.clone(),
                vm_id.clone(),
            ));

            // 5. Wait for the VM to reach `ready` (bounded by VM_READY_TIMEOUT_MS).
            wait_for_vm_ready(&mut events, &vm_id, crate::VM_READY_TIMEOUT_MS).await?;

            // Forward packages to the sidecar. The sidecar owns manifest parsing and
            // command discovery for the `/opt/agentos` projection.
            let packages = build_package_descriptors(&config);

            // Native plugin mounts configured on the client.
            let mounts = serialize_mounts(&config)?;
            let configured_mounts = mounts.clone();

            // 6. Configure the VM (vm scope). The sidecar owns the `/opt/agentos` package
            // projection: it builds the staging dir + registers the read-only host_dir
            // mount itself from the forwarded `packages`.
            let projected_commands = match transport
                .request_wire(
                    wire_vm_ownership(&connection_id, &session_id, &vm_id),
                    wire::RequestPayload::ConfigureVmRequest(wire::ConfigureVmRequest {
                        mounts,
                        // The legacy `software`/SoftwareDescriptor provisioning path is
                        // retired: all boot software is projected via `packages`.
                        software: Vec::new(),
                        // CreateVm already resolved the selected defaults profile
                        // and any explicit overrides. Preserve that sidecar-owned
                        // policy while configuring mounts and packages.
                        permissions: None,
                        // Client-side `moduleAccessCwd` was removed in favor of an
                        // explicit `nodeModulesMount(...)` entry in `mounts`; the
                        // agentos wire field is left unset.
                        module_access_cwd: None,
                        instructions: Vec::new(),
                        projected_modules: Vec::new(),
                        command_permissions: HashMap::new(),
                        loopback_exempt_ports: config.loopback_exempt_ports.clone(),
                        packages,
                        packages_mount_at: config.packages_mount_at.clone().unwrap_or_default(),
                        bootstrap_commands: Vec::new(),
                        host_function_shim_commands: Vec::new(),
                    }),
                )
                .await?
            {
                wire::ResponsePayload::VmConfiguredResponse(configured) => configured
                    .projected_commands
                    .into_iter()
                    .map(|command| (command.name, command.guest_path))
                    .collect(),
                wire::ResponsePayload::RejectedResponse(rejected) => {
                    return Err(rejected_to_error(rejected));
                }
                wire::ResponsePayload::AuthenticatedResponse(_)
                | wire::ResponsePayload::SessionOpenedResponse(_)
                | wire::ResponsePayload::VmCreatedResponse(_)
                | wire::ResponsePayload::VmDisposedResponse(_)
                | wire::ResponsePayload::VmConfigComparedResponse(_)
                | wire::ResponsePayload::RootFilesystemBootstrappedResponse(_)
                | wire::ResponsePayload::HostCallbacksRegisteredResponse(_)
                | wire::ResponsePayload::LayerCreatedResponse(_)
                | wire::ResponsePayload::LayerSealedResponse(_)
                | wire::ResponsePayload::SnapshotImportedResponse(_)
                | wire::ResponsePayload::SnapshotExportedResponse(_)
                | wire::ResponsePayload::OverlayCreatedResponse(_)
                | wire::ResponsePayload::GuestFilesystemResultResponse(_)
                | wire::ResponsePayload::RootFilesystemSnapshotResponse(_)
                | wire::ResponsePayload::ProcessStartedResponse(_)
                | wire::ResponsePayload::StdinWrittenResponse(_)
                | wire::ResponsePayload::PtyResizedResponse(_)
                | wire::ResponsePayload::StdinClosedResponse(_)
                | wire::ResponsePayload::ProcessKilledResponse(_)
                | wire::ResponsePayload::ProcessSnapshotResponse(_)
                | wire::ResponsePayload::ListenerSnapshotResponse(_)
                | wire::ResponsePayload::BoundUdpSnapshotResponse(_)
                | wire::ResponsePayload::SignalStateResponse(_)
                | wire::ResponsePayload::ZombieTimerCountResponse(_)
                | wire::ResponsePayload::FilesystemResultResponse(_)
                | wire::ResponsePayload::PermissionDecisionResponse(_)
                | wire::ResponsePayload::PersistenceStateResponse(_)
                | wire::ResponsePayload::PersistenceFlushedResponse(_)
                | wire::ResponsePayload::VmFetchResponse(_)
                | wire::ResponsePayload::ExtEnvelope(_)
                | wire::ResponsePayload::GuestKernelResultResponse(_)
                | wire::ResponsePayload::ResourceSnapshotResponse(_)
                | wire::ResponsePayload::PackageLinkedResponse(_)
                | wire::ResponsePayload::PackageUnlinkedResponse(_)
                | wire::ResponsePayload::PackageAcquiredResponse(_)
                | wire::ResponsePayload::PackageInstalledResponse(_)
                | wire::ResponsePayload::PackageCacheStatsResponse(_)
                | wire::ResponsePayload::ProvidedCommandsResponse(_)
                | wire::ResponsePayload::ListMountsResponse(_)
                | wire::ResponsePayload::ExecutionAcceptedResponse(_)
                | wire::ResponsePayload::ExecutionCompletedResponse(_)
                | wire::ResponsePayload::ExecutionEvaluationResponse(_)
                | wire::ResponsePayload::TypeScriptCheckResponse(_)
                | wire::ResponsePayload::ExecutionDescriptorResponse(_)
                | wire::ResponsePayload::ExecutionListResponse(_)
                | wire::ResponsePayload::ExecutionDeletedResponse(_)
                | wire::ResponsePayload::ExecutionIoResponse(_)
                | wire::ResponsePayload::ExecutionOutputPageResponse(_)
                | wire::ResponsePayload::ProcessOutputPageResponse(_) => {
                    return Err(ClientError::Sidecar(
                        "unexpected configure_vm response".to_string(),
                    ));
                }
            };

            // 6b. Register host binding kits (if any): forward each binding definition via `register_host_callbacks`,
            //     record the host execute callbacks in the per-VM registry, and install the shared
            //     host-callback that routes guest binding calls back to the host by VM.
            let resolved_host_functions = resolve_host_functions(&config.host_functions)
                .map_err(ClientError::InvalidConfig)?;
            if !resolved_host_functions.is_empty() {
                let mut host_function_map: HashMap<String, ResolvedHostFunction> = HashMap::new();
                for collection in &resolved_host_functions {
                    let mut bindings = HashMap::new();
                    for binding in &collection.functions {
                        bindings.insert(
                            binding.name.clone(),
                            wire::RegisteredHostCallbackDefinition {
                                description: binding.description.clone(),
                                input_schema: json_utf8(
                                    &binding.input_schema,
                                    "host callback input schema",
                                )?,
                                timeout_ms: binding.timeout_ms,
                                examples: Vec::new(),
                            },
                        );
                        host_function_map.insert(
                            format!("{}:{}", collection.name, binding.name),
                            binding.clone(),
                        );
                    }
                    match transport
                        .request_wire(
                            wire_vm_ownership(&connection_id, &session_id, &vm_id),
                            wire::RequestPayload::RegisterHostCallbacksRequest(
                                wire::RegisterHostCallbacksRequest {
                                    name: collection.name.clone(),
                                    description: String::new(),
                                    command_aliases: vec![format!("agentos-{}", collection.name)],
                                    registry_command_aliases: vec![String::from("agentos")],
                                    callbacks: bindings,
                                },
                            ),
                        )
                        .await?
                    {
                        wire::ResponsePayload::HostCallbacksRegisteredResponse(_) => {}
                        wire::ResponsePayload::RejectedResponse(rejected) => {
                            return Err(rejected_to_error(rejected));
                        }
                        wire::ResponsePayload::AuthenticatedResponse(_)
                        | wire::ResponsePayload::SessionOpenedResponse(_)
                        | wire::ResponsePayload::VmCreatedResponse(_)
                        | wire::ResponsePayload::VmDisposedResponse(_)
                        | wire::ResponsePayload::VmConfigComparedResponse(_)
                        | wire::ResponsePayload::RootFilesystemBootstrappedResponse(_)
                        | wire::ResponsePayload::VmConfiguredResponse(_)
                        | wire::ResponsePayload::LayerCreatedResponse(_)
                        | wire::ResponsePayload::LayerSealedResponse(_)
                        | wire::ResponsePayload::SnapshotImportedResponse(_)
                        | wire::ResponsePayload::SnapshotExportedResponse(_)
                        | wire::ResponsePayload::OverlayCreatedResponse(_)
                        | wire::ResponsePayload::GuestFilesystemResultResponse(_)
                        | wire::ResponsePayload::RootFilesystemSnapshotResponse(_)
                        | wire::ResponsePayload::ProcessStartedResponse(_)
                        | wire::ResponsePayload::StdinWrittenResponse(_)
                        | wire::ResponsePayload::PtyResizedResponse(_)
                        | wire::ResponsePayload::StdinClosedResponse(_)
                        | wire::ResponsePayload::ProcessKilledResponse(_)
                        | wire::ResponsePayload::ProcessSnapshotResponse(_)
                        | wire::ResponsePayload::ListenerSnapshotResponse(_)
                        | wire::ResponsePayload::BoundUdpSnapshotResponse(_)
                        | wire::ResponsePayload::SignalStateResponse(_)
                        | wire::ResponsePayload::ZombieTimerCountResponse(_)
                        | wire::ResponsePayload::FilesystemResultResponse(_)
                        | wire::ResponsePayload::PermissionDecisionResponse(_)
                        | wire::ResponsePayload::PersistenceStateResponse(_)
                        | wire::ResponsePayload::PersistenceFlushedResponse(_)
                        | wire::ResponsePayload::VmFetchResponse(_)
                        | wire::ResponsePayload::ExtEnvelope(_)
                        | wire::ResponsePayload::GuestKernelResultResponse(_)
                        | wire::ResponsePayload::ResourceSnapshotResponse(_)
                        | wire::ResponsePayload::PackageLinkedResponse(_)
                        | wire::ResponsePayload::PackageUnlinkedResponse(_)
                        | wire::ResponsePayload::PackageAcquiredResponse(_)
                        | wire::ResponsePayload::PackageInstalledResponse(_)
                        | wire::ResponsePayload::PackageCacheStatsResponse(_)
                        | wire::ResponsePayload::ProvidedCommandsResponse(_)
                        | wire::ResponsePayload::ListMountsResponse(_)
                        | wire::ResponsePayload::ExecutionAcceptedResponse(_)
                        | wire::ResponsePayload::ExecutionCompletedResponse(_)
                        | wire::ResponsePayload::ExecutionEvaluationResponse(_)
                        | wire::ResponsePayload::TypeScriptCheckResponse(_)
                        | wire::ResponsePayload::ExecutionDescriptorResponse(_)
                        | wire::ResponsePayload::ExecutionListResponse(_)
                        | wire::ResponsePayload::ExecutionDeletedResponse(_)
                        | wire::ResponsePayload::ExecutionIoResponse(_)
                        | wire::ResponsePayload::ExecutionOutputPageResponse(_)
                        | wire::ResponsePayload::ProcessOutputPageResponse(_) => {
                            return Err(ClientError::Sidecar(
                                "unexpected register_host_callbacks response".to_string(),
                            ));
                        }
                    }
                }
                let _ = vm_host_functions().insert(
                    vm_id.clone(),
                    Arc::new(VmHostFunctionRegistry {
                        host_functions: resolved_host_functions.clone(),
                        host_function_map,
                    }),
                );
                transport.register_wire_callback("host_callback", host_callback_callback());
            }

            // 7. Lease this VM on the (possibly shared) sidecar, build cron, and assemble the client.
            let driver = config
                .schedule_driver
                .clone()
                .unwrap_or_else(|| Arc::new(TimerScheduleDriver::new()));
            let cron = Arc::new(CronManager::new(driver));

            let inner = AgentOsInner {
                transport,
                connection_id,
                session_id,
                vm_id,
                projected_commands: parking_lot::Mutex::new(projected_commands),
                vm_configuration_operation: tokio::sync::Mutex::new(()),
                installed_software: parking_lot::Mutex::new(BTreeMap::new()),
                process_registry_lock: parking_lot::Mutex::new(()),
                pending_process_registrations: AtomicUsize::new(0),
                processes: SccHashMap::new(),
                process_counter: AtomicU64::new(1),
                synthetic_pid_counter: AtomicU64::new(SYNTHETIC_PID_BASE),
                observed_process_time_lock: parking_lot::Mutex::new(()),
                observed_process_start_times: SccHashMap::new(),
                observed_process_exit_times: SccHashMap::new(),
                shells: SccHashMap::new(),
                shell_counter: AtomicU64::new(0),
                pending_shell_exits: SccHashMap::new(),
                closed_shells: parking_lot::Mutex::new(VecDeque::new()),
                terminals: SccHashMap::new(),
                terminal_count: AtomicUsize::new(0),
                terminal_lifecycle_lock: tokio::sync::Mutex::new(()),
                cron,
                config,
                sidecar: sidecar.clone(),
                sidecar_lease: parking_lot::Mutex::new(lease.take()),
                dynamic_mounts: parking_lot::Mutex::new(configured_mounts),
                disposed: AtomicBool::new(false),
                shutdown_result: tokio::sync::Mutex::new(None),
                vm_disposed: AtomicBool::new(false),
            };

            let client = AgentOs {
                inner: Arc::new(inner),
            };
            // Host bindings can read JSON arguments from the guest filesystem. Keep
            // a weak VM route for that trusted Core-only callback without exposing
            // any product-specific orchestration protocol.
            let _ = vm_clients().insert(client.inner.vm_id.clone(), Arc::downgrade(&client.inner));
            Ok(client)
        }
        .await;
        if result.is_err() {
            let mut cleanup_confirmed = true;
            if let Some((transport, connection_id, session_id, vm_id)) = created_vm {
                let cleanup = transport
                    .request_wire(
                        wire_vm_ownership(&connection_id, &session_id, &vm_id),
                        wire::RequestPayload::DisposeVmRequest(wire::DisposeVmRequest {
                            reason: wire::DisposeReason::Requested,
                        }),
                    )
                    .await
                    .map_err(ClientError::from)
                    .and_then(|response| vm_dispose_response(&vm_id, response));
                if let Err(error) = cleanup {
                    cleanup_confirmed = false;
                    tracing::error!(%vm_id, %error, "failed to dispose partially initialized VM; retaining sidecar ownership until explicit worker shutdown");
                }
                vm_host_functions().remove(&vm_id);
                vm_clients().remove(&vm_id);
            }
            if let Some(key) = created_session_key {
                session_js_bridge_callbacks().remove(&key);
                session_sqlite_callbacks().remove(&key);
            }
            if let Some(lease) = lease {
                if cleanup_confirmed {
                    lease.dispose().await?;
                } else {
                    lease.retain_until_worker_shutdown();
                }
            }
            if let Err(error) = sidecar.dispose_if_unused().await {
                tracing::error!(%error, "failed to stop sidecar after VM creation failed");
            }
        }
        result
    }

    /// Dispose the VM (= TS `dispose`). Teardown order:
    /// 1. cron dispose
    /// 2. kill all shells + snapshot pending exits
    /// 3. kill all connected terminals
    /// 4. drain tracked shell-exit tasks (two-phase, bounded by
    ///    [`crate::SHELL_DISPOSE_TIMEOUT_MS`])
    /// 5. release the lease (or tear down the transport)
    ///
    /// Idempotent (guarded by `disposed`).
    /// Dynamically link a software package into the RUNNING VM (parity with the
    /// TS client's `linkSoftware`). Forwarded to the sidecar, which owns the
    /// `/opt/agentos` projection and appends the package to its live staging dir,
    /// so the package's commands appear under `/opt/agentos/bin` (on `$PATH`)
    /// immediately with no reboot. Errors if a command name is already linked.
    pub async fn link_software(&self, descriptor: PackageDescriptor) -> Result<(), ClientError> {
        let vm_configuration = try_vm_mutation(&self.inner.vm_configuration_operation)?;
        let package_id = format!("path:{}", descriptor.path);
        self.link_software_path(&descriptor.path, &package_id, &vm_configuration)
            .await
    }

    /// Resolve, verify, pin, and project one exact software artifact inside the
    /// sidecar. The client sends only the source, never a resolved host path.
    pub async fn install_software(
        &self,
        source: crate::software::PackageSource,
    ) -> Result<crate::software::InstalledSoftware, ClientError> {
        let _vm_configuration = try_vm_mutation(&self.inner.vm_configuration_operation)?;
        let source = match source {
            crate::software::PackageSource::Url {
                url,
                expected_digest,
            } => wire::PackageAcquisitionSource::PackageUrlSource(wire::PackageUrlSource {
                url,
                expected_digest,
            }),
            crate::software::PackageSource::Path {
                path,
                expected_digest,
            } => wire::PackageAcquisitionSource::PackagePathSource(wire::PackagePathSource {
                path,
                expected_digest,
            }),
        };
        let options = self.inner.config.package_resolver.as_ref();
        let response = self
            .transport()
            .request_wire(
                wire_vm_ownership(
                    &self.inner.connection_id,
                    &self.inner.session_id,
                    &self.inner.vm_id,
                ),
                wire::RequestPayload::InstallPackageRequest(wire::InstallPackageRequest {
                    acquisition: wire::AcquirePackageRequest {
                        source,
                        advisory: false,
                        timeout_ms: None,
                        max_package_bytes: options.map(|options| options.max_package_bytes),
                        download_timeout_ms: options.map(|options| options.download_timeout_ms),
                        connect_timeout_ms: options.map(|options| options.connect_timeout_ms),
                        max_redirects: options.map(|options| options.max_redirects as u32),
                        allow_insecure_local_http: options
                            .is_some_and(|options| options.allow_insecure_local_http),
                    },
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::PackageInstalledResponse(installed) => {
                let package = installed.package;
                let info = crate::software::InstalledSoftware {
                    package_id: package.package_id,
                    digest: package.digest,
                    size_bytes: package.size,
                    package_name: package.package_name,
                    version: package.version,
                    commands: package.commands,
                };
                let mut projected = self.inner.projected_commands.lock();
                for command in installed.projected_commands {
                    projected.insert(command.name, command.guest_path);
                }
                self.inner
                    .installed_software
                    .lock()
                    .insert(info.package_id.clone(), info.clone());
                Ok(info)
            }
            wire::ResponsePayload::RejectedResponse(rejected) => {
                Err(ClientError::from_rejection(rejected))
            }
            other => Err(ClientError::Sidecar(format!(
                "unexpected install_package response: {other:?}"
            ))),
        }
    }

    /// Remove one exact package identity from the live VM. Missing ids are a
    /// typed error so actor desired state cannot silently drift from Core.
    pub async fn uninstall_software(
        &self,
        package_id: &str,
    ) -> Result<crate::software::InstalledSoftware, ClientError> {
        let _vm_configuration = try_vm_mutation(&self.inner.vm_configuration_operation)?;
        let installed = self
            .inner
            .installed_software
            .lock()
            .get(package_id)
            .cloned()
            .ok_or_else(|| ClientError::SoftwareNotFound(package_id.to_owned()))?;
        let inner = self.inner();
        let response = self
            .transport()
            .request_wire(
                wire_vm_ownership(&inner.connection_id, &inner.session_id, &inner.vm_id),
                wire::RequestPayload::UnlinkPackageRequest(wire::UnlinkPackageRequest {
                    package_id: package_id.to_owned(),
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::PackageUnlinkedResponse(unlinked) => {
                let mut commands = inner.projected_commands.lock();
                for command in unlinked.removed_commands {
                    commands.remove(&command);
                }
                inner.installed_software.lock().remove(package_id);
                Ok(installed)
            }
            wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
            other => Err(ClientError::Sidecar(format!(
                "unexpected unlink_package response: {other:?}"
            ))),
        }
    }

    /// Content-addressed packages installed through [`Self::install_software`].
    /// The legacy package-name view remains available through
    /// [`Self::list_software`] for trusted path configuration.
    pub fn installed_software(&self) -> Vec<crate::software::InstalledSoftware> {
        self.inner
            .installed_software
            .lock()
            .values()
            .cloned()
            .collect()
    }

    async fn link_software_path(
        &self,
        path: &str,
        package_id: &str,
        _vm_configuration: &tokio::sync::MutexGuard<'_, ()>,
    ) -> Result<(), ClientError> {
        let inner = self.inner();
        let response = self
            .transport()
            .request_wire(
                wire_vm_ownership(&inner.connection_id, &inner.session_id, &inner.vm_id),
                wire::RequestPayload::LinkPackageRequest(wire::LinkPackageRequest {
                    // The wire `PackageDescriptor` carries the packed package
                    // `path`; the sidecar reads metadata from that payload.
                    package: wire::PackageDescriptor {
                        path: path.to_owned(),
                    },
                    package_id: package_id.to_owned(),
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::PackageLinkedResponse(linked) => {
                let mut guard = inner.projected_commands.lock();
                for command in linked.projected_commands {
                    guard.insert(command.name, command.guest_path);
                }
                Ok(())
            }
            wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
            other => Err(ClientError::Sidecar(format!(
                "unexpected link_package response: {other:?}"
            ))),
        }
    }

    pub async fn list_software(&self) -> Result<Vec<SoftwareInfo>, ClientError> {
        let inner = self.inner();
        let response = self
            .transport()
            .request_wire(
                wire_vm_ownership(&inner.connection_id, &inner.session_id, &inner.vm_id),
                wire::RequestPayload::ProvidedCommandsRequest,
            )
            .await?;
        match response {
            wire::ResponsePayload::ProvidedCommandsResponse(provided) => Ok(provided
                .packages
                .into_iter()
                .map(|package| SoftwareInfo {
                    package_name: package.package_name,
                    commands: package.commands,
                })
                .collect()),
            wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
            other => Err(ClientError::Sidecar(format!(
                "unexpected list_software response: {other:?}"
            ))),
        }
    }

    pub async fn shutdown(&self) -> Result<(), ClientError> {
        // Serialize callers and retain only completed outcomes. Cancellation
        // must allow retry, not turn an unfinished DisposeVm into success.
        let mut shutdown_result = self.inner.shutdown_result.lock().await;
        if let Some(result) = shutdown_result.as_ref() {
            return result.clone();
        }
        self.inner.disposed.store(true, Ordering::SeqCst);

        // The `/opt/agentos` projection staging dir is owned + cleaned up by the
        // sidecar on VM dispose, so the client no longer removes it here.

        // 1. Cron dispose (cancel armed timers + tear down the driver).
        self.inner.cron.dispose();

        // Drain the SDK-spawned process registry. Per-process output tasks await
        // a broadcast `Closed` that retained sender clones otherwise prevent.
        crate::process::drain_process_output_tasks(&self.inner.processes);

        // 2-5. Best-effort drain tracked shell and terminal tasks before the VM is disposed, bounded
        //      by SHELL_DISPOSE_TIMEOUT_MS so late output cannot race a closed transport.
        let mut exit_tasks = Vec::new();
        self.inner.pending_shell_exits.retain(|_, task| {
            exit_tasks.push(std::mem::replace(task, tokio::spawn(async {})));
            false
        });

        {
            let _terminal_lifecycle_guard = self.inner.terminal_lifecycle_lock.lock().await;
            let mut terminal_entries = Vec::new();
            self.inner.terminals.retain(|process_id, entry| {
                terminal_entries.push((
                    process_id.clone(),
                    std::mem::replace(&mut entry.exit_task, tokio::spawn(async {})),
                ));
                false
            });
            self.inner.terminal_count.store(0, Ordering::SeqCst);
            for (process_id, _) in &terminal_entries {
                let transport = self.transport().clone();
                let ownership = wire::OwnershipScope::VmOwnership(wire::VmOwnership {
                    connection_id: self.inner.connection_id.clone(),
                    session_id: self.inner.session_id.clone(),
                    vm_id: self.inner.vm_id.clone(),
                });
                let process_id = process_id.clone();
                exit_tasks.push(tokio::spawn(async move {
                    let _ = transport
                        .request_wire(
                            ownership,
                            wire::RequestPayload::KillProcessRequest(wire::KillProcessRequest {
                                process_id,
                                signal: String::from("SIGTERM"),
                            }),
                        )
                        .await;
                }));
            }
            for (_, task) in terminal_entries {
                exit_tasks.push(task);
            }
        }

        if !exit_tasks.is_empty() {
            let mut drain_tasks = exit_tasks;
            if tokio::time::timeout(
                Duration::from_millis(crate::SHELL_DISPOSE_TIMEOUT_MS),
                futures::future::join_all(drain_tasks.iter_mut()),
            )
            .await
            .is_err()
            {
                for task in drain_tasks {
                    task.abort();
                }
            }
        }

        // 6-7. Release this VM and its lease. Preserve disposal failure while
        // completing secondary cleanup. The transport is shared across
        //      VMs on the same sidecar, so it is only torn down when this was the last VM (matching
        //      the TS lease/shared-sidecar lifecycle); otherwise sibling VMs keep using it.
        let mut disposal_result = if self.inner.vm_disposed.load(Ordering::SeqCst) {
            Ok(())
        } else {
            self.transport()
                .request_wire(
                    wire::OwnershipScope::VmOwnership(wire::VmOwnership {
                        connection_id: self.inner.connection_id.clone(),
                        session_id: self.inner.session_id.clone(),
                        vm_id: self.inner.vm_id.clone(),
                    }),
                    wire::RequestPayload::DisposeVmRequest(wire::DisposeVmRequest {
                        reason: wire::DisposeReason::Requested,
                    }),
                )
                .await
                .map_err(ClientError::from)
                .and_then(|response| vm_dispose_response(&self.inner.vm_id, response))
        };
        if disposal_result.is_ok() {
            self.inner.vm_disposed.store(true, Ordering::SeqCst);
            // A failed DisposeVm may need host SQLite callbacks on retry.
            // Unregister routes only after the sidecar confirms VM disposal.
            let _ = vm_host_functions().remove(&self.inner.vm_id);
            let _ = vm_clients().remove(&self.inner.vm_id);
            let _ = session_js_bridge_callbacks().remove(&sidecar_session_key(
                &self.inner.connection_id,
                &self.inner.session_id,
            ));
            let _ = session_sqlite_callbacks().remove(&sidecar_session_key(
                &self.inner.connection_id,
                &self.inner.session_id,
            ));
        }
        let sidecar = self.inner.sidecar.clone();
        let lease = if disposal_result.is_ok() {
            self.inner.sidecar_lease.lock().take()
        } else {
            None
        };
        if let Some(lease) = lease {
            retain_shutdown_failure(&mut disposal_result, lease.dispose().await);
        }
        retain_shutdown_failure(&mut disposal_result, sidecar.dispose_if_unused().await);
        if disposal_result.is_ok() {
            *shutdown_result = Some(Ok(()));
        }
        disposal_result
    }

    // --- internal accessors used by sibling impl blocks ---

    pub(crate) fn inner(&self) -> &AgentOsInner {
        &self.inner
    }

    pub(crate) fn transport(&self) -> &Arc<SidecarProcess> {
        &self.inner.transport
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.inner.connection_id
    }

    pub(crate) fn wire_session_id(&self) -> &str {
        &self.inner.session_id
    }

    pub(crate) fn vm_id(&self) -> &str {
        &self.inner.vm_id
    }

    pub(crate) fn cron(&self) -> &Arc<CronManager> {
        &self.inner.cron
    }

    /// The (possibly shared) sidecar handle backing this VM. Public for parity with TS
    /// `AgentOs.sidecar` (e.g. `describe()` reports `active_vm_count` across VMs sharing a pool).
    pub fn sidecar(&self) -> Arc<AgentOsSidecar> {
        self.inner.sidecar.clone()
    }

    #[cfg(feature = "actor-internals")]
    pub(crate) async fn vm_config_equivalent(
        &self,
        before: &AgentOsConfig,
        after: &AgentOsConfig,
        before_restart_identity: Vec<String>,
        after_restart_identity: Vec<String>,
    ) -> Result<bool, ClientError> {
        let before_mounts = serialize_mounts(before)?;
        let after_mounts = serialize_mounts(after)?;
        let before = serialize_create_vm_config_for_sidecar(before)?;
        let after = serialize_create_vm_config_for_sidecar(after)?;
        let response = self
            .transport()
            .request_wire(
                wire_session_ownership(self.connection_id(), self.wire_session_id()),
                wire::RequestPayload::CompareVmConfigRequest(wire::CompareVmConfigRequest {
                    before: serde_json::to_string(&before).map_err(|error| {
                        ClientError::Sidecar(format!("serialize before VM config: {error}"))
                    })?,
                    after: serde_json::to_string(&after).map_err(|error| {
                        ClientError::Sidecar(format!("serialize after VM config: {error}"))
                    })?,
                    before_mounts,
                    after_mounts,
                    before_restart_identity,
                    after_restart_identity,
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::VmConfigComparedResponse(result) => Ok(result.equivalent),
            wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
            _ => Err(ClientError::Sidecar(String::from(
                "unexpected compare VM config response",
            ))),
        }
    }
}

/// Convert a sidecar's client-side placement into the wire `SidecarPlacement` for OpenSession.
pub(crate) fn sidecar_wire_placement(sidecar: &AgentOsSidecar) -> wire::SidecarPlacement {
    match &sidecar.placement {
        AgentOsSidecarPlacement::Shared { pool } => {
            wire::SidecarPlacement::SidecarPlacementShared(wire::SidecarPlacementShared {
                pool: pool.clone(),
            })
        }
        AgentOsSidecarPlacement::Explicit { sidecar_id } => {
            wire::SidecarPlacement::SidecarPlacementExplicit(wire::SidecarPlacementExplicit {
                sidecar_id: sidecar_id.clone(),
            })
        }
    }
}

fn wire_connection_ownership(connection_id: &str) -> wire::OwnershipScope {
    wire::OwnershipScope::ConnectionOwnership(wire::ConnectionOwnership {
        connection_id: connection_id.to_string(),
    })
}

fn wire_session_ownership(connection_id: &str, session_id: &str) -> wire::OwnershipScope {
    wire::OwnershipScope::SessionOwnership(wire::SessionOwnership {
        connection_id: connection_id.to_string(),
        session_id: session_id.to_string(),
    })
}

fn wire_vm_ownership(connection_id: &str, session_id: &str, vm_id: &str) -> wire::OwnershipScope {
    wire::OwnershipScope::VmOwnership(wire::VmOwnership {
        connection_id: connection_id.to_string(),
        session_id: session_id.to_string(),
        vm_id: vm_id.to_string(),
    })
}

fn serialize_create_vm_config_for_sidecar(
    config: &AgentOsConfig,
) -> Result<vm_config::CreateVmConfig, ClientError> {
    let (root_filesystem, native_root) =
        serialize_root_filesystem_config_for_sidecar(&config.root_filesystem)?;
    let mut create = vm_config::CreateVmConfig {
        defaults_profile: Some(
            config
                .defaults_profile
                .unwrap_or(vm_config::VmDefaultsProfile::AgentOs),
        ),
        wasm_backend: config.wasm_backend.map(|backend| match backend {
            crate::process::StandaloneWasmBackend::V8 => vm_config::StandaloneWasmBackend::V8,
            crate::process::StandaloneWasmBackend::Wasmtime => {
                vm_config::StandaloneWasmBackend::Wasmtime
            }
            crate::process::StandaloneWasmBackend::WasmtimeThreads => {
                vm_config::StandaloneWasmBackend::WasmtimeThreads
            }
        }),
        database: config.database.clone(),
        cwd: None,
        env: config.environment.clone(),
        user: config.user.clone(),
        root_filesystem,
        permissions: permissions_policy_config(config),
        limits: serialize_limits_config_for_sidecar(config.limits.as_ref())?,
        dns: None,
        native_root,
        listen: None,
        loopback_exempt_ports: config.loopback_exempt_ports.clone(),
        // 0.3: the Node builtin allow-list moved from ConfigureVmRequest to
        // VM creation. `None` => engine default allow-list; `Some([..])` =>
        // exactly those (`Some([])` denies all). Platform/module-resolution
        // keep their engine defaults (full Node emulation), matching prior
        // behavior where Agent OS only ever constrained the builtin allow-list.
        js_runtime: (config.allowed_node_builtins.is_some()
            || config.high_resolution_time.is_some())
        .then(|| vm_config::JsRuntimeConfig {
            platform: vm_config::JsRuntimePlatform::default(),
            module_resolution: vm_config::JsModuleResolution::default(),
            allowed_builtins: config.allowed_node_builtins.clone(),
            high_resolution_time: config.high_resolution_time,
        }),
        bootstrap_commands: None,
    };
    create
        .normalize()
        .map_err(|error| ClientError::Sidecar(format!("invalid VM config: {error}")))?;
    Ok(create)
}

pub(crate) fn validate_config(config: &AgentOsConfig) -> Result<(), ClientError> {
    if matches!(
        config.database,
        Some(vm_config::VmSqliteDescriptor::HostCallback { .. })
    ) && config.sidecar_sqlite_callback.is_none()
    {
        return Err(ClientError::Sidecar(String::from(
            "database type host_callback requires sidecar_sqlite_callback",
        )));
    }
    if let Some(options) = &config.package_resolver {
        options.validate()?;
    }
    let create = serialize_create_vm_config_for_sidecar(config)?;
    create
        .validate(wire::DEFAULT_MAX_FRAME_BYTES)
        .map_err(|error| ClientError::Sidecar(format!("invalid VM config: {error}")))?;
    serialize_mounts(config)?;
    Ok(())
}

fn serialize_root_filesystem_config_for_sidecar(
    config: &RootFilesystemConfig,
) -> Result<
    (
        vm_config::RootFilesystemConfig,
        Option<vm_config::NativeRootFilesystemConfig>,
    ),
    ClientError,
> {
    let mode = match config.mode.unwrap_or(ConfigRootFilesystemMode::Ephemeral) {
        ConfigRootFilesystemMode::Ephemeral => vm_config::RootFilesystemMode::Ephemeral,
        ConfigRootFilesystemMode::ReadOnly => vm_config::RootFilesystemMode::ReadOnly,
    };
    match config.kind {
        RootFilesystemKind::Overlay => {
            if config.native_plugin.is_some() {
                return Err(ClientError::Sidecar(
                    "rootFilesystem.nativePlugin requires type \"native\"".to_string(),
                ));
            }
            let lowers = config
                .lowers
                .iter()
                .map(serialize_root_lower_config_for_sidecar)
                .collect::<Result<Vec<_>, _>>()?;
            Ok((
                vm_config::RootFilesystemConfig {
                    mode,
                    disable_default_base_layer: config.disable_default_base_layer,
                    lowers,
                    bootstrap_entries: Vec::new(),
                },
                None,
            ))
        }
        RootFilesystemKind::Native => {
            if !config.lowers.is_empty() {
                return Err(ClientError::Sidecar(
                    "native root filesystems do not support rootFilesystem.lowers".to_string(),
                ));
            }
            let plugin = config.native_plugin.as_ref().ok_or_else(|| {
                ClientError::Sidecar(
                    "rootFilesystem.nativePlugin is required for type \"native\"".to_string(),
                )
            })?;
            Ok((
                vm_config::RootFilesystemConfig {
                    mode,
                    disable_default_base_layer: config.disable_default_base_layer,
                    lowers: Vec::new(),
                    bootstrap_entries: Vec::new(),
                },
                Some(vm_config::NativeRootFilesystemConfig {
                    plugin: vm_config::MountPluginDescriptor {
                        id: plugin.id.clone(),
                        config: plugin
                            .config
                            .clone()
                            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new())),
                    },
                    read_only: config.mode == Some(ConfigRootFilesystemMode::ReadOnly),
                }),
            ))
        }
    }
}

fn serialize_root_lower_config_for_sidecar(
    lower: &RootLowerInput,
) -> Result<vm_config::RootFilesystemLowerDescriptor, ClientError> {
    match lower {
        RootLowerInput::BundledBaseFilesystem => {
            Ok(vm_config::RootFilesystemLowerDescriptor::BundledBaseFilesystem)
        }
        RootLowerInput::SnapshotExport(snapshot) => {
            let entries = snapshot
                .source
                .filesystem
                .entries
                .iter()
                .map(serialize_filesystem_entry_config_for_sidecar)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(vm_config::RootFilesystemLowerDescriptor::Snapshot { entries })
        }
    }
}

fn serialize_filesystem_entry_config_for_sidecar(
    entry: &crate::fs::FilesystemEntry,
) -> Result<vm_config::RootFilesystemEntry, ClientError> {
    let mode = u32::from_str_radix(entry.mode.trim_start_matches("0o"), 8).map_err(|error| {
        ClientError::Sidecar(format!(
            "invalid root filesystem mode {} for {}: {error}",
            entry.mode, entry.path
        ))
    })?;
    let kind = match entry.entry_type {
        crate::fs::DirEntryType::File => vm_config::RootFilesystemEntryKind::File,
        crate::fs::DirEntryType::Directory => vm_config::RootFilesystemEntryKind::Directory,
        crate::fs::DirEntryType::Symlink => vm_config::RootFilesystemEntryKind::Symlink,
    };
    let encoding = entry.encoding.map(|encoding| match encoding {
        crate::fs::FilesystemEntryEncoding::Utf8 => vm_config::RootFilesystemEntryEncoding::Utf8,
        crate::fs::FilesystemEntryEncoding::Base64 => {
            vm_config::RootFilesystemEntryEncoding::Base64
        }
    });

    Ok(vm_config::RootFilesystemEntry {
        path: entry.path.clone(),
        kind,
        mode: Some(mode),
        uid: Some(entry.uid),
        gid: Some(entry.gid),
        content: entry.content.clone(),
        encoding,
        target: entry.target.clone(),
        executable: entry.entry_type == crate::fs::DirEntryType::File && (mode & 0o111) != 0,
    })
}

fn serialize_limits_config_for_sidecar(
    limits: Option<&AgentOsLimits>,
) -> Result<Option<vm_config::VmLimitsConfig>, ClientError> {
    let Some(limits) = limits else {
        return Ok(None);
    };
    let value = serde_json::to_value(limits).map_err(|error| {
        ClientError::Sidecar(format!("failed to serialize VM limits config: {error}"))
    })?;
    serde_json::from_value(value).map(Some).map_err(|error| {
        ClientError::Sidecar(format!("failed to encode VM limits config: {error}"))
    })
}

fn permissions_policy_config(config: &AgentOsConfig) -> Option<vm_config::PermissionsPolicy> {
    config
        .permissions
        .as_ref()
        .map(|permissions| vm_config::PermissionsPolicy {
            fs: permissions.fs.as_ref().map(serialize_fs_permissions_config),
            network: permissions
                .network
                .as_ref()
                .map(serialize_pattern_permissions_config),
            child_process: permissions
                .child_process
                .as_ref()
                .map(serialize_pattern_permissions_config),
            process: permissions
                .process
                .as_ref()
                .map(serialize_pattern_permissions_config),
            env: permissions
                .env
                .as_ref()
                .map(serialize_pattern_permissions_config),
            host_function: permissions
                .host_function
                .as_ref()
                .map(serialize_pattern_permissions_config),
        })
}

fn serialize_fs_permissions_config(
    permissions: &crate::config::FsPermissions,
) -> vm_config::FsPermissionScope {
    match permissions {
        crate::config::FsPermissions::Mode(mode) => {
            vm_config::FsPermissionScope::Mode(serialize_permission_mode_config(*mode))
        }
        crate::config::FsPermissions::Rules(rules) => {
            vm_config::FsPermissionScope::Rules(vm_config::FsPermissionRuleSet {
                default: rules.default.map(serialize_permission_mode_config),
                rules: rules
                    .rules
                    .iter()
                    .map(|rule| vm_config::FsPermissionRule {
                        mode: serialize_permission_mode_config(rule.mode),
                        operations: operation_wildcard_if_omitted(&rule.operations),
                        paths: resource_wildcard_if_omitted(&rule.paths),
                    })
                    .collect(),
            })
        }
    }
}

fn serialize_pattern_permissions_config(
    permissions: &crate::config::PatternPermissions,
) -> vm_config::PatternPermissionScope {
    match permissions {
        crate::config::PatternPermissions::Mode(mode) => {
            vm_config::PatternPermissionScope::Mode(serialize_permission_mode_config(*mode))
        }
        crate::config::PatternPermissions::Rules(rules) => {
            vm_config::PatternPermissionScope::Rules(vm_config::PatternPermissionRuleSet {
                default: rules.default.map(serialize_permission_mode_config),
                rules: rules
                    .rules
                    .iter()
                    .map(|rule| vm_config::PatternPermissionRule {
                        mode: serialize_permission_mode_config(rule.mode),
                        operations: operation_wildcard_if_omitted(&rule.operations),
                        patterns: resource_wildcard_if_omitted(&rule.patterns),
                    })
                    .collect(),
            })
        }
    }
}

fn serialize_permission_mode_config(
    mode: crate::config::PermissionMode,
) -> vm_config::PermissionMode {
    match mode {
        crate::config::PermissionMode::Allow => vm_config::PermissionMode::Allow,
        crate::config::PermissionMode::Deny => vm_config::PermissionMode::Deny,
    }
}

/// Await the `ready` VM lifecycle event for `vm_id`, bounded by `timeout_ms`.
async fn wait_for_vm_ready(
    events: &mut broadcast::Receiver<(wire::OwnershipScope, wire::EventPayload)>,
    vm_id: &str,
    timeout_ms: u64,
) -> Result<(), ClientError> {
    let wait = async {
        loop {
            match events.recv().await {
                Ok((ownership, payload)) => match payload {
                    wire::EventPayload::VmLifecycleEvent(event) => {
                        if matches!(event.state, wire::VmLifecycleState::Ready)
                            && wire_ownership_vm_id(&ownership) == Some(vm_id)
                        {
                            return Ok(());
                        }
                    }
                    wire::EventPayload::ProcessOutputEvent(_)
                    | wire::EventPayload::ProcessExitedEvent(_)
                    | wire::EventPayload::ExecutionOutputEvent(_)
                    | wire::EventPayload::ExecutionCompletedEvent(_)
                    | wire::EventPayload::StructuredEvent(_)
                    | wire::EventPayload::ExtEnvelope(_) => {}
                },
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(ClientError::Sidecar(
                        "sidecar transport closed before the VM became ready".to_string(),
                    ));
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_millis(timeout_ms), wait)
        .await
        .map_err(|_| {
            ClientError::Sidecar("timed out waiting for the VM to become ready".to_string())
        })?
}

/// Process-global per-VM host_function registry. The shared transport's single host callback routes to
/// the right VM's host_functions by frame ownership.
static VM_HOST_FUNCTIONS: OnceCell<SccHashMap<String, Arc<VmHostFunctionRegistry>>> =
    OnceCell::new();

#[derive(Clone)]
struct VmHostFunctionRegistry {
    host_functions: Vec<ResolvedHostFunctions>,
    host_function_map: HashMap<String, ResolvedHostFunction>,
}

fn vm_host_functions() -> &'static SccHashMap<String, Arc<VmHostFunctionRegistry>> {
    VM_HOST_FUNCTIONS.get_or_init(SccHashMap::new)
}

/// Process-global map of VM id to client state used by trusted host bindings
/// that read JSON arguments from the guest filesystem. `Weak` prevents the
/// registry from extending VM lifetime.
static VM_CLIENTS: OnceCell<SccHashMap<String, Weak<AgentOsInner>>> = OnceCell::new();

fn vm_clients() -> &'static SccHashMap<String, Weak<AgentOsInner>> {
    VM_CLIENTS.get_or_init(SccHashMap::new)
}

/// Process-global map of sidecar session -> Rust-host js_bridge callback.
///
/// Native root plugins can issue callbacks while `CreateVm` is still in flight, before the client
/// knows the generated VM id. Session ownership is already known by then and stays stable for the VM.
static SESSION_JS_BRIDGE_CALLBACKS: OnceCell<SccHashMap<String, SidecarJsBridgeCallback>> =
    OnceCell::new();

fn session_js_bridge_callbacks() -> &'static SccHashMap<String, SidecarJsBridgeCallback> {
    SESSION_JS_BRIDGE_CALLBACKS.get_or_init(SccHashMap::new)
}

/// Process-global map of sidecar session to the trusted VM SQLite callback.
/// Registration happens before CreateVm because filesystem/Core migrations run
/// while the VM is being constructed.
static SESSION_SQLITE_CALLBACKS: OnceCell<SccHashMap<String, SidecarSqliteCallback>> =
    OnceCell::new();

fn session_sqlite_callbacks() -> &'static SccHashMap<String, SidecarSqliteCallback> {
    SESSION_SQLITE_CALLBACKS.get_or_init(SccHashMap::new)
}

fn sidecar_session_key(connection_id: &str, session_id: &str) -> String {
    format!("{connection_id}\0{session_id}")
}

fn wire_ownership_session_key(ownership: &wire::OwnershipScope) -> Option<String> {
    match ownership {
        wire::OwnershipScope::SessionOwnership(ownership) => Some(sidecar_session_key(
            &ownership.connection_id,
            &ownership.session_id,
        )),
        wire::OwnershipScope::VmOwnership(ownership) => Some(sidecar_session_key(
            &ownership.connection_id,
            &ownership.session_id,
        )),
        wire::OwnershipScope::ConnectionOwnership(_) => None,
    }
}

fn js_bridge_call_callback() -> WireSidecarCallback {
    Arc::new(|payload, ownership| {
        Box::pin(async move {
            let request = match payload {
                wire::SidecarRequestPayload::JsBridgeCallRequest(request) => request,
                wire::SidecarRequestPayload::HostCallbackRequest(_) => {
                    return Ok(wire::SidecarResponsePayload::JsBridgeResultResponse(
                        wire::JsBridgeResultResponse {
                            call_id: "unknown".to_string(),
                            result: None,
                            error: Some(
                                "js-bridge callback received a host callback request".to_string(),
                            ),
                        },
                    ));
                }
                wire::SidecarRequestPayload::ExtEnvelope(_) => {
                    return Ok(wire::SidecarResponsePayload::JsBridgeResultResponse(
                        wire::JsBridgeResultResponse {
                            call_id: "unknown".to_string(),
                            result: None,
                            error: Some(
                                "js-bridge callback received an extension request".to_string(),
                            ),
                        },
                    ));
                }
            };
            Ok(wire::SidecarResponsePayload::JsBridgeResultResponse(
                run_js_bridge_callback(&ownership, request).await,
            ))
        })
    })
}

fn sqlite_callback_callback() -> WireSidecarCallback {
    Arc::new(|payload, ownership| {
        Box::pin(async move {
            let envelope = match payload {
                wire::SidecarRequestPayload::ExtEnvelope(envelope) => envelope,
                _ => {
                    return Err(TransportError::Sidecar(String::from(
                        "SQLite callback received a non-extension request",
                    )))
                }
            };
            if envelope.namespace != vm_config::VM_SQLITE_CALLBACK_NAMESPACE {
                return Ok(wire::SidecarResponsePayload::ExtEnvelope(
                    wire::ExtEnvelope {
                        namespace: envelope.namespace,
                        payload: serde_json::to_vec(&vm_config::VmSqliteCallbackResponse::Error {
                            message: String::from("unsupported sidecar extension namespace"),
                        })
                        .expect("serialize SQLite extension error"),
                    },
                ));
            }
            let response = match serde_json::from_slice::<vm_config::VmSqliteCallbackRequest>(
                &envelope.payload,
            ) {
                Ok(request) => {
                    let callback = wire_ownership_session_key(&ownership).and_then(|key| {
                        session_sqlite_callbacks().read(&key, |_, callback| callback.clone())
                    });
                    match callback {
                        Some(callback) => match callback(request).await {
                            Ok(response) => response,
                            Err(message) => vm_config::VmSqliteCallbackResponse::Error { message },
                        },
                        None => vm_config::VmSqliteCallbackResponse::Error {
                            message: String::from(
                                "no SQLite callback registered for sidecar session",
                            ),
                        },
                    }
                }
                Err(error) => vm_config::VmSqliteCallbackResponse::Error {
                    message: format!("invalid SQLite callback request: {error}"),
                },
            };
            Ok(wire::SidecarResponsePayload::ExtEnvelope(
                wire::ExtEnvelope {
                    namespace: vm_config::VM_SQLITE_CALLBACK_NAMESPACE.to_owned(),
                    payload: serde_json::to_vec(&response).map_err(|error| {
                        TransportError::Sidecar(format!(
                            "serialize SQLite callback response: {error}"
                        ))
                    })?,
                },
            ))
        })
    })
}

async fn run_js_bridge_callback(
    ownership: &wire::OwnershipScope,
    request: wire::JsBridgeCallRequest,
) -> wire::JsBridgeResultResponse {
    let call_id = request.call_id;
    let args = match serde_json::from_str::<Value>(&request.args) {
        Ok(args) => args,
        Err(error) => {
            return wire::JsBridgeResultResponse {
                call_id,
                result: None,
                error: Some(format!("Invalid js_bridge args: {error}")),
            };
        }
    };
    let callback = wire_ownership_session_key(ownership)
        .and_then(|key| session_js_bridge_callbacks().read(&key, |_, callback| callback.clone()));
    let Some(callback) = callback else {
        return wire::JsBridgeResultResponse {
            call_id,
            result: None,
            error: Some("No js_bridge callback registered for sidecar session".to_string()),
        };
    };

    let call = SidecarJsBridgeCall {
        call_id: call_id.clone(),
        mount_id: request.mount_id,
        operation: request.operation,
        args,
    };
    match callback(call).await {
        Ok(result) => match result {
            Some(value) => match serde_json::to_string(&value) {
                Ok(result) => wire::JsBridgeResultResponse {
                    call_id,
                    result: Some(result),
                    error: None,
                },
                Err(error) => wire::JsBridgeResultResponse {
                    call_id,
                    result: None,
                    error: Some(format!("Invalid js_bridge result: {error}")),
                },
            },
            None => wire::JsBridgeResultResponse {
                call_id,
                result: None,
                error: None,
            },
        },
        Err(error) => wire::JsBridgeResultResponse {
            call_id,
            result: None,
            error: Some(error),
        },
    }
}

/// The transport callback that answers guest binding invocations by running the matching host binding.
fn host_callback_callback() -> WireSidecarCallback {
    Arc::new(|payload, ownership| {
        Box::pin(async move {
            let request = match payload {
                wire::SidecarRequestPayload::HostCallbackRequest(request) => request,
                wire::SidecarRequestPayload::JsBridgeCallRequest(_) => {
                    return Ok(wire::SidecarResponsePayload::HostCallbackResultResponse(
                        wire::HostCallbackResultResponse {
                            invocation_id: "unknown".to_string(),
                            result: None,
                            error: Some(
                                "host callback received a non-host-function request".to_string(),
                            ),
                        },
                    ));
                }
                wire::SidecarRequestPayload::ExtEnvelope(envelope) => {
                    return Ok(wire::SidecarResponsePayload::ExtEnvelope(
                        wire::ExtEnvelope {
                            namespace: envelope.namespace,
                            payload: b"host-callback received an extension request".to_vec(),
                        },
                    ));
                }
            };
            Ok(wire::SidecarResponsePayload::HostCallbackResultResponse(
                run_host_callback(&ownership, request).await,
            ))
        })
    })
}

/// Run one host-function invocation against the per-VM host-function registry, honoring the timeout. Mirrors
/// TS `handleHostCallback` (unknown function, timeout, and error shapes).
async fn run_host_callback(
    ownership: &wire::OwnershipScope,
    request: wire::HostCallbackRequest,
) -> wire::HostCallbackResultResponse {
    let input = match serde_json::from_str::<Value>(&request.input) {
        Ok(input) => input,
        Err(error) => {
            return wire::HostCallbackResultResponse {
                invocation_id: request.invocation_id,
                result: None,
                error: Some(format!("Invalid host callback input: {error}")),
            };
        }
    };
    let vm_id = wire_ownership_vm_id(ownership).unwrap_or("");
    let registry = vm_host_functions().read(vm_id, |_, registry| registry.clone());
    let Some(registry) = registry else {
        return wire::HostCallbackResultResponse {
            invocation_id: request.invocation_id,
            result: None,
            error: Some(format!(
                "Unknown host function \"{}\"",
                request.callback_key
            )),
        };
    };

    if let Some(command) = parse_host_command_callback_input(&input) {
        return match run_host_command_callback(ownership, registry.as_ref(), command).await {
            Ok(value) => match host_callback_json_result(value) {
                Ok(result) => wire::HostCallbackResultResponse {
                    invocation_id: request.invocation_id,
                    result: Some(result),
                    error: None,
                },
                Err(error) => wire::HostCallbackResultResponse {
                    invocation_id: request.invocation_id,
                    result: None,
                    error: Some(error),
                },
            },
            Err(error) => wire::HostCallbackResultResponse {
                invocation_id: request.invocation_id,
                result: None,
                error: Some(error),
            },
        };
    }

    let host_function = registry
        .host_function_map
        .get(&request.callback_key)
        .cloned();
    let Some(host_function) = host_function else {
        return wire::HostCallbackResultResponse {
            invocation_id: request.invocation_id,
            result: None,
            error: Some(format!(
                "Unknown host function \"{}\"",
                request.callback_key
            )),
        };
    };
    let timeout = Duration::from_millis(request.timeout_ms.max(1));
    match tokio::time::timeout(timeout, (host_function.execute)(input)).await {
        Ok(Ok(value)) => match host_callback_json_result(value) {
            Ok(result) => wire::HostCallbackResultResponse {
                invocation_id: request.invocation_id,
                result: Some(result),
                error: None,
            },
            Err(error) => wire::HostCallbackResultResponse {
                invocation_id: request.invocation_id,
                result: None,
                error: Some(error),
            },
        },
        Ok(Err(error)) => wire::HostCallbackResultResponse {
            invocation_id: request.invocation_id,
            result: None,
            error: Some(error),
        },
        Err(_) => wire::HostCallbackResultResponse {
            invocation_id: request.invocation_id,
            result: None,
            error: Some(format!(
                "Host function \"{}\" timed out after {}ms",
                request.callback_key, request.timeout_ms
            )),
        },
    }
}

#[derive(Debug, Deserialize)]
struct HostCommandCallbackInput {
    #[serde(rename = "type")]
    kind: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: String,
}

fn parse_host_command_callback_input(input: &Value) -> Option<HostCommandCallbackInput> {
    let command = serde_json::from_value::<HostCommandCallbackInput>(input.clone()).ok()?;
    if command.kind == "command" {
        Some(command)
    } else {
        None
    }
}

async fn run_host_command_callback(
    ownership: &wire::OwnershipScope,
    registry: &VmHostFunctionRegistry,
    command: HostCommandCallbackInput,
) -> Result<Value, String> {
    if command.command == "agentos" {
        return handle_agentos_registry_command(ownership, registry, &command).await;
    }
    let Some(collection) = registry
        .host_functions
        .iter()
        .find(|collection| format!("agentos-{}", collection.name) == command.command)
    else {
        return Err(format!(
            "Unknown host callback command \"{}\"",
            command.command
        ));
    };
    handle_agentos_host_function_command(ownership, registry, &command, collection).await
}

async fn handle_agentos_registry_command(
    ownership: &wire::OwnershipScope,
    registry: &VmHostFunctionRegistry,
    command: &HostCommandCallbackInput,
) -> Result<Value, String> {
    let Some(subcommand) = command.args.first() else {
        return Ok(json_object([(
            "usage",
            Value::String(String::from(
                "agentos <command>: list-host-functions [collection], <collection> --help, or <collection> <function> ...",
            )),
        )]));
    };
    if is_help_flag(subcommand) {
        return Ok(json_object([(
            "usage",
            Value::String(String::from(
                "agentos <command>: list-host-functions [collection], <collection> --help, or <collection> <function> ...",
            )),
        )]));
    }
    if subcommand == "list-host-functions" || subcommand == "list-bindings" {
        return match command.args.get(1) {
            Some(collection_name) => {
                describe_host_functions_payload(&registry.host_functions, collection_name)
            }
            None => Ok(list_host_functions_payload(&registry.host_functions)),
        };
    }

    let Some(collection) = registry
        .host_functions
        .iter()
        .find(|collection| collection.name == *subcommand)
    else {
        return Err(format!(
            "No host function collection \"{subcommand}\". Available: {}",
            host_functions_names(&registry.host_functions)
        ));
    };

    let Some(host_function_name) = command.args.get(1) else {
        return describe_host_functions_payload(&registry.host_functions, subcommand);
    };
    if is_help_flag(host_function_name) {
        return describe_host_functions_payload(&registry.host_functions, subcommand);
    }
    if command.args.get(2).is_some_and(|value| is_help_flag(value)) {
        return describe_host_function_payload(collection, host_function_name);
    }
    invoke_host_function(
        ownership,
        registry,
        collection,
        host_function_name,
        command.args.get(2..).unwrap_or_default(),
        &command.cwd,
    )
    .await
}

async fn handle_agentos_host_function_command(
    ownership: &wire::OwnershipScope,
    registry: &VmHostFunctionRegistry,
    command: &HostCommandCallbackInput,
    collection: &ResolvedHostFunctions,
) -> Result<Value, String> {
    let Some(host_function_name) = command.args.first() else {
        return describe_host_functions_payload(&registry.host_functions, &collection.name);
    };
    if is_help_flag(host_function_name) {
        return describe_host_functions_payload(&registry.host_functions, &collection.name);
    }
    if command.args.get(1).is_some_and(|value| is_help_flag(value)) {
        return describe_host_function_payload(collection, host_function_name);
    }
    invoke_host_function(
        ownership,
        registry,
        collection,
        host_function_name,
        command.args.get(1..).unwrap_or_default(),
        &command.cwd,
    )
    .await
}

async fn invoke_host_function(
    ownership: &wire::OwnershipScope,
    registry: &VmHostFunctionRegistry,
    collection: &ResolvedHostFunctions,
    host_function_name: &str,
    args: &[String],
    cwd: &str,
) -> Result<Value, String> {
    let callback_key = format!("{}:{host_function_name}", collection.name);
    let Some(host_function) = registry.host_function_map.get(&callback_key).cloned() else {
        return Err(format!(
            "No host function \"{host_function_name}\" in collection \"{}\". Available: {}",
            collection.name,
            host_function_names(collection)
        ));
    };

    // The sidecar checks the `hostFunction` permission scope before it forwards a
    // host function call here, for both the collection command and the registry command.

    let input = parse_host_function_input(ownership, &host_function, args, cwd).await?;
    validate_host_function_input(&host_function.input_schema, &input)
        .map_err(|error| error.to_string())?;

    let timeout = Duration::from_millis(host_function.timeout_ms.unwrap_or(30_000).max(1));
    match tokio::time::timeout(timeout, (host_function.execute)(input)).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(format!(
            "Host function \"{callback_key}\" timed out after {}ms",
            host_function.timeout_ms.unwrap_or(30_000)
        )),
    }
}

async fn parse_host_function_input(
    ownership: &wire::OwnershipScope,
    host_function: &ResolvedHostFunction,
    args: &[String],
    cwd: &str,
) -> Result<Value, String> {
    if args.first().is_some_and(|arg| arg == "--json") {
        let value = args
            .get(1)
            .ok_or_else(|| String::from("Flag --json requires a value"))?;
        return serde_json::from_str(value)
            .map_err(|error| format!("Invalid JSON for --json: {error}"));
    }

    if args.first().is_some_and(|arg| arg == "--json-file") {
        let path = args
            .get(1)
            .ok_or_else(|| String::from("Flag --json-file requires a value"))?;
        let guest_path = normalize_guest_path(if path.starts_with('/') {
            path.clone()
        } else {
            format!("{cwd}/{path}")
        });
        let vm_id = wire_ownership_vm_id(ownership).unwrap_or("");
        let inner = vm_clients()
            .read(vm_id, |_, weak| weak.clone())
            .and_then(|weak| weak.upgrade())
            .ok_or_else(|| String::from("Invalid JSON file: VM is no longer available"))?;
        let bytes = AgentOs { inner }
            .read_file(&guest_path)
            .await
            .map_err(|error| format!("Invalid JSON file: {error}"))?;
        let text =
            String::from_utf8(bytes).map_err(|error| format!("Invalid JSON file: {error}"))?;
        return serde_json::from_str(&text).map_err(|error| format!("Invalid JSON file: {error}"));
    }

    parse_host_function_argv(&host_function.input_schema, args)
}

fn host_callback_json_result(value: Value) -> Result<String, String> {
    serde_json::to_string(&value).map_err(|error| format!("Invalid host callback result: {error}"))
}

fn parse_host_function_argv(schema: &Value, argv: &[String]) -> Result<Value, String> {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();

    let mut flag_to_field = BTreeMap::new();
    for (field_name, field_schema) in &properties {
        flag_to_field.insert(
            camel_to_kebab(field_name),
            (field_name.clone(), field_schema.clone()),
        );
    }

    let mut input = Map::new();
    let mut index = 0;
    while index < argv.len() {
        let arg = &argv[index];
        if !arg.starts_with("--") {
            return Err(format!("Unexpected positional argument: \"{arg}\""));
        }

        let raw_flag = &arg[2..];
        let (flag_name, negated) = raw_flag
            .strip_prefix("no-")
            .map(|name| (name, true))
            .unwrap_or((raw_flag, false));
        let Some((field_name, field_schema)) = flag_to_field.get(flag_name) else {
            return Err(format!("Unknown flag: --{raw_flag}"));
        };
        let field_type = json_schema_type(field_schema);

        if negated {
            if field_type != Some("boolean") {
                return Err(format!("Unknown flag: --{raw_flag}"));
            }
            input.insert(field_name.clone(), Value::Bool(false));
            index += 1;
            continue;
        }

        match field_type {
            Some("boolean") => {
                input.insert(field_name.clone(), Value::Bool(true));
                index += 1;
            }
            Some("number") | Some("integer") => {
                let value = argv
                    .get(index + 1)
                    .ok_or_else(|| format!("Flag --{raw_flag} requires a value"))?;
                let number = value
                    .parse::<f64>()
                    .map_err(|_| format!("Flag --{raw_flag} expects a number, got \"{value}\""))?;
                let number = serde_json::Number::from_f64(number).ok_or_else(|| {
                    format!("Flag --{raw_flag} expects a finite number, got \"{value}\"")
                })?;
                input.insert(field_name.clone(), Value::Number(number));
                index += 2;
            }
            Some("array") => {
                let value = argv
                    .get(index + 1)
                    .ok_or_else(|| format!("Flag --{raw_flag} requires a value"))?;
                let item_type = field_schema.get("items").and_then(json_schema_type);
                let parsed_value = match item_type {
                    Some("number") | Some("integer") => {
                        let number = value.parse::<f64>().map_err(|_| {
                            format!("Flag --{raw_flag} expects a number value, got \"{value}\"")
                        })?;
                        let number = serde_json::Number::from_f64(number).ok_or_else(|| {
                            format!(
                                "Flag --{raw_flag} expects a finite number value, got \"{value}\""
                            )
                        })?;
                        Value::Number(number)
                    }
                    Some("boolean") => {
                        let boolean = value.parse::<bool>().map_err(|_| {
                            format!("Flag --{raw_flag} expects a boolean value, got \"{value}\"")
                        })?;
                        Value::Bool(boolean)
                    }
                    _ => Value::String(value.clone()),
                };
                input
                    .entry(field_name.clone())
                    .or_insert_with(|| Value::Array(Vec::new()))
                    .as_array_mut()
                    .expect("array field should always contain an array")
                    .push(parsed_value);
                index += 2;
            }
            _ => {
                let value = argv
                    .get(index + 1)
                    .ok_or_else(|| format!("Flag --{raw_flag} requires a value"))?;
                input.insert(field_name.clone(), Value::String(value.clone()));
                index += 2;
            }
        }
    }

    for field_name in required {
        if !input.contains_key(&field_name) {
            return Err(format!(
                "Missing required flag: --{}",
                camel_to_kebab(&field_name)
            ));
        }
    }

    Ok(Value::Object(input))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HostFunctionInputSchemaViolation {
    path: String,
    expected: String,
    actual: String,
}

impl HostFunctionInputSchemaViolation {
    fn new(
        path: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            expected: expected.into(),
            actual: actual.into(),
        }
    }
}

impl std::fmt::Display for HostFunctionInputSchemaViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "HostFunctionInputSchemaViolation at {}: expected {}, got {}",
            self.path, self.expected, self.actual
        )
    }
}

fn validate_host_function_input(
    schema: &Value,
    input: &Value,
) -> Result<(), HostFunctionInputSchemaViolation> {
    validate_host_function_input_at_path(schema, input, "$")
}

fn validate_host_function_input_at_path(
    schema: &Value,
    input: &Value,
    path: &str,
) -> Result<(), HostFunctionInputSchemaViolation> {
    if schema.is_null() || schema.as_object().is_some_and(|object| object.is_empty()) {
        return Ok(());
    }
    if let Some(branches) = schema.get("anyOf").and_then(Value::as_array) {
        return validate_schema_branches(branches, input, path, "anyOf");
    }
    if let Some(branches) = schema.get("oneOf").and_then(Value::as_array) {
        return validate_schema_branches(branches, input, path, "oneOf");
    }
    if let Some(enum_values) = schema.get("enum").and_then(Value::as_array) {
        if enum_values.iter().any(|candidate| candidate == input) {
            return Ok(());
        }
        return Err(HostFunctionInputSchemaViolation::new(
            path,
            format!(
                "one of {}",
                enum_values
                    .iter()
                    .map(compact_json)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            describe_value(input),
        ));
    }
    if let Some(expected) = schema.get("const") {
        if expected == input {
            return Ok(());
        }
        return Err(HostFunctionInputSchemaViolation::new(
            path,
            format!("constant {}", compact_json(expected)),
            describe_value(input),
        ));
    }

    match schema.get("type") {
        Some(Value::String(expected_type)) => {
            validate_typed_host_function_input(schema, input, path, expected_type)
        }
        Some(Value::Array(expected_types)) => {
            let mut first_error = None;
            for expected_type in expected_types.iter().filter_map(Value::as_str) {
                match validate_typed_host_function_input(schema, input, path, expected_type) {
                    Ok(()) => return Ok(()),
                    Err(error) if first_error.is_none() => first_error = Some(error),
                    Err(_) => {}
                }
            }
            Err(first_error.unwrap_or_else(|| {
                HostFunctionInputSchemaViolation::new(
                    path,
                    describe_expected(schema),
                    describe_value(input),
                )
            }))
        }
        Some(_) => Ok(()),
        None if has_object_keywords(schema) => {
            validate_typed_host_function_input(schema, input, path, "object")
        }
        None => Ok(()),
    }
}

fn validate_schema_branches(
    branches: &[Value],
    input: &Value,
    path: &str,
    keyword: &str,
) -> Result<(), HostFunctionInputSchemaViolation> {
    let mut first_error = None;
    for branch in branches {
        match validate_host_function_input_at_path(branch, input, path) {
            Ok(()) => return Ok(()),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    Err(first_error.unwrap_or_else(|| {
        HostFunctionInputSchemaViolation::new(
            path,
            format!(
                "{keyword} branch ({})",
                branches
                    .iter()
                    .map(describe_expected)
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
            describe_value(input),
        )
    }))
}

fn validate_typed_host_function_input(
    schema: &Value,
    input: &Value,
    path: &str,
    expected_type: &str,
) -> Result<(), HostFunctionInputSchemaViolation> {
    match expected_type {
        "null" if input.is_null() => Ok(()),
        "null" => Err(type_violation(path, expected_type, input)),
        "boolean" if input.is_boolean() => Ok(()),
        "boolean" => Err(type_violation(path, expected_type, input)),
        "string" => validate_string_host_function_input(schema, input, path),
        "number" => validate_number_host_function_input(schema, input, path, false),
        "integer" => validate_number_host_function_input(schema, input, path, true),
        "array" => validate_array_host_function_input(schema, input, path),
        "object" => validate_object_host_function_input(schema, input, path),
        _ => Ok(()),
    }
}

fn validate_string_host_function_input(
    schema: &Value,
    input: &Value,
    path: &str,
) -> Result<(), HostFunctionInputSchemaViolation> {
    let Some(value) = input.as_str() else {
        return Err(type_violation(path, "string", input));
    };
    if let Some(min_length) = schema.get("minLength").and_then(Value::as_u64) {
        if value.chars().count() < min_length as usize {
            return Err(HostFunctionInputSchemaViolation::new(
                path,
                format!("string with minLength {min_length}"),
                format!("string length {}", value.chars().count()),
            ));
        }
    }
    if let Some(max_length) = schema.get("maxLength").and_then(Value::as_u64) {
        if value.chars().count() > max_length as usize {
            return Err(HostFunctionInputSchemaViolation::new(
                path,
                format!("string with maxLength {max_length}"),
                format!("string length {}", value.chars().count()),
            ));
        }
    }
    Ok(())
}

fn validate_number_host_function_input(
    schema: &Value,
    input: &Value,
    path: &str,
    expect_integer: bool,
) -> Result<(), HostFunctionInputSchemaViolation> {
    let Some(number) = input.as_f64() else {
        return Err(type_violation(
            path,
            if expect_integer { "integer" } else { "number" },
            input,
        ));
    };
    if expect_integer && number.fract() != 0.0 {
        return Err(type_violation(path, "integer", input));
    }
    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
        if number < minimum {
            return Err(HostFunctionInputSchemaViolation::new(
                path,
                format!(
                    "{} >= {}",
                    if expect_integer { "integer" } else { "number" },
                    minimum
                ),
                compact_json(input),
            ));
        }
    }
    if let Some(minimum) = schema.get("exclusiveMinimum").and_then(Value::as_f64) {
        if number <= minimum {
            return Err(HostFunctionInputSchemaViolation::new(
                path,
                format!(
                    "{} > {}",
                    if expect_integer { "integer" } else { "number" },
                    minimum
                ),
                compact_json(input),
            ));
        }
    }
    if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64) {
        if number > maximum {
            return Err(HostFunctionInputSchemaViolation::new(
                path,
                format!(
                    "{} <= {}",
                    if expect_integer { "integer" } else { "number" },
                    maximum
                ),
                compact_json(input),
            ));
        }
    }
    if let Some(maximum) = schema.get("exclusiveMaximum").and_then(Value::as_f64) {
        if number >= maximum {
            return Err(HostFunctionInputSchemaViolation::new(
                path,
                format!(
                    "{} < {}",
                    if expect_integer { "integer" } else { "number" },
                    maximum
                ),
                compact_json(input),
            ));
        }
    }
    Ok(())
}

fn validate_array_host_function_input(
    schema: &Value,
    input: &Value,
    path: &str,
) -> Result<(), HostFunctionInputSchemaViolation> {
    let Some(items) = input.as_array() else {
        return Err(type_violation(path, "array", input));
    };
    if let Some(min_items) = schema.get("minItems").and_then(Value::as_u64) {
        if items.len() < min_items as usize {
            return Err(HostFunctionInputSchemaViolation::new(
                path,
                format!("array with minItems {min_items}"),
                format!("array length {}", items.len()),
            ));
        }
    }
    if let Some(max_items) = schema.get("maxItems").and_then(Value::as_u64) {
        if items.len() > max_items as usize {
            return Err(HostFunctionInputSchemaViolation::new(
                path,
                format!("array with maxItems {max_items}"),
                format!("array length {}", items.len()),
            ));
        }
    }
    if let Some(item_schema) = schema.get("items") {
        for (index, item) in items.iter().enumerate() {
            validate_host_function_input_at_path(item_schema, item, &format!("{path}[{index}]"))?;
        }
    }
    Ok(())
}

fn validate_object_host_function_input(
    schema: &Value,
    input: &Value,
    path: &str,
) -> Result<(), HostFunctionInputSchemaViolation> {
    let Some(object) = input.as_object() else {
        return Err(type_violation(path, "object", input));
    };
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for field in required.iter().filter_map(Value::as_str) {
        if !object.contains_key(field) {
            let field_path = format!("{path}.{field}");
            let expected = properties
                .get(field)
                .map(describe_expected)
                .unwrap_or_else(|| String::from("required value"));
            return Err(HostFunctionInputSchemaViolation::new(
                field_path,
                expected,
                "missing value",
            ));
        }
    }
    for (field, value) in object {
        let field_path = format!("{path}.{field}");
        if let Some(field_schema) = properties.get(field) {
            validate_host_function_input_at_path(field_schema, value, &field_path)?;
            continue;
        }
        match schema.get("additionalProperties") {
            Some(Value::Bool(false)) => {
                return Err(HostFunctionInputSchemaViolation::new(
                    field_path,
                    "no additional properties",
                    describe_value(value),
                ));
            }
            Some(additional_schema) => {
                validate_host_function_input_at_path(additional_schema, value, &field_path)?;
            }
            None => {}
        }
    }
    Ok(())
}

fn has_object_keywords(schema: &Value) -> bool {
    schema.get("properties").is_some()
        || schema.get("required").is_some()
        || schema.get("additionalProperties").is_some()
}

fn type_violation(path: &str, expected: &str, input: &Value) -> HostFunctionInputSchemaViolation {
    HostFunctionInputSchemaViolation::new(path, expected, describe_value(input))
}

fn describe_expected(schema: &Value) -> String {
    if let Some(enum_values) = schema.get("enum").and_then(Value::as_array) {
        return format!(
            "one of {}",
            enum_values
                .iter()
                .map(compact_json)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if let Some(expected) = schema.get("const") {
        return format!("constant {}", compact_json(expected));
    }
    match schema.get("type") {
        Some(Value::String(expected_type)) => expected_type.clone(),
        Some(Value::Array(expected_types)) => expected_types
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" | "),
        _ if has_object_keywords(schema) => String::from("object"),
        _ => String::from("value"),
    }
}

fn describe_value(value: &Value) -> String {
    match value {
        Value::Null => String::from("null"),
        Value::Bool(_) => String::from("boolean"),
        Value::Number(number) => {
            let is_integer = number.as_i64().is_some()
                || number.as_u64().is_some()
                || number.as_f64().is_some_and(|float| float.fract() == 0.0);
            if is_integer {
                String::from("integer")
            } else {
                String::from("number")
            }
        }
        Value::String(_) => String::from("string"),
        Value::Array(_) => String::from("array"),
        Value::Object(_) => String::from("object"),
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| String::from("<invalid json>"))
}

fn list_host_functions_payload(host_functions: &[ResolvedHostFunctions]) -> Value {
    Value::Object(Map::from_iter([(
        String::from("hostFunctions"),
        Value::Array(
            host_functions
                .iter()
                .map(|collection| {
                    json_object([
                        ("name", Value::String(collection.name.clone())),
                        (
                            "functions",
                            Value::Array(
                                collection
                                    .functions
                                    .iter()
                                    .map(|host_function| Value::String(host_function.name.clone()))
                                    .collect(),
                            ),
                        ),
                    ])
                })
                .collect(),
        ),
    )]))
}

fn describe_host_functions_payload(
    host_functions: &[ResolvedHostFunctions],
    collection_name: &str,
) -> Result<Value, String> {
    let Some(collection) = host_functions
        .iter()
        .find(|collection| collection.name == collection_name)
    else {
        return Err(format!(
            "No host function collection \"{collection_name}\". Available: {}",
            host_functions_names(host_functions)
        ));
    };
    Ok(json_object([
        ("name", Value::String(collection.name.clone())),
        (
            "functions",
            Value::Object(Map::from_iter(collection.functions.iter().map(
                |host_function| {
                    (
                        host_function.name.clone(),
                        json_object([
                            (
                                "description",
                                Value::String(host_function.description.clone()),
                            ),
                            (
                                "flags",
                                Value::Array(describe_host_function_flags(
                                    &host_function.input_schema,
                                )),
                            ),
                        ]),
                    )
                },
            ))),
        ),
    ]))
}

fn describe_host_function_payload(
    collection: &ResolvedHostFunctions,
    host_function_name: &str,
) -> Result<Value, String> {
    let Some(host_function) = collection
        .functions
        .iter()
        .find(|host_function| host_function.name == host_function_name)
    else {
        return Err(format!(
            "No host function \"{host_function_name}\" in collection \"{}\". Available: {}",
            collection.name,
            host_function_names(collection)
        ));
    };
    Ok(json_object([
        ("collection", Value::String(collection.name.clone())),
        ("function", Value::String(host_function_name.to_string())),
        (
            "description",
            Value::String(host_function.description.clone()),
        ),
        (
            "flags",
            Value::Array(describe_host_function_flags(&host_function.input_schema)),
        ),
        ("examples", Value::Array(Vec::new())),
    ]))
}

fn describe_host_function_flags(schema: &Value) -> Vec<Value> {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    properties
        .into_iter()
        .map(|(field_name, field_schema)| {
            json_object([
                (
                    "name",
                    Value::String(format!("--{}", camel_to_kebab(&field_name))),
                ),
                (
                    "type",
                    Value::String(describe_host_function_flag_type(&field_schema)),
                ),
                ("required", Value::Bool(required.contains(&field_name))),
            ])
        })
        .collect()
}

fn describe_host_function_flag_type(schema: &Value) -> String {
    match json_schema_type(schema) {
        Some("array") => {
            let item_type = schema
                .get("items")
                .and_then(json_schema_type)
                .unwrap_or("string");
            format!("{item_type}[]")
        }
        Some("string") => schema
            .get("enum")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .filter(|values| !values.is_empty())
            .map(|values| values.join("|"))
            .unwrap_or_else(|| String::from("string")),
        Some(other) => other.to_string(),
        None => String::from("string"),
    }
}

fn host_functions_names(host_functions: &[ResolvedHostFunctions]) -> String {
    host_functions
        .iter()
        .map(|collection| collection.name.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

fn host_function_names(collection: &ResolvedHostFunctions) -> String {
    collection
        .functions
        .iter()
        .map(|host_function| host_function.name.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_help_flag(value: &str) -> bool {
    matches!(value, "--help" | "-h")
}

fn json_schema_type(schema: &Value) -> Option<&str> {
    schema.get("type").and_then(Value::as_str)
}

fn camel_to_kebab(value: &str) -> String {
    let mut output = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() && index > 0 {
            output.push('-');
        }
        output.push(ch.to_ascii_lowercase());
    }
    output
}

fn normalize_guest_path(path: String) -> String {
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let normalized = parts.join("/");
    if absolute {
        format!("/{normalized}")
    } else {
        normalized
    }
}

fn json_object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Object(Map::from_iter(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value)),
    ))
}

/// Build the wire [`wire::PackageDescriptor`]s for the `/opt/agentos` projection.
/// The sidecar reads package metadata from the forwarded package path.
pub(crate) fn build_package_descriptors(config: &AgentOsConfig) -> Vec<wire::PackageDescriptor> {
    config
        .packages
        .iter()
        .map(|package| wire::PackageDescriptor {
            path: package.path.clone(),
        })
        .collect()
}

pub(crate) fn serialize_mounts(
    config: &AgentOsConfig,
) -> Result<Vec<wire::MountDescriptor>, ClientError> {
    config
        .mounts
        .iter()
        .map(|mount| match mount {
            MountConfig::Native {
                path,
                plugin,
                guest_source,
                guest_fstype,
                read_only,
            } => {
                let plugin_config = plugin
                    .config
                    .clone()
                    .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
                Ok(wire::MountDescriptor {
                    guest_path: path.clone(),
                    guest_source: guest_source.clone().unwrap_or_else(|| plugin.id.clone()),
                    guest_fstype: guest_fstype.clone().unwrap_or_else(|| plugin.id.clone()),
                    read_only: *read_only,
                    plugin: wire::MountPluginDescriptor {
                        id: plugin.id.clone(),
                        config: json_utf8(&plugin_config, "native mount plugin config")?,
                    },
                })
            }
            MountConfig::Plain { .. } => Err(ClientError::Sidecar(
                "plain mounts cannot be configured during Rust client VM creation".to_string(),
            )),
            MountConfig::Overlay { .. } => Err(ClientError::Sidecar(
                "overlay mounts cannot be configured during Rust client VM creation".to_string(),
            )),
        })
        .collect()
}

fn json_utf8(value: &serde_json::Value, context: &str) -> Result<String, ClientError> {
    serde_json::to_string(value)
        .map_err(|error| ClientError::Sidecar(format!("failed to serialize {context}: {error}")))
}

fn operation_wildcard_if_omitted(values: &Option<Vec<String>>) -> Vec<String> {
    values.clone().unwrap_or_else(|| vec!["*".to_string()])
}

fn resource_wildcard_if_omitted(values: &Option<Vec<String>>) -> Vec<String> {
    values.clone().unwrap_or_else(|| vec!["**".to_string()])
}

/// Extract the `vm_id` from a generated ownership scope, if it is VM-scoped.
fn wire_ownership_vm_id(ownership: &wire::OwnershipScope) -> Option<&str> {
    match ownership {
        wire::OwnershipScope::VmOwnership(ownership) => Some(ownership.vm_id.as_str()),
        wire::OwnershipScope::ConnectionOwnership(_)
        | wire::OwnershipScope::SessionOwnership(_) => None,
    }
}

fn vm_dispose_response(vm_id: &str, response: wire::ResponsePayload) -> Result<(), ClientError> {
    match response {
        wire::ResponsePayload::VmDisposedResponse(disposed) if disposed.vm_id == vm_id => Ok(()),
        wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
        other => Err(ClientError::Sidecar(format!(
            "unexpected dispose_vm response: {other:?}"
        ))),
    }
}

fn retain_shutdown_failure(result: &mut Result<(), ClientError>, cleanup: Result<(), ClientError>) {
    if let Err(error) = cleanup {
        eprintln!("agentOS secondary shutdown cleanup failed: {error}");
        if result.is_ok() {
            *result = Err(error);
        }
    }
}

fn concurrent_vm_mutation_error() -> ClientError {
    ClientError::Sidecar(String::from(
        "invalid_state: another VM mount or software mutation is already in progress; wait for it to finish before retrying",
    ))
}

fn try_vm_mutation(
    operation: &tokio::sync::Mutex<()>,
) -> Result<tokio::sync::MutexGuard<'_, ()>, ClientError> {
    operation
        .try_lock()
        .map_err(|_| concurrent_vm_mutation_error())
}

/// Preserve typed sidecar error codes and structured admission/deadline fields.
fn rejected_to_error(rejected: wire::RejectedResponse) -> ClientError {
    ClientError::from_rejection(rejected)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        serialize_create_vm_config_for_sidecar, serialize_root_filesystem_config_for_sidecar,
        try_vm_mutation, AgentOsCreationTask,
    };
    use crate::config::{
        AgentOsConfig, AgentOsLimits, FsPermissionRule, FsPermissions, HostFunctionLimits,
        HttpLimits, JsRuntimeLimits, MountPlugin, PatternPermissions, PermissionMode, Permissions,
        PythonLimits, ResourceLimits, RootFilesystemConfig, RootFilesystemKind, RootFilesystemMode,
        RootLowerInput, RulePermissions, WasmLimits,
    };
    use crate::fs::{
        DirEntryType, FilesystemEntry, FilesystemEntryEncoding, FilesystemSnapshotEntries,
        FilesystemSnapshotExport, RootSnapshotExport, SnapshotExportKind,
    };
    use agentos_vm_config::{
        FsPermissionScope, PatternPermissionScope, PermissionMode as ConfigPermissionMode,
        RootFilesystemEntryKind, RootFilesystemLowerDescriptor,
        RootFilesystemMode as ConfigRootFilesystemMode, VmDefaultsProfile,
    };

    #[tokio::test]
    async fn cancelled_creation_waiter_keeps_the_owned_task_running() {
        let (release, wait_for_release) = tokio::sync::oneshot::channel();
        let (completed, wait_for_completion) = tokio::sync::oneshot::channel();
        let guard = AgentOsCreationTask {
            task: Some(tokio::spawn(async move {
                let _ = wait_for_release.await;
                let _ = completed.send(());
                Err(crate::ClientError::Sidecar(String::from(
                    "injected creation failure",
                )))
            })),
        };

        drop(guard);
        release.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), wait_for_completion)
            .await
            .expect("detached creation task remains owned")
            .expect("creation task reports completion");
    }

    #[tokio::test]
    async fn concurrent_vm_mutations_fail_without_queueing() {
        let operation = tokio::sync::Mutex::new(());
        let active = try_vm_mutation(&operation).expect("first mutation is admitted");
        let error = try_vm_mutation(&operation).expect_err("second mutation is rejected");
        assert!(error.to_string().contains("invalid_state"));
        drop(active);
        assert!(try_vm_mutation(&operation).is_ok());
    }

    #[test]
    fn vm_dispose_preserves_timeout_details_and_original_error_after_cleanup() {
        use super::{retain_shutdown_failure, vm_dispose_response, wire, ClientError};

        let mut result = vm_dispose_response(
            "vm-timeout",
            wire::ResponsePayload::RejectedResponse(wire::RejectedResponse {
                code: "timeout".into(),
                message: "SQLite close is unconfirmed; raise limits.reactor.shutdownDeadlineMs"
                    .into(),
                limit_name: Some("reactor.shutdownDeadlineMs".into()),
                configured_limit: Some(5_000),
                current_usage: None,
                requested: None,
                unit: Some("milliseconds".into()),
                scope: Some("vm".into()),
                vm_id: Some("vm-timeout".into()),
                session_generation: None,
                capability_id: None,
                operation: Some("vm.dispose".into()),
                configuration_path: Some("limits.reactor.shutdownDeadlineMs".into()),
                retryable: Some(false),
                errno: Some("ETIMEDOUT".into()),
            }),
        );
        retain_shutdown_failure(
            &mut result,
            Err(ClientError::Sidecar("secondary cleanup".into())),
        );
        let ClientError::OperationTimedOut { message, details } = result.unwrap_err() else {
            panic!("expected original typed timeout");
        };
        assert!(message.contains("SQLite close is unconfirmed"));
        assert_eq!(
            details.limit_name.as_deref(),
            Some("reactor.shutdownDeadlineMs")
        );
        assert_eq!(details.configured_limit, Some(5_000));
        assert_eq!(details.unit.as_deref(), Some("milliseconds"));
        assert_eq!(details.vm_id.as_deref(), Some("vm-timeout"));
        assert_eq!(details.operation.as_deref(), Some("vm.dispose"));
        assert_eq!(
            details.configuration_path.as_deref(),
            Some("limits.reactor.shutdownDeadlineMs")
        );
        assert_eq!(details.retryable, Some(false));
        assert_eq!(details.errno.as_deref(), Some("ETIMEDOUT"));

        assert!(vm_dispose_response(
            "vm-ok",
            wire::ResponsePayload::VmDisposedResponse(wire::VmDisposedResponse {
                vm_id: "vm-ok".into()
            })
        )
        .is_ok());
        assert!(vm_dispose_response(
            "vm-ok",
            wire::ResponsePayload::VmDisposedResponse(wire::VmDisposedResponse {
                vm_id: "other-vm".into()
            })
        )
        .is_err());
        let mut result = Ok(());
        retain_shutdown_failure(&mut result, Err(ClientError::Sidecar("cleanup".into())));
        assert!(result.is_err());
    }

    #[test]
    fn create_vm_omits_permissions_so_sidecar_applies_product_defaults() {
        let config = serialize_create_vm_config_for_sidecar(&AgentOsConfig::default()).unwrap();
        assert_eq!(config.defaults_profile, Some(VmDefaultsProfile::AgentOs));
        assert_eq!(config.permissions, None);
    }

    #[test]
    fn create_vm_can_select_secure_sidecar_defaults() {
        let config = serialize_create_vm_config_for_sidecar(&AgentOsConfig {
            defaults_profile: Some(VmDefaultsProfile::Secure),
            ..AgentOsConfig::default()
        })
        .unwrap();
        assert_eq!(config.defaults_profile, Some(VmDefaultsProfile::Secure));
        assert_eq!(config.permissions, None);
    }

    #[test]
    fn create_vm_sends_only_explicit_permission_overrides() {
        let config = serialize_create_vm_config_for_sidecar(&AgentOsConfig {
            permissions: Some(Permissions {
                network: Some(PatternPermissions::Mode(PermissionMode::Deny)),
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
        let policy = config.permissions.expect("explicit policy");
        assert_eq!(
            policy.network,
            Some(PatternPermissionScope::Mode(ConfigPermissionMode::Deny))
        );
        assert_eq!(policy.child_process, None);
    }

    #[test]
    fn create_vm_expands_omitted_rule_fields_to_domain_wildcards() {
        let config = serialize_create_vm_config_for_sidecar(&AgentOsConfig {
            permissions: Some(Permissions {
                fs: Some(FsPermissions::Rules(RulePermissions {
                    default: Some(PermissionMode::Deny),
                    rules: vec![FsPermissionRule {
                        mode: PermissionMode::Allow,
                        operations: None,
                        paths: Some(vec!["/workspace/**".to_string()]),
                    }],
                })),
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
        let policy = config.permissions.expect("explicit fs policy");
        let Some(FsPermissionScope::Rules(rules)) = policy.fs else {
            panic!("expected fs rule set");
        };
        assert_eq!(rules.default, Some(ConfigPermissionMode::Deny));
        assert_eq!(rules.rules[0].operations, vec!["*"]);
        assert_eq!(rules.rules[0].paths, vec!["/workspace/**"]);

        let config = serialize_create_vm_config_for_sidecar(&AgentOsConfig {
            permissions: Some(Permissions {
                network: Some(PatternPermissions::Rules(RulePermissions {
                    default: Some(PermissionMode::Allow),
                    rules: vec![crate::config::PatternPermissionRule {
                        mode: PermissionMode::Deny,
                        operations: None,
                        patterns: None,
                    }],
                })),
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
        let policy = config.permissions.expect("explicit network policy");
        let Some(PatternPermissionScope::Rules(rules)) = policy.network else {
            panic!("expected network rule set");
        };
        assert_eq!(rules.default, Some(ConfigPermissionMode::Allow));
        assert_eq!(rules.rules[0].operations, vec!["*"]);
        assert_eq!(rules.rules[0].patterns, vec!["**"]);
    }

    #[test]
    fn root_filesystem_serializer_preserves_configured_descriptor() {
        let (descriptor, native_root) =
            serialize_root_filesystem_config_for_sidecar(&RootFilesystemConfig {
                mode: Some(RootFilesystemMode::ReadOnly),
                disable_default_base_layer: true,
                lowers: vec![
                    RootLowerInput::BundledBaseFilesystem,
                    RootLowerInput::SnapshotExport(RootSnapshotExport {
                        kind: SnapshotExportKind::SnapshotExport,
                        source: FilesystemSnapshotExport {
                            format: "agentos-filesystem-snapshot-v1".to_string(),
                            filesystem: FilesystemSnapshotEntries {
                                entries: vec![
                                    FilesystemEntry {
                                        path: "/bin/run".to_string(),
                                        entry_type: DirEntryType::File,
                                        mode: "0755".to_string(),
                                        uid: 1000,
                                        gid: 1000,
                                        content: Some("#!/bin/sh".to_string()),
                                        encoding: Some(FilesystemEntryEncoding::Utf8),
                                        target: None,
                                    },
                                    FilesystemEntry {
                                        path: "/link".to_string(),
                                        entry_type: DirEntryType::Symlink,
                                        mode: "0777".to_string(),
                                        uid: 0,
                                        gid: 0,
                                        content: None,
                                        encoding: None,
                                        target: Some("/bin/run".to_string()),
                                    },
                                ],
                            },
                        },
                    }),
                ],
                ..Default::default()
            })
            .expect("serialize root filesystem");

        assert!(native_root.is_none());
        assert_eq!(descriptor.mode, ConfigRootFilesystemMode::ReadOnly);
        assert!(descriptor.disable_default_base_layer);
        assert_eq!(descriptor.bootstrap_entries, Vec::new());
        assert!(matches!(
            descriptor.lowers[0],
            RootFilesystemLowerDescriptor::BundledBaseFilesystem
        ));

        let RootFilesystemLowerDescriptor::Snapshot { entries } = &descriptor.lowers[1] else {
            panic!("expected snapshot lower");
        };
        assert_eq!(entries[0].path, "/bin/run");
        assert_eq!(entries[0].kind, RootFilesystemEntryKind::File);
        assert_eq!(entries[0].mode, Some(0o755));
        assert!(entries[0].executable);
        assert_eq!(entries[1].kind, RootFilesystemEntryKind::Symlink);
        assert_eq!(entries[1].target.as_deref(), Some("/bin/run"));
    }

    #[test]
    fn create_vm_config_preserves_native_root_config() {
        let config = serialize_create_vm_config_for_sidecar(&AgentOsConfig {
            root_filesystem: RootFilesystemConfig {
                kind: RootFilesystemKind::Native,
                mode: Some(RootFilesystemMode::ReadOnly),
                native_plugin: Some(MountPlugin {
                    id: "sqlite_vfs".to_string(),
                    config: Some(serde_json::json!({
                        "databasePath": "/tmp/agentos-root.sqlite"
                    })),
                }),
                ..Default::default()
            },
            ..Default::default()
        })
        .expect("serialize create VM config");
        let native_root = config.native_root.expect("native root config");

        assert_eq!(native_root.plugin.id, "sqlite_vfs");
        assert_eq!(
            native_root.plugin.config,
            serde_json::json!({ "databasePath": "/tmp/agentos-root.sqlite" })
        );
        assert!(native_root.read_only);
    }

    #[test]
    fn create_vm_config_preserves_environment_and_timer_policy() {
        let environment = BTreeMap::from([
            (String::from("EMPTY"), String::new()),
            (String::from("PATH"), String::from("/opt/agentos/bin")),
        ]);
        let config = serialize_create_vm_config_for_sidecar(&AgentOsConfig {
            environment: Some(environment.clone()),
            high_resolution_time: Some(false),
            ..Default::default()
        })
        .expect("serialize create VM config");

        assert_eq!(config.env, Some(environment));
        let js_runtime = config.js_runtime.expect("explicit timer policy");
        assert_eq!(js_runtime.allowed_builtins, None);
        assert_eq!(js_runtime.high_resolution_time, Some(false));

        let empty = serialize_create_vm_config_for_sidecar(&AgentOsConfig {
            environment: Some(BTreeMap::new()),
            ..Default::default()
        })
        .expect("serialize empty environment");
        assert_eq!(empty.env, Some(BTreeMap::new()));

        let defaulted = serialize_create_vm_config_for_sidecar(&AgentOsConfig::default())
            .expect("serialize default environment");
        assert_eq!(defaulted.env, None);
    }

    #[test]
    fn create_vm_config_preserves_typed_limits() {
        let config = serialize_create_vm_config_for_sidecar(&AgentOsConfig {
            limits: Some(AgentOsLimits {
                agentos_packages: Some(crate::config::AgentOsPackageLimits {
                    max_mounts: Some(8192),
                }),
                resources: Some(ResourceLimits {
                    max_processes: Some(7),
                    max_filesystem_bytes: Some(4096),
                    ..Default::default()
                }),
                http: Some(HttpLimits {
                    max_fetch_response_bytes: Some(1024),
                }),
                tls: Some(crate::TlsLimits {
                    max_buffered_bytes: Some(2048),
                }),
                execution: Some(crate::ExecutionLimits {
                    completed_ttl_ms: Some(60_000),
                    max_completed_executions: Some(128),
                    live_execution_warning_threshold: Some(32),
                }),
                host_functions: Some(HostFunctionLimits {
                    default_timeout_ms: Some(500),
                    max_registered_functions_per_vm: Some(12),
                    ..Default::default()
                }),
                js_runtime: Some(JsRuntimeLimits {
                    v8_heap_limit_mb: Some(64),
                    sync_rpc_wait_timeout_ms: Some(2_000),
                    cpu_time_limit_ms: Some(30_000),
                    wall_clock_limit_ms: Some(0),
                    import_cache_materialize_timeout_ms: Some(30_000),
                    ..Default::default()
                }),
                python: Some(PythonLimits {
                    max_old_space_mb: Some(256),
                    ..Default::default()
                }),
                wasm: Some(WasmLimits {
                    prewarm_timeout_ms: Some(30_000),
                    runner_heap_limit_mb: Some(2_048),
                    active_cpu_time_limit_ms: Some(60_000),
                    wall_clock_limit_ms: Some(120_000),
                    deterministic_fuel: Some(1_000_000),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        })
        .expect("serialize create VM config");
        let limits = config.limits.expect("limits config");

        assert_eq!(
            limits.tls.expect("TLS limits").max_buffered_bytes,
            Some(2048)
        );
        let execution = limits.execution.expect("execution limits");
        assert_eq!(execution.completed_ttl_ms, Some(60_000));
        assert_eq!(execution.max_completed_executions, Some(128));
        assert_eq!(execution.live_execution_warning_threshold, Some(32));
        assert_eq!(
            limits.agentos_packages.expect("package limits").max_mounts,
            Some(8192)
        );
        let resources = limits.resources.expect("resource limits");
        assert_eq!(resources.max_processes, Some(7));
        assert_eq!(resources.max_filesystem_bytes, Some(4096));
        assert_eq!(
            limits.http.expect("http limits").max_fetch_response_bytes,
            Some(1024)
        );
        assert_eq!(
            limits
                .host_functions
                .as_ref()
                .expect("host_function limits")
                .default_timeout_ms,
            Some(500)
        );
        assert_eq!(
            limits
                .host_functions
                .expect("host_function limits")
                .max_registered_functions_per_vm,
            Some(12)
        );
        assert_eq!(
            limits
                .js_runtime
                .as_ref()
                .expect("js runtime limits")
                .v8_heap_limit_mb,
            Some(64)
        );
        let js_runtime = limits.js_runtime.expect("js runtime limits");
        assert_eq!(js_runtime.sync_rpc_wait_timeout_ms, Some(2_000));
        assert_eq!(js_runtime.cpu_time_limit_ms, Some(30_000));
        assert_eq!(js_runtime.wall_clock_limit_ms, Some(0));
        assert_eq!(js_runtime.import_cache_materialize_timeout_ms, Some(30_000));
        assert_eq!(
            limits.python.expect("python limits").max_old_space_mb,
            Some(256)
        );
        let wasm = limits.wasm.expect("wasm limits");
        assert_eq!(wasm.prewarm_timeout_ms, Some(30_000));
        assert_eq!(wasm.runner_heap_limit_mb, Some(2_048));
        assert_eq!(wasm.active_cpu_time_limit_ms, Some(60_000));
        assert_eq!(wasm.wall_clock_limit_ms, Some(120_000));
        assert_eq!(wasm.deterministic_fuel, Some(1_000_000));
    }

    #[test]
    fn tls_execution_limit_validation_stays_in_shared_vm_config() {
        let defaulted = serialize_create_vm_config_for_sidecar(&AgentOsConfig::default()).unwrap();
        assert!(defaulted.limits.is_none());
        let limits = AgentOsLimits {
            tls: Some(crate::TlsLimits::default()),
            execution: Some(crate::ExecutionLimits::default()),
            ..Default::default()
        };
        let config = AgentOsConfig {
            limits: Some(limits),
            ..Default::default()
        };
        super::validate_config(&config).unwrap();
        let empty = serialize_create_vm_config_for_sidecar(&config)
            .unwrap()
            .limits
            .unwrap();
        assert_eq!(empty.tls.unwrap().max_buffered_bytes, None);
        assert_eq!(
            empty.execution.unwrap(),
            agentos_vm_config::ExecutionLimitsConfig::default()
        );

        for (group, field) in [
            ("tls", "maxBufferedBytes"),
            ("execution", "completedTtlMs"),
            ("execution", "maxCompletedExecutions"),
            ("execution", "liveExecutionWarningThreshold"),
        ] {
            for value in [0, u64::MAX] {
                let mut limits: AgentOsLimits =
                    serde_json::from_value(serde_json::json!({group: {field: value}})).unwrap();
                // Shared preflight compares explicit parent/child overrides;
                // it does not materialize sidecar-owned default buffer limits.
                limits.resources = Some(ResourceLimits {
                    max_socket_buffered_bytes: Some(4096),
                    ..Default::default()
                });
                let config = AgentOsConfig {
                    limits: Some(limits),
                    ..Default::default()
                };
                let error = super::validate_config(&config).unwrap_err().to_string();
                assert!(
                    error.contains(&format!("limits.{group}.{field}")),
                    "{error}"
                );
            }
        }
    }
}
