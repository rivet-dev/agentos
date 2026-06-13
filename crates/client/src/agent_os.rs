//! The `AgentOs` struct (all fields from ADR-001 §3), the `create` builder, and the `shutdown`
//! (dispose) teardown.
//!
//! `AgentOs` is `Arc`-cloneable; all interior state lives behind concurrent maps / atomics /
//! channels so `&self` methods never need an outer lock. Module files add only `impl AgentOs` blocks
//! and never introduce new struct fields.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use scc::{HashMap as SccHashMap, HashSet as SccHashSet};
use tokio::sync::{broadcast, oneshot, watch};
use tokio::task::JoinHandle;

use agent_os_sidecar::protocol::{
    ConfigureVmRequest, CreateVmRequest, DisposeReason, DisposeVmRequest, EventPayload,
    FsPermissionRule as WireFsPermissionRule, FsPermissionRuleSet as WireFsPermissionRuleSet,
    FsPermissionScope, GuestRuntimeKind, KillProcessRequest, MountDescriptor,
    MountPluginDescriptor, OpenSessionRequest, OwnershipScope,
    PatternPermissionRule as WirePatternPermissionRule,
    PatternPermissionRuleSet as WirePatternPermissionRuleSet, PatternPermissionScope,
    PermissionMode as WirePermissionMode, PermissionsPolicy, RegisterToolkitRequest,
    RegisteredToolDefinition, RequestPayload, ResponsePayload, RootFilesystemDescriptor,
    RootFilesystemEntry, RootFilesystemEntryEncoding, RootFilesystemEntryKind,
    SidecarPermissionResultResponse, SidecarPlacement, SidecarRequestPayload,
    SidecarResponsePayload, SoftwareDescriptor, ToolInvocationRequest,
    ToolInvocationResultResponse, VmLifecycleState,
};

use crate::config::{AgentOsConfig, HostTool, MountConfig, SoftwareKind, TimerScheduleDriver};
use crate::cron::CronManager;
use crate::error::ClientError;
use crate::json_rpc::SequencedEvent;
use crate::process::SYNTHETIC_PID_BASE;
use crate::session::{
    AgentCapabilities, AgentInfo, PermissionReply, PermissionRequest, SessionConfigOption,
    SessionModeState,
};
use crate::sidecar::{AgentOsSidecar, AgentOsSidecarPlacement, AgentOsSidecarVmLease};
use crate::transport::{SidecarCallback, SidecarTransport};

use once_cell::sync::OnceCell;

const OS_INSTRUCTIONS: &str =
    include_str!("../../../packages/core/fixtures/AGENTOS_SYSTEM_PROMPT.md");

// ---------------------------------------------------------------------------
// Registry entries
// ---------------------------------------------------------------------------

/// An SDK-spawned process (TS `_processes` value). Keyed by user-facing pid.
pub(crate) struct ProcessEntry {
    pub command: String,
    pub args: Vec<String>,
    pub stdout_tx: broadcast::Sender<Vec<u8>>,
    pub stderr_tx: broadcast::Sender<Vec<u8>>,
    /// Seeded `None`; the already-exited branch fires immediately once it holds `Some(code)`.
    pub exit_tx: watch::Sender<Option<i32>>,
    /// The sidecar-side process id used on the wire.
    pub process_id: String,
    /// The kernel pid returned by the `Execute` response, seeded once the spawn lands. The TS native
    /// path builds `displayPidByKernelPid` from this so `all_processes`/`process_tree` report the
    /// public spawn pid (the map key) for the spawned root, not the raw kernel pid.
    pub kernel_pid: watch::Sender<Option<u32>>,
}

/// A PTY-backed shell (TS `_shells` value). Keyed by synthetic `shell-N` id.
///
/// `data_tx` carries stdout only, matching TS where the kernel handle's `onData` is fed exclusively
/// by `stdoutHandlers`. `stderr_tx` is the dedicated stderr channel that backs the `on_stderr` option
/// and `on_shell_stderr`, matching TS where stderr reaches the host only through `stderrHandlers`.
pub(crate) struct ShellEntry {
    pub pid: u32,
    pub data_tx: broadcast::Sender<Vec<u8>>,
    pub stderr_tx: broadcast::Sender<Vec<u8>>,
    /// The sidecar-side process id used on the wire.
    pub process_id: String,
    /// Spawn-readiness gate. Seeded `false`; flips to `true` once the background `Execute` request is
    /// acked. TS `openShell` is fully synchronous so `writeShell` always addresses a live spawn; the
    /// Rust wire spawn is async, so `write_shell`/`close_shell` await this gate before issuing their
    /// wire request to preserve the deterministic ordering and avoid dropping early input.
    pub spawned_tx: watch::Sender<bool>,
}

/// A connected ACP terminal process and its output fan-out task.
pub(crate) struct AcpTerminalEntry {
    pub exit_task: JoinHandle<()>,
}

/// An ACP session (TS `_sessions` value). Keyed by ACP session id.
pub(crate) struct SessionEntry {
    pub agent_type: String,
    pub modes: parking_lot::Mutex<Option<SessionModeState>>,
    pub config_options: parking_lot::Mutex<Vec<SessionConfigOption>>,
    pub capabilities: parking_lot::Mutex<Option<AgentCapabilities>>,
    pub agent_info: parking_lot::Mutex<Option<AgentInfo>>,
    pub config_overrides: parking_lot::Mutex<std::collections::BTreeMap<String, String>>,
    /// Bounded event ring (cap [`crate::ACP_SESSION_EVENT_RETENTION_LIMIT`]).
    pub event_ring: parking_lot::Mutex<VecDeque<SequencedEvent>>,
    /// Highest seen sequence number (ack-based; separate from the truncated ring; negative for
    /// synthetic events).
    pub highest_sequence_number: AtomicI64,
    pub event_tx: broadcast::Sender<SequencedEvent>,
    pub permission_tx: broadcast::Sender<PermissionRequest>,
    pub pending_permission_replies: SccHashMap<String, oneshot::Sender<PermissionReply>>,
    pub pending_session_request_lock: parking_lot::Mutex<()>,
    /// Pending prompt resolvers, for cancel prompt-fallback + abort-on-close.
    ///
    /// The resolver carries the intended [`JsonRpcResponse`], mirroring the TS resolver shape
    /// `{ method, resolve: (response) => void }`. The cause (close vs cancel) decides the payload at
    /// the abort/cancel site: abort-on-close resolves with the `-32000` `Session closed: <id>` error,
    /// while prompt-cancel resolves with `{ result: { stopReason: "cancelled" } }`. The shape is NOT
    /// re-derived from the method downstream.
    pub pending_prompt_resolvers:
        SccHashMap<i64, oneshot::Sender<crate::json_rpc::JsonRpcResponse>>,
}

// ---------------------------------------------------------------------------
// AgentOs
// ---------------------------------------------------------------------------

/// The high-level client. Cheaply cloneable via `Arc`.
#[derive(Clone)]
pub struct AgentOs {
    inner: Arc<AgentOsInner>,
}

pub(crate) struct AgentOsInner {
    // Transport / connection / VM handle.
    pub(crate) transport: Arc<SidecarTransport>,
    pub(crate) connection_id: String,
    pub(crate) session_id: String,
    pub(crate) vm_id: String,
    pub(crate) request_counter: AtomicI64,

    // Process registries.
    pub(crate) process_registry_lock: parking_lot::Mutex<()>,
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
    pub(crate) acp_terminals: SccHashMap<String, AcpTerminalEntry>,
    pub(crate) acp_terminal_count: AtomicUsize,
    pub(crate) acp_terminal_lifecycle_lock: tokio::sync::Mutex<()>,

    // Session registries.
    pub(crate) sessions: SccHashMap<String, SessionEntry>,
    /// Bounded ordered set (cap [`crate::CLOSED_SESSION_ID_RETENTION_LIMIT`]) for close idempotence.
    pub(crate) closed_session_ids: parking_lot::Mutex<VecDeque<String>>,
    /// Session ids with an in-flight close in progress. Mirrors TS `_sessionClosePromises`: because
    /// `close_session` runs the actual close on a detached task, this set keeps the id "known" during
    /// the window between removal from `sessions` and insertion into `closed_session_ids`, so a second
    /// `close_session` (or close-after-destroy) does not spuriously throw `SessionNotFound`.
    pub(crate) closing_session_ids: SccHashSet<String>,

    // Cron.
    pub(crate) cron: Arc<CronManager>,

    // Config / lifecycle.
    pub(crate) config: Arc<AgentOsConfig>,
    pub(crate) sidecar: Arc<AgentOsSidecar>,
    pub(crate) sidecar_lease: parking_lot::Mutex<Option<AgentOsSidecarVmLease>>,
    pub(crate) in_process_mounts: SccHashMap<String, crate::fs::MountedFs>,
    pub(crate) disposed: AtomicBool,
}

impl AgentOs {
    /// The sole public VM entry point. Processes software, spawns/authenticates the sidecar, creates
    /// the VM, waits for ready (10s), configures it, takes a lease, and constructs the cron manager
    /// (default [`crate::config::TimerScheduleDriver`]).
    pub async fn create(options: AgentOsConfig) -> Result<AgentOs, ClientError> {
        let config = Arc::new(options);

        // 1. Resolve the sidecar handle (shared "default" pool unless configured otherwise) and
        //    establish/reuse its shared process + authenticated connection. A shared sidecar hosts
        //    multiple VMs in one process, each opening its own session + VM below.
        let sidecar = match &config.sidecar {
            Some(crate::config::AgentOsSidecarConfig::Explicit { handle }) => handle.clone(),
            Some(crate::config::AgentOsSidecarConfig::Shared { pool }) => {
                AgentOs::get_shared_sidecar(pool.clone(), config.sidecar_binary_path.clone()).await?
            }
            None => {
                AgentOs::get_shared_sidecar(None, config.sidecar_binary_path.clone()).await?
            }
        };
        let (transport, connection_id, _) = sidecar.ensure_connection().await?;

        // 2. Open a session for this VM (connection scope) on the shared connection.
        let session = match transport
            .request(
                OwnershipScope::connection(&connection_id),
                RequestPayload::OpenSession(OpenSessionRequest {
                    placement: sidecar_wire_placement(&sidecar),
                    metadata: BTreeMap::new(),
                }),
            )
            .await?
        {
            ResponsePayload::SessionOpened(opened) => opened,
            ResponsePayload::Rejected(rejected) => return Err(rejected_to_error(rejected)),
            _ => {
                return Err(ClientError::Sidecar(
                    "unexpected open_session response".to_string(),
                ));
            }
        };
        let session_id = session.session_id;

        // 3. Subscribe to events BEFORE CreateVm so the `ready` lifecycle event cannot be missed.
        let mut events = transport.subscribe_events();
        let permissions = permissions_policy(&config);

        // 4. Create the VM (session scope). Default root filesystem keeps the bundled base layer.
        let vm = match transport
            .request(
                OwnershipScope::session(&connection_id, &session_id),
                RequestPayload::CreateVm(CreateVmRequest {
                    runtime: GuestRuntimeKind::JavaScript,
                    metadata: BTreeMap::new(),
                    root_filesystem: root_filesystem_descriptor(&config),
                    permissions: Some(permissions.clone()),
                }),
            )
            .await?
        {
            ResponsePayload::VmCreated(created) => created,
            ResponsePayload::Rejected(rejected) => return Err(rejected_to_error(rejected)),
            _ => {
                return Err(ClientError::Sidecar(
                    "unexpected create_vm response".to_string(),
                ));
            }
        };
        let vm_id = vm.vm_id;

        // 5. Wait for the VM to reach `ready` (bounded by VM_READY_TIMEOUT_MS).
        wait_for_vm_ready(&mut events, &vm_id, crate::VM_READY_TIMEOUT_MS).await?;

        // Resolve software packages to host roots (port of TS `processSoftware` for the
        // ConfigureVm descriptors). Each `package` is resolved under `module_access_cwd/node_modules`;
        // an unresolvable package is an explicit error rather than a silent no-op. Wasm command
        // packages additionally become `/__agentos/commands/{index}/` mounts so the sidecar can
        // discover and resolve guest commands.
        let resolved_software = resolve_software(&config)?;
        let command_mounts = build_command_mounts(&resolved_software);
        let software: Vec<SoftwareDescriptor> = resolved_software
            .into_iter()
            .map(|entry| entry.descriptor)
            .collect();

        // Native plugin mounts configured on the client, combined with the wasm command-dir mounts.
        let mut mounts = serialize_mounts(&config)?;
        mounts.extend(command_mounts);

        // 6. Configure the VM (vm scope).
        match transport
            .request(
                OwnershipScope::vm(&connection_id, &session_id, &vm_id),
                RequestPayload::ConfigureVm(ConfigureVmRequest {
                    mounts,
                    software,
                    permissions: Some(permissions),
                    module_access_cwd: config.module_access_cwd.clone(),
                    instructions: config.additional_instructions.clone().into_iter().collect(),
                    projected_modules: Vec::new(),
                    command_permissions: BTreeMap::new(),
                    allowed_node_builtins: config.allowed_node_builtins.clone().unwrap_or_default(),
                    loopback_exempt_ports: config.loopback_exempt_ports.clone(),
                }),
            )
            .await?
        {
            ResponsePayload::VmConfigured(_) => {}
            ResponsePayload::Rejected(rejected) => return Err(rejected_to_error(rejected)),
            _ => {
                return Err(ClientError::Sidecar(
                    "unexpected configure_vm response".to_string(),
                ));
            }
        }

        // 6b. Register host tool kits (if any): forward each tool definition via `register_toolkit`,
        //     record the host execute callbacks in the per-VM registry, and install the shared
        //     tool-invocation callback that routes guest tool calls back to the host by VM.
        if !config.tool_kits.is_empty() {
            let mut tool_map: std::collections::HashMap<String, HostTool> =
                std::collections::HashMap::new();
            for kit in &config.tool_kits {
                let mut tools = BTreeMap::new();
                for tool in &kit.tools {
                    tools.insert(
                        tool.name.clone(),
                        RegisteredToolDefinition {
                            description: tool.description.clone(),
                            input_schema: tool.input_schema.clone(),
                            timeout_ms: tool.timeout_ms,
                            examples: Vec::new(),
                        },
                    );
                    tool_map.insert(format!("{}:{}", kit.name, tool.name), tool.clone());
                }
                match transport
                    .request(
                        OwnershipScope::vm(&connection_id, &session_id, &vm_id),
                        RequestPayload::RegisterToolkit(RegisterToolkitRequest {
                            name: kit.name.clone(),
                            description: kit.description.clone(),
                            tools,
                        }),
                    )
                    .await?
                {
                    ResponsePayload::ToolkitRegistered(_) => {}
                    ResponsePayload::Rejected(rejected) => return Err(rejected_to_error(rejected)),
                    _ => {
                        return Err(ClientError::Sidecar(
                            "unexpected register_toolkit response".to_string(),
                        ));
                    }
                }
            }
            let _ = vm_tools().insert(vm_id.clone(), Arc::new(tool_map));
            transport.register_callback("tool_invocation", tool_invocation_callback());
        }

        // 7. Lease this VM on the (possibly shared) sidecar, build cron, and assemble the client.
        sidecar.active_vm_count.fetch_add(1, Ordering::SeqCst);
        let lease = AgentOsSidecarVmLease {
            sidecar: sidecar.clone(),
        };

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
            request_counter: AtomicI64::new(1),
            process_registry_lock: parking_lot::Mutex::new(()),
            processes: SccHashMap::new(),
            process_counter: AtomicU64::new(1),
            synthetic_pid_counter: AtomicU64::new(SYNTHETIC_PID_BASE),
            observed_process_time_lock: parking_lot::Mutex::new(()),
            observed_process_start_times: SccHashMap::new(),
            observed_process_exit_times: SccHashMap::new(),
            shells: SccHashMap::new(),
            shell_counter: AtomicU64::new(0),
            pending_shell_exits: SccHashMap::new(),
            acp_terminals: SccHashMap::new(),
            acp_terminal_count: AtomicUsize::new(0),
            acp_terminal_lifecycle_lock: tokio::sync::Mutex::new(()),
            sessions: SccHashMap::new(),
            closed_session_ids: parking_lot::Mutex::new(VecDeque::new()),
            closing_session_ids: SccHashSet::new(),
            cron,
            config,
            sidecar,
            sidecar_lease: parking_lot::Mutex::new(Some(lease)),
            in_process_mounts: SccHashMap::new(),
            disposed: AtomicBool::new(false),
        };

        let client = AgentOs {
            inner: Arc::new(inner),
        };
        // Register the permission router and callback unconditionally (unlike `tool_invocation`,
        // which is gated on configured tool kits): any agent session can raise a permission
        // request. Re-registering on a shared transport replaces an identical stateless callback,
        // same as the `tool_invocation` pattern.
        let _ = vm_permission_routers()
            .insert(client.inner.vm_id.clone(), Arc::downgrade(&client.inner));
        client
            .inner
            .transport
            .register_callback("permission_request", permission_request_callback());
        Ok(client)
    }

    /// Dispose the VM (= TS `dispose`). Teardown order:
    /// 1. cron dispose
    /// 2. close all sessions (swallow errors)
    /// 3. kill all shells + snapshot pending exits
    /// 4. kill all ACP terminals
    /// 5. drain tracked shell-exit tasks (two-phase, bounded by
    ///    [`crate::SHELL_DISPOSE_TIMEOUT_MS`])
    /// 6. unregister the sidecar event listener
    /// 7. release the lease (or tear down the transport)
    ///
    /// Idempotent (guarded by `disposed`).
    pub async fn shutdown(&self) -> Result<(), ClientError> {
        // Idempotent: only the first caller runs teardown.
        if self.inner.disposed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        // 1. Cron dispose (cancel armed timers + tear down the driver).
        self.inner.cron.dispose();

        // 2-5. Best-effort drain tracked shell and terminal tasks before the VM is disposed, bounded
        //      by SHELL_DISPOSE_TIMEOUT_MS so late output cannot race a closed transport.
        let mut exit_tasks = Vec::new();
        self.inner.pending_shell_exits.retain(|_, task| {
            exit_tasks.push(std::mem::replace(task, tokio::spawn(async {})));
            false
        });

        {
            let _terminal_lifecycle_guard = self.inner.acp_terminal_lifecycle_lock.lock().await;
            let mut terminal_entries = Vec::new();
            self.inner.acp_terminals.retain(|process_id, entry| {
                terminal_entries.push((
                    process_id.clone(),
                    std::mem::replace(&mut entry.exit_task, tokio::spawn(async {})),
                ));
                false
            });
            self.inner.acp_terminal_count.store(0, Ordering::SeqCst);
            for (process_id, _) in &terminal_entries {
                let transport = self.transport().clone();
                let ownership = OwnershipScope::vm(
                    self.inner.connection_id.clone(),
                    self.inner.session_id.clone(),
                    self.inner.vm_id.clone(),
                );
                let process_id = process_id.clone();
                exit_tasks.push(tokio::spawn(async move {
                    let _ = transport
                        .request(
                            ownership,
                            RequestPayload::KillProcess(KillProcessRequest {
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

        // 6-7. Release this VM (DisposeVm best-effort) and its lease. The transport is shared across
        //      VMs on the same sidecar, so it is only torn down when this was the last VM (matching
        //      the TS lease/shared-sidecar lifecycle); otherwise sibling VMs keep using it.
        let lease = self.inner.sidecar_lease.lock().take();
        let _ = self
            .transport()
            .request(
                OwnershipScope::vm(
                    &self.inner.connection_id,
                    &self.inner.session_id,
                    &self.inner.vm_id,
                ),
                RequestPayload::DisposeVm(DisposeVmRequest {
                    reason: DisposeReason::Requested,
                }),
            )
            .await;
        let _ = vm_tools().remove(&self.inner.vm_id);
        let _ = vm_permission_routers().remove(&self.inner.vm_id);
        let sidecar = self.inner.sidecar.clone();
        if let Some(lease) = lease {
            lease.dispose().await?;
        }
        if sidecar.active_vm_count.load(Ordering::SeqCst) == 0 {
            sidecar.kill_connection().await;
            let _ = sidecar.dispose().await;
        }

        Ok(())
    }

    // --- internal accessors used by sibling impl blocks ---

    pub(crate) fn inner(&self) -> &AgentOsInner {
        &self.inner
    }

    pub(crate) fn transport(&self) -> &Arc<SidecarTransport> {
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

    pub(crate) fn config(&self) -> &Arc<AgentOsConfig> {
        &self.inner.config
    }

    pub(crate) fn cron(&self) -> &Arc<CronManager> {
        &self.inner.cron
    }

    /// The (possibly shared) sidecar handle backing this VM. Public for parity with TS
    /// `AgentOs.sidecar` (e.g. `describe()` reports `active_vm_count` across VMs sharing a pool).
    pub fn sidecar(&self) -> Arc<AgentOsSidecar> {
        self.inner.sidecar.clone()
    }
}

/// Convert a sidecar's client-side placement into the wire `SidecarPlacement` for OpenSession.
fn sidecar_wire_placement(sidecar: &AgentOsSidecar) -> SidecarPlacement {
    match &sidecar.placement {
        AgentOsSidecarPlacement::Shared { pool } => SidecarPlacement::Shared { pool: pool.clone() },
        AgentOsSidecarPlacement::Explicit { sidecar_id } => SidecarPlacement::Explicit {
            sidecar_id: sidecar_id.clone(),
        },
    }
}

/// Await the `ready` VM lifecycle event for `vm_id`, bounded by `timeout_ms`.
async fn wait_for_vm_ready(
    events: &mut broadcast::Receiver<(OwnershipScope, EventPayload)>,
    vm_id: &str,
    timeout_ms: u64,
) -> Result<(), ClientError> {
    let wait = async {
        loop {
            match events.recv().await {
                Ok((ownership, payload)) => match payload {
                    EventPayload::VmLifecycle(event) => {
                        if matches!(event.state, VmLifecycleState::Ready)
                            && ownership_vm_id(&ownership) == Some(vm_id)
                        {
                            return Ok(());
                        }
                    }
                    EventPayload::ProcessOutput(_)
                    | EventPayload::ProcessExited(_)
                    | EventPayload::Structured(_) => {}
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

/// Process-global per-VM host-tool registry (vm_id -> tools keyed by `<toolkit>:<tool>`). The shared
/// transport's single tool-invocation callback routes to the right VM's tools by frame ownership.
static VM_TOOLS: OnceCell<SccHashMap<String, Arc<std::collections::HashMap<String, HostTool>>>> =
    OnceCell::new();

fn vm_tools() -> &'static SccHashMap<String, Arc<std::collections::HashMap<String, HostTool>>> {
    VM_TOOLS.get_or_init(SccHashMap::new)
}

/// Process-global map of vm id -> client inner, so the shared `permission_request` transport
/// callback can route a sidecar permission request to the owning client. `Weak` so the registry
/// never extends a client's lifetime; entries are removed in `shutdown`.
static VM_PERMISSION_ROUTERS: OnceCell<SccHashMap<String, Weak<AgentOsInner>>> = OnceCell::new();

fn vm_permission_routers() -> &'static SccHashMap<String, Weak<AgentOsInner>> {
    VM_PERMISSION_ROUTERS.get_or_init(SccHashMap::new)
}

/// The transport callback that answers sidecar permission requests by routing them to the owning
/// client's `on_permission_request` subscribers. Mirrors TS `_handlePermissionSidecarRequest`.
fn permission_request_callback() -> SidecarCallback {
    Arc::new(|payload, ownership| {
        Box::pin(async move {
            let request = match payload {
                SidecarRequestPayload::PermissionRequest(request) => request,
                SidecarRequestPayload::ToolInvocation(_)
                | SidecarRequestPayload::AcpRequest(_)
                | SidecarRequestPayload::JsBridgeCall(_) => {
                    return Ok(SidecarResponsePayload::PermissionRequestResult(
                        SidecarPermissionResultResponse {
                            permission_id: "unknown".to_string(),
                            reply: None,
                            error: Some(
                                "permission callback received a non-permission request".to_string(),
                            ),
                        },
                    ));
                }
            };
            let vm_id = ownership_vm_id(&ownership).unwrap_or("");
            let inner = vm_permission_routers()
                .read(vm_id, |_, weak| weak.clone())
                .and_then(|weak| weak.upgrade());
            let Some(inner) = inner else {
                return Ok(SidecarResponsePayload::PermissionRequestResult(
                    SidecarPermissionResultResponse {
                        permission_id: request.permission_id,
                        reply: None,
                        error: Some(format!("no client registered for vm: {vm_id}")),
                    },
                ));
            };
            let client = AgentOs { inner };
            Ok(SidecarResponsePayload::PermissionRequestResult(
                client.deliver_sidecar_permission_request(request).await,
            ))
        })
    })
}

/// The transport callback that answers guest tool invocations by running the matching host tool.
fn tool_invocation_callback() -> SidecarCallback {
    Arc::new(|payload, ownership| {
        Box::pin(async move {
            let request = match payload {
                SidecarRequestPayload::ToolInvocation(request) => request,
                _ => {
                    return Ok(SidecarResponsePayload::ToolInvocationResult(
                        ToolInvocationResultResponse {
                            invocation_id: "unknown".to_string(),
                            result: None,
                            error: Some(
                                "tool-invocation callback received a non-tool request".to_string(),
                            ),
                        },
                    ));
                }
            };
            Ok(SidecarResponsePayload::ToolInvocationResult(
                run_tool_invocation(&ownership, request).await,
            ))
        })
    })
}

/// Run a single tool invocation against the per-VM host-tool registry, honoring the timeout. Mirrors
/// TS `handleToolInvocation` (unknown-tool + timeout + error shapes).
async fn run_tool_invocation(
    ownership: &OwnershipScope,
    request: ToolInvocationRequest,
) -> ToolInvocationResultResponse {
    let vm_id = ownership_vm_id(ownership).unwrap_or("");
    let tool = vm_tools()
        .read(vm_id, |_, map| map.clone())
        .and_then(|map| map.get(&request.tool_key).cloned());
    let Some(tool) = tool else {
        return ToolInvocationResultResponse {
            invocation_id: request.invocation_id,
            result: None,
            error: Some(format!("Unknown tool \"{}\"", request.tool_key)),
        };
    };
    let timeout = Duration::from_millis(request.timeout_ms.max(1));
    match tokio::time::timeout(timeout, (tool.execute)(request.input)).await {
        Ok(Ok(value)) => ToolInvocationResultResponse {
            invocation_id: request.invocation_id,
            result: Some(value),
            error: None,
        },
        Ok(Err(error)) => ToolInvocationResultResponse {
            invocation_id: request.invocation_id,
            result: None,
            error: Some(error),
        },
        Err(_) => ToolInvocationResultResponse {
            invocation_id: request.invocation_id,
            result: None,
            error: Some(format!(
                "Tool \"{}\" timed out after {}ms",
                request.tool_key, request.timeout_ms
            )),
        },
    }
}

/// A software package resolved to its host root, paired with the kind that decides how it is mounted.
struct ResolvedSoftware {
    descriptor: SoftwareDescriptor,
    kind: SoftwareKind,
}

/// Resolve `config.software` package inputs to host roots, each rooted at its host `node_modules`
/// directory under `module_access_cwd` (default `.`). An absolute `package` path bypasses the
/// `node_modules` prefix (via `Path::join` semantics), which is how wasm command directories are
/// passed directly. Mirrors the TS `processSoftware` mapping. An unresolvable package is an explicit
/// error, not a silent no-op.
fn resolve_software(config: &AgentOsConfig) -> Result<Vec<ResolvedSoftware>, ClientError> {
    if config.software.is_empty() {
        return Ok(Vec::new());
    }
    let module_access_cwd = config
        .module_access_cwd
        .clone()
        .unwrap_or_else(|| ".".to_string());
    let mut resolved = Vec::with_capacity(config.software.len());
    for input in &config.software {
        let root = std::path::Path::new(&module_access_cwd)
            .join("node_modules")
            .join(&input.package);
        if !root.exists() {
            return Err(ClientError::Sidecar(format!(
                "software package not found: {} (looked in {})",
                input.package,
                root.display()
            )));
        }
        resolved.push(ResolvedSoftware {
            descriptor: SoftwareDescriptor {
                package_name: input.package.clone(),
                root: root.to_string_lossy().into_owned(),
            },
            kind: input.kind,
        });
    }
    Ok(resolved)
}

/// Build the `host_dir` mount descriptors that expose each wasm command directory at
/// `/__agentos/commands/{index}/` in the guest, so the sidecar's `discover_command_guest_paths` can
/// resolve guest commands. Indices are zero-padded so the sidecar's lexical sort preserves numeric
/// resolution priority past nine packages. Agent/tool packages are skipped here (they are not
/// command directories). Mirrors the TS `commandDirs` mount loop in `agent-os.ts`.
fn build_command_mounts(resolved: &[ResolvedSoftware]) -> Vec<MountDescriptor> {
    let mut mounts = Vec::new();
    for entry in resolved {
        match entry.kind {
            SoftwareKind::WasmCommands => {
                let index = mounts.len();
                mounts.push(MountDescriptor {
                    guest_path: format!("/__agentos/commands/{index:03}"),
                    read_only: true,
                    plugin: MountPluginDescriptor {
                        id: String::from("host_dir"),
                        config: serde_json::json!({
                            "hostPath": entry.descriptor.root,
                            "readOnly": true,
                        }),
                    },
                });
            }
            SoftwareKind::Agent | SoftwareKind::Tool => {}
        }
    }
    mounts
}

fn serialize_mounts(config: &AgentOsConfig) -> Result<Vec<MountDescriptor>, ClientError> {
    config
        .mounts
        .iter()
        .map(|mount| match mount {
            MountConfig::Native {
                path,
                plugin,
                read_only,
            } => Ok(MountDescriptor {
                guest_path: path.clone(),
                read_only: *read_only,
                plugin: MountPluginDescriptor {
                    id: plugin.id.clone(),
                    config: plugin
                        .config
                        .clone()
                        .unwrap_or_else(|| serde_json::Value::Object(Default::default())),
                },
            }),
            MountConfig::Plain { .. } => Err(ClientError::Sidecar(
                "plain mounts cannot be configured during Rust client VM creation".to_string(),
            )),
            MountConfig::Overlay { .. } => Err(ClientError::Sidecar(
                "overlay mounts cannot be configured during Rust client VM creation".to_string(),
            )),
        })
        .collect()
}

fn permissions_policy(config: &AgentOsConfig) -> PermissionsPolicy {
    let Some(permissions) = config.permissions.as_ref() else {
        return PermissionsPolicy::allow_all();
    };

    PermissionsPolicy {
        fs: Some(
            permissions
                .fs
                .as_ref()
                .map(serialize_fs_permissions)
                .unwrap_or(FsPermissionScope::Mode(WirePermissionMode::Allow)),
        ),
        network: Some(
            permissions
                .network
                .as_ref()
                .map(serialize_pattern_permissions)
                .unwrap_or(PatternPermissionScope::Mode(WirePermissionMode::Allow)),
        ),
        child_process: Some(
            permissions
                .child_process
                .as_ref()
                .map(serialize_pattern_permissions)
                .unwrap_or(PatternPermissionScope::Mode(WirePermissionMode::Allow)),
        ),
        process: Some(
            permissions
                .process
                .as_ref()
                .map(serialize_pattern_permissions)
                .unwrap_or(PatternPermissionScope::Mode(WirePermissionMode::Allow)),
        ),
        env: Some(
            permissions
                .env
                .as_ref()
                .map(serialize_pattern_permissions)
                .unwrap_or(PatternPermissionScope::Mode(WirePermissionMode::Allow)),
        ),
        tool: Some(
            permissions
                .tool
                .as_ref()
                .map(serialize_pattern_permissions)
                .unwrap_or(PatternPermissionScope::Mode(WirePermissionMode::Allow)),
        ),
    }
}

fn serialize_fs_permissions(permissions: &crate::config::FsPermissions) -> FsPermissionScope {
    match permissions {
        crate::config::FsPermissions::Mode(mode) => {
            FsPermissionScope::Mode(serialize_permission_mode(*mode))
        }
        crate::config::FsPermissions::Rules(rules) => {
            FsPermissionScope::Rules(WireFsPermissionRuleSet {
                default: rules.default.map(serialize_permission_mode),
                rules: rules
                    .rules
                    .iter()
                    .map(|rule| WireFsPermissionRule {
                        mode: serialize_permission_mode(rule.mode),
                        operations: operation_wildcard_if_omitted(&rule.operations),
                        paths: resource_wildcard_if_omitted(&rule.paths),
                    })
                    .collect(),
            })
        }
    }
}

fn serialize_pattern_permissions(
    permissions: &crate::config::PatternPermissions,
) -> PatternPermissionScope {
    match permissions {
        crate::config::PatternPermissions::Mode(mode) => {
            PatternPermissionScope::Mode(serialize_permission_mode(*mode))
        }
        crate::config::PatternPermissions::Rules(rules) => {
            PatternPermissionScope::Rules(WirePatternPermissionRuleSet {
                default: rules.default.map(serialize_permission_mode),
                rules: rules
                    .rules
                    .iter()
                    .map(|rule| WirePatternPermissionRule {
                        mode: serialize_permission_mode(rule.mode),
                        operations: operation_wildcard_if_omitted(&rule.operations),
                        patterns: resource_wildcard_if_omitted(&rule.patterns),
                    })
                    .collect(),
            })
        }
    }
}

fn serialize_permission_mode(mode: crate::config::PermissionMode) -> WirePermissionMode {
    match mode {
        crate::config::PermissionMode::Allow => WirePermissionMode::Allow,
        crate::config::PermissionMode::Deny => WirePermissionMode::Deny,
    }
}

fn operation_wildcard_if_omitted(values: &Option<Vec<String>>) -> Vec<String> {
    values.clone().unwrap_or_else(|| vec!["*".to_string()])
}

fn resource_wildcard_if_omitted(values: &Option<Vec<String>>) -> Vec<String> {
    values.clone().unwrap_or_else(|| vec!["**".to_string()])
}

fn root_filesystem_descriptor(config: &AgentOsConfig) -> RootFilesystemDescriptor {
    RootFilesystemDescriptor {
        bootstrap_entries: vec![RootFilesystemEntry {
            path: "/etc/agentos/instructions.md".to_string(),
            kind: RootFilesystemEntryKind::File,
            mode: Some(0o644),
            uid: Some(0),
            gid: Some(0),
            content: Some(build_os_instructions(
                config.additional_instructions.as_deref(),
            )),
            encoding: Some(RootFilesystemEntryEncoding::Utf8),
            target: None,
            executable: false,
        }],
        ..RootFilesystemDescriptor::default()
    }
}

fn build_os_instructions(additional: Option<&str>) -> String {
    match additional {
        Some(additional) if !additional.is_empty() => format!("{OS_INSTRUCTIONS}\n{additional}"),
        Some(_) | None => OS_INSTRUCTIONS.to_string(),
    }
}

/// Extract the `vm_id` from an ownership scope, if it is VM-scoped.
fn ownership_vm_id(ownership: &OwnershipScope) -> Option<&str> {
    match ownership {
        OwnershipScope::Vm { vm_id, .. } => Some(vm_id),
        OwnershipScope::Connection { .. } | OwnershipScope::Session { .. } => None,
    }
}

/// Map a `Rejected` response into a [`ClientError::Kernel`] so the errno `code` survives.
fn rejected_to_error(rejected: agent_os_sidecar::protocol::RejectedResponse) -> ClientError {
    ClientError::Kernel {
        code: rejected.code,
        message: rejected.message,
    }
}

#[cfg(test)]
mod tests {
    use super::{PatternPermissionScope, WirePermissionMode, permissions_policy};
    use crate::config::{
        AgentOsConfig, FsPermissionRule, FsPermissions, PatternPermissions, PermissionMode,
        Permissions, RulePermissions,
    };

    #[test]
    fn permissions_policy_defaults_to_allow_all_when_unset() {
        assert_eq!(
            permissions_policy(&AgentOsConfig::default()),
            agent_os_sidecar::protocol::PermissionsPolicy::allow_all()
        );
    }

    #[test]
    fn permissions_policy_preserves_configured_denies_and_allows_omitted_domains() {
        let policy = permissions_policy(&AgentOsConfig {
            permissions: Some(Permissions {
                network: Some(PatternPermissions::Mode(PermissionMode::Deny)),
                ..Default::default()
            }),
            ..Default::default()
        });

        assert_eq!(
            policy.network,
            Some(PatternPermissionScope::Mode(WirePermissionMode::Deny))
        );
        assert_eq!(
            policy.child_process,
            Some(PatternPermissionScope::Mode(WirePermissionMode::Allow))
        );
    }

    #[test]
    fn permissions_policy_expands_omitted_rule_fields_to_domain_wildcards() {
        let policy = permissions_policy(&AgentOsConfig {
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
        });

        let Some(agent_os_sidecar::protocol::FsPermissionScope::Rules(rules)) = policy.fs else {
            panic!("expected fs rule set");
        };
        assert_eq!(rules.default, Some(WirePermissionMode::Deny));
        assert_eq!(rules.rules[0].operations, vec!["*"]);
        assert_eq!(rules.rules[0].paths, vec!["/workspace/**"]);

        let policy = permissions_policy(&AgentOsConfig {
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
        });

        let Some(PatternPermissionScope::Rules(rules)) = policy.network else {
            panic!("expected network rule set");
        };
        assert_eq!(rules.default, Some(WirePermissionMode::Allow));
        assert_eq!(rules.rules[0].operations, vec!["*"]);
        assert_eq!(rules.rules[0].patterns, vec!["**"]);
    }
}
