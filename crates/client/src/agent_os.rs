//! The `AgentOs` struct (all fields from ADR-001 §3), the `create` builder, and the `shutdown`
//! (dispose) teardown.
//!
//! `AgentOs` is `Arc`-cloneable; all interior state lives behind concurrent maps / atomics /
//! channels so `&self` methods never need an outer lock. Module files add only `impl AgentOs` blocks
//! and never introduce new struct fields.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use scc::HashMap as SccHashMap;
use serde_json::Value;
use tokio::sync::{broadcast, oneshot, watch};
use tokio::task::JoinHandle;

use agentos_protocol::generated::v1::{
    AcpCallback, AcpCallbackResponse, AcpEvent, AcpHostRequestCallbackResponse,
    AcpPermissionCallbackResponse,
};
use agentos_protocol::ACP_EXTENSION_NAMESPACE;
use agentos_sidecar_client::wire;
use agentos_vm_config as vm_config;

use crate::config::{
    AgentOsConfig, AgentOsLimits, HostTool, Permissions, RootFilesystemConfig, RootFilesystemKind,
    RootFilesystemMode as ConfigRootFilesystemMode, RootLowerInput, SidecarJsBridgeCall,
    SidecarJsBridgeCallback,
};
use crate::cron::CronManager;
use crate::error::ClientError;
use crate::json_rpc::JsonRpcNotification;
use crate::session::{
    record_live_session_event, AgentExitEvent, PermissionReply, PermissionRequest,
    PermissionRouteRequest, PermissionRouteResult,
};
use crate::sidecar::{AgentOsSidecar, AgentOsSidecarPlacement, AgentOsSidecarVmLease};
use crate::transport::{SidecarProcess, WireSidecarCallback};
use agentos_sidecar_client::TransportError;

use once_cell::sync::OnceCell;

// ---------------------------------------------------------------------------
// Registry entries
// ---------------------------------------------------------------------------

/// An SDK-spawned process (TS `_processes` value). Keyed by user-facing pid.
#[derive(Debug, Clone)]
pub(crate) enum ProcessExit {
    Exited(i32),
    Failed(String),
}

pub(crate) struct ProcessEntry {
    pub stdout_tx: broadcast::Sender<Vec<u8>>,
    pub stderr_tx: broadcast::Sender<Vec<u8>>,
    /// Seeded `None`; the already-exited branch fires immediately once it holds `Some(code)`.
    pub exit_tx: watch::Sender<Option<ProcessExit>>,
    /// The sidecar-side process id used on the wire.
    pub process_id: String,
    /// Handles for the per-process output-callback tasks seeded at spawn (`on_stdout`/`on_stderr`).
    /// The entry retains its own `stdout_tx`/`stderr_tx` clones for late subscribers, so these tasks
    /// never observe the broadcast `Closed`; `shutdown` aborts them when draining the registry.
    pub output_tasks: Vec<JoinHandle<()>>,
}

/// A PTY-backed shell host route, keyed by the sidecar-owned process id.
///
/// `data_tx` carries stdout only, matching TS where the kernel handle's `onData` is fed exclusively
/// by `stdoutHandlers`. `stderr_tx` is the dedicated stderr channel that backs the `on_stderr` option
/// and `on_shell_stderr`, matching TS where stderr reaches the host only through `stderrHandlers`.
pub(crate) struct ShellEntry {
    pub data_tx: broadcast::Sender<Vec<u8>>,
    pub stderr_tx: broadcast::Sender<Vec<u8>>,
    /// The sidecar-side process id used on the wire.
    pub process_id: String,
    /// Exit-code channel backing `wait_shell` (TS `ShellHandle.wait`). Seeded `None`; the background
    /// event loop publishes `Some(exit_code)` when the shell process exits.
    pub exit_tx: watch::Sender<Option<i32>>,
    /// Host handle state after an explicit close. Process lifecycle remains
    /// sidecar-owned; this only prevents further writes through a closed handle.
    pub closing: AtomicBool,
}

/// An ACP session (TS `_sessions` value). Keyed by ACP session id.
pub(crate) struct SessionEntry {
    pub event_tx: broadcast::Sender<JsonRpcNotification>,
    pub permission_tx: broadcast::Sender<PermissionRequest>,
    pub agent_exit_tx: broadcast::Sender<AgentExitEvent>,
    pub pending_permission_replies: SccHashMap<String, oneshot::Sender<PermissionReply>>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedAgent {
    pub id: String,
    pub acp_entrypoint: String,
    pub adapter_entrypoint: String,
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
    /// Projected agents reported by the sidecar.
    pub(crate) projected_agents: parking_lot::Mutex<Vec<ProjectedAgent>>,

    // Process registries.
    pub(crate) process_registry_lock: parking_lot::Mutex<()>,
    pub(crate) processes: SccHashMap<u32, ProcessEntry>,
    // Shell registries.
    pub(crate) shells: SccHashMap<String, ShellEntry>,
    pub(crate) pending_shell_exits: SccHashMap<String, JoinHandle<()>>,

    // Session registries.
    pub(crate) sessions: SccHashMap<String, SessionEntry>,

    // Cron.
    pub(crate) cron: Arc<CronManager>,

    // Lifecycle.
    pub(crate) sidecar: Arc<AgentOsSidecar>,
    pub(crate) sidecar_lease: parking_lot::Mutex<Option<AgentOsSidecarVmLease>>,
    pub(crate) disposed: AtomicBool,
    /// Handle for the background ACP event-pump task (`spawn_acp_event_pump`). Stored so `shutdown`
    /// can abort it; the pump only exits on its own when the shared transport's event channel closes,
    /// which does not happen while sibling VMs keep the transport alive. Mirrors `pending_shell_exits`.
    pub(crate) acp_event_pump: parking_lot::Mutex<Option<JoinHandle<()>>>,
}

impl AgentOs {
    /// The sole public VM entry point. It forwards explicit configuration in one atomic sidecar
    /// initialization request, takes a lease, and constructs the thin host adapters. VM readiness,
    /// defaults, configuration ordering, and rollback are sidecar-owned.
    pub async fn create(options: AgentOsConfig) -> Result<AgentOs, ClientError> {
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
        let (transport, connection_id, _) = sidecar.ensure_connection().await?;

        // 2. Open a session for this VM (connection scope) on the shared connection.
        let session = match transport
            .request_wire(
                wire_connection_ownership(&connection_id),
                wire::RequestPayload::OpenSessionRequest(wire::OpenSessionRequest {
                    placement: sidecar_wire_placement(&sidecar),
                }),
            )
            .await?
        {
            wire::ResponsePayload::SessionOpenedResponse(opened) => opened,
            wire::ResponsePayload::RejectedResponse(rejected) => {
                return Err(rejected_to_error(rejected));
            }
            other => {
                return Err(ClientError::Sidecar(format!(
                    "unexpected open_session response: {other:?}"
                )));
            }
        };
        let session_id = session.session_id;

        // 3. Serialize explicit caller input. Omitted collections remain omitted so the sidecar
        //    owns defaults rather than receiving client-authored empty/default policy.
        let create_vm_config = serialize_create_vm_config_for_sidecar(&config)?;
        let packages = build_package_descriptors(&config);
        let mounts = serialize_mounts(&config)?;
        let mut tool_map: HashMap<String, HostTool> = HashMap::new();
        let mut host_callbacks = Vec::with_capacity(config.tool_kits.len());
        for kit in &config.tool_kits {
            let mut callbacks = HashMap::new();
            for tool in &kit.tools {
                callbacks.insert(
                    tool.name.clone(),
                    wire::RegisteredHostCallbackDefinition {
                        description: tool.description.clone(),
                        input_schema: json_utf8(&tool.input_schema, "host callback input schema")?,
                        timeout_ms: tool.timeout_ms,
                        examples: Vec::new(),
                    },
                );
                tool_map.insert(format!("{}:{}", kit.name, tool.name), tool.clone());
            }
            host_callbacks.push(wire::RegisterHostCallbacksRequest {
                name: kit.name.clone(),
                description: kit.description.clone(),
                callbacks,
            });
        }

        // JS-backed mounts are the one host-only route initialization itself may call: the sidecar
        // can perform VFS operations while applying explicit mount descriptors. Install that route
        // before the request, but keep all bootstrap/default policy in the sidecar.
        let js_bridge_session_key = config.sidecar_js_bridge_callback.clone().map(|callback| {
            let key = sidecar_session_key(&connection_id, &session_id);
            let _ = session_js_bridge_callbacks().insert(key.clone(), callback);
            transport.register_wire_callback("js_bridge_call", js_bridge_call_callback());
            key
        });

        // 4. Create, await readiness, configure, and register callback metadata as one sidecar-owned
        //    transaction. On any failure the sidecar disposes the partially initialized VM.
        let initialization_response = match transport
            .request_wire(
                wire_session_ownership(&connection_id, &session_id),
                wire::RequestPayload::InitializeVmRequest(wire::InitializeVmRequest {
                    runtime: wire::GuestRuntimeKind::JavaScript,
                    config: serde_json::to_string(&create_vm_config).map_err(|error| {
                        ClientError::Sidecar(format!(
                            "failed to serialize create VM config: {error}"
                        ))
                    })?,
                    mounts: (!mounts.is_empty()).then_some(mounts),
                    packages: (!packages.is_empty()).then_some(packages),
                    packages_mount_at: config.packages_mount_at.clone(),
                    host_callbacks: (!host_callbacks.is_empty()).then_some(host_callbacks),
                }),
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                if let Some(key) = &js_bridge_session_key {
                    let _ = session_js_bridge_callbacks().remove(key);
                }
                return Err(error.into());
            }
        };
        let initialized = match initialization_response {
            wire::ResponsePayload::VmInitializedResponse(initialized) => initialized,
            wire::ResponsePayload::RejectedResponse(rejected) => {
                if let Some(key) = &js_bridge_session_key {
                    let _ = session_js_bridge_callbacks().remove(key);
                }
                return Err(rejected_to_error(rejected));
            }
            other => {
                if let Some(key) = &js_bridge_session_key {
                    let _ = session_js_bridge_callbacks().remove(key);
                }
                return Err(ClientError::Sidecar(format!(
                    "unexpected initialize_vm response: {other:?}"
                )));
            }
        };
        let vm_id = initialized.vm_id;
        let projected_commands = initialized
            .projected_commands
            .into_iter()
            .map(|command| (command.name, command.guest_path))
            .collect();
        let projected_agents = projected_agents_from_wire(initialized.agents);

        // Tool callback implementations are host-only state but are not invoked during metadata
        // registration, so install them only after the sidecar commits initialization.
        if !tool_map.is_empty() {
            let _ = vm_tools().insert(vm_id.clone(), Arc::new(VmHostToolRegistry { tool_map }));
            transport.register_wire_callback("host_callback", host_callback_callback());
        }

        // 7. Lease this VM on the (possibly shared) sidecar, build cron, and assemble the client.
        sidecar.active_vm_count.fetch_add(1, Ordering::SeqCst);
        let lease = AgentOsSidecarVmLease {
            sidecar: sidecar.clone(),
        };

        let cron = Arc::new(CronManager::new());

        let inner = AgentOsInner {
            transport,
            connection_id,
            session_id,
            vm_id,
            projected_commands: parking_lot::Mutex::new(projected_commands),
            projected_agents: parking_lot::Mutex::new(projected_agents),
            process_registry_lock: parking_lot::Mutex::new(()),
            processes: SccHashMap::new(),
            shells: SccHashMap::new(),
            pending_shell_exits: SccHashMap::new(),
            sessions: SccHashMap::new(),
            cron,
            sidecar,
            sidecar_lease: parking_lot::Mutex::new(Some(lease)),
            disposed: AtomicBool::new(false),
            acp_event_pump: parking_lot::Mutex::new(None),
        };

        let client = AgentOs {
            inner: Arc::new(inner),
        };
        // Register the permission router and callback unconditionally (unlike `host_callback`,
        // which is gated on configured tool kits): any agent session can raise a permission
        // request. Re-registering on a shared transport replaces an identical stateless callback,
        // same as the `host_callback` pattern.
        let _ = vm_permission_routers()
            .insert(client.inner.vm_id.clone(), Arc::downgrade(&client.inner));
        client
            .inner
            .transport
            .register_wire_callback("ext", permission_request_callback());
        spawn_acp_event_pump(&client);
        Ok(client)
    }

    /// Dispose the VM (= TS `dispose`). Teardown order:
    /// 1. cron dispose
    /// 2. close all sessions (swallow errors)
    /// 3. kill all shells + snapshot pending exits
    /// 4. drain tracked shell-exit tasks (two-phase, bounded by
    ///    [`crate::SHELL_DISPOSE_TIMEOUT_MS`])
    /// 5. unregister the sidecar event listener
    /// 6. release the lease (or tear down the transport)
    ///
    /// Idempotent (guarded by `disposed`).
    /// Dynamically link a software package into the RUNNING VM (parity with the
    /// TS client's `linkSoftware`). Forwarded to the sidecar, which owns the
    /// `/opt/agentos` projection and appends the package to its live staging dir,
    /// so the package's commands appear under `/opt/agentos/bin` (on `$PATH`)
    /// immediately with no reboot. Errors if a command name is already linked.
    pub async fn link_software(&self, descriptor: PackageDescriptor) -> Result<(), ClientError> {
        let inner = self.inner();
        let response = self
            .transport()
            .request_wire(
                wire_vm_ownership(&inner.connection_id, &inner.session_id, &inner.vm_id),
                wire::RequestPayload::LinkPackageRequest(wire::LinkPackageRequest {
                    // The wire `PackageDescriptor` carries the packed package
                    // `path`; the sidecar reads metadata from that payload.
                    package: wire::PackageDescriptor {
                        path: descriptor.path,
                    },
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::PackageLinkedResponse(linked) => {
                let mut guard = inner.projected_commands.lock();
                for command in linked.projected_commands {
                    guard.insert(command.name, command.guest_path);
                }
                register_projected_agents(
                    &inner.projected_agents,
                    projected_agents_from_wire(linked.agents),
                );
                Ok(())
            }
            wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
            other => Err(ClientError::Sidecar(format!(
                "unexpected link_package response: {other:?}"
            ))),
        }
    }

    pub async fn provided_commands(&self) -> Result<BTreeMap<String, Vec<String>>, ClientError> {
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
                .map(|package| (package.package_name, package.commands))
                .collect()),
            wire::ResponsePayload::RejectedResponse(rejected) => Err(rejected_to_error(rejected)),
            other => Err(ClientError::Sidecar(format!(
                "unexpected provided_commands response: {other:?}"
            ))),
        }
    }

    pub async fn shutdown(&self) -> Result<(), ClientError> {
        // Idempotent: only the first caller runs teardown.
        if self.inner.disposed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        // The `/opt/agentos` projection staging dir is owned + cleaned up by the
        // sidecar on VM dispose, so the client no longer removes it here.

        // 1. Cron dispose (abort the host alarm and release callback routes).
        self.inner.cron.dispose();

        // Abort the background ACP event pump and drain the SDK-spawned process registry. Neither
        // ends on its own while a shared transport stays alive: the pump only exits on transport
        // close, and the per-process output tasks await a broadcast `Closed` that the entry's own
        // retained sender clones prevent. Aborting + clearing here stops both from leaking past
        // dispose.
        abort_tracked_task(&self.inner.acp_event_pump);
        crate::process::drain_process_output_tasks(&self.inner.processes);

        // 2-4. Best-effort drain tracked shell tasks before the VM is disposed, bounded by
        //      SHELL_DISPOSE_TIMEOUT_MS so late output cannot race a closed transport.
        let mut exit_tasks = Vec::new();
        self.inner.pending_shell_exits.retain(|_, task| {
            exit_tasks.push(std::mem::replace(task, tokio::spawn(async {})));
            false
        });

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

        // 5-6. Release this VM (DisposeVm best-effort) and its lease. The transport is shared across
        //      VMs on the same sidecar, so it is only torn down when this was the last VM (matching
        //      the TS lease/shared-sidecar lifecycle); otherwise sibling VMs keep using it.
        let lease = self.inner.sidecar_lease.lock().take();
        let _ = self
            .transport()
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
            .await;
        let _ = vm_tools().remove(&self.inner.vm_id);
        let _ = vm_permission_routers().remove(&self.inner.vm_id);
        let _ = session_js_bridge_callbacks().remove(&sidecar_session_key(
            &self.inner.connection_id,
            &self.inner.session_id,
        ));
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

    pub(crate) fn downgrade_inner(&self) -> Weak<AgentOsInner> {
        Arc::downgrade(&self.inner)
    }

    pub(crate) fn from_inner(inner: Arc<AgentOsInner>) -> Self {
        Self { inner }
    }

    /// The (possibly shared) sidecar handle backing this VM. Public for parity with TS
    /// `AgentOs.sidecar` (e.g. `describe()` reports `active_vm_count` across VMs sharing a pool).
    pub fn sidecar(&self) -> Arc<AgentOsSidecar> {
        self.inner.sidecar.clone()
    }

    pub fn projected_agents(&self) -> Vec<ProjectedAgent> {
        self.inner.projected_agents.lock().clone()
    }
}

/// Abort and clear a single tracked background-task handle (e.g. the ACP event pump) so it cannot
/// outlive the disposed VM. Mirrors the `pending_shell_exits` drain in `shutdown`.
fn abort_tracked_task(slot: &parking_lot::Mutex<Option<JoinHandle<()>>>) {
    if let Some(handle) = slot.lock().take() {
        handle.abort();
    }
}

fn spawn_acp_event_pump(client: &AgentOs) {
    let mut events = client.transport().subscribe_wire_events();
    let inner = Arc::downgrade(&client.inner);
    let handle = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => match &event.payload {
                    wire::EventPayload::ExtEnvelope(envelope) => {
                        let Some(inner) = inner.upgrade() else {
                            break;
                        };
                        if inner.disposed.load(Ordering::SeqCst) {
                            break;
                        }
                        if wire_ownership_vm_id(&event.ownership) != Some(inner.vm_id.as_str()) {
                            continue;
                        }
                        if let Err(error) = deliver_acp_ext_event(&inner, envelope) {
                            tracing::warn!(?error, "failed to deliver acp extension event");
                        }
                    }
                    wire::EventPayload::CronDispatchEvent(dispatch) => {
                        let Some(inner) = inner.upgrade() else {
                            break;
                        };
                        if inner.disposed.load(Ordering::SeqCst)
                            || wire_ownership_vm_id(&event.ownership) != Some(inner.vm_id.as_str())
                        {
                            continue;
                        }
                        let client = AgentOs::from_inner(inner.clone());
                        if let Err(error) = inner
                            .cron
                            .consume_dispatch(
                                &client,
                                dispatch.alarm.clone(),
                                dispatch.runs.clone(),
                                dispatch.events.clone(),
                            )
                            .await
                        {
                            tracing::error!(?error, "failed to consume sidecar cron dispatch");
                        }
                    }
                    wire::EventPayload::VmLifecycleEvent(_)
                    | wire::EventPayload::ProcessOutputEvent(_)
                    | wire::EventPayload::ProcessExitedEvent(_)
                    | wire::EventPayload::StructuredEvent(_) => {}
                },
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    *client.inner.acp_event_pump.lock() = Some(handle);
}

fn deliver_acp_ext_event(
    inner: &AgentOsInner,
    envelope: &wire::ExtEnvelope,
) -> Result<(), ClientError> {
    if envelope.namespace != ACP_EXTENSION_NAMESPACE {
        return Ok(());
    }
    let event: AcpEvent = serde_bare::from_slice(&envelope.payload)
        .map_err(|error| ClientError::Sidecar(format!("invalid ACP event: {error}")))?;
    match event {
        AcpEvent::AcpSessionEvent(event) => {
            let notification: JsonRpcNotification = serde_json::from_str(&event.notification)
                .map_err(|error| {
                    ClientError::Sidecar(format!("invalid ACP session notification: {error}"))
                })?;
            let delivered = inner
                .sessions
                .read(&event.session_id, |_, entry| {
                    record_live_session_event(entry, notification.clone());
                })
                .is_some();
            if !delivered {
                tracing::warn!(
                    session_id = event.session_id,
                    "received acp event for unknown session"
                );
            }
            Ok(())
        }
        AcpEvent::AcpAgentStderrEvent(event) => {
            if !event.session_id.is_empty()
                && inner.sessions.read(&event.session_id, |_, _| ()).is_none()
            {
                tracing::warn!(
                    session_id = event.session_id,
                    agent_type = event.agent_type,
                    process_id = event.process_id,
                    "received acp stderr event for unknown session"
                );
            }

            let mut stderr = std::io::stderr().lock();
            if let Err(error) = stderr.write_all(&event.chunk).and_then(|_| stderr.flush()) {
                tracing::warn!(?error, "failed to write acp stderr event");
            }
            Ok(())
        }
        AcpEvent::AcpAgentExitedEvent(event) => {
            tracing::warn!(
                session_id = event.session_id,
                agent_type = event.agent_type,
                process_id = event.process_id,
                exit_code = ?event.exit_code,
                restart = event.restart,
                restart_count = event.restart_count,
                max_restarts = event.max_restarts,
                "acp agent adapter exited unexpectedly"
            );
            let delivered = inner
                .sessions
                .read(&event.session_id, |_, entry| {
                    let _ = entry.agent_exit_tx.send(AgentExitEvent {
                        session_id: event.session_id.clone(),
                        agent_type: event.agent_type.clone(),
                        process_id: event.process_id.clone(),
                        exit_code: event.exit_code,
                        restart: event.restart.clone(),
                        restart_count: event.restart_count,
                        max_restarts: event.max_restarts,
                    });
                })
                .is_some();
            if !delivered {
                tracing::warn!(
                    session_id = event.session_id,
                    "received acp agent exit event for unknown session"
                );
            }
            Ok(())
        }
    }
}

/// Convert a sidecar's client-side placement into the wire `SidecarPlacement` for OpenSession.
fn sidecar_wire_placement(sidecar: &AgentOsSidecar) -> wire::SidecarPlacement {
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
    let root_filesystem =
        (root_filesystem != vm_config::RootFilesystemConfig::default()).then_some(root_filesystem);
    Ok(vm_config::CreateVmConfig {
        cwd: None,
        env: None,
        root_filesystem,
        permissions: config.permissions.as_ref().map(permissions_policy_config),
        limits: serialize_limits_config_for_sidecar(config.limits.as_ref())?,
        dns: None,
        native_root,
        listen: None,
        loopback_exempt_ports: (!config.loopback_exempt_ports.is_empty())
            .then(|| config.loopback_exempt_ports.clone()),
        // 0.3: the Node builtin allow-list moved from ConfigureVmRequest to
        // VM creation. `None` => engine default allow-list; `Some([..])` =>
        // exactly those (`Some([])` denies all). Platform/module-resolution
        // keep their engine defaults (full Node emulation), matching prior
        // behavior where Agent OS only ever constrained the builtin allow-list.
        js_runtime: (config.allowed_node_builtins.is_some()
            || config.high_resolution_time.is_some())
        .then(|| vm_config::JsRuntimeConfig {
            allowed_builtins: config.allowed_node_builtins.clone(),
            high_resolution_time: config.high_resolution_time,
            ..Default::default()
        }),
        agent_additional_instructions: config.additional_instructions.clone(),
    })
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
    let mode = config.mode.map(|mode| match mode {
        ConfigRootFilesystemMode::Ephemeral => vm_config::RootFilesystemMode::Ephemeral,
        ConfigRootFilesystemMode::ReadOnly => vm_config::RootFilesystemMode::ReadOnly,
    });
    let disable_default_base_layer = config.disable_default_base_layer.then_some(true);
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
                    disable_default_base_layer,
                    lowers: (!lowers.is_empty()).then_some(lowers),
                    bootstrap_entries: None,
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
                    disable_default_base_layer,
                    lowers: None,
                    bootstrap_entries: None,
                },
                Some(vm_config::NativeRootFilesystemConfig {
                    plugin: vm_config::MountPluginDescriptor {
                        id: plugin.id.clone(),
                        config: plugin.config.clone(),
                    },
                    read_only: config
                        .mode
                        .map(|mode| mode == ConfigRootFilesystemMode::ReadOnly),
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

fn permissions_policy_config(permissions: &Permissions) -> vm_config::PermissionsPolicy {
    vm_config::PermissionsPolicy {
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
        binding: permissions
            .binding
            .as_ref()
            .map(serialize_pattern_permissions_config),
    }
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
                        operations: rule.operations.clone(),
                        paths: rule.paths.clone(),
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
                        operations: rule.operations.clone(),
                        patterns: rule.patterns.clone(),
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

/// Process-global per-VM host-tool registry. The shared transport's single host-callback routes to
/// the right VM's toolkits by frame ownership.
static VM_TOOLS: OnceCell<SccHashMap<String, Arc<VmHostToolRegistry>>> = OnceCell::new();

#[derive(Clone)]
struct VmHostToolRegistry {
    tool_map: HashMap<String, HostTool>,
}

fn vm_tools() -> &'static SccHashMap<String, Arc<VmHostToolRegistry>> {
    VM_TOOLS.get_or_init(SccHashMap::new)
}

/// Process-global map of vm id -> client inner, so the shared `permission_request` transport
/// callback can route a sidecar permission request to the owning client. `Weak` so the registry
/// never extends a client's lifetime; entries are removed in `shutdown`.
static VM_PERMISSION_ROUTERS: OnceCell<SccHashMap<String, Weak<AgentOsInner>>> = OnceCell::new();

fn vm_permission_routers() -> &'static SccHashMap<String, Weak<AgentOsInner>> {
    VM_PERMISSION_ROUTERS.get_or_init(SccHashMap::new)
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

/// The transport callback that answers sidecar permission requests by routing them to the owning
/// client's `on_permission_request` subscribers. Mirrors TS `_handlePermissionSidecarRequest`.
fn permission_request_callback() -> WireSidecarCallback {
    Arc::new(|payload, ownership| {
        Box::pin(async move {
            match payload {
                wire::SidecarRequestPayload::ExtEnvelope(envelope) => {
                    handle_acp_ext_callback(envelope, &ownership)
                        .await
                        .map_err(|error| TransportError::Sidecar(error.to_string()))
                }
                wire::SidecarRequestPayload::HostCallbackRequest(_)
                | wire::SidecarRequestPayload::JsBridgeCallRequest(_) => Ok(
                    wire::SidecarResponsePayload::ExtEnvelope(wire::ExtEnvelope {
                        namespace: ACP_EXTENSION_NAMESPACE.to_string(),
                        payload: b"permission callback received a non-extension request".to_vec(),
                    }),
                ),
            }
        })
    })
}

async fn handle_acp_ext_callback(
    envelope: wire::ExtEnvelope,
    ownership: &wire::OwnershipScope,
) -> Result<wire::SidecarResponsePayload, ClientError> {
    if envelope.namespace != ACP_EXTENSION_NAMESPACE {
        return Ok(wire::SidecarResponsePayload::ExtEnvelope(
            wire::ExtEnvelope {
                namespace: envelope.namespace,
                payload: b"unknown extension namespace".to_vec(),
            },
        ));
    }
    let callback: AcpCallback = serde_bare::from_slice(&envelope.payload)
        .map_err(|error| ClientError::Sidecar(format!("invalid ACP callback: {error}")))?;
    let response = match callback {
        AcpCallback::AcpPermissionCallback(callback) => {
            let params = serde_json::from_str(&callback.params).map_err(|error| {
                ClientError::Sidecar(format!(
                    "invalid ACP permission callback params for {}: {error}",
                    callback.permission_id
                ))
            })?;
            let result = route_permission_request(
                ownership,
                PermissionRouteRequest {
                    session_id: callback.session_id,
                    permission_id: callback.permission_id.clone(),
                    params,
                    timeout_ms: callback.timeout_ms,
                },
            )
            .await;
            AcpCallbackResponse::AcpPermissionCallbackResponse(AcpPermissionCallbackResponse {
                permission_id: callback.permission_id,
                reply: result.reply,
            })
        }
        AcpCallback::AcpHostRequestCallback(_) => {
            AcpCallbackResponse::AcpHostRequestCallbackResponse(AcpHostRequestCallbackResponse {
                response: None,
            })
        }
    };
    let payload = serde_bare::to_vec(&response).map_err(|error| {
        ClientError::Sidecar(format!("failed to encode ACP callback response: {error}"))
    })?;
    Ok(wire::SidecarResponsePayload::ExtEnvelope(
        wire::ExtEnvelope {
            namespace: ACP_EXTENSION_NAMESPACE.to_string(),
            payload,
        },
    ))
}

async fn route_permission_request(
    ownership: &wire::OwnershipScope,
    request: PermissionRouteRequest,
) -> PermissionRouteResult {
    let Some(vm_id) = wire_ownership_vm_id(ownership) else {
        return PermissionRouteResult { reply: None };
    };
    let inner = vm_permission_routers()
        .read(vm_id, |_, weak| weak.clone())
        .and_then(|weak| weak.upgrade());
    let Some(inner) = inner else {
        return PermissionRouteResult { reply: None };
    };
    let client = AgentOs { inner };
    client.deliver_sidecar_permission_request(request).await
}

/// The transport callback that answers guest tool invocations by running the matching host tool.
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
                            error: Some("host-callback received a non-tool request".to_string()),
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

/// Run a single tool invocation against the per-VM host-tool registry. The sidecar owns parsing,
/// permission checks, and the authoritative timeout; the host validates the forwarded input and
/// executes the callback.
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
    let Some(vm_id) = wire_ownership_vm_id(ownership) else {
        return wire::HostCallbackResultResponse {
            invocation_id: request.invocation_id,
            result: None,
            error: Some(String::from("host callback is missing VM ownership")),
        };
    };
    let registry = vm_tools().read(vm_id, |_, registry| registry.clone());
    let Some(registry) = registry else {
        return wire::HostCallbackResultResponse {
            invocation_id: request.invocation_id,
            result: None,
            error: Some(format!("Unknown tool \"{}\"", request.callback_key)),
        };
    };

    let tool = registry.tool_map.get(&request.callback_key).cloned();
    let Some(tool) = tool else {
        return wire::HostCallbackResultResponse {
            invocation_id: request.invocation_id,
            result: None,
            error: Some(format!("Unknown tool \"{}\"", request.callback_key)),
        };
    };
    if let Err(error) = validate_tool_input(&tool.input_schema, &input) {
        return wire::HostCallbackResultResponse {
            invocation_id: request.invocation_id,
            result: None,
            error: Some(error.to_string()),
        };
    }
    match (tool.execute)(input).await {
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
    }
}

fn host_callback_json_result(value: Value) -> Result<String, String> {
    serde_json::to_string(&value).map_err(|error| format!("Invalid host callback result: {error}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolInputSchemaViolation {
    path: String,
    expected: String,
    actual: String,
}

impl ToolInputSchemaViolation {
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

impl std::fmt::Display for ToolInputSchemaViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ToolInputSchemaViolation at {}: expected {}, got {}",
            self.path, self.expected, self.actual
        )
    }
}

fn validate_tool_input(schema: &Value, input: &Value) -> Result<(), ToolInputSchemaViolation> {
    validate_tool_input_at_path(schema, input, "$")
}

fn validate_tool_input_at_path(
    schema: &Value,
    input: &Value,
    path: &str,
) -> Result<(), ToolInputSchemaViolation> {
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
        return Err(ToolInputSchemaViolation::new(
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
        return Err(ToolInputSchemaViolation::new(
            path,
            format!("constant {}", compact_json(expected)),
            describe_value(input),
        ));
    }

    match schema.get("type") {
        Some(Value::String(expected_type)) => {
            validate_typed_tool_input(schema, input, path, expected_type)
        }
        Some(Value::Array(expected_types)) => {
            let mut first_error = None;
            for expected_type in expected_types.iter().filter_map(Value::as_str) {
                match validate_typed_tool_input(schema, input, path, expected_type) {
                    Ok(()) => return Ok(()),
                    Err(error) if first_error.is_none() => first_error = Some(error),
                    Err(_) => {}
                }
            }
            Err(first_error.unwrap_or_else(|| {
                ToolInputSchemaViolation::new(
                    path,
                    describe_expected(schema),
                    describe_value(input),
                )
            }))
        }
        Some(_) => Ok(()),
        None if has_object_keywords(schema) => {
            validate_typed_tool_input(schema, input, path, "object")
        }
        None => Ok(()),
    }
}

fn validate_schema_branches(
    branches: &[Value],
    input: &Value,
    path: &str,
    keyword: &str,
) -> Result<(), ToolInputSchemaViolation> {
    let mut first_error = None;
    for branch in branches {
        match validate_tool_input_at_path(branch, input, path) {
            Ok(()) => return Ok(()),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    Err(first_error.unwrap_or_else(|| {
        ToolInputSchemaViolation::new(
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

fn validate_typed_tool_input(
    schema: &Value,
    input: &Value,
    path: &str,
    expected_type: &str,
) -> Result<(), ToolInputSchemaViolation> {
    match expected_type {
        "null" if input.is_null() => Ok(()),
        "null" => Err(type_violation(path, expected_type, input)),
        "boolean" if input.is_boolean() => Ok(()),
        "boolean" => Err(type_violation(path, expected_type, input)),
        "string" => validate_string_tool_input(schema, input, path),
        "number" => validate_number_tool_input(schema, input, path, false),
        "integer" => validate_number_tool_input(schema, input, path, true),
        "array" => validate_array_tool_input(schema, input, path),
        "object" => validate_object_tool_input(schema, input, path),
        _ => Ok(()),
    }
}

fn validate_string_tool_input(
    schema: &Value,
    input: &Value,
    path: &str,
) -> Result<(), ToolInputSchemaViolation> {
    let Some(value) = input.as_str() else {
        return Err(type_violation(path, "string", input));
    };
    if let Some(min_length) = schema.get("minLength").and_then(Value::as_u64) {
        if value.chars().count() < min_length as usize {
            return Err(ToolInputSchemaViolation::new(
                path,
                format!("string with minLength {min_length}"),
                format!("string length {}", value.chars().count()),
            ));
        }
    }
    if let Some(max_length) = schema.get("maxLength").and_then(Value::as_u64) {
        if value.chars().count() > max_length as usize {
            return Err(ToolInputSchemaViolation::new(
                path,
                format!("string with maxLength {max_length}"),
                format!("string length {}", value.chars().count()),
            ));
        }
    }
    Ok(())
}

fn validate_number_tool_input(
    schema: &Value,
    input: &Value,
    path: &str,
    expect_integer: bool,
) -> Result<(), ToolInputSchemaViolation> {
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
            return Err(ToolInputSchemaViolation::new(
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
            return Err(ToolInputSchemaViolation::new(
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
            return Err(ToolInputSchemaViolation::new(
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
            return Err(ToolInputSchemaViolation::new(
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

fn validate_array_tool_input(
    schema: &Value,
    input: &Value,
    path: &str,
) -> Result<(), ToolInputSchemaViolation> {
    let Some(items) = input.as_array() else {
        return Err(type_violation(path, "array", input));
    };
    if let Some(min_items) = schema.get("minItems").and_then(Value::as_u64) {
        if items.len() < min_items as usize {
            return Err(ToolInputSchemaViolation::new(
                path,
                format!("array with minItems {min_items}"),
                format!("array length {}", items.len()),
            ));
        }
    }
    if let Some(max_items) = schema.get("maxItems").and_then(Value::as_u64) {
        if items.len() > max_items as usize {
            return Err(ToolInputSchemaViolation::new(
                path,
                format!("array with maxItems {max_items}"),
                format!("array length {}", items.len()),
            ));
        }
    }
    if let Some(item_schema) = schema.get("items") {
        for (index, item) in items.iter().enumerate() {
            validate_tool_input_at_path(item_schema, item, &format!("{path}[{index}]"))?;
        }
    }
    Ok(())
}

fn validate_object_tool_input(
    schema: &Value,
    input: &Value,
    path: &str,
) -> Result<(), ToolInputSchemaViolation> {
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
            return Err(ToolInputSchemaViolation::new(
                field_path,
                expected,
                "missing value",
            ));
        }
    }
    for (field, value) in object {
        let field_path = format!("{path}.{field}");
        if let Some(field_schema) = properties.get(field) {
            validate_tool_input_at_path(field_schema, value, &field_path)?;
            continue;
        }
        match schema.get("additionalProperties") {
            Some(Value::Bool(false)) => {
                return Err(ToolInputSchemaViolation::new(
                    field_path,
                    "no additional properties",
                    describe_value(value),
                ));
            }
            Some(additional_schema) => {
                validate_tool_input_at_path(additional_schema, value, &field_path)?;
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

fn type_violation(path: &str, expected: &str, input: &Value) -> ToolInputSchemaViolation {
    ToolInputSchemaViolation::new(path, expected, describe_value(input))
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

/// Build the wire [`wire::PackageDescriptor`]s for the `/opt/agentos` projection.
/// The sidecar reads package metadata from the forwarded package path.
fn build_package_descriptors(config: &AgentOsConfig) -> Vec<wire::PackageDescriptor> {
    config
        .packages
        .iter()
        .map(|package| wire::PackageDescriptor {
            path: package.path.clone(),
        })
        .collect()
}

fn projected_agents_from_wire(agents: Vec<wire::AgentosProjectedAgent>) -> Vec<ProjectedAgent> {
    agents
        .into_iter()
        .map(|agent| ProjectedAgent {
            id: agent.id,
            acp_entrypoint: agent.acp_entrypoint,
            adapter_entrypoint: agent.adapter_entrypoint,
        })
        .collect()
}

fn register_projected_agents(
    projected_agents: &parking_lot::Mutex<Vec<ProjectedAgent>>,
    agents: Vec<ProjectedAgent>,
) {
    let mut guard = projected_agents.lock();
    for agent in agents {
        guard.retain(|existing| existing.id != agent.id);
        guard.push(agent);
    }
}

fn serialize_mounts(config: &AgentOsConfig) -> Result<Vec<wire::MountDescriptor>, ClientError> {
    config
        .mounts
        .iter()
        .map(|mount| {
            Ok(wire::MountDescriptor {
                guest_path: mount.path.clone(),
                read_only: mount.read_only,
                plugin: wire::MountPluginDescriptor {
                    id: mount.plugin.id.clone(),
                    config: mount
                        .plugin
                        .config
                        .as_ref()
                        .map(|config| json_utf8(config, "native mount plugin config"))
                        .transpose()?,
                },
            })
        })
        .collect()
}

fn json_utf8(value: &serde_json::Value, context: &str) -> Result<String, ClientError> {
    serde_json::to_string(value)
        .map_err(|error| ClientError::Sidecar(format!("failed to serialize {context}: {error}")))
}

/// Extract the `vm_id` from a generated ownership scope, if it is VM-scoped.
fn wire_ownership_vm_id(ownership: &wire::OwnershipScope) -> Option<&str> {
    match ownership {
        wire::OwnershipScope::VmOwnership(ownership) => Some(ownership.vm_id.as_str()),
        wire::OwnershipScope::ConnectionOwnership(_)
        | wire::OwnershipScope::SessionOwnership(_) => None,
    }
}

/// Map a `Rejected` response into a [`ClientError::Kernel`] so the errno `code` survives.
fn rejected_to_error(rejected: wire::RejectedResponse) -> ClientError {
    ClientError::Kernel {
        code: rejected.code,
        message: rejected.message,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        abort_tracked_task, handle_acp_ext_callback, permissions_policy_config,
        serialize_create_vm_config_for_sidecar, serialize_mounts,
        serialize_root_filesystem_config_for_sidecar, wire_connection_ownership, JoinHandle,
    };
    use crate::config::{
        AgentOsConfig, AgentOsLimits, FsPermissionRule, FsPermissions, HttpLimits, JsRuntimeLimits,
        MountPlugin, PatternPermissions, PermissionMode, Permissions, PythonLimits, ResourceLimits,
        RootFilesystemConfig, RootFilesystemKind, RootFilesystemMode, RootLowerInput,
        RulePermissions, ToolLimits, WasmLimits,
    };
    use crate::fs::{
        DirEntryType, FilesystemEntry, FilesystemEntryEncoding, FilesystemSnapshotEntries,
        FilesystemSnapshotExport, RootSnapshotExport, SnapshotExportKind,
    };
    use agentos_vm_config::{
        FsPermissionScope, PatternPermissionScope, PermissionMode as ConfigPermissionMode,
        RootFilesystemEntryKind, RootFilesystemLowerDescriptor,
        RootFilesystemMode as ConfigRootFilesystemMode,
    };

    #[tokio::test]
    async fn malformed_permission_callback_params_are_not_replaced_with_empty_json() {
        let callback = agentos_protocol::generated::v1::AcpCallback::AcpPermissionCallback(
            agentos_protocol::generated::v1::AcpPermissionCallback {
                session_id: String::from("session-1"),
                permission_id: String::from("permission-1"),
                params: String::from("not-json"),
                timeout_ms: 120_000,
            },
        );
        let payload = serde_bare::to_vec(&callback).expect("encode ACP callback");
        let error = handle_acp_ext_callback(
            agentos_sidecar_client::wire::ExtEnvelope {
                namespace: agentos_protocol::ACP_EXTENSION_NAMESPACE.to_string(),
                payload,
            },
            &wire_connection_ownership("connection-1"),
        )
        .await
        .expect_err("malformed callback params must fail");

        assert!(
            error
                .to_string()
                .contains("invalid ACP permission callback params for permission-1"),
            "unexpected error: {error}"
        );
    }

    /// Regression for the ACP event-pump leak (M7): `spawn_acp_event_pump` now stores its task
    /// handle in `AgentOsInner::acp_event_pump`, and `shutdown` aborts it through `abort_tracked_task`
    /// so the pump cannot outlive the disposed VM (it otherwise only ends on a shared-transport
    /// close that never comes while sibling VMs hold the transport open).
    ///
    /// Gap: driving `spawn_acp_event_pump` itself needs a live `AgentOs` (it calls
    /// `client.transport().subscribe_wire_events()`), which requires a real sidecar transport and so
    /// is out of reach at unit level. We instead exercise the exact field (`Mutex<Option<JoinHandle>>`)
    /// and the precise store-then-abort sequence the production code uses: `acp_event_pump` is
    /// initialized to `None`, `spawn_acp_event_pump` does `*slot.lock() = Some(handle)`, and
    /// `shutdown` does `abort_tracked_task(&slot)`.
    #[tokio::test]
    async fn abort_tracked_task_aborts_and_clears_the_handle() {
        // Mirrors `AgentOsInner` init (`acp_event_pump: parking_lot::Mutex::new(None)`).
        let slot: parking_lot::Mutex<Option<JoinHandle<()>>> = parking_lot::Mutex::new(None);
        assert!(
            slot.lock().is_none(),
            "pump slot starts empty like AgentOsInner"
        );

        let task = tokio::spawn(async {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        });
        let abort_handle = task.abort_handle();
        // Mirrors the tail of `spawn_acp_event_pump`: `*client.inner.acp_event_pump.lock() = Some(handle)`.
        *slot.lock() = Some(task);
        assert!(
            slot.lock().is_some(),
            "spawning the pump must populate the tracked handle"
        );

        assert!(!abort_handle.is_finished(), "pump task should start alive");

        abort_tracked_task(&slot);

        assert!(
            slot.lock().is_none(),
            "tracked handle must be taken on abort"
        );

        // The abort is asynchronous; give the runtime a bounded window to reap the cancelled task.
        for _ in 0..100 {
            if abort_handle.is_finished() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(
            abort_handle.is_finished(),
            "pump task must be aborted on shutdown"
        );
    }

    #[test]
    fn create_vm_config_omits_client_owned_defaults() {
        let config = serialize_create_vm_config_for_sidecar(&AgentOsConfig::default())
            .expect("serialize default create VM config");

        assert!(config.env.is_none());
        assert!(config.root_filesystem.is_none());
        assert!(config.loopback_exempt_ports.is_none());
        assert!(config.permissions.is_none());
        let encoded = serde_json::to_value(&config).expect("encode default create VM config");
        assert!(encoded.get("env").is_none());
        assert!(encoded.get("rootFilesystem").is_none());
        assert!(encoded.get("loopbackExemptPorts").is_none());
    }

    #[test]
    fn js_runtime_overrides_do_not_fill_sidecar_defaults() {
        let config = AgentOsConfig {
            allowed_node_builtins: Some(vec![String::from("path")]),
            high_resolution_time: Some(true),
            ..Default::default()
        };
        let encoded = serde_json::to_value(
            serialize_create_vm_config_for_sidecar(&config).expect("serialize VM config"),
        )
        .expect("encode VM config");
        assert_eq!(
            encoded.get("jsRuntime"),
            Some(&serde_json::json!({
                "allowedBuiltins": ["path"],
                "highResolutionTime": true
            }))
        );
    }

    #[test]
    fn permissions_policy_preserves_configured_denies_and_omits_unspecified_domains() {
        let policy = permissions_policy_config(&Permissions {
            network: Some(PatternPermissions::Mode(PermissionMode::Deny)),
            ..Default::default()
        });

        assert_eq!(
            policy.network,
            Some(PatternPermissionScope::Mode(ConfigPermissionMode::Deny))
        );
        assert!(policy.child_process.is_none());
    }

    #[test]
    fn permissions_policy_preserves_omitted_rule_fields_for_sidecar_defaults() {
        let policy = permissions_policy_config(&Permissions {
            fs: Some(FsPermissions::Rules(RulePermissions {
                default: Some(PermissionMode::Deny),
                rules: vec![FsPermissionRule {
                    mode: PermissionMode::Allow,
                    operations: None,
                    paths: Some(vec!["/workspace/**".to_string()]),
                }],
            })),
            ..Default::default()
        });

        let Some(FsPermissionScope::Rules(rules)) = policy.fs else {
            panic!("expected fs rule set");
        };
        assert_eq!(rules.default, Some(ConfigPermissionMode::Deny));
        assert!(rules.rules[0].operations.is_none());
        assert_eq!(
            rules.rules[0].paths,
            Some(vec!["/workspace/**".to_string()])
        );

        let policy = permissions_policy_config(&Permissions {
            network: Some(PatternPermissions::Rules(RulePermissions {
                default: Some(PermissionMode::Allow),
                rules: vec![crate::config::PatternPermissionRule {
                    mode: PermissionMode::Deny,
                    operations: None,
                    patterns: None,
                }],
            })),
            ..Default::default()
        });

        let Some(PatternPermissionScope::Rules(rules)) = policy.network else {
            panic!("expected network rule set");
        };
        assert_eq!(rules.default, Some(ConfigPermissionMode::Allow));
        assert!(rules.rules[0].operations.is_none());
        assert!(rules.rules[0].patterns.is_none());
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
        assert_eq!(descriptor.mode, Some(ConfigRootFilesystemMode::ReadOnly));
        assert_eq!(descriptor.disable_default_base_layer, Some(true));
        assert!(descriptor.bootstrap_entries.is_none());
        let lowers = descriptor.lowers.as_ref().expect("configured lowers");
        assert!(matches!(
            lowers[0],
            RootFilesystemLowerDescriptor::BundledBaseFilesystem
        ));

        let RootFilesystemLowerDescriptor::Snapshot { entries } = &lowers[1] else {
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
    fn root_filesystem_serializer_does_not_fill_sidecar_defaults() {
        let config = AgentOsConfig {
            root_filesystem: RootFilesystemConfig {
                mode: Some(RootFilesystemMode::Ephemeral),
                ..Default::default()
            },
            ..Default::default()
        };
        let encoded = serde_json::to_value(
            serialize_create_vm_config_for_sidecar(&config).expect("serialize VM config"),
        )
        .expect("encode VM config");
        assert_eq!(
            encoded.get("rootFilesystem"),
            Some(&serde_json::json!({ "mode": "ephemeral" }))
        );

        let native = AgentOsConfig {
            root_filesystem: RootFilesystemConfig {
                kind: RootFilesystemKind::Native,
                native_plugin: Some(MountPlugin {
                    id: "chunked_local".to_string(),
                    config: None,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let encoded = serde_json::to_value(
            serialize_create_vm_config_for_sidecar(&native).expect("serialize native VM config"),
        )
        .expect("encode native VM config");
        assert!(encoded.get("rootFilesystem").is_none());
        assert_eq!(
            encoded.get("nativeRoot"),
            Some(&serde_json::json!({
                "plugin": { "id": "chunked_local" }
            }))
        );
    }

    #[test]
    fn mount_serializer_does_not_fill_sidecar_defaults() {
        let mounts = serialize_mounts(&AgentOsConfig {
            mounts: vec![crate::config::MountConfig {
                path: String::from("/workspace"),
                plugin: MountPlugin {
                    id: String::from("js_bridge"),
                    config: None,
                },
                read_only: None,
            }],
            ..Default::default()
        })
        .expect("serialize mounts");

        assert_eq!(mounts.len(), 1);
        assert!(mounts[0].read_only.is_none());
        assert!(mounts[0].plugin.config.is_none());
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
            Some(serde_json::json!({ "databasePath": "/tmp/agentos-root.sqlite" }))
        );
        assert_eq!(native_root.read_only, Some(true));
    }

    #[test]
    fn create_vm_config_preserves_typed_limits() {
        let config = serialize_create_vm_config_for_sidecar(&AgentOsConfig {
            limits: Some(AgentOsLimits {
                resources: Some(ResourceLimits {
                    max_processes: Some(7),
                    max_captured_output_bytes: Some(2048),
                    max_filesystem_bytes: Some(4096),
                    ..Default::default()
                }),
                http: Some(HttpLimits {
                    max_fetch_response_bytes: Some(1024),
                }),
                tools: Some(ToolLimits {
                    default_tool_timeout_ms: Some(500),
                    max_registered_tools_per_vm: Some(12),
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
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        })
        .expect("serialize create VM config");
        let limits = config.limits.expect("limits config");

        let resources = limits.resources.expect("resource limits");
        assert_eq!(resources.max_processes, Some(7));
        assert_eq!(resources.max_captured_output_bytes, Some(2048));
        assert_eq!(resources.max_filesystem_bytes, Some(4096));
        assert_eq!(
            limits.http.expect("http limits").max_fetch_response_bytes,
            Some(1024)
        );
        assert_eq!(
            limits
                .tools
                .as_ref()
                .expect("tool limits")
                .default_tool_timeout_ms,
            Some(500)
        );
        assert_eq!(
            limits
                .tools
                .expect("tool limits")
                .max_registered_tools_per_vm,
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
    }
}
