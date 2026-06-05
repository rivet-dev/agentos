//! The `AgentOs` struct (all fields from ADR-001 §3), the `create` builder, and the `shutdown`
//! (dispose) teardown.
//!
//! `AgentOs` is `Arc`-cloneable; all interior state lives behind concurrent maps / atomics /
//! channels so `&self` methods never need an outer lock. Module files add only `impl AgentOs` blocks
//! and never introduce new struct fields.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use scc::{HashMap as SccHashMap, HashSet as SccHashSet};
use tokio::sync::{broadcast, oneshot, watch};
use tokio::task::JoinHandle;

use agent_os_sidecar::protocol::{
    ConfigureVmRequest, CreateVmRequest, DisposeReason, DisposeVmRequest, EventPayload,
    GuestRuntimeKind, OpenSessionRequest, OwnershipScope, PermissionsPolicy,
    RegisterToolkitRequest, RegisteredToolDefinition, RequestPayload, ResponsePayload,
    RootFilesystemDescriptor, SidecarPlacement, SidecarRequestPayload, SidecarResponsePayload,
    SoftwareDescriptor, ToolInvocationRequest, ToolInvocationResultResponse, VmLifecycleState,
};

use crate::config::{AgentOsConfig, HostTool, TimerScheduleDriver};
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
    pub(crate) processes: SccHashMap<u32, ProcessEntry>,
    /// Wire `process_id` allocator for `exec` (the kernel-process view). Distinct from the
    /// spawn synthetic-pid space so an `exec` call never perturbs the observable `spawn` pid sequence
    /// (TS `nextSyntheticPid` is advanced only by `spawn`, never by `exec`).
    pub(crate) process_counter: AtomicU64,
    /// Synthetic display-pid allocator for `spawn` (TS `nextSyntheticPid`, seeded at
    /// [`crate::process::SYNTHETIC_PID_BASE`]). The first spawned process gets `SYNTHETIC_PID_BASE`.
    pub(crate) synthetic_pid_counter: AtomicU64,
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
    pub(crate) acp_terminal_pids: SccHashSet<u32>,

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
                AgentOs::get_shared_sidecar(pool.clone()).await?
            }
            None => AgentOs::get_shared_sidecar(None).await?,
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
                ))
            }
        };
        let session_id = session.session_id;

        // 3. Subscribe to events BEFORE CreateVm so the `ready` lifecycle event cannot be missed.
        let mut events = transport.subscribe_events();

        // 4. Create the VM (session scope). Default root filesystem keeps the bundled base layer.
        let vm = match transport
            .request(
                OwnershipScope::session(&connection_id, &session_id),
                RequestPayload::CreateVm(CreateVmRequest {
                    runtime: GuestRuntimeKind::JavaScript,
                    metadata: BTreeMap::new(),
                    root_filesystem: RootFilesystemDescriptor::default(),
                    permissions: Some(PermissionsPolicy::allow_all()),
                }),
            )
            .await?
        {
            ResponsePayload::VmCreated(created) => created,
            ResponsePayload::Rejected(rejected) => return Err(rejected_to_error(rejected)),
            _ => {
                return Err(ClientError::Sidecar(
                    "unexpected create_vm response".to_string(),
                ))
            }
        };
        let vm_id = vm.vm_id;

        // 5. Wait for the VM to reach `ready` (bounded by VM_READY_TIMEOUT_MS).
        wait_for_vm_ready(&mut events, &vm_id, crate::VM_READY_TIMEOUT_MS).await?;

        // Resolve software packages to host roots (port of TS `processSoftware` for the
        // ConfigureVm descriptors). Each `package` is resolved under `module_access_cwd/node_modules`;
        // an unresolvable package is an explicit error rather than a silent no-op.
        let software = resolve_software(&config)?;

        // 6. Configure the VM (vm scope).
        match transport
            .request(
                OwnershipScope::vm(&connection_id, &session_id, &vm_id),
                RequestPayload::ConfigureVm(ConfigureVmRequest {
                    mounts: Vec::new(),
                    software,
                    permissions: Some(PermissionsPolicy::allow_all()),
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
                ))
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
                        ))
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
            processes: SccHashMap::new(),
            process_counter: AtomicU64::new(1),
            synthetic_pid_counter: AtomicU64::new(SYNTHETIC_PID_BASE),
            observed_process_start_times: SccHashMap::new(),
            observed_process_exit_times: SccHashMap::new(),
            shells: SccHashMap::new(),
            shell_counter: AtomicU64::new(0),
            pending_shell_exits: SccHashMap::new(),
            acp_terminal_pids: SccHashSet::new(),
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

        Ok(AgentOs {
            inner: Arc::new(inner),
        })
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

        // 2-5. Best-effort kill every tracked shell and drain its pending exit task (two-phase
        //      teardown, bounded by SHELL_DISPOSE_TIMEOUT_MS) so late shell output cannot race a
        //      closed transport.
        let mut exit_tasks = Vec::new();
        self.inner.pending_shell_exits.retain(|_, task| {
            exit_tasks.push(std::mem::replace(task, tokio::spawn(async {})));
            false
        });
        if !exit_tasks.is_empty() {
            let drain = async {
                for task in exit_tasks {
                    let _ = task.await;
                }
            };
            let _ = tokio::time::timeout(
                Duration::from_millis(crate::SHELL_DISPOSE_TIMEOUT_MS),
                drain,
            )
            .await;
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

/// Resolve `config.software` package inputs to `SoftwareDescriptor`s, each rooted at its host
/// `node_modules` directory under `module_access_cwd` (default `.`). Mirrors the TS `processSoftware`
/// mapping for the ConfigureVm descriptors. An unresolvable package is an explicit error, not a
/// silent no-op.
fn resolve_software(config: &AgentOsConfig) -> Result<Vec<SoftwareDescriptor>, ClientError> {
    if config.software.is_empty() {
        return Ok(Vec::new());
    }
    let module_access_cwd = config
        .module_access_cwd
        .clone()
        .unwrap_or_else(|| ".".to_string());
    let mut descriptors = Vec::with_capacity(config.software.len());
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
        descriptors.push(SoftwareDescriptor {
            package_name: input.package.clone(),
            root: root.to_string_lossy().into_owned(),
        });
    }
    Ok(descriptors)
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
