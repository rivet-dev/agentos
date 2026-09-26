//! VM lifecycle functions: create, configure, dispose, bootstrap, snapshot.
//!
//! Extracted from service.rs as part of the service.rs split (Step 0a).
//! Contains VM lifecycle methods on VmManager<B> and associated helpers.

use crate::bootstrap::{
    apply_root_filesystem_entry, discover_command_guest_paths, discover_kernel_commands,
    root_snapshot_entries, root_snapshot_entry, root_snapshot_from_entries, KernelCommandInventory,
};
use crate::bridge::{bridge_permissions, build_mount_plugin_registry, MountPluginContext};
use crate::execution::terminate_child_process_tree;
use crate::extension::Extension;
use crate::process_event_broker::ProcessEventBroker;
use crate::protocol::{
    ConfigureVmRequest, CreateLayerRequest, CreateOverlayRequest, DisposeReason, EventFrame,
    EventPayload, ExportSnapshotRequest, ImportSnapshotRequest, LinkPackageRequest,
    ListMountsRequest, MountDescriptor, MountInfo, MountPluginDescriptor, OwnershipScope,
    PackageCommands, ProcessExitedEvent, ProjectedCommand, ProvidedCommandsRequest,
    RootFilesystemDescriptor, RootFilesystemEntry, SealLayerRequest, SnapshotRootFilesystemRequest,
    UnlinkPackageRequest, VmLifecycleState,
};
use crate::request_operations::OperationCancellationReason;
use crate::service::{
    audit_fields, dirname, emit_security_audit_event, emit_structured_event, kernel_error,
    normalize_path, plugin_error, root_filesystem_error, validate_permissions_policy, vfs_error,
};
use crate::state::{
    BridgeError, KernelSocketReadinessEvent, KernelSocketReadinessRegistry,
    KernelSocketReadinessTarget, QuarantinedVmGeneration, VmConfiguration, VmDnsConfig,
    VmExecutionEngines, VmHandle, VmListenPolicy, VmPendingByteBudget, VmQuarantineReason,
    VmReconciliationSnapshot, VmState, DISPOSE_VM_SIGKILL_GRACE, DISPOSE_VM_SIGTERM_GRACE,
    EXECUTION_DRIVER_NAME, JAVASCRIPT_COMMAND, PYTHON_COMMAND, WASM_COMMAND,
};
use crate::{DispatchResult, VmError, VmManager, VmManagerHost};

use crate::core::ca::{
    CA_CERTIFICATES_BUNDLE, CA_CERTIFICATES_GUEST_PATH, CA_CERTIFICATES_SYMLINK_PATH,
    CA_CERTIFICATES_SYMLINK_TARGET,
};
use crate::core::permissions::{
    deny_all_policy, resolve_permissions_policy, resolve_profile_permissions_policy,
};
use crate::core::{
    layer_created_response, layer_sealed_response, mounts_listed_response,
    overlay_created_response, package_linked_response, package_unlinked_response,
    protocol_root_filesystem_mode, provided_commands_response,
    root_filesystem_bootstrapped_response, root_filesystem_protocol_descriptor_from_config,
    root_filesystem_snapshot_response, snapshot_exported_response, snapshot_imported_response,
    vm_configured_response, vm_created_response, vm_disposed_response,
    vm_lifecycle_event as shared_vm_lifecycle_event, VmLayerStore,
};
use agentos_driver_tokio::accounting::{ResourceClass, ResourceLedger, ResourceLimit};
use agentos_driver_tokio::capability::CapabilityRegistry;
use agentos_vm_config as vm_config;
use agentos_vm_host_interface::{
    FilesystemSnapshot, FlushFilesystemStateRequest, LifecycleState, LoadFilesystemStateRequest,
};
use agentos_vm_kernel::command_registry::CommandDriver;
use agentos_vm_kernel::kernel::{KernelVm, KernelVmConfig};
use agentos_vm_kernel::mount_plugin::OpenFileSystemPluginRequest;
use agentos_vm_kernel::mount_table::{DetachedMount, MountOptions, MountTable, MountedFileSystem};
use agentos_vm_kernel::permissions::filter_env;
use agentos_vm_kernel::root_fs::{
    encode_snapshot as encode_root_snapshot, load_bundled_base_environment,
    FilesystemEntryKind as KernelFilesystemEntryKind, ROOT_FILESYSTEM_SNAPSHOT_FORMAT,
};
use agentos_vm_kernel::socket_table::{SocketReadiness, SocketReadinessKind};
use openssl::rand::rand_bytes;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const ROOT_BOOTSTRAP_DIRS: &[(&str, u32, u32, u32)] = &[
    ("/dev", 0o755, 0, 0),
    ("/proc", 0o755, 0, 0),
    ("/tmp", 0o1777, 0, 0),
    ("/bin", 0o755, 0, 0),
    ("/lib", 0o755, 0, 0),
    ("/sbin", 0o755, 0, 0),
    ("/boot", 0o755, 0, 0),
    ("/etc", 0o755, 0, 0),
    // agentOS retains `/root/node_modules` as a compatibility projection.
    // Permit traversal without allowing the default guest to list `/root`.
    ("/root", 0o711, 0, 0),
    ("/run", 0o755, 0, 0),
    ("/srv", 0o755, 0, 0),
    ("/sys", 0o555, 0, 0),
    ("/opt", 0o755, 0, 0),
    ("/mnt", 0o755, 0, 0),
    ("/media", 0o755, 0, 0),
    ("/home", 0o755, 0, 0),
    ("/home/agentos", 0o2755, 1000, 1000),
    ("/usr", 0o755, 0, 0),
    ("/usr/bin", 0o755, 0, 0),
    ("/usr/games", 0o755, 0, 0),
    ("/usr/include", 0o755, 0, 0),
    ("/usr/lib", 0o755, 0, 0),
    ("/usr/libexec", 0o755, 0, 0),
    ("/usr/man", 0o755, 0, 0),
    ("/usr/local", 0o755, 0, 0),
    ("/usr/local/bin", 0o755, 0, 0),
    ("/usr/sbin", 0o755, 0, 0),
    ("/usr/share", 0o755, 0, 0),
    ("/usr/share/man", 0o755, 0, 0),
    ("/var", 0o755, 0, 0),
    ("/var/cache", 0o755, 0, 0),
    ("/var/empty", 0o555, 0, 0),
    ("/var/lib", 0o755, 0, 0),
    ("/var/lock", 0o777, 0, 0),
    ("/var/log", 0o755, 0, 0),
    ("/var/run", 0o777, 0, 0),
    ("/var/spool", 0o755, 0, 0),
    ("/var/tmp", 0o1777, 0, 0),
    ("/etc/agentos", 0o755, 0, 0),
    // Non-Alpine default agent working directory (also present in the base
    // filesystem snapshot); scaffold it here so it exists even when the
    // default base layer is disabled. It is the default cwd and mount root,
    // kept separate from $HOME (/home/agentos).
    ("/workspace", 0o755, 1000, 1000),
];

fn create_vm_unix_socket_host_dir() -> Result<PathBuf, VmError> {
    for _ in 0..32 {
        let mut nonce = [0_u8; 16];
        rand_bytes(&mut nonce).map_err(|error| {
            VmError::Io(format!("failed to generate Unix socket namespace: {error}"))
        })?;
        let suffix = nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let path = std::env::temp_dir().join(format!("agentos-uds-{suffix}"));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&path) {
            Ok(()) => {
                if let Err(error) = fs::set_permissions(&path, fs::Permissions::from_mode(0o700)) {
                    let cleanup_error = fs::remove_dir(&path).err();
                    return Err(VmError::Io(format!(
                        "failed to set private Unix socket namespace {} to mode 0700: {error}{}",
                        path.display(),
                        cleanup_error
                            .map(|cleanup| format!("; cleanup failed: {cleanup}"))
                            .unwrap_or_default()
                    )));
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(VmError::Io(format!(
                    "failed to create private Unix socket namespace {}: {error}",
                    path.display()
                )));
            }
        }
    }
    Err(VmError::Io(String::from(
        "failed to allocate a unique private Unix socket namespace after 32 attempts",
    )))
}

fn send_kernel_socket_readiness_event(
    target: KernelSocketReadinessTarget,
    readiness: SocketReadiness,
) {
    if !target.live.load(Ordering::Acquire) {
        return;
    }
    let flags = match (target.event, readiness.kind) {
        (KernelSocketReadinessEvent::Accept, SocketReadinessKind::Accept) => {
            agentos_driver_tokio::readiness::ReadyFlags::ACCEPT
        }
        (KernelSocketReadinessEvent::Data, SocketReadinessKind::Data) => {
            agentos_driver_tokio::readiness::ReadyFlags::READABLE
        }
        (KernelSocketReadinessEvent::Data, SocketReadinessKind::Hangup) => {
            agentos_driver_tokio::readiness::ReadyFlags::END
        }
        (KernelSocketReadinessEvent::Datagram, SocketReadinessKind::Data) => {
            agentos_driver_tokio::readiness::ReadyFlags::DATAGRAM
        }
        _ => return,
    };
    if target.live.load(Ordering::Acquire) {
        if let Some(notify) = &target.notify {
            notify.notify_one();
        }
    }
    if target.live.load(Ordering::Acquire) {
        let Some(session) = &target.session else {
            return;
        };
        if let Err(error) = session.publish_readiness(
            target.capability_id,
            target.capability_generation,
            crate::executor::backend::ExecutionReadyFlags::from_bits(flags.bits()),
        ) {
            eprintln!(
                "ERR_AGENTOS_KERNEL_READINESS_WAKE: failed to publish capability={} generation={} target={}: {error}",
                target.capability_id, target.capability_generation, target.target_id
            );
        }
    }
}

pub(crate) const DEFAULT_GUEST_PATH_ENV: &str =
    "/usr/local/sbin:/usr/local/bin:/opt/agentos/bin:/usr/sbin:/usr/bin:/sbin:/bin";
#[cfg(test)]
const KERNEL_COMMAND_STUB: &[u8] = b"#!/bin/sh\n# kernel command stub\n";

#[cfg(test)]
fn projected_command_guest_path(command: &str) -> String {
    format!("{}/{command}", crate::package_projection::OPT_AGENTOS_BIN)
}

fn projected_commands_from_guest_paths(
    command_guest_paths: &BTreeMap<String, String>,
    provided_commands: &BTreeMap<String, Vec<String>>,
) -> Vec<ProjectedCommand> {
    let names = provided_commands
        .values()
        .flatten()
        .collect::<BTreeSet<_>>();
    command_guest_paths
        .iter()
        .filter(|(name, _)| names.contains(name))
        .map(|(name, guest_path)| ProjectedCommand {
            name: name.clone(),
            guest_path: guest_path.clone(),
        })
        .collect()
}

#[cfg(test)]
fn projected_commands_from_provided_commands(
    provided_commands: &BTreeMap<String, Vec<String>>,
    kernel_commands: &KernelCommandInventory,
) -> Vec<ProjectedCommand> {
    let mut commands = BTreeMap::new();
    for command in provided_commands.values().flatten() {
        if kernel_commands.names.contains(command) {
            continue;
        }
        commands
            .entry(command.clone())
            .or_insert_with(|| ProjectedCommand {
                name: command.clone(),
                guest_path: projected_command_guest_path(command),
            });
    }
    commands.into_values().collect()
}

fn execution_driver_commands(
    kernel_commands: &KernelCommandInventory,
    provided_commands: &BTreeMap<String, Vec<String>>,
    additional_commands: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut commands = BTreeSet::from([
        String::from(JAVASCRIPT_COMMAND),
        String::from(PYTHON_COMMAND),
        String::from("python3"),
        String::from(WASM_COMMAND),
    ]);
    commands.extend(kernel_commands.names.iter().cloned());
    commands.extend(provided_commands.values().flatten().cloned());
    commands.extend(additional_commands);
    commands.into_iter().collect()
}

/// Owned request context for work serialized by one VM's lifecycle ordering key.
///
/// Preparing this value performs the central ownership lookup once and clones the
/// per-VM handle. Executing the operation can then happen after the
/// `VmManager` coordinator borrow has ended.
#[derive(Clone)]
pub(crate) struct OwnedVmLifecycleRequest {
    request: crate::protocol::RequestFrame,
    connection_id: String,
    session_id: String,
    vm_id: String,
    vm: VmHandle,
}

/// Cloneable dependencies needed by the owned `ConfigureVm` implementation.
pub(crate) struct ConfigureVmOwnedInput<B> {
    lifecycle: OwnedVmLifecycleRequest,
    bridge: crate::state::SharedBridge<B>,
    sidecar_requests: crate::state::SharedSidecarRequestClient,
}

impl<B> Clone for ConfigureVmOwnedInput<B> {
    fn clone(&self) -> Self {
        Self {
            lifecycle: self.lifecycle.clone(),
            bridge: self.bridge.clone(),
            sidecar_requests: self.sidecar_requests.clone(),
        }
    }
}

/// Cloneable dependencies needed by the owned `LinkPackage` implementation.
pub(crate) struct LinkPackageOwnedInput<B> {
    lifecycle: OwnedVmLifecycleRequest,
    bridge: crate::state::SharedBridge<B>,
    sidecar_requests: crate::state::SharedSidecarRequestClient,
}

/// Create work detached from the process coordinator.
///
/// VM identity and resource admission are reserved during preparation. Database
/// resolution, schema migration, filesystem bootstrap, and kernel construction
/// happen when this owned value is executed.
pub struct PreparedCreateVm<B> {
    request: crate::protocol::RequestFrame,
    payload: crate::protocol::CreateVmRequest,
    connection_id: String,
    session_id: String,
    vm_id: String,
    vm_generation: u64,
    create_config: vm_config::CreateVmConfig,
    root_filesystem: RootFilesystemDescriptor,
    permissions_policy: agentos_vm_config::PermissionsPolicy,
    limits: crate::limits::VmLimits,
    vm_resources: Arc<ResourceLedger>,
    vm_runtime_context: agentos_driver_tokio::DriverHandle,
    dns: VmDnsConfig,
    listen_policy: VmListenPolicy,
    create_loopback_exempt_ports: BTreeSet<u16>,
    bridge: crate::state::SharedBridge<B>,
    dns_resolver: agentos_vm_kernel::dns::SharedDnsResolver,
    sidecar_requests: crate::state::SharedSidecarRequestClient,
    process_event_notify: Arc<tokio::sync::Notify>,
    extensions: Vec<Arc<dyn Extension>>,
}

/// Fully constructed VM awaiting a short session/registry publication command.
pub struct CompletedCreateVm<B> {
    request: crate::protocol::RequestFrame,
    connection_id: String,
    session_id: String,
    vm_id: String,
    vm: VmState,
    events: Vec<EventFrame>,
    bridge: crate::state::SharedBridge<B>,
}

/// Validated dispose intent. The stdio owner begins VM disposal and waits for
/// operation drain before converting this plan into [`PreparedDisposeVm`].
pub struct DisposeVmPlan<B> {
    request: Option<crate::protocol::RequestFrame>,
    connection_id: String,
    session_id: String,
    vm_id: String,
    reason: DisposeReason,
    bridge: crate::state::SharedBridge<B>,
    sidecar_requests: crate::state::SharedSidecarRequestClient,
    process_event_broker: ProcessEventBroker,
}

/// Detached VM teardown payload.
///
/// Construction is a short coordinator command performed only after the
/// ownership coordinator has entered `Closing` and drained VM operations. The
/// value then owns all state needed for bounded teardown and reconciliation, so
/// no lifecycle permit or `&mut VmManager` is retained across its awaits.
pub struct PreparedDisposeVm<B> {
    plan: DisposeVmPlan<B>,
    vm: VmState,
}

/// Teardown result awaiting short central tracking/quarantine finalization.
pub struct CompletedDisposeVm {
    request: Option<crate::protocol::RequestFrame>,
    connection_id: String,
    session_id: String,
    vm_id: String,
    events: Vec<EventFrame>,
    quarantine: Option<QuarantinedVmGeneration>,
    result: Result<(), VmError>,
}

impl<B> Clone for LinkPackageOwnedInput<B> {
    fn clone(&self) -> Self {
        Self {
            lifecycle: self.lifecycle.clone(),
            bridge: self.bridge.clone(),
            sidecar_requests: self.sidecar_requests.clone(),
        }
    }
}
// ---------------------------------------------------------------------------
// VmManager VM lifecycle methods
// ---------------------------------------------------------------------------

impl<B> VmManager<B>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    pub(crate) fn prepare_vm_lifecycle_request(
        &self,
        request: &crate::protocol::RequestFrame,
    ) -> Result<OwnedVmLifecycleRequest, VmError> {
        let (connection_id, session_id, vm_id) = self.vm_scope_for(&request.ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
        let vm = self.vms.handle(&vm_id).ok_or_else(|| {
            VmError::InvalidState(format!(
                "VM {vm_id} no longer exists while preparing its lifecycle operation"
            ))
        })?;
        Ok(OwnedVmLifecycleRequest {
            request: request.clone(),
            connection_id,
            session_id,
            vm_id,
            vm,
        })
    }

    pub(crate) fn prepare_configure_vm_request(
        &self,
        request: &crate::protocol::RequestFrame,
    ) -> Result<ConfigureVmOwnedInput<B>, VmError> {
        let lifecycle = self.prepare_vm_lifecycle_request(request)?;
        Ok(ConfigureVmOwnedInput {
            lifecycle,
            bridge: self.bridge.clone(),
            sidecar_requests: self.sidecar_requests.clone(),
        })
    }

    pub(crate) fn prepare_link_package_request(
        &self,
        request: &crate::protocol::RequestFrame,
    ) -> Result<LinkPackageOwnedInput<B>, VmError> {
        Ok(LinkPackageOwnedInput {
            lifecycle: self.prepare_vm_lifecycle_request(request)?,
            bridge: self.bridge.clone(),
            sidecar_requests: self.sidecar_requests.clone(),
        })
    }

    pub fn prepare_create_vm(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: crate::protocol::CreateVmRequest,
    ) -> Result<PreparedCreateVm<B>, VmError> {
        let (connection_id, session_id) = self.session_scope_for(&request.ownership)?;
        self.require_owned_session(&connection_id, &session_id)?;
        let mut create_config: vm_config::CreateVmConfig = serde_json::from_str(&payload.config)
            .map_err(|error| {
                VmError::InvalidState(format!("invalid create VM config JSON: {error}"))
            })?;
        create_config
            .normalize()
            .map_err(|error| VmError::InvalidState(format!("invalid create VM config: {error}")))?;
        create_config
            .validate(self.config.max_frame_bytes)
            .map_err(|error| VmError::InvalidState(format!("invalid create VM config: {error}")))?;
        let root_filesystem =
            root_filesystem_protocol_descriptor_from_config(&create_config.root_filesystem);
        let permissions_policy = resolve_profile_permissions_policy(
            create_config.defaults_profile(),
            create_config.permissions.clone(),
        );
        validate_permissions_policy(&permissions_policy)?;
        let limits = crate::limits::vm_limits_from_config(
            create_config.limits.as_ref(),
            self.config.max_frame_bytes,
        )?;
        let dns = vm_dns_config_from_config(create_config.dns.as_ref())?;
        let listen_policy = vm_listen_policy_from_config(create_config.listen.as_ref())?;
        let create_loopback_exempt_ports = create_config
            .loopback_exempt_ports
            .iter()
            .copied()
            .collect();
        let (vm_id, vm_generation) = self.allocate_vm_identity()?;
        let process_runtime_context = self.runtime_context.as_ref().cloned().ok_or_else(|| {
            VmError::InvalidState(String::from(
                "ERR_AGENTOS_RUNTIME_UNAVAILABLE: VM admission requires DriverHandle",
            ))
        })?;
        let vm_resources = Arc::new(vm_resource_ledger(
            &vm_id,
            vm_generation,
            &limits,
            Arc::clone(process_runtime_context.resources()),
        )?);
        let vm_runtime_context =
            process_runtime_context.scoped_for_vm(Arc::clone(&vm_resources), vm_generation);

        Ok(PreparedCreateVm {
            request: request.clone(),
            payload,
            connection_id,
            session_id,
            vm_id,
            vm_generation,
            create_config,
            root_filesystem,
            permissions_policy,
            limits,
            vm_resources,
            vm_runtime_context,
            dns,
            listen_policy,
            create_loopback_exempt_ports,
            bridge: self.bridge.clone(),
            dns_resolver: Arc::clone(&self.dns_resolver),
            sidecar_requests: self.sidecar_requests.clone(),
            process_event_notify: Arc::clone(&self.process_event_notify),
            extensions: self.extensions.values().cloned().collect(),
        })
    }

    pub fn complete_create_vm(
        &mut self,
        completed: CompletedCreateVm<B>,
    ) -> Result<DispatchResult, VmError> {
        let CompletedCreateVm {
            request,
            connection_id,
            session_id,
            vm_id,
            vm,
            events,
            bridge,
        } = completed;
        if let Err(error) = self.require_owned_session(&connection_id, &session_id) {
            cleanup_unpublished_vm(&bridge, &vm_id, &vm);
            return Err(error);
        }
        if self.vms.contains_key(&vm_id) {
            cleanup_unpublished_vm(&bridge, &vm_id, &vm);
            return Err(VmError::InvalidState(format!(
                "VM {vm_id} already exists during create finalization"
            )));
        }
        let cleanup_cwd = vm.runtime_scratch_root.clone();
        let cleanup_socket_dir = vm.unix_socket_host_dir.clone();
        if let Err(error) = self.vms.insert(vm_id.clone(), vm) {
            cleanup_path(&cleanup_cwd, "unpublished VM shadow root");
            cleanup_path(&cleanup_socket_dir, "unpublished VM Unix socket namespace");
            if let Err(cleanup_error) = bridge.clear_vm_permissions(&vm_id) {
                eprintln!(
                    "ERR_AGENTOS_VM_CREATE_CLEANUP: vm_id={vm_id} phase=permission_reset error={cleanup_error}"
                );
            }
            return Err(error);
        }
        self.sessions
            .get_mut(&session_id)
            .expect("owned session should exist during create finalization")
            .vm_ids
            .insert(vm_id.clone());
        self.observe_active_vm_generations();
        Ok(DispatchResult {
            response: vm_created_response(&request, vm_id),
            events,
        })
    }

    pub fn prepare_dispose_vm(
        &self,
        request: &crate::protocol::RequestFrame,
        payload: crate::protocol::DisposeVmRequest,
    ) -> Result<DisposeVmPlan<B>, VmError> {
        let (connection_id, session_id, vm_id) = self.vm_scope_for(&request.ownership)?;
        self.prepare_owned_vm_disposal(
            connection_id,
            session_id,
            vm_id,
            payload.reason,
            Some(request.clone()),
        )
    }

    /// Prepare teardown for a VM owned by a non-protocol lifecycle source.
    ///
    /// Extension session cleanup uses this entry point for its bound VMs. It
    /// deliberately has no request envelope: callers complete it with
    /// [`Self::complete_owned_vm_disposal`] and receive only lifecycle events.
    pub(crate) fn prepare_internal_vm_disposal(
        &self,
        connection_id: String,
        session_id: String,
        vm_id: String,
        reason: DisposeReason,
    ) -> Result<DisposeVmPlan<B>, VmError> {
        self.prepare_owned_vm_disposal(connection_id, session_id, vm_id, reason, None)
    }

    fn prepare_owned_vm_disposal(
        &self,
        connection_id: String,
        session_id: String,
        vm_id: String,
        reason: DisposeReason,
        request: Option<crate::protocol::RequestFrame>,
    ) -> Result<DisposeVmPlan<B>, VmError> {
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
        Ok(DisposeVmPlan {
            request,
            connection_id,
            session_id,
            vm_id,
            reason,
            bridge: self.bridge.clone(),
            sidecar_requests: self.sidecar_requests.clone(),
            process_event_broker: self.process_event_broker.clone(),
        })
    }

    /// Detach a VM after ownership coordination has entered `Closing` and all
    /// previously admitted VM operations have drained.
    pub fn detach_vm_for_disposal(
        &mut self,
        plan: DisposeVmPlan<B>,
    ) -> Result<PreparedDisposeVm<B>, VmError> {
        self.require_owned_vm(&plan.connection_id, &plan.session_id, &plan.vm_id)?;
        self.cancel_in_process_services(&plan.vm_id);
        let cancellation_reason = dispose_cancellation_reason(&plan.reason);
        if let Err(error) = plan.process_event_broker.dispose_vm(
            &plan.connection_id,
            &plan.session_id,
            &plan.vm_id,
            cancellation_reason,
        ) {
            eprintln!(
                "ERR_AGENTOS_PROCESS_EVENT_VM_DISPOSAL: connection_id={} session_id={} vm_id={} error={error}",
                plan.connection_id, plan.session_id, plan.vm_id
            );
        }
        let vm = self
            .vms
            .try_remove(&plan.vm_id, "begin owned dispose")?
            .expect("owned VM should exist during dispose detachment");
        Ok(PreparedDisposeVm { plan, vm })
    }

    pub fn complete_dispose_vm(
        &mut self,
        completed: CompletedDisposeVm,
    ) -> Result<DispatchResult, VmError> {
        let request = completed.request.clone().ok_or_else(|| {
            VmError::InvalidState(String::from(
                "protocol VM disposal completed without a request envelope",
            ))
        })?;
        let vm_id = completed.vm_id.clone();
        let events = self.complete_owned_vm_disposal(completed)?;
        Ok(DispatchResult {
            response: vm_disposed_response(&request, vm_id),
            events,
        })
    }

    /// Finalize a detached VM teardown without constructing a protocol
    /// response. This is the short central-state mutation paired with
    /// [`Self::prepare_internal_vm_disposal`].
    pub(crate) fn complete_owned_vm_disposal(
        &mut self,
        completed: CompletedDisposeVm,
    ) -> Result<Vec<EventFrame>, VmError> {
        let CompletedDisposeVm {
            request: _,
            connection_id,
            session_id,
            vm_id,
            mut events,
            quarantine,
            result,
        } = completed;
        self.reclaim_vm_tracking(&session_id, &vm_id);
        if let Some(quarantine) = quarantine {
            self.retain_quarantined_vm(quarantine)?;
        } else {
            self.observe_active_vm_generations();
        }
        result?;
        events.push(shared_vm_lifecycle_event(
            &connection_id,
            &session_id,
            &vm_id,
            VmLifecycleState::Disposed,
        ));
        Ok(events)
    }

    pub(crate) fn allocate_vm_identity(&mut self) -> Result<(String, u64), VmError> {
        self.reap_reconciled_quarantined_vms();
        self.ensure_vm_generation_capacity()?;
        let next = self.next_vm_id.checked_add(1).ok_or_else(|| {
            VmError::host(
                "ERR_AGENTOS_VM_ID_EXHAUSTED",
                String::from("VM id counter overflowed"),
            )
        })?;
        let generation = self
            .runtime_context
            .as_ref()
            .ok_or_else(|| {
                VmError::host(
                    "ERR_AGENTOS_RUNTIME_UNAVAILABLE",
                    String::from("VM generation allocation requires DriverHandle"),
                )
            })?
            .allocate_vm_generation()
            .map_err(|error| VmError::InvalidState(error.to_string()))?;
        self.next_vm_id = next;
        Ok((format!("vm-{next}"), generation))
    }

    pub(crate) async fn create_vm(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: crate::protocol::CreateVmRequest,
    ) -> Result<DispatchResult, VmError> {
        let (connection_id, session_id) = self.session_scope_for(&request.ownership)?;
        self.require_owned_session(&connection_id, &session_id)?;
        let mut create_config: vm_config::CreateVmConfig = serde_json::from_str(&payload.config)
            .map_err(|error| {
                VmError::InvalidState(format!("invalid create VM config JSON: {error}"))
            })?;
        create_config
            .normalize()
            .map_err(|error| VmError::InvalidState(format!("invalid create VM config: {error}")))?;
        create_config
            .validate(self.config.max_frame_bytes)
            .map_err(|error| VmError::InvalidState(format!("invalid create VM config: {error}")))?;
        let (vm_id, events) = self
            .create_vm_owned(connection_id, session_id, payload.runtime, create_config)
            .await?;

        Ok(DispatchResult {
            response: vm_created_response(request, vm_id),
            events,
        })
    }

    pub(crate) fn compare_vm_config(
        &self,
        request: &crate::protocol::RequestFrame,
        payload: crate::protocol::CompareVmConfigRequest,
    ) -> Result<DispatchResult, VmError> {
        let (connection_id, session_id) = self.session_scope_for(&request.ownership)?;
        self.require_owned_session(&connection_id, &session_id)?;
        let before = serde_json::from_str(&payload.before).map_err(|error| {
            VmError::InvalidState(format!("invalid before VM config JSON: {error}"))
        })?;
        let after = serde_json::from_str(&payload.after).map_err(|error| {
            VmError::InvalidState(format!("invalid after VM config JSON: {error}"))
        })?;
        let mut before_mounts = payload.before_mounts;
        let mut after_mounts = payload.after_mounts;
        canonicalize_comparison_mounts(&self.mount_plugins, &mut before_mounts)?;
        canonicalize_comparison_mounts(&self.mount_plugins, &mut after_mounts)?;
        let equivalent = equivalent_vm_creation_config(before, after, self.config.max_frame_bytes)?
            && before_mounts == after_mounts
            && payload.before_restart_identity == payload.after_restart_identity;
        Ok(DispatchResult {
            response: self.respond(
                request,
                crate::protocol::ResponsePayload::VmConfigCompared(
                    crate::protocol::VmConfigComparedResponse { equivalent },
                ),
            ),
            events: Vec::new(),
        })
    }

    pub(crate) async fn create_vm_owned(
        &mut self,
        connection_id: String,
        session_id: String,
        runtime: crate::wire::GuestRuntimeKind,
        create_config: vm_config::CreateVmConfig,
    ) -> Result<(String, Vec<EventFrame>), VmError> {
        let __t = Instant::now();
        let root_filesystem =
            root_filesystem_protocol_descriptor_from_config(&create_config.root_filesystem);
        let permissions_policy = resolve_profile_permissions_policy(
            create_config.defaults_profile(),
            create_config.permissions.clone(),
        );
        validate_permissions_policy(&permissions_policy)?;

        let (vm_id, vm_generation) = self.allocate_vm_identity()?;
        let runtime_scratch_root = create_vm_runtime_scratch_root(&vm_id)?;
        let (guest_cwd, host_cwd) =
            resolve_vm_cwds(create_config.cwd.as_ref(), &runtime_scratch_root)?;
        fs::create_dir_all(&host_cwd)
            .map_err(|error| VmError::Io(format!("failed to create VM cwd: {error}")))?;
        let limits = crate::limits::vm_limits_from_config(
            create_config.limits.as_ref(),
            self.config.max_frame_bytes,
        )?;
        let resource_limits = limits.resources.clone();
        let process_runtime_context = self.runtime_context.as_ref().cloned().ok_or_else(|| {
            VmError::host(
                "ERR_AGENTOS_RUNTIME_UNAVAILABLE",
                String::from("VM admission requires DriverHandle"),
            )
        })?;
        let process_resources = Arc::clone(process_runtime_context.resources());
        let vm_resources = Arc::new(vm_resource_ledger(
            &vm_id,
            vm_generation,
            &limits,
            process_resources,
        )?);
        let vm_runtime_context =
            process_runtime_context.scoped_for_vm(Arc::clone(&vm_resources), vm_generation);
        let database = match create_config.database.as_ref() {
            Some(descriptor) => {
                let database = crate::vm_sqlite::resolve_vm_sqlite(
                    descriptor,
                    vm_runtime_context.clone(),
                    limits.sqlite.max_result_bytes,
                    Some((
                        self.sidecar_requests.clone(),
                        crate::protocol::OwnershipScope::session(
                            connection_id.clone(),
                            session_id.clone(),
                        ),
                    )),
                )
                .await
                .map_err(|error| {
                    VmError::InvalidState(format!("failed to resolve VM SQLite database: {error}"))
                })?;
                crate::plugins::chunked_sqlite::bootstrap_schema(database.as_ref())
                    .await
                    .map_err(|error| {
                        VmError::InvalidState(format!(
                            "failed to migrate VM SQLite database: {error}"
                        ))
                    })?;
                for extension in self.extensions.values() {
                    extension
                        .bootstrap_vm_database(database.clone())
                        .await
                        .map_err(|error| {
                            VmError::InvalidState(format!(
                                "failed to migrate extension VM database schema: {error}"
                            ))
                        })?;
                }
                Some(database)
            }
            None => None,
        };
        let capabilities = CapabilityRegistry::new(vm_generation, Arc::clone(&vm_resources));
        let dns = vm_dns_config_from_config(create_config.dns.as_ref())?;
        let listen_policy = vm_listen_policy_from_config(create_config.listen.as_ref())?;
        let create_loopback_exempt_ports: BTreeSet<u16> = create_config
            .loopback_exempt_ports
            .iter()
            .copied()
            .collect();
        self.bridge
            .set_vm_permissions(&vm_id, &permissions_policy)?;
        let permissions = bridge_permissions(self.bridge.clone(), &vm_id);
        let mut guest_env = filter_env(
            &vm_id,
            &create_vm_environment(&create_config)?,
            &permissions,
        );

        let native_root = native_root_plugin_from_config(create_config.native_root.as_ref())?;
        let loaded_snapshot = if native_root.is_some() {
            None
        } else {
            self.bridge.with_mut(|bridge| {
                bridge.load_filesystem_state(LoadFilesystemStateRequest {
                    vm_id: vm_id.clone(),
                })
            })?
        };
        let mut config = KernelVmConfig::new(vm_id.clone());
        config.vm_generation = vm_generation;
        config.cwd = guest_cwd.clone();
        config.env = guest_env.clone();
        if let Some(user) = create_config.user.as_ref() {
            config.user = agentos_vm_kernel::user::UserConfig {
                uid: user.uid,
                gid: user.gid,
                euid: user.euid,
                egid: user.egid,
                username: user.username.clone(),
                homedir: user.homedir.clone(),
                shell: user.shell.clone(),
                gecos: user.gecos.clone(),
                group_name: user.group_name.clone(),
                supplementary_gids: user.supplementary_gids.clone().unwrap_or_default(),
                accounts: user
                    .accounts
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|account| agentos_vm_kernel::user::UserAccount {
                        uid: account.uid,
                        gid: account.gid,
                        username: account.username.clone(),
                        homedir: account.homedir.clone(),
                        shell: account.shell.clone(),
                        gecos: account.gecos.clone().unwrap_or_default(),
                        supplementary_gids: account.supplementary_gids.clone(),
                    })
                    .collect(),
                groups: user
                    .groups
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|group| agentos_vm_kernel::user::GroupRecord {
                        gid: group.gid,
                        name: group.name.clone(),
                        members: group.members.clone(),
                    })
                    .collect(),
            };
        }
        config.permissions = permissions;
        config.dns = agentos_vm_kernel::dns::DnsConfig {
            name_servers: dns.name_servers.clone(),
            overrides: dns.overrides.clone(),
        };
        if self.runtime_context.is_none() {
            return Err(VmError::InvalidState(String::from(
                "VM creation requires the process DriverHandle",
            )));
        }
        config.dns_resolver = Arc::clone(&self.dns_resolver);
        config.loopback_exempt_ports = create_loopback_exempt_ports.clone();
        let root_mount_table = if let Some(native_root) = native_root.as_ref() {
            build_native_root_mount_table(
                &self.mount_plugins,
                native_root,
                &root_filesystem,
                MountPluginContext {
                    bridge: self.bridge.clone(),
                    runtime_context: vm_runtime_context.clone(),
                    connection_id: connection_id.clone(),
                    session_id: session_id.clone(),
                    vm_id: vm_id.clone(),
                    sidecar_requests: self.sidecar_requests.clone(),
                    database: database.clone(),
                    max_pread_bytes: resource_limits.max_pread_bytes,
                },
            )?
        } else {
            crate::core::build_root_mount_table_with_loaded_snapshot(
                &create_config.root_filesystem,
                loaded_snapshot.as_ref(),
                &resource_limits,
            )
            .map_err(|error| VmError::InvalidState(error.to_string()))?
        };
        config.resources = resource_limits;
        let mut kernel = KernelVm::new(root_mount_table, config);
        kernel
            .set_socket_resource_ledger(Arc::clone(&vm_resources))
            .map_err(kernel_error)?;
        let kernel_socket_readiness: KernelSocketReadinessRegistry = Arc::new(
            crate::state::KernelSocketReadinessRegistryState::new(limits.reactor.max_capabilities),
        );
        let readiness_targets = Arc::clone(&kernel_socket_readiness);
        kernel.set_socket_readiness_sink(Some(move |readiness: SocketReadiness| {
            for target in readiness_targets.targets(readiness.socket_id) {
                send_kernel_socket_readiness_event(target, readiness);
            }
        }));
        let kernel_commands = discover_kernel_commands(&mut kernel)?;
        refresh_guest_command_path_env(&mut guest_env, &kernel_commands.search_roots);
        let execution_commands = execution_driver_commands(
            &kernel_commands,
            &BTreeMap::new(),
            create_config.bootstrap_commands.iter().flatten().cloned(),
        );
        kernel
            .register_driver_for_operator(CommandDriver::new(
                EXECUTION_DRIVER_NAME,
                execution_commands,
            ))
            .map_err(kernel_error)?;
        self.bridge
            .set_vm_permissions(&vm_id, &permissions_policy)?;

        self.bridge
            .emit_lifecycle(&vm_id, LifecycleState::Starting)?;
        self.bridge.emit_lifecycle(&vm_id, LifecycleState::Ready)?;
        self.bridge.emit_log(
            &vm_id,
            format!("created VM {vm_id} for session {session_id}"),
        )?;

        self.sessions
            .get_mut(&session_id)
            .expect("owned session should exist")
            .vm_ids
            .insert(vm_id.clone());
        let unix_socket_host_dir = create_vm_unix_socket_host_dir()?;
        let pending_stdin_bytes_budget = VmPendingByteBudget::new(
            limits.process.pending_stdin_bytes,
            agentos_resource_accounting::queue_tracker::TrackedLimit::PendingKernelStdinBytes,
        );
        let pending_event_bytes_budget = VmPendingByteBudget::new(
            limits.process.pending_event_bytes,
            agentos_resource_accounting::queue_tracker::TrackedLimit::PendingExecutionEventBytes,
        );
        let pending_child_sync_count_budget = VmPendingByteBudget::new(
            limits.process.max_pending_child_sync_count,
            agentos_resource_accounting::queue_tracker::TrackedLimit::PendingChildProcessSyncCount,
        );
        let pending_child_sync_bytes_budget = VmPendingByteBudget::new(
            limits.process.max_pending_child_sync_bytes,
            agentos_resource_accounting::queue_tracker::TrackedLimit::PendingChildProcessSyncBytes,
        );
        self.vms.insert(
            vm_id.clone(),
            VmState {
                execution_engines: VmExecutionEngines::new(
                    vm_id.clone(),
                    vm_runtime_context.clone(),
                    Arc::clone(&self.process_event_notify),
                ),
                connection_id: connection_id.clone(),
                session_id: session_id.clone(),
                generation: vm_generation,
                limits,
                pending_stdin_bytes_budget,
                pending_event_bytes_budget,
                pending_child_sync_count_budget,
                pending_child_sync_bytes_budget,
                resources: vm_resources,
                runtime_context: vm_runtime_context,
                database,
                capabilities,
                dns,
                listen_policy,
                create_loopback_exempt_ports,
                base_guest_env: guest_env.clone(),
                guest_env,
                standalone_wasm_backend: match create_config.wasm_backend.unwrap_or_default() {
                    vm_config::StandaloneWasmBackend::V8 => {
                        crate::executor::StandaloneWasmBackend::V8
                    }
                    vm_config::StandaloneWasmBackend::Wasmtime => {
                        crate::executor::StandaloneWasmBackend::Wasmtime
                    }
                    vm_config::StandaloneWasmBackend::WasmtimeThreads => {
                        crate::executor::StandaloneWasmBackend::WasmtimeThreads
                    }
                },
                requested_runtime: runtime,
                root_filesystem_mode: protocol_root_filesystem_mode(root_filesystem.mode),
                guest_cwd,
                runtime_scratch_root,
                host_cwd,
                kernel,
                kernel_socket_readiness,
                managed_host_net_descriptions: Arc::new(Mutex::new(BTreeMap::new())),
                host_net_transfer_descriptions: Arc::new(Mutex::new(BTreeMap::new())),
                loaded_snapshot,
                configuration: VmConfiguration {
                    defaults_profile: create_config.defaults_profile(),
                    permissions: permissions_policy,
                    js_runtime: create_config.js_runtime.clone(),
                    ..VmConfiguration::default()
                },
                layers: VmLayerStore::default(),
                command_guest_paths: BTreeMap::new(),
                provided_commands: BTreeMap::new(),
                package_descriptors: Vec::new(),
                runtime_linked_package_ids: BTreeSet::new(),
                installed_package_pins: BTreeMap::new(),
                package_mount_roots: BTreeMap::new(),
                package_mount_paths: BTreeMap::new(),
                package_created_mountpoints: BTreeMap::new(),
                command_permissions: BTreeMap::new(),
                host_functions: BTreeMap::new(),
                active_processes: BTreeMap::new(),
                process_output_replays: BTreeMap::new(),
                process_output_replay_order: VecDeque::new(),
                vm_fetch_streams: BTreeMap::new(),
                next_vm_fetch_stream_id: 0,
                executions: BTreeMap::new(),
                execution_processes: BTreeMap::new(),
                next_public_execution_id: 0,
                execution_retention_wake_deadline_ms: None,
                execution_retention_wake_task: None,
                package_mutation_execution_id: None,
                typescript_compiler_staged: false,
                exited_process_snapshots: VecDeque::new(),
                detached_child_processes: BTreeSet::new(),
                attached_child_event_cursor: 0,
                detached_child_event_cursor: 0,
                packages_staging_root: None,
                unix_address_registry: Arc::new(Mutex::new(BTreeMap::new())),
                unix_socket_host_dir,
            },
        )?;
        self.observe_active_vm_generations();

        let events = vec![
            self.vm_lifecycle_event(
                &connection_id,
                &session_id,
                &vm_id,
                VmLifecycleState::Creating,
            ),
            self.vm_lifecycle_event(&connection_id, &session_id, &vm_id, VmLifecycleState::Ready),
        ];

        tracing::info!(target: "agentos_vm::perf", phase = "create_vm", elapsed_ms = __t.elapsed().as_millis() as u64, "vm phase");
        Ok((vm_id, events))
    }

    pub(crate) async fn dispose_vm(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: crate::protocol::DisposeVmRequest,
    ) -> Result<DispatchResult, VmError> {
        let plan = self.prepare_dispose_vm(request, payload)?;
        let prepared = self.detach_vm_for_disposal(plan)?;
        let completed = prepared.execute().await;
        self.complete_dispose_vm(completed)
    }

    pub(crate) fn bootstrap_root_filesystem(
        &mut self,
        request: &crate::protocol::RequestFrame,
        entries: Vec<RootFilesystemEntry>,
    ) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static {
        let input = self.prepare_vm_lifecycle_request(request);
        async move { bootstrap_root_filesystem_owned(input?, entries).await }
    }

    pub(crate) fn configure_vm(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: ConfigureVmRequest,
    ) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static {
        let input = self.prepare_configure_vm_request(request);
        async move { configure_vm_owned(input?, payload) }
    }

    /// Runtime dynamic `linkSoftware`: add one package's tar/current/bin leaf
    /// mounts to the live VM so commands appear under `/opt/agentos/bin`
    /// immediately, with no reboot. Returns the linked command names.
    pub(crate) fn link_package(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: LinkPackageRequest,
    ) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static {
        let input = self.prepare_link_package_request(request);
        async move { link_package_owned(input?, payload).await }
    }

    pub(crate) fn install_package(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: crate::protocol::InstallPackageRequest,
    ) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static {
        let input = self.prepare_link_package_request(request);
        let request = request.clone();
        async move { crate::service::install_package_owned(request, input, payload).await }
    }

    pub(crate) fn unlink_package(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: UnlinkPackageRequest,
    ) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static {
        let input = self.prepare_link_package_request(request);
        async move { unlink_package_owned(input?, payload).await }
    }

    pub(crate) fn provided_commands(
        &mut self,
        request: &crate::protocol::RequestFrame,
        _payload: ProvidedCommandsRequest,
    ) -> crate::execution::OwnedVmRouteFuture {
        let input = self.prepare_owned_vm_route(request);
        Box::pin(async move {
            let input = input?;
            let packages = input.vm.try_read("list provided commands", |vm| {
                vm.provided_commands
                    .iter()
                    .map(|(package_name, commands)| PackageCommands {
                        package_name: package_name.clone(),
                        commands: commands.clone(),
                    })
                    .collect()
            })?;
            Ok(DispatchResult {
                response: provided_commands_response(&input.request, packages),
                events: Vec::new(),
            })
        })
    }

    pub(crate) fn create_layer(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: CreateLayerRequest,
    ) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static {
        let input = self.prepare_vm_lifecycle_request(request);
        async move { create_layer_owned(input?, payload).await }
    }

    pub(crate) fn seal_layer(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: SealLayerRequest,
    ) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static {
        let input = self.prepare_vm_lifecycle_request(request);
        async move { seal_layer_owned(input?, payload).await }
    }

    pub(crate) fn import_snapshot(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: ImportSnapshotRequest,
    ) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static {
        let input = self.prepare_vm_lifecycle_request(request);
        async move { import_snapshot_owned(input?, payload).await }
    }

    pub(crate) fn export_snapshot(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: ExportSnapshotRequest,
    ) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static {
        let input = self.prepare_vm_lifecycle_request(request);
        async move { export_snapshot_owned(input?, payload).await }
    }

    pub(crate) fn create_overlay(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: CreateOverlayRequest,
    ) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static {
        let input = self.prepare_vm_lifecycle_request(request);
        async move { create_overlay_owned(input?, payload).await }
    }

    pub(crate) fn snapshot_root_filesystem(
        &mut self,
        request: &crate::protocol::RequestFrame,
        payload: SnapshotRootFilesystemRequest,
    ) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static {
        let input = self.prepare_vm_lifecycle_request(request);
        async move { snapshot_root_filesystem_owned(input?, payload).await }
    }

    pub(crate) fn list_mounts(
        &mut self,
        request: &crate::protocol::RequestFrame,
        _payload: ListMountsRequest,
    ) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static {
        let input = self.prepare_vm_lifecycle_request(request);
        async move {
            let input = input?;
            let mounts = input.vm.try_read("list mounts", |vm| {
                vm.kernel
                    .mounted_filesystems()
                    .into_iter()
                    .map(|mount| MountInfo {
                        path: mount.path,
                        kind: mount.plugin_id,
                        read_only: mount.read_only,
                    })
                    .collect()
            })?;
            Ok(DispatchResult {
                response: mounts_listed_response(&input.request, mounts),
                events: Vec::new(),
            })
        }
    }

    pub(crate) async fn dispose_vm_internal(
        &mut self,
        connection_id: &str,
        session_id: &str,
        vm_id: &str,
        reason: DisposeReason,
    ) -> Result<Vec<EventFrame>, VmError> {
        self.require_owned_vm(connection_id, session_id, vm_id)?;

        let cancellation_reason = match &reason {
            DisposeReason::Requested => OperationCancellationReason::Explicit,
            DisposeReason::ConnectionClosed => OperationCancellationReason::ConnectionClosed,
            DisposeReason::HostShutdown => OperationCancellationReason::Shutdown,
        };
        if let Err(error) = self.process_event_broker.dispose_vm(
            connection_id,
            session_id,
            vm_id,
            cancellation_reason,
        ) {
            eprintln!(
                "ERR_AGENTOS_PROCESS_EVENT_VM_DISPOSAL: connection_id={connection_id} session_id={session_id} vm_id={vm_id} error={error}"
            );
        }

        let mut events = vec![self.vm_lifecycle_event(
            connection_id,
            session_id,
            vm_id,
            VmLifecycleState::Disposing,
        )];
        // Process termination needs the VM live in `self.vms` (it looks up and
        // signals the VM's active processes). Capture its result but keep tearing
        // down: a process that refuses to die must not strand the VM's tracking
        // entries for the process lifetime.
        let terminate_result = self.terminate_vm_processes(vm_id, &mut events).await;
        if let Some(mut vm) = self.vms.get_mut(vm_id) {
            if let Some(task) = vm.execution_retention_wake_task.take() {
                task.abort();
            }
            vm.execution_retention_wake_deadline_ms = None;
            for execution in vm.executions.values_mut() {
                if let Some(task) = execution.deadline_task.take() {
                    task.abort();
                }
            }
        }

        // Process and database teardown can require the VM blocking executor
        // to flush state and close a local SQLite connection. Keep blocking-job
        // admission open until both have completed; the VM is detached below
        // before running the remaining teardown work.
        let (vm_runtime_context, vm_capabilities, vm_generation) = self
            .vms
            .get(vm_id)
            .map(|vm| {
                (
                    vm.runtime_context.clone(),
                    vm.capabilities.clone(),
                    vm.generation,
                )
            })
            .expect("owned VM should exist before disposal");
        // No new guest capabilities may appear while trusted database cleanup
        // is still allowed to use the VM's blocking executor.
        let capability_admission_error = close_vm_capability_admission(&vm_capabilities).err();
        if let Some(error) = capability_admission_error.as_ref() {
            eprintln!("ERR_AGENTOS_VM_CAPABILITY_ADMISSION_CLOSE: vm_id={vm_id} error={error}");
        }
        // Detach the VM from `self.vms` BEFORE the remaining fallible teardown so
        // no `?` below can leave the registry entry (or any per-VM map) behind.
        let mut vm = self
            .vms
            .try_remove(vm_id, "dispose")?
            .expect("owned VM should exist before disposal");

        // `continue_on_error = true` => `shutdown_configured_mounts` never returns
        // `Err` on the dispose path (it logs and presses on), so its result is
        // intentionally discarded rather than `?`-ed.
        let mount_context = MountPluginContext {
            bridge: self.bridge.clone(),
            runtime_context: vm.runtime_context.clone(),
            connection_id: connection_id.to_owned(),
            session_id: session_id.to_owned(),
            vm_id: vm_id.to_owned(),
            sidecar_requests: self.sidecar_requests.clone(),
            database: vm.database.clone(),
            max_pread_bytes: vm.kernel.resource_limits().max_pread_bytes,
        };
        if let Err(error) = shutdown_configured_mounts(&mut vm, &mount_context, "dispose_vm", true)
        {
            eprintln!(
                "ERR_AGENTOS_MOUNT_TEARDOWN: mount shutdown returned an unexpected error for VM {vm_id}: {error}"
            );
        }

        // Snapshot/flush/kernel-dispose/permission-reset can each fail; run them
        // in a helper whose result is captured so cleanup below is unconditional.
        let mut teardown_result = self.finish_vm_teardown(vm_id, &mut vm).await;
        let shutdown_deadline = Duration::from_millis(vm.limits.reactor.shutdown_deadline_ms);
        let teardown_deadline = tokio::time::Instant::now() + shutdown_deadline;
        let mut sqlite_close_timed_out = false;
        if let Some(database) = vm.database.take() {
            let (close_result, timed_out) = close_vm_database_before_deadline(
                vm_id,
                database.as_ref(),
                teardown_deadline,
                vm.limits.reactor.shutdown_deadline_ms,
            )
            .await;
            sqlite_close_timed_out = timed_out;
            if let Err(error) = close_result {
                eprintln!(
                    "ERR_AGENTOS_VM_TEARDOWN_CLEANUP: vm_id={vm_id} phase=sqlite_close error={error}"
                );
                if teardown_result.is_ok() {
                    teardown_result = Err(error);
                }
            }
        }

        vm_runtime_context.close_admission();
        let fairness_retirement_result = retire_vm_fairness(&vm_runtime_context, vm_generation);
        if let Err(error) = fairness_retirement_result.as_ref() {
            eprintln!("ERR_AGENTOS_VM_FAIRNESS_RETIRE: vm_id={vm_id} error={error}");
        }

        // Reclaim EVERY per-VM tracking entry on EVERY exit path — even when a
        // teardown step above errored. Pre-fix these ran only after the fallible
        // steps' `?`, so any failure stranded the engine/extension maps (H1) and
        // the output-buffer map was never reclaimed at all (M6).
        self.reclaim_vm_tracking(session_id, vm_id);
        if let Err(error) = fs::remove_dir_all(&vm.runtime_scratch_root) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "ERR_AGENTOS_VM_SCRATCH_CLEANUP: failed to remove {}: {error}",
                    vm.runtime_scratch_root.display()
                );
            }
        }
        if let Some(staging_root) = vm.packages_staging_root.take() {
            if let Err(error) = fs::remove_dir_all(&staging_root) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    eprintln!(
                        "ERR_AGENTOS_PACKAGE_STAGING_CLEANUP: failed to remove {}: {error}",
                        staging_root.display()
                    );
                }
            }
        }
        if let Err(error) = fs::remove_dir_all(&vm.unix_socket_host_dir) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    path = %vm.unix_socket_host_dir.display(),
                    %error,
                    "failed to remove private Unix socket namespace during VM teardown"
                );
            }
        }

        let (reconciliation, reconciliation_timed_out) = wait_for_vm_reconciliation(
            vm.resources.as_ref(),
            &vm.runtime_context,
            &vm.capabilities,
            teardown_deadline.saturating_duration_since(tokio::time::Instant::now()),
        )
        .await;
        let deadline_expired = sqlite_close_timed_out || reconciliation_timed_out;
        let quarantine_reason = vm_quarantine_reason(
            capability_admission_error.is_some(),
            fairness_retirement_result.is_err(),
            reconciliation,
            deadline_expired,
        );

        if let Some(reason) = quarantine_reason {
            let teardown_deadline_expired = matches!(&reason, VmQuarantineReason::TeardownDeadline);
            let mut diagnostic = match reason {
                VmQuarantineReason::TeardownDeadline => format!(
                    "ERR_AGENTOS_VM_TEARDOWN_DEADLINE: vm_id={vm_id} generation={} active_tasks={} outstanding_capabilities={} ledger_zero={} deadline_ms={}; raise limits.reactor.shutdownDeadlineMs",
                    vm.generation,
                    reconciliation.active_tasks,
                    reconciliation.outstanding_capabilities,
                    reconciliation.ledger_zero,
                    vm.limits.reactor.shutdown_deadline_ms
                ),
                VmQuarantineReason::ResourceIntegrity => format!(
                    "ERR_AGENTOS_VM_RESOURCE_INTEGRITY: vm_id={vm_id} generation={} accounting integrity failed; generation cannot be reaped",
                    vm.generation
                ),
                VmQuarantineReason::CapabilityRegistryIntegrity => format!(
                    "ERR_AGENTOS_VM_CAPABILITY_INTEGRITY: vm_id={vm_id} generation={} capability admission could not be closed; generation cannot be reaped; error={}",
                    vm.generation,
                    capability_admission_error.as_deref().unwrap_or("unknown")
                ),
                VmQuarantineReason::FairnessIntegrity => format!(
                    "ERR_AGENTOS_VM_FAIRNESS_INTEGRITY: vm_id={vm_id} generation={} fairness membership could not be retired; generation cannot be reaped; error={}",
                    vm.generation,
                    fairness_retirement_result
                        .as_ref()
                        .expect_err("fairness quarantine requires a retirement error")
                ),
            };
            if sqlite_close_timed_out {
                diagnostic.push_str("; SQLite close is unconfirmed; generation cannot be reaped; restart the sidecar worker to release quarantine");
            }
            eprintln!("{diagnostic}");
            if let Err(error) = terminate_result.as_ref() {
                eprintln!(
                    "ERR_AGENTOS_VM_TEARDOWN_CLEANUP: vm_id={vm_id} phase=processes error={error}"
                );
            }
            if let Err(error) = teardown_result.as_ref() {
                eprintln!(
                    "ERR_AGENTOS_VM_TEARDOWN_CLEANUP: vm_id={vm_id} phase=kernel_or_bridge error={error}"
                );
            }
            self.retain_quarantined_vm(QuarantinedVmGeneration {
                connection_id: connection_id.to_owned(),
                session_id: session_id.to_owned(),
                vm_id: vm_id.to_owned(),
                generation: vm.generation,
                resources: Arc::clone(&vm.resources),
                runtime_context: vm.runtime_context.clone(),
                capabilities: vm.capabilities.clone(),
                reason,
                sqlite_close_unconfirmed: sqlite_close_timed_out,
            })?;
            return Err(if teardown_deadline_expired {
                VmError::VmTeardownDeadline {
                    message: diagnostic,
                    vm_id: vm_id.to_owned(),
                    deadline_ms: vm.limits.reactor.shutdown_deadline_ms,
                }
            } else {
                VmError::Execution(diagnostic)
            });
        }

        self.observe_active_vm_generations();
        // Surface the first failure only AFTER cleanup has completed.
        fairness_retirement_result?;
        terminate_result?;
        teardown_result?;

        events.push(self.vm_lifecycle_event(
            connection_id,
            session_id,
            vm_id,
            VmLifecycleState::Disposed,
        ));
        Ok(events)
    }

    /// Run every fallible second-half cleanup step, retaining the first error
    /// while logging later failures. Teardown must reach kernel disposal and
    /// permission reset even when snapshot or bridge work fails.
    async fn finish_vm_teardown(&mut self, vm_id: &str, vm: &mut VmState) -> Result<(), VmError> {
        let mut first_error = None;
        let snapshot = if vm.kernel.root_filesystem_mut().is_some() {
            match vm
                .kernel
                .snapshot_root_filesystem()
                .map_err(kernel_error)
                .and_then(|snapshot| encode_root_snapshot(&snapshot).map_err(root_filesystem_error))
            {
                Ok(bytes) => Some(FilesystemSnapshot {
                    format: String::from(ROOT_FILESYSTEM_SNAPSHOT_FORMAT),
                    bytes,
                }),
                Err(error) => {
                    record_vm_teardown_error(vm_id, "snapshot", error, &mut first_error);
                    None
                }
            }
        } else {
            None
        };

        if let Err(error) = self
            .bridge
            .emit_lifecycle(vm_id, LifecycleState::Terminated)
        {
            record_vm_teardown_error(vm_id, "lifecycle", error, &mut first_error);
        }
        if let Err(error) = vm.kernel.dispose().map_err(kernel_error) {
            record_vm_teardown_error(vm_id, "kernel", error, &mut first_error);
        }
        if let Some(snapshot) = snapshot {
            if let Err(error) = self.bridge.with_mut(|bridge| {
                bridge.flush_filesystem_state(FlushFilesystemStateRequest {
                    vm_id: vm_id.to_owned(),
                    snapshot,
                })
            }) {
                record_vm_teardown_error(vm_id, "filesystem_flush", error, &mut first_error);
            }
        }
        if let Err(error) = self.bridge.clear_vm_permissions(vm_id) {
            record_vm_teardown_error(vm_id, "permission_reset", error, &mut first_error);
        }
        first_error.map_or(Ok(()), Err)
    }

    pub(crate) async fn terminate_vm_processes(
        &mut self,
        vm_id: &str,
        events: &mut Vec<EventFrame>,
    ) -> Result<(), VmError> {
        let process_ids = self
            .vms
            .get(vm_id)
            .map(|vm| vm.active_processes.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        if process_ids.is_empty() {
            return Ok(());
        }

        for process_id in process_ids {
            if self
                .vms
                .get(vm_id)
                .is_some_and(|vm| vm.active_processes.contains_key(&process_id))
            {
                self.kill_process_internal(vm_id, &process_id, "SIGTERM")?;
            }
        }
        self.wait_for_vm_processes_to_exit(vm_id, DISPOSE_VM_SIGTERM_GRACE, events)
            .await?;

        if !self.vm_has_active_processes(vm_id) {
            return Ok(());
        }

        let remaining = self
            .vms
            .get(vm_id)
            .map(|vm| vm.active_processes.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for process_id in remaining {
            if self
                .vms
                .get(vm_id)
                .is_some_and(|vm| vm.active_processes.contains_key(&process_id))
            {
                self.kill_process_internal(vm_id, &process_id, "SIGKILL")?;
            }
        }
        self.wait_for_vm_processes_to_exit(vm_id, DISPOSE_VM_SIGKILL_GRACE, events)
            .await?;

        if self.vm_has_active_processes(vm_id) {
            // A shared V8 execution can take longer than the bounded event-pump
            // grace to report its exit after SIGKILL. VM teardown must still
            // finalize process-owned bridge state before snapshotting the root
            // filesystem; dropping ActiveProcess after the snapshot loses
            // committed host-materialized SQLite/WAL pages.
            let mut vm = self
                .vms
                .get_mut(vm_id)
                .expect("active VM should exist during process teardown");
            let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
            let unix_address_registry = Arc::clone(&vm.unix_address_registry);
            let remaining = std::mem::take(&mut vm.active_processes);
            eprintln!(
                "ERR_AGENTOS_VM_FORCED_PROCESS_FINALIZE: vm_id={vm_id} process_count={}",
                remaining.len()
            );
            for (process_id, mut process) in remaining {
                terminate_child_process_tree(
                    &mut vm.kernel,
                    &mut process,
                    &kernel_readiness,
                    &unix_address_registry,
                );
                process
                    .kernel_handle
                    .finish_signaled(nix::libc::SIGKILL, false);
                let exit_code = vm
                    .kernel
                    .list_processes()
                    .get(&process.kernel_pid)
                    .and_then(|entry| entry.exit_code)
                    .unwrap_or(137);
                events.push(forced_process_exit_event::<B>(
                    &mut vm,
                    vm_id,
                    &process_id,
                    exit_code,
                ));
                if let Err(error) = vm.kernel.wait_and_reap(process.kernel_pid) {
                    eprintln!(
                        "ERR_AGENTOS_PROCESS_REAP: vm_id={vm_id} pid={} error={error}",
                        process.kernel_pid
                    );
                }
            }
        }

        Ok(())
    }

    pub(crate) async fn wait_for_vm_processes_to_exit(
        &mut self,
        vm_id: &str,
        timeout: Duration,
        events: &mut Vec<EventFrame>,
    ) -> Result<(), VmError> {
        let ownership = self.vm_ownership(vm_id)?;
        let deadline = Instant::now() + timeout;

        while self.vm_has_active_processes(vm_id) && Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if let Some(event) = self.poll_event(&ownership, remaining).await? {
                events.push(event);
            }
        }

        Ok(())
    }
}

impl<B> PreparedCreateVm<B>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    pub async fn execute(self) -> Result<CompletedCreateVm<B>, VmError> {
        let __t = Instant::now();
        let PreparedCreateVm {
            request,
            payload,
            connection_id,
            session_id,
            vm_id,
            vm_generation,
            create_config,
            root_filesystem,
            permissions_policy,
            limits,
            vm_resources,
            vm_runtime_context,
            dns,
            listen_policy,
            create_loopback_exempt_ports,
            bridge,
            dns_resolver,
            sidecar_requests,
            process_event_notify,
            extensions,
        } = self;
        let cwd = create_vm_runtime_scratch_root(&vm_id)?;
        let cleanup_bridge = bridge.clone();
        let cleanup_vm_id = vm_id.clone();
        let cleanup_cwd = cwd.clone();
        let result = async move {
            let (guest_cwd, host_cwd) = resolve_vm_cwds(create_config.cwd.as_ref(), &cwd)?;
            fs::create_dir_all(&host_cwd)
                .map_err(|error| VmError::Io(format!("failed to create VM cwd: {error}")))?;
            let resource_limits = limits.resources.clone();
            let database = match create_config.database.as_ref() {
                Some(descriptor) => {
                    let database = crate::vm_sqlite::resolve_vm_sqlite(
                        descriptor,
                        vm_runtime_context.clone(),
                        limits.sqlite.max_result_bytes,
                        Some((
                            sidecar_requests.clone(),
                            crate::protocol::OwnershipScope::session(
                                connection_id.clone(),
                                session_id.clone(),
                            ),
                        )),
                    )
                    .await
                    .map_err(|error| {
                        VmError::InvalidState(format!(
                            "failed to resolve VM SQLite database: {error}"
                        ))
                    })?;
                    crate::plugins::chunked_sqlite::bootstrap_schema(database.as_ref())
                        .await
                        .map_err(|error| {
                            VmError::InvalidState(format!(
                                "failed to migrate VM SQLite database: {error}"
                            ))
                        })?;
                    for extension in extensions {
                        extension
                            .bootstrap_vm_database(database.clone())
                            .await
                            .map_err(|error| {
                                VmError::InvalidState(format!(
                                    "failed to migrate extension VM database schema: {error}"
                                ))
                            })?;
                    }
                    Some(database)
                }
                None => None,
            };
            let capabilities = CapabilityRegistry::new(vm_generation, Arc::clone(&vm_resources));
            bridge.set_vm_permissions(&vm_id, &permissions_policy)?;
            let permissions = bridge_permissions(bridge.clone(), &vm_id);
            let configured_env = create_vm_environment(&create_config)?;
            let mut guest_env = filter_env(&vm_id, &configured_env, &permissions);
        // Trusted bootstrap uses operator-only kernel paths; guest permission
        // enforcement remains active throughout creation.
            let native_root = native_root_plugin_from_config(create_config.native_root.as_ref())?;
            let loaded_snapshot = if native_root.is_some() {
                None
            } else {
                bridge.with_mut(|bridge| {
                    bridge.load_filesystem_state(LoadFilesystemStateRequest {
                        vm_id: vm_id.clone(),
                    })
                })?
            };

            let mut config = KernelVmConfig::new(vm_id.clone());
            config.vm_generation = vm_generation;
            config.cwd = guest_cwd.clone();
            config.env = guest_env.clone();
            if let Some(user) = create_config.user.as_ref() {
                config.user = agentos_vm_kernel::user::UserConfig {
                    uid: user.uid,
                    gid: user.gid,
                    euid: user.euid,
                    egid: user.egid,
                    username: user.username.clone(),
                    homedir: user.homedir.clone(),
                    shell: user.shell.clone(),
                    gecos: user.gecos.clone(),
                    group_name: user.group_name.clone(),
                    supplementary_gids: user.supplementary_gids.clone().unwrap_or_default(),
                    accounts: user
                        .accounts
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .map(|account| agentos_vm_kernel::user::UserAccount {
                            uid: account.uid,
                            gid: account.gid,
                            username: account.username.clone(),
                            homedir: account.homedir.clone(),
                            shell: account.shell.clone(),
                            gecos: account.gecos.clone().unwrap_or_default(),
                            supplementary_gids: account.supplementary_gids.clone(),
                        })
                        .collect(),
                    groups: user
                        .groups
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .map(|group| agentos_vm_kernel::user::GroupRecord {
                            gid: group.gid,
                            name: group.name.clone(),
                            members: group.members.clone(),
                        })
                        .collect(),
                };
            }
            config.permissions = permissions;
            config.dns = agentos_vm_kernel::dns::DnsConfig {
                name_servers: dns.name_servers.clone(),
                overrides: dns.overrides.clone(),
            };
            config.dns_resolver = dns_resolver;
            config.loopback_exempt_ports = create_loopback_exempt_ports.clone();
            let mount_plugins = build_mount_plugin_registry::<B>()?;
            let root_mount_table = if let Some(native_root) = native_root.as_ref() {
                build_native_root_mount_table(
                    &mount_plugins,
                    native_root,
                    &root_filesystem,
                    MountPluginContext {
                        bridge: bridge.clone(),
                        runtime_context: vm_runtime_context.clone(),
                        connection_id: connection_id.clone(),
                        session_id: session_id.clone(),
                        vm_id: vm_id.clone(),
                        sidecar_requests,
                        database: database.clone(),
                        max_pread_bytes: resource_limits.max_pread_bytes,
                    },
                )?
            } else {
                crate::core::build_root_mount_table_with_loaded_snapshot(
                    &create_config.root_filesystem,
                    loaded_snapshot.as_ref(),
                    &resource_limits,
                )
                .map_err(|error| VmError::InvalidState(error.to_string()))?
            };
            config.resources = resource_limits;
            let mut kernel = KernelVm::new(root_mount_table, config);
            kernel
                .set_socket_resource_ledger(Arc::clone(&vm_resources))
                .map_err(kernel_error)?;
            let kernel_socket_readiness: KernelSocketReadinessRegistry = Arc::new(
                crate::state::KernelSocketReadinessRegistryState::new(
                    limits.reactor.max_capabilities,
                ),
            );
            let readiness_targets = Arc::clone(&kernel_socket_readiness);
            kernel.set_socket_readiness_sink(Some(move |readiness: SocketReadiness| {
                for target in readiness_targets.targets(readiness.socket_id) {
                    send_kernel_socket_readiness_event(target, readiness);
                }
            }));
            let command_guest_paths = discover_command_guest_paths(&mut kernel)?;
            refresh_guest_command_path_env_from_map(&mut guest_env, &command_guest_paths);
            let mut execution_commands =
                default_execution_commands(create_config.defaults_profile());
            if let Some(bootstrap_commands) = &create_config.bootstrap_commands {
                execution_commands.extend(bootstrap_commands.iter().cloned());
            }
            execution_commands.extend(command_guest_paths.keys().cloned());
            kernel
                .register_driver_for_operator(CommandDriver::new(
                    EXECUTION_DRIVER_NAME,
                    execution_commands,
                ))
                .map_err(kernel_error)?;
            if let Some(root) = kernel.root_filesystem_mut() {
                root.finish_bootstrap();
            }
            bridge.set_vm_permissions(&vm_id, &permissions_policy)?;
            bridge.emit_lifecycle(&vm_id, LifecycleState::Starting)?;
            bridge.emit_lifecycle(&vm_id, LifecycleState::Ready)?;
            bridge.emit_log(
                &vm_id,
                format!("created VM {vm_id} for session {session_id}"),
            )?;

            let unix_socket_host_dir = create_vm_unix_socket_host_dir()?;
            let pending_stdin_bytes_budget = VmPendingByteBudget::new(
                limits.process.pending_stdin_bytes,
                agentos_resource_accounting::queue_tracker::TrackedLimit::PendingKernelStdinBytes,
            );
            let pending_event_bytes_budget = VmPendingByteBudget::new(
                limits.process.pending_event_bytes,
                agentos_resource_accounting::queue_tracker::TrackedLimit::PendingExecutionEventBytes,
            );
            let events = vec![
                shared_vm_lifecycle_event(
                    &connection_id,
                    &session_id,
                    &vm_id,
                    VmLifecycleState::Creating,
                ),
                shared_vm_lifecycle_event(
                    &connection_id,
                    &session_id,
                    &vm_id,
                    VmLifecycleState::Ready,
                ),
            ];
            let vm = VmState {
                execution_engines: VmExecutionEngines::new(vm_id.clone(), vm_runtime_context.clone(), process_event_notify),
                connection_id: connection_id.clone(),
                session_id: session_id.clone(),
                generation: vm_generation,
                pending_child_sync_count_budget: VmPendingByteBudget::new(limits.process.max_pending_child_sync_count, agentos_resource_accounting::queue_tracker::TrackedLimit::PendingChildProcessSyncCount),
                pending_child_sync_bytes_budget: VmPendingByteBudget::new(limits.process.max_pending_child_sync_bytes, agentos_resource_accounting::queue_tracker::TrackedLimit::PendingChildProcessSyncBytes),
                limits,
                pending_stdin_bytes_budget,
                pending_event_bytes_budget,
                resources: vm_resources,
                runtime_context: vm_runtime_context,
                database,
                capabilities,
                dns,
                listen_policy,
                create_loopback_exempt_ports,
                base_guest_env: guest_env.clone(),
                guest_env,
                standalone_wasm_backend: match create_config.wasm_backend.unwrap_or_default() {
                    vm_config::StandaloneWasmBackend::V8 => crate::executor::StandaloneWasmBackend::V8,
                    vm_config::StandaloneWasmBackend::Wasmtime => crate::executor::StandaloneWasmBackend::Wasmtime,
                    vm_config::StandaloneWasmBackend::WasmtimeThreads => crate::executor::StandaloneWasmBackend::WasmtimeThreads,
                },
                requested_runtime: payload.runtime,
                root_filesystem_mode: protocol_root_filesystem_mode(root_filesystem.mode),
                guest_cwd,
                runtime_scratch_root: cwd,
                host_cwd,
                kernel,
                kernel_socket_readiness,
                managed_host_net_descriptions: Arc::new(Mutex::new(BTreeMap::new())),
                host_net_transfer_descriptions: Arc::new(Mutex::new(BTreeMap::new())),
                loaded_snapshot,
                configuration: VmConfiguration {
                    defaults_profile: create_config.defaults_profile(),
                    permissions: permissions_policy,
                    js_runtime: create_config.js_runtime.clone(),
                    ..VmConfiguration::default()
                },
                layers: VmLayerStore::default(),
                command_guest_paths,
                provided_commands: BTreeMap::new(),
                package_descriptors: Vec::new(),
                runtime_linked_package_ids: BTreeSet::new(),
                installed_package_pins: BTreeMap::new(),
                package_mount_roots: BTreeMap::new(),
                package_mount_paths: BTreeMap::new(),
                package_created_mountpoints: BTreeMap::new(),
                command_permissions: BTreeMap::new(),
                host_functions: BTreeMap::new(),
                active_processes: BTreeMap::new(),
                process_output_replays: BTreeMap::new(),
                process_output_replay_order: VecDeque::new(),
                vm_fetch_streams: BTreeMap::new(),
                next_vm_fetch_stream_id: 0,
                executions: BTreeMap::new(),
                execution_processes: BTreeMap::new(),
                next_public_execution_id: 0,
                execution_retention_wake_deadline_ms: None,
                execution_retention_wake_task: None,
                package_mutation_execution_id: None,
                typescript_compiler_staged: false,
                exited_process_snapshots: VecDeque::new(),
                detached_child_processes: BTreeSet::new(),
                attached_child_event_cursor: 0,
                detached_child_event_cursor: 0,
                packages_staging_root: None,
                unix_address_registry: Arc::new(Mutex::new(BTreeMap::new())),
                unix_socket_host_dir,
            };
            tracing::info!(target: "agentos_vm::perf", phase = "create_vm", elapsed_ms = __t.elapsed().as_millis() as u64, "vm phase");
            Ok(CompletedCreateVm {
                request,
                connection_id,
                session_id,
                vm_id,
                vm,
                events,
                bridge,
            })
        }
        .await;
        if result.is_err() {
            cleanup_path(&cleanup_cwd, "failed VM create shadow root");
            if let Err(error) = cleanup_bridge.clear_vm_permissions(&cleanup_vm_id) {
                eprintln!(
                    "ERR_AGENTOS_VM_CREATE_CLEANUP: vm_id={cleanup_vm_id} phase=permission_reset error={error}"
                );
            }
        }
        result
    }
}

impl<B> PreparedDisposeVm<B>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    pub async fn execute(mut self) -> CompletedDisposeVm {
        let DisposeVmPlan {
            request,
            connection_id,
            session_id,
            vm_id,
            reason: _,
            bridge,
            sidecar_requests,
            process_event_broker: _,
        } = self.plan;
        let mut events = vec![shared_vm_lifecycle_event(
            &connection_id,
            &session_id,
            &vm_id,
            VmLifecycleState::Disposing,
        )];

        let terminate_result =
            terminate_detached_vm_processes::<B>(&bridge, &vm_id, &mut self.vm, &mut events).await;
        if let Some(task) = self.vm.execution_retention_wake_task.take() {
            task.abort();
        }
        self.vm.execution_retention_wake_deadline_ms = None;
        for execution in self.vm.executions.values_mut() {
            if let Some(task) = execution.deadline_task.take() {
                task.abort();
            }
        }

        let capability_admission_error = close_vm_capability_admission(&self.vm.capabilities).err();
        if let Some(error) = capability_admission_error.as_ref() {
            eprintln!("ERR_AGENTOS_VM_CAPABILITY_ADMISSION_CLOSE: vm_id={vm_id} error={error}");
        }

        let mount_context = MountPluginContext {
            bridge: bridge.clone(),
            runtime_context: self.vm.runtime_context.clone(),
            connection_id: connection_id.clone(),
            session_id: session_id.clone(),
            vm_id: vm_id.clone(),
            sidecar_requests,
            database: self.vm.database.clone(),
            max_pread_bytes: self.vm.kernel.resource_limits().max_pread_bytes,
        };
        if let Err(error) =
            shutdown_configured_mounts(&mut self.vm, &mount_context, "dispose_vm", true)
        {
            eprintln!(
                "ERR_AGENTOS_VM_TEARDOWN_CLEANUP: vm_id={vm_id} phase=mount_shutdown error={error}"
            );
        }
        let mut teardown_result = finish_vm_teardown_owned(&bridge, &vm_id, &mut self.vm);
        let shutdown_deadline = Duration::from_millis(self.vm.limits.reactor.shutdown_deadline_ms);
        let teardown_deadline = tokio::time::Instant::now() + shutdown_deadline;
        let mut sqlite_close_timed_out = false;
        if let Some(database) = self.vm.database.take() {
            let (close_result, timed_out) = close_vm_database_before_deadline(
                &vm_id,
                database.as_ref(),
                teardown_deadline,
                self.vm.limits.reactor.shutdown_deadline_ms,
            )
            .await;
            sqlite_close_timed_out = timed_out;
            if let Err(error) = close_result {
                eprintln!(
                    "ERR_AGENTOS_VM_TEARDOWN_CLEANUP: vm_id={vm_id} phase=sqlite_close error={error}"
                );
                if teardown_result.is_ok() {
                    teardown_result = Err(error);
                }
            }
        }

        self.vm.runtime_context.close_admission();
        let fairness_retirement_result =
            retire_vm_fairness(&self.vm.runtime_context, self.vm.generation);
        if let Err(error) = fairness_retirement_result.as_ref() {
            eprintln!("ERR_AGENTOS_VM_FAIRNESS_RETIRE: vm_id={vm_id} error={error}");
        }

        cleanup_path(&self.vm.runtime_scratch_root, "disposed VM shadow root");
        if let Some(staging_root) = self.vm.packages_staging_root.take() {
            cleanup_path(&staging_root, "disposed VM package staging root");
        }
        cleanup_path(
            &self.vm.unix_socket_host_dir,
            "disposed VM Unix socket namespace",
        );

        let (reconciliation, reconciliation_timed_out) = wait_for_vm_reconciliation(
            self.vm.resources.as_ref(),
            &self.vm.runtime_context,
            &self.vm.capabilities,
            teardown_deadline.saturating_duration_since(tokio::time::Instant::now()),
        )
        .await;
        let deadline_expired = sqlite_close_timed_out || reconciliation_timed_out;
        let quarantine_reason = vm_quarantine_reason(
            capability_admission_error.is_some(),
            fairness_retirement_result.is_err(),
            reconciliation,
            deadline_expired,
        );

        let (quarantine, result) = if let Some(reason) = quarantine_reason {
            let teardown_deadline_expired = matches!(&reason, VmQuarantineReason::TeardownDeadline);
            let mut diagnostic = quarantine_diagnostic(
                &vm_id,
                &self.vm,
                reconciliation,
                reason.clone(),
                capability_admission_error.as_deref(),
                fairness_retirement_result.as_ref().err(),
            );
            if sqlite_close_timed_out {
                diagnostic.push_str("; SQLite close is unconfirmed; generation cannot be reaped; restart the sidecar worker to release quarantine");
            }
            eprintln!("{diagnostic}");
            if let Err(error) = terminate_result.as_ref() {
                eprintln!(
                    "ERR_AGENTOS_VM_TEARDOWN_CLEANUP: vm_id={vm_id} phase=processes error={error}"
                );
            }
            if let Err(error) = teardown_result.as_ref() {
                eprintln!(
                    "ERR_AGENTOS_VM_TEARDOWN_CLEANUP: vm_id={vm_id} phase=kernel_or_bridge error={error}"
                );
            }
            (
                Some(QuarantinedVmGeneration {
                    connection_id: connection_id.clone(),
                    session_id: session_id.clone(),
                    vm_id: vm_id.clone(),
                    generation: self.vm.generation,
                    resources: Arc::clone(&self.vm.resources),
                    runtime_context: self.vm.runtime_context.clone(),
                    capabilities: self.vm.capabilities.clone(),
                    reason,
                    sqlite_close_unconfirmed: sqlite_close_timed_out,
                }),
                Err(if teardown_deadline_expired {
                    VmError::VmTeardownDeadline {
                        message: diagnostic,
                        vm_id: vm_id.clone(),
                        deadline_ms: self.vm.limits.reactor.shutdown_deadline_ms,
                    }
                } else {
                    VmError::Execution(diagnostic)
                }),
            )
        } else {
            let result = fairness_retirement_result
                .and(terminate_result)
                .and(teardown_result);
            (None, result)
        };

        CompletedDisposeVm {
            request,
            connection_id,
            session_id,
            vm_id,
            events: std::mem::take(&mut events),
            quarantine,
            result,
        }
    }
}

fn forced_process_exit_event<B>(
    vm: &mut VmState,
    vm_id: &str,
    process_id: &str,
    exit_code: i32,
) -> EventFrame
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    vm.record_process_exit(process_id, exit_code);
    let payload = VmManager::<B>::complete_public_execution_in_vm(vm, vm_id, process_id, exit_code)
        .unwrap_or_else(|| {
            EventPayload::ProcessExited(ProcessExitedEvent {
                process_id: process_id.to_owned(),
                exit_code,
            })
        });
    EventFrame::new(
        OwnershipScope::vm(&vm.connection_id, &vm.session_id, vm_id),
        payload,
    )
}

async fn terminate_detached_vm_processes<B>(
    bridge: &crate::state::SharedBridge<B>,
    vm_id: &str,
    vm: &mut VmState,
    events: &mut Vec<EventFrame>,
) -> Result<(), VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let mut first_error = None;
    let process_ids = vm.active_processes.keys().cloned().collect::<Vec<_>>();
    for process_id in &process_ids {
        if let Err(error) =
            VmManager::<B>::kill_process_in_vm(bridge, vm, vm_id, process_id, "SIGTERM")
        {
            record_vm_teardown_error(vm_id, "sigterm", error, &mut first_error);
        }
    }
    if !process_ids.is_empty() {
        tokio::time::sleep(DISPOSE_VM_SIGTERM_GRACE).await;
    }
    for process_id in &process_ids {
        if let Err(error) =
            VmManager::<B>::kill_process_in_vm(bridge, vm, vm_id, process_id, "SIGKILL")
        {
            record_vm_teardown_error(vm_id, "sigkill", error, &mut first_error);
        }
    }
    if !process_ids.is_empty() {
        tokio::time::sleep(DISPOSE_VM_SIGKILL_GRACE).await;
    }

    let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
    let unix_address_registry = Arc::clone(&vm.unix_address_registry);
    let remaining = std::mem::take(&mut vm.active_processes);
    if !remaining.is_empty() {
        eprintln!(
            "ERR_AGENTOS_VM_FORCED_PROCESS_FINALIZE: vm_id={vm_id} process_count={}",
            remaining.len()
        );
    }
    for (process_id, mut process) in remaining {
        terminate_child_process_tree(
            &mut vm.kernel,
            &mut process,
            &kernel_readiness,
            &unix_address_registry,
        );
        process
            .kernel_handle
            .finish_signaled(nix::libc::SIGKILL, false);
        let exit_code = vm
            .kernel
            .list_processes()
            .get(&process.kernel_pid)
            .and_then(|entry| entry.exit_code)
            .unwrap_or(137);
        events.push(forced_process_exit_event::<B>(
            vm,
            vm_id,
            &process_id,
            exit_code,
        ));
        if let Err(error) = vm.kernel.wait_and_reap(process.kernel_pid) {
            record_vm_teardown_error(vm_id, "process_reap", kernel_error(error), &mut first_error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn finish_vm_teardown_owned<B>(
    bridge: &crate::state::SharedBridge<B>,
    vm_id: &str,
    vm: &mut VmState,
) -> Result<(), VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let mut first_error = None;
    let snapshot = if vm.kernel.root_filesystem_mut().is_some() {
        match vm
            .kernel
            .snapshot_root_filesystem()
            .map_err(kernel_error)
            .and_then(|snapshot| encode_root_snapshot(&snapshot).map_err(root_filesystem_error))
        {
            Ok(bytes) => Some(FilesystemSnapshot {
                format: String::from(ROOT_FILESYSTEM_SNAPSHOT_FORMAT),
                bytes,
            }),
            Err(error) => {
                record_vm_teardown_error(vm_id, "snapshot", error, &mut first_error);
                None
            }
        }
    } else {
        None
    };
    if let Err(error) = bridge.emit_lifecycle(vm_id, LifecycleState::Terminated) {
        record_vm_teardown_error(vm_id, "lifecycle", error, &mut first_error);
    }
    if let Err(error) = vm.kernel.dispose().map_err(kernel_error) {
        record_vm_teardown_error(vm_id, "kernel", error, &mut first_error);
    }
    if let Some(snapshot) = snapshot {
        if let Err(error) = bridge.with_mut(|bridge| {
            bridge.flush_filesystem_state(FlushFilesystemStateRequest {
                vm_id: vm_id.to_owned(),
                snapshot,
            })
        }) {
            record_vm_teardown_error(vm_id, "filesystem_flush", error, &mut first_error);
        }
    }
    if let Err(error) = bridge.clear_vm_permissions(vm_id) {
        record_vm_teardown_error(vm_id, "permission_reset", error, &mut first_error);
    }
    first_error.map_or(Ok(()), Err)
}

fn quarantine_diagnostic(
    vm_id: &str,
    vm: &VmState,
    reconciliation: VmReconciliationSnapshot,
    reason: VmQuarantineReason,
    capability_admission_error: Option<&str>,
    fairness_retirement_error: Option<&VmError>,
) -> String {
    match reason {
        VmQuarantineReason::TeardownDeadline => format!(
            "ERR_AGENTOS_VM_TEARDOWN_DEADLINE: vm_id={vm_id} generation={} active_tasks={} outstanding_capabilities={} ledger_zero={} deadline_ms={}; raise limits.reactor.shutdownDeadlineMs",
            vm.generation,
            reconciliation.active_tasks,
            reconciliation.outstanding_capabilities,
            reconciliation.ledger_zero,
            vm.limits.reactor.shutdown_deadline_ms
        ),
        VmQuarantineReason::ResourceIntegrity => format!(
            "ERR_AGENTOS_VM_RESOURCE_INTEGRITY: vm_id={vm_id} generation={} accounting integrity failed; generation cannot be reaped",
            vm.generation
        ),
        VmQuarantineReason::CapabilityRegistryIntegrity => format!(
            "ERR_AGENTOS_VM_CAPABILITY_INTEGRITY: vm_id={vm_id} generation={} capability admission could not be closed; generation cannot be reaped; error={}",
            vm.generation,
            capability_admission_error.unwrap_or("unknown")
        ),
        VmQuarantineReason::FairnessIntegrity => format!(
            "ERR_AGENTOS_VM_FAIRNESS_INTEGRITY: vm_id={vm_id} generation={} fairness membership could not be retired; generation cannot be reaped; error={}",
            vm.generation,
            fairness_retirement_error
                .map(ToString::to_string)
                .unwrap_or_else(|| String::from("unknown"))
        ),
    }
}

fn dispose_cancellation_reason(reason: &DisposeReason) -> OperationCancellationReason {
    match reason {
        DisposeReason::Requested => OperationCancellationReason::Explicit,
        DisposeReason::ConnectionClosed => OperationCancellationReason::ConnectionClosed,
        DisposeReason::HostShutdown => OperationCancellationReason::Shutdown,
    }
}

fn cleanup_unpublished_vm<B>(bridge: &crate::state::SharedBridge<B>, vm_id: &str, vm: &VmState)
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    cleanup_path(&vm.runtime_scratch_root, "unpublished VM shadow root");
    cleanup_path(
        &vm.unix_socket_host_dir,
        "unpublished VM Unix socket namespace",
    );
    if let Err(error) = bridge.clear_vm_permissions(vm_id) {
        eprintln!(
            "ERR_AGENTOS_VM_CREATE_CLEANUP: vm_id={vm_id} phase=permission_reset error={error}"
        );
    }
}

fn cleanup_path(path: &Path, label: &str) {
    if let Err(error) = fs::remove_dir_all(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!(
                "ERR_AGENTOS_VM_PATH_CLEANUP: label={label:?} path={} error={error}",
                path.display()
            );
        }
    }
}

pub(crate) async fn bootstrap_root_filesystem_owned(
    input: OwnedVmLifecycleRequest,
    entries: Vec<RootFilesystemEntry>,
) -> Result<DispatchResult, VmError> {
    input.vm.try_command("bootstrap root filesystem", |vm| {
        let root = vm.kernel.root_filesystem_mut().ok_or_else(|| {
            VmError::InvalidState(String::from("VM root filesystem is unavailable"))
        })?;
        for entry in &entries {
            apply_root_filesystem_entry(root, entry)?;
        }
        Ok(())
    })?;

    Ok(DispatchResult {
        response: root_filesystem_bootstrapped_response(&input.request, entries.len() as u32),
        events: Vec::new(),
    })
}

pub(crate) fn configure_vm_owned<B>(
    input: ConfigureVmOwnedInput<B>,
    payload: ConfigureVmRequest,
) -> Result<DispatchResult, VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let __t = Instant::now();
    let ConfigureVmOwnedInput {
        lifecycle,
        bridge,
        sidecar_requests,
    } = input;
    let OwnedVmLifecycleRequest {
        request,
        connection_id,
        session_id,
        vm_id,
        vm,
    } = lifecycle;

    let original_permissions = vm.try_read("read configure VM permissions", |vm| {
        vm.configuration.permissions.clone()
    })?;
    // A new policy replaces the VM's policy and is resolved over the sidecar
    // defaults. Omitting it keeps the VM's current policy.
    let configured_permissions = match payload.permissions.clone() {
        Some(policy) => resolve_permissions_policy(Some(
            &crate::wire::permissions_policy_config_from_wire(policy),
        )),
        None => original_permissions.clone(),
    };
    validate_permissions_policy(&configured_permissions)?;

    let boot_package_descriptors = package_descriptors_from_wire(&payload.packages)?;
    let mount_plugins = build_mount_plugin_registry::<B>()?;

    let reconfigure_result = vm.try_command("configure VM", |vm| {
        let mut effective_mounts = payload.mounts.clone();
        append_module_access_mount(&mut effective_mounts, payload.module_access_cwd.as_ref())?;
        let mut package_descriptors = payload
            .packages
            .iter()
            .zip(boot_package_descriptors.iter().cloned())
            .map(|(package, descriptor)| (format!("path:{}", package.path), descriptor))
            .collect::<Vec<_>>();
        let boot_package_ids = package_descriptors
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<BTreeSet<_>>();
        let boot_mount_root = normalized_package_mount_root(&payload.packages_mount_at);
        let mut package_mount_roots = BTreeMap::new();
        for (id, descriptor) in &package_descriptors {
            if vm.runtime_linked_package_ids.contains(id) {
                let existing = vm.package_descriptors.iter().find(|(existing, _)| existing == id);
                if existing.map(|(_, descriptor)| descriptor) != Some(descriptor)
                    || vm.package_mount_roots.get(id) != Some(&boot_mount_root)
                {
                    return Err(VmError::InvalidState(format!(
                        "package id {id:?} is already linked with a different descriptor or projection root; unlink it before replacing it"
                    )));
                }
            }
            package_mount_roots.insert(id.clone(), boot_mount_root.clone());
        }
        // LinkPackage is an explicit live mutation. ConfigureVm replaces the
        // boot package set, but must not discard packages linked afterwards by
        // another client simply because this caller only knows its boot list.
        for (id, descriptor) in &vm.package_descriptors {
            if !vm.runtime_linked_package_ids.contains(id) || boot_package_ids.contains(id) {
                continue;
            }
            let mount_root = package_mount_root(vm, id)?;
            package_descriptors.push((id.clone(), descriptor.clone()));
            package_mount_roots.insert(id.clone(), mount_root.to_owned());
        }
        let mut package_mountpoint_paths = BTreeMap::new();
        let mut owned_mount_count = 0;
        for (id, descriptor) in &package_descriptors {
            let mounts = build_complete_package_projection(
                &vm_id,
                descriptor,
                &package_mount_roots[id],
                owned_mount_count,
                vm.limits.agentos_packages.max_mounts,
            )?;
            // The builder has checked this sum before appending each leaf.
            owned_mount_count += mounts.len();
            package_mountpoint_paths.insert(
                id.clone(),
                mounts.iter().map(|mount| mount.guest_path.clone()).collect::<BTreeSet<_>>(),
            );
            effective_mounts.extend(mounts);
        }
        check_package_mount_limit(
            &vm_id,
            0,
            owned_mount_count,
            vm.limits.agentos_packages.max_mounts,
        )?;
        let mut mount_paths = BTreeSet::new();
        for mount in &effective_mounts {
            if !mount_paths.insert(normalize_path(&mount.guest_path)) {
                return Err(VmError::InvalidState(format!(
                    "duplicate VM mount path: {}",
                    mount.guest_path
                )));
            }
        }
        validate_package_names_and_commands(package_descriptors.iter().map(|(_, descriptor)| descriptor))?;
        let mut provided_commands: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (_, descriptor) in &package_descriptors {
            provided_commands.insert(
                descriptor.name.clone(),
                descriptor
                    .commands
                    .iter()
                    .map(|target| target.command.clone())
                    .collect(),
            );
        }
        let existing_created_mountpoints = vm.package_created_mountpoints.clone();
        let mut created_mountpoints = BTreeMap::new();
        for (package_id, paths) in &package_mountpoint_paths {
            let existing = existing_created_mountpoints.get(package_id);
            let mut created = BTreeSet::new();
            for path in paths {
                if existing.is_some_and(|paths| paths.contains(path))
                    || !vm.kernel.exists_for_operator(path).map_err(kernel_error)?
                {
                    created.insert(path.clone());
                }
            }
            created_mountpoints.insert(package_id.clone(), created);
        }
        let mut next_guest_env = vm.base_guest_env.clone();
        apply_package_provides_env(
            &mut next_guest_env,
            &package_descriptors
                .iter()
                .map(|(_, descriptor)| descriptor.clone())
                .collect::<Vec<_>>(),
        );
        let mount_context = MountPluginContext {
            bridge: bridge.clone(),
            runtime_context: vm.runtime_context.clone(),
            connection_id: connection_id.clone(),
            session_id: session_id.clone(),
            vm_id: vm_id.clone(),
            sidecar_requests,
            database: vm.database.clone(),
            max_pread_bytes: vm.kernel.resource_limits().max_pread_bytes,
        };
        reconcile_mounts(&mount_plugins, vm, &effective_mounts, mount_context)?;
        vm.guest_env = next_guest_env;

        vm.command_guest_paths = discover_command_guest_paths(&mut vm.kernel)?;
        // Package command leaves live under their projection root (normally
        // `/opt/agentos/bin`), outside the legacy discovery tree. Register them
        // explicitly for both absolute and PATH-based execution.
        for (id, descriptor) in &package_descriptors {
            let mount_root = &package_mount_roots[id];
            for target in &descriptor.commands {
                vm.command_guest_paths
                    .entry(target.command.clone())
                    .or_insert_with(|| package_command_guest_path(mount_root, &target.command));
            }
        }
        let command_guest_paths = vm.command_guest_paths.clone();
        refresh_guest_command_path_env_from_map(&mut vm.guest_env, &command_guest_paths);
        let mut execution_commands = default_execution_commands(vm.configuration.defaults_profile);
        execution_commands.extend(payload.bootstrap_commands.iter().cloned());
        execution_commands.extend(payload.host_function_shim_commands.iter().cloned());
        execution_commands.extend(vm.command_guest_paths.keys().cloned());
        vm.kernel
            .register_driver_for_operator(CommandDriver::new(
                EXECUTION_DRIVER_NAME,
                execution_commands,
            ))
            .map_err(kernel_error)?;
        vm.command_permissions = payload.command_permissions.clone().into_iter().collect();
        let mut loopback_exempt_ports = vm.create_loopback_exempt_ports.clone();
        loopback_exempt_ports.extend(payload.loopback_exempt_ports.iter().copied());
        vm.kernel.set_loopback_exempt_ports(loopback_exempt_ports);
        vm.configuration = VmConfiguration {
            defaults_profile: vm.configuration.defaults_profile,
            mounts: effective_mounts.clone(),
            software: payload.software.clone(),
            permissions: configured_permissions.clone(),
            module_access_cwd: payload.module_access_cwd.clone(),
            instructions: payload.instructions.clone(),
            projected_modules: payload.projected_modules.clone(),
            command_permissions: payload.command_permissions.clone().into_iter().collect(),
            provided_commands: provided_commands.clone(),
            // jsRuntime is create-time only; preserve what create_vm stored.
            js_runtime: vm.configuration.js_runtime.clone(),
            loopback_exempt_ports: payload.loopback_exempt_ports.clone(),
        };
        vm.provided_commands = provided_commands.clone();
        vm.package_descriptors = package_descriptors;
        vm.package_mount_roots = package_mount_roots;
        vm.package_mount_paths = package_mountpoint_paths;
        vm.package_created_mountpoints = created_mountpoints;
        let projected_commands = projected_commands_from_guest_paths(
            &vm.command_guest_paths,
            &vm.provided_commands,
        );
        Ok((projected_commands, effective_mounts.len() as u32))
    });

    // Publishing the final policy is part of configuration. If it fails,
    // restore the prior policy just as for a mount/driver failure.
    let reconfigure_result = reconfigure_result.and_then(|configured| {
        bridge.set_vm_permissions(&vm_id, &configured_permissions)?;
        Ok(configured)
    });
    let (projected_commands, applied_mounts) = match reconfigure_result {
        Ok(configured) => configured,
        Err(error) => {
            let (restored_permissions, error) = match bridge.restore_vm_permissions_fail_closed(
                &vm_id,
                &original_permissions,
                "configure_vm rollback",
                &error,
            ) {
                Ok(()) => (original_permissions, error),
                Err(rollback_error) => (deny_all_policy(), rollback_error),
            };
            // The mount/driver phase may already have stored its desired
            // configuration. Keep reported policy equal to bridge enforcement.
            vm.try_command("configure VM permission rollback", |vm| {
                vm.configuration.permissions = restored_permissions;
                Ok(())
            })
            .map_err(|state_error| {
                VmError::InvalidState(format!(
                    "{error}; recording restored VM permissions failed: {state_error}"
                ))
            })?;
            return Err(error);
        }
    };

    let configured_software = payload.software.len() as u32;

    tracing::info!(target: "agentos_vm::perf", phase = "configure_vm", elapsed_ms = __t.elapsed().as_millis() as u64, applied_mounts = applied_mounts as u64, "vm phase");
    Ok(DispatchResult {
        response: vm_configured_response(
            &request,
            applied_mounts,
            configured_software,
            projected_commands,
        ),
        events: Vec::new(),
    })
}

pub(crate) async fn link_package_owned<B>(
    input: LinkPackageOwnedInput<B>,
    payload: LinkPackageRequest,
) -> Result<DispatchResult, VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    link_package_owned_with_pin(input, payload, None).await
}

pub(crate) async fn link_verified_package_owned<B>(
    input: LinkPackageOwnedInput<B>,
    payload: LinkPackageRequest,
    package: agentos_client::VerifiedPackage,
) -> Result<DispatchResult, VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    link_package_owned_with_pin(input, payload, Some(package)).await
}

async fn link_package_owned_with_pin<B>(
    input: LinkPackageOwnedInput<B>,
    payload: LinkPackageRequest,
    mut package_pin: Option<agentos_client::VerifiedPackage>,
) -> Result<DispatchResult, VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let LinkPackageOwnedInput {
        lifecycle,
        bridge,
        sidecar_requests,
    } = input;
    let OwnedVmLifecycleRequest {
        request,
        connection_id,
        session_id,
        vm_id,
        vm,
    } = lifecycle;
    let descriptor =
        crate::package_projection::read_package_manifest_from_path(&payload.package.path)?;
    if payload.package_id.is_empty() || payload.package_id.len() > 128 {
        return Err(VmError::InvalidState(String::from(
            "package id must contain 1..=128 bytes",
        )));
    }
    let commands = descriptor
        .commands
        .iter()
        .map(|target| target.command.clone())
        .collect::<Vec<_>>();
    let mount_plugins = build_mount_plugin_registry::<B>()?;

    let mount_root = vm.try_command("link VM package", |vm| {
        if let Some((_, existing)) = vm
            .package_descriptors
            .iter()
            .find(|(package_id, _)| package_id == &payload.package_id)
        {
            if existing != &descriptor {
                return Err(VmError::InvalidState(format!(
                    "package id {:?} is already linked with a different descriptor; unlink it before replacing it",
                    payload.package_id
                )));
            }
            if let Some(pin) = package_pin.as_ref() {
                match vm.installed_package_pins.get(&payload.package_id) {
                    Some(installed) if installed.digest == pin.digest => {}
                    _ => {
                        return Err(VmError::InvalidState(format!(
                            "package id {:?} is already linked outside verified installation; unlink it before installing",
                            payload.package_id
                        )))
                    }
                }
            }
            let mount_root = package_mount_root(vm, &payload.package_id)?.to_owned();
            // An explicit link also pins a boot-projected package across
            // later ConfigureVm replacements of the boot package list.
            vm.runtime_linked_package_ids
                .insert(payload.package_id.clone());
            return Ok(mount_root);
        }
        validate_package_names_and_commands(
            vm.package_descriptors.iter().map(|(_, descriptor)| descriptor)
                .chain(std::iter::once(&descriptor)),
        )?;
        let existing_mounts = vm.package_mount_paths.values().try_fold(0usize, |total, paths| {
            total.checked_add(paths.len()).ok_or(VmError::PackageMountLimit {
                used: total,
                requested: paths.len(),
                limit: vm.limits.agentos_packages.max_mounts,
            })
        })?;
        let new_mounts = build_complete_package_projection(
            &vm_id,
            &descriptor,
            crate::package_projection::OPT_AGENTOS_ROOT,
            existing_mounts,
            vm.limits.agentos_packages.max_mounts,
        )?;
        check_package_mount_limit(
            &vm_id,
            existing_mounts,
            new_mounts.len(),
            vm.limits.agentos_packages.max_mounts,
        )?;
        let mut new_mount_paths = BTreeSet::new();
        for mount in &new_mounts {
            if !new_mount_paths.insert(normalize_path(&mount.guest_path)) {
                return Err(VmError::InvalidState(format!(
                    "duplicate package mount path: {}", mount.guest_path
                )));
            }
            if vm
                .configuration
                .mounts
                .iter()
                .any(|existing| normalize_path(&existing.guest_path) == normalize_path(&mount.guest_path))
            {
                if let Some(command) = mount
                    .guest_path
                    .strip_prefix(crate::package_projection::OPT_AGENTOS_BIN)
                    .and_then(|path| path.strip_prefix('/'))
                    .filter(|path| !path.is_empty())
                {
                    return Err(VmError::InvalidState(format!(
                        "command {command:?} is already provided by another package"
                    )));
                }
                return Err(VmError::InvalidState(format!(
                    "agentos package mount already exists at {}",
                    mount.guest_path
                )));
            }
        }
        let created_mountpoints = new_mounts
            .iter()
            .map(|mount| {
                vm.kernel
                    .exists_for_operator(&mount.guest_path)
                    .map(|exists| (!exists).then(|| mount.guest_path.clone()))
                    .map_err(kernel_error)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        let mount_context = MountPluginContext {
            bridge,
            runtime_context: vm.runtime_context.clone(),
            connection_id,
            session_id,
            vm_id: vm_id.clone(),
            sidecar_requests,
            database: vm.database.clone(),
            max_pread_bytes: vm.kernel.resource_limits().max_pread_bytes,
        };
        mount_leaf_descriptors(&mount_plugins, vm, &new_mounts, mount_context)?;
        vm.configuration.mounts.extend(new_mounts);
        vm.package_descriptors
            .push((payload.package_id.clone(), descriptor.clone()));
        vm.runtime_linked_package_ids
            .insert(payload.package_id.clone());
        vm.package_mount_roots.insert(
            payload.package_id.clone(),
            crate::package_projection::OPT_AGENTOS_ROOT.to_owned(),
        );
        vm.package_mount_paths.insert(payload.package_id.clone(), new_mount_paths);
        vm.package_created_mountpoints
            .insert(payload.package_id.clone(), created_mountpoints);
        if let Some(pin) = package_pin.take() {
            vm.installed_package_pins
                .insert(payload.package_id.clone(), pin);
        }
        refresh_package_runtime_state(vm)?;
        Ok(crate::package_projection::OPT_AGENTOS_ROOT.to_owned())
    })?;

    let projected_commands = commands
        .iter()
        .map(|command| ProjectedCommand {
            name: command.clone(),
            guest_path: package_command_guest_path(&mount_root, command),
        })
        .collect();
    Ok(DispatchResult {
        response: package_linked_response(&request, projected_commands),
        events: Vec::new(),
    })
}

pub(crate) async fn unlink_package_owned<B>(
    input: LinkPackageOwnedInput<B>,
    payload: UnlinkPackageRequest,
) -> Result<DispatchResult, VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    if payload.package_id.is_empty() || payload.package_id.len() > 128 {
        return Err(VmError::InvalidState(String::from(
            "package id must contain 1..=128 bytes",
        )));
    }
    let LinkPackageOwnedInput {
        lifecycle,
        bridge,
        sidecar_requests: _,
    } = input;
    let OwnedVmLifecycleRequest {
        request,
        connection_id: _,
        session_id: _,
        vm_id,
        vm,
    } = lifecycle;

    let removed_commands = vm.try_command("unlink VM package", |vm| {
        let descriptor = vm
            .package_descriptors
            .iter()
            .find(|(package_id, _)| package_id == &payload.package_id)
            .map(|(_, descriptor)| descriptor.clone())
            .ok_or_else(|| {
                VmError::InvalidState(format!(
                    "software package not found: {}",
                    payload.package_id
                ))
            })?;
        let target_paths = vm
            .package_mount_paths
            .get(&payload.package_id)
            .cloned()
            .ok_or_else(|| {
                VmError::InvalidState(format!(
                    "software package {:?} is missing its installed mount paths",
                    payload.package_id
                ))
            })?;
        let created_mountpoints = vm
            .package_created_mountpoints
            .get(&payload.package_id)
            .cloned()
            .unwrap_or_default();
        if let Some(missing) = target_paths.iter().find(|path| {
            !vm.configuration
                .mounts
                .iter()
                .any(|mount| &mount.guest_path == *path)
        }) {
            return Err(VmError::InvalidState(format!(
                "software package {} is missing its live mount at {missing}",
                payload.package_id
            )));
        }

        let mut ordered_paths = target_paths.iter().collect::<Vec<_>>();
        ordered_paths.sort_by_key(|path| std::cmp::Reverse(mount_path_depth(path)));
        for path in ordered_paths {
            vm.kernel
                .unmount_filesystem_for_operator(path)
                .map_err(kernel_error)?;
            emit_security_audit_event(
                &bridge,
                &vm_id,
                "security.mount.unmounted",
                audit_fields([
                    (String::from("guest_path"), path.clone()),
                    (String::from("plugin_id"), String::from("agentos_packages")),
                    (String::from("read_only"), String::from("true")),
                ]),
            );
        }
        let mut created_paths = created_mountpoints.iter().collect::<Vec<_>>();
        created_paths.sort_by_key(|path| std::cmp::Reverse(mount_path_depth(path)));
        for path in created_paths {
            // Some read-only parents cannot materialize cosmetic mountpoints.
            // Remove only empty directories we created, never guest contents.
            if vm.kernel.exists_for_operator(path).map_err(kernel_error)? {
                vm.kernel
                    .remove_dir_for_operator(path)
                    .map_err(kernel_error)?;
            }
        }
        vm.configuration
            .mounts
            .retain(|mount| !target_paths.contains(&mount.guest_path));
        vm.package_descriptors
            .retain(|(package_id, _)| package_id != &payload.package_id);
        vm.runtime_linked_package_ids.remove(&payload.package_id);
        vm.package_mount_roots.remove(&payload.package_id);
        vm.package_mount_paths.remove(&payload.package_id);
        vm.package_created_mountpoints.remove(&payload.package_id);
        refresh_package_runtime_state(vm)?;
        vm.installed_package_pins.remove(&payload.package_id);
        Ok(descriptor
            .commands
            .into_iter()
            .map(|target| target.command)
            .collect::<Vec<_>>())
    })?;

    Ok(DispatchResult {
        response: package_unlinked_response(&request, removed_commands),
        events: Vec::new(),
    })
}

pub(crate) async fn create_layer_owned(
    input: OwnedVmLifecycleRequest,
    _payload: CreateLayerRequest,
) -> Result<DispatchResult, VmError> {
    let layer_id = input.vm.try_command("create VM layer", |vm| {
        vm.layers
            .create_writable_layer()
            .map_err(sidecar_core_error)
    })?;
    Ok(DispatchResult {
        response: layer_created_response(&input.request, layer_id),
        events: Vec::new(),
    })
}

pub(crate) async fn seal_layer_owned(
    input: OwnedVmLifecycleRequest,
    payload: SealLayerRequest,
) -> Result<DispatchResult, VmError> {
    let layer_id = input.vm.try_command("seal VM layer", |vm| {
        vm.layers
            .seal_layer(&payload.layer_id)
            .map_err(sidecar_core_error)
    })?;
    Ok(DispatchResult {
        response: layer_sealed_response(&input.request, layer_id),
        events: Vec::new(),
    })
}

pub(crate) async fn import_snapshot_owned(
    input: OwnedVmLifecycleRequest,
    payload: ImportSnapshotRequest,
) -> Result<DispatchResult, VmError> {
    let snapshot = root_snapshot_from_entries(&payload.entries)?;
    let layer_id = input.vm.try_command("import VM snapshot", |vm| {
        vm.layers
            .import_snapshot(snapshot)
            .map_err(sidecar_core_error)
    })?;
    Ok(DispatchResult {
        response: snapshot_imported_response(&input.request, layer_id),
        events: Vec::new(),
    })
}

pub(crate) async fn export_snapshot_owned(
    input: OwnedVmLifecycleRequest,
    payload: ExportSnapshotRequest,
) -> Result<DispatchResult, VmError> {
    let snapshot = input.vm.try_command("export VM snapshot", |vm| {
        vm.layers
            .export_snapshot(&payload.layer_id)
            .map_err(sidecar_core_error)
    })?;
    Ok(DispatchResult {
        response: snapshot_exported_response(
            &input.request,
            payload.layer_id,
            root_snapshot_entries(&snapshot),
        ),
        events: Vec::new(),
    })
}

pub(crate) async fn create_overlay_owned(
    input: OwnedVmLifecycleRequest,
    payload: CreateOverlayRequest,
) -> Result<DispatchResult, VmError> {
    let layer_id = input.vm.try_command("create VM overlay", |vm| {
        vm.layers
            .create_overlay_layer(
                protocol_root_filesystem_mode(payload.mode),
                payload.upper_layer_id,
                payload.lower_layer_ids,
            )
            .map_err(sidecar_core_error)
    })?;
    Ok(DispatchResult {
        response: overlay_created_response(&input.request, layer_id),
        events: Vec::new(),
    })
}

pub(crate) async fn snapshot_root_filesystem_owned(
    input: OwnedVmLifecycleRequest,
    payload: SnapshotRootFilesystemRequest,
) -> Result<DispatchResult, VmError> {
    let snapshot = input.vm.try_command("snapshot VM root filesystem", |vm| {
        vm.kernel
            .snapshot_root_filesystem_bounded(payload.max_bytes)
            .map_err(kernel_error)
    })?;
    Ok(DispatchResult {
        response: root_filesystem_snapshot_response(
            &input.request,
            snapshot.entries.iter().map(root_snapshot_entry).collect(),
        ),
        events: Vec::new(),
    })
}

fn record_vm_teardown_error(
    vm_id: &str,
    phase: &str,
    error: VmError,
    first_error: &mut Option<VmError>,
) {
    eprintln!("ERR_AGENTOS_VM_TEARDOWN_CLEANUP: vm_id={vm_id} phase={phase} error={error}");
    if first_error.is_none() {
        *first_error = Some(error);
    }
}

fn vm_reconciliation_snapshot(
    resources: &ResourceLedger,
    runtime_context: &agentos_driver_tokio::DriverHandle,
    capabilities: &CapabilityRegistry,
) -> VmReconciliationSnapshot {
    VmReconciliationSnapshot {
        active_tasks: runtime_context.tasks().active_scoped(),
        outstanding_capabilities: capabilities.outstanding_len(),
        ledger_zero: resources.is_zero(),
        integrity_ok: resources.integrity_ok(),
    }
}

fn close_vm_capability_admission(capabilities: &CapabilityRegistry) -> Result<(), String> {
    capabilities
        .close_admission()
        .map_err(|error| error.to_string())
}

fn retire_vm_fairness(
    runtime_context: &agentos_driver_tokio::DriverHandle,
    vm_generation: u64,
) -> Result<(), VmError> {
    runtime_context
        .fairness()
        .retire_vm(vm_generation)
        .map(|_| ())
        .map_err(|error| {
            VmError::host(
                "ERR_AGENTOS_FAIRNESS_RETIRE_VM",
                format!("generation={vm_generation}: {error}"),
            )
        })
}

fn vm_quarantine_reason(
    capability_registry_integrity_failed: bool,
    fairness_integrity_failed: bool,
    reconciliation: VmReconciliationSnapshot,
    deadline_expired: bool,
) -> Option<VmQuarantineReason> {
    if capability_registry_integrity_failed {
        Some(VmQuarantineReason::CapabilityRegistryIntegrity)
    } else if fairness_integrity_failed {
        Some(VmQuarantineReason::FairnessIntegrity)
    } else if !reconciliation.integrity_ok {
        Some(VmQuarantineReason::ResourceIntegrity)
    } else if deadline_expired
        || reconciliation.active_tasks != 0
        || reconciliation.outstanding_capabilities != 0
        || !reconciliation.ledger_zero
    {
        Some(VmQuarantineReason::TeardownDeadline)
    } else {
        None
    }
}

/// Submit the close before observing the timer so a timed-out local close still
/// owns its already-admitted blocking job. Reconciliation then observes that
/// job. Canceling a host callback can lose completion observation, though, so
/// every timed-out close conservatively prevents automatic quarantine reaping.
async fn close_vm_database_before_deadline(
    vm_id: &str,
    database: &dyn crate::vm_sqlite::VmSqliteDatabase,
    deadline: tokio::time::Instant,
    shutdown_deadline_ms: u64,
) -> (Result<(), VmError>, bool) {
    tokio::select! {
        biased;
        result = database.close() => (
            result.map_err(|error| {
                VmError::InvalidState(format!("close VM SQLite database: {error}"))
            }),
            false,
        ),
        _ = tokio::time::sleep_until(deadline) => (
            Err(VmError::VmTeardownDeadline {
                message: format!(
                    "ERR_AGENTOS_VM_TEARDOWN_DEADLINE: vm_id={vm_id} phase=sqlite_close deadline_ms={shutdown_deadline_ms}; raise limits.reactor.shutdownDeadlineMs"
                ),
                vm_id: vm_id.to_owned(),
                deadline_ms: shutdown_deadline_ms,
            }),
            true,
        ),
    }
}

async fn wait_for_vm_reconciliation(
    resources: &ResourceLedger,
    runtime_context: &agentos_driver_tokio::DriverHandle,
    capabilities: &CapabilityRegistry,
    deadline: Duration,
) -> (VmReconciliationSnapshot, bool) {
    let initial = vm_reconciliation_snapshot(resources, runtime_context, capabilities);
    if initial.active_tasks == 0
        && initial.outstanding_capabilities == 0
        && initial.ledger_zero
        && initial.integrity_ok
    {
        return (initial, false);
    }

    let wait_for_ledger = async {
        loop {
            if resources.is_zero() || !resources.integrity_ok() {
                return;
            }
            resources.capacity_changed().await;
        }
    };
    let barrier = async {
        tokio::join!(
            runtime_context.tasks().wait_empty(),
            capabilities.wait_empty(),
            wait_for_ledger
        );
    };
    let deadline_expired = tokio::time::timeout(deadline, barrier).await.is_err();
    (
        vm_reconciliation_snapshot(resources, runtime_context, capabilities),
        deadline_expired,
    )
}

fn vm_resource_ledger(
    vm_id: &str,
    generation: u64,
    limits: &crate::limits::VmLimits,
    process: Arc<ResourceLedger>,
) -> Result<ResourceLedger, VmError> {
    let socket_limit = limits.resources.max_sockets.ok_or_else(|| {
        VmError::InvalidState(String::from(
            "limits.resources.maxSockets must be bounded for sidecar VMs",
        ))
    })?;
    let connection_limit = limits.resources.max_connections.ok_or_else(|| {
        VmError::InvalidState(String::from(
            "limits.resources.maxConnections must be bounded for sidecar VMs",
        ))
    })?;
    let buffered_byte_limit = limits.resources.max_socket_buffered_bytes.ok_or_else(|| {
        VmError::InvalidState(String::from(
            "limits.resources.maxSocketBufferedBytes must be bounded for sidecar VMs",
        ))
    })?;
    let datagram_limit = limits
        .resources
        .max_socket_datagram_queue_len
        .ok_or_else(|| {
            VmError::InvalidState(String::from(
                "limits.resources.maxSocketDatagramQueueLen must be bounded for sidecar VMs",
            ))
        })?;
    let wasm_linear_memory_limit = limits.resources.max_wasm_memory_bytes.ok_or_else(|| {
        VmError::InvalidState(String::from(
            "limits.resources.maxWasmMemoryBytes must be bounded for sidecar VMs",
        ))
    })?;
    let _wasm_linear_memory_limit = usize::try_from(wasm_linear_memory_limit).map_err(|_| {
        VmError::InvalidState(String::from(
            "limits.resources.maxWasmMemoryBytes exceeds the host address space",
        ))
    })?;
    // `limits.resources.maxWasmMemoryBytes` is the accessible linear-memory
    // cap for each guest memory. Wasmtime's admission reservation also includes
    // bounded table and async-stack envelopes, so reusing the linear cap as the
    // child ledger's aggregate maximum makes one otherwise-valid Store
    // impossible to admit. Keep aggregate Store admission under the distinct,
    // process-wide runtime envelope; the child ledger still gives each VM a
    // bounded scope while the Store limiter independently enforces the exact
    // per-memory linear cap.
    let wasm_aggregate_memory_limit = process
        .usage(ResourceClass::WasmMemoryBytes)
        .limit
        .ok_or_else(|| {
            VmError::InvalidState(String::from(
                "runtime.resources.maxWasmMemoryBytes must be bounded for sidecar VMs",
            ))
        })?;
    let child_limits = [
        (
            ResourceClass::Capabilities,
            ResourceLimit::new(
                limits.reactor.max_capabilities,
                "limits.reactor.maxCapabilities",
            ),
        ),
        (
            ResourceClass::ReadyHandles,
            ResourceLimit::new(
                limits.reactor.max_ready_handles,
                "limits.reactor.maxReadyHandles",
            ),
        ),
        (
            ResourceClass::Sockets,
            ResourceLimit::new(socket_limit, "limits.resources.maxSockets"),
        ),
        (
            ResourceClass::Connections,
            ResourceLimit::new(connection_limit, "limits.resources.maxConnections"),
        ),
        (
            ResourceClass::BufferedBytes,
            ResourceLimit::new(
                buffered_byte_limit,
                "limits.resources.maxSocketBufferedBytes",
            ),
        ),
        (
            ResourceClass::Datagrams,
            ResourceLimit::new(datagram_limit, "limits.resources.maxSocketDatagramQueueLen"),
        ),
        (
            ResourceClass::Timers,
            ResourceLimit::new(limits.js_runtime.max_timers, "limits.jsRuntime.maxTimers"),
        ),
        (
            ResourceClass::HandleCommands,
            ResourceLimit::new(
                limits.reactor.max_handle_commands,
                "limits.reactor.maxHandleCommands",
            ),
        ),
        (
            ResourceClass::HandleCommandBytes,
            ResourceLimit::new(
                limits.reactor.max_handle_command_bytes,
                "limits.reactor.maxHandleCommandBytes",
            ),
        ),
        (
            ResourceClass::BridgeCalls,
            ResourceLimit::new(
                limits.reactor.max_bridge_calls,
                "limits.reactor.maxBridgeCalls",
            ),
        ),
        (
            ResourceClass::BridgeRequestBytes,
            ResourceLimit::new(
                limits.reactor.max_bridge_request_bytes,
                "limits.reactor.maxBridgeRequestBytes",
            ),
        ),
        (
            ResourceClass::BridgeResponseBytes,
            ResourceLimit::new(
                limits.reactor.max_bridge_response_bytes,
                "limits.reactor.maxBridgeResponseBytes",
            ),
        ),
        (
            ResourceClass::AsyncCompletions,
            ResourceLimit::new(
                limits.reactor.max_async_completions,
                "limits.reactor.maxAsyncCompletions",
            ),
        ),
        (
            ResourceClass::AsyncCompletionBytes,
            ResourceLimit::new(
                limits.reactor.max_async_completion_bytes,
                "limits.reactor.maxAsyncCompletionBytes",
            ),
        ),
        (
            ResourceClass::UdpDatagrams,
            ResourceLimit::new(
                limits.udp.max_buffered_datagrams,
                "limits.udp.maxBufferedDatagrams",
            ),
        ),
        (
            ResourceClass::UdpBytes,
            ResourceLimit::new(limits.udp.max_buffered_bytes, "limits.udp.maxBufferedBytes"),
        ),
        (
            ResourceClass::TlsBytes,
            ResourceLimit::new(limits.tls.max_buffered_bytes, "limits.tls.maxBufferedBytes"),
        ),
        (
            ResourceClass::Tasks,
            ResourceLimit::new(limits.reactor.max_tasks, "limits.reactor.maxTasks"),
        ),
        (
            ResourceClass::ExecutorSlots,
            ResourceLimit::new(
                limits.reactor.max_blocking_jobs,
                "limits.reactor.maxBlockingJobs",
            ),
        ),
        (
            ResourceClass::ExecutorBytes,
            ResourceLimit::new(
                limits.reactor.max_blocking_bytes,
                "limits.reactor.maxBlockingBytes",
            ),
        ),
        (
            ResourceClass::WasmMemoryBytes,
            ResourceLimit::new(
                wasm_aggregate_memory_limit,
                "runtime.resources.maxWasmMemoryBytes",
            ),
        ),
        (
            ResourceClass::WasmThreads,
            ResourceLimit::new(
                limits.wasm.max_concurrent_threads,
                "limits.wasm.maxConcurrentThreads",
            ),
        ),
        (
            ResourceClass::Http2Connections,
            ResourceLimit::new(limits.http2.max_connections, "limits.http2.maxConnections"),
        ),
        (
            ResourceClass::Http2Streams,
            ResourceLimit::new(limits.http2.max_streams, "limits.http2.maxStreams"),
        ),
        (
            ResourceClass::Http2BufferedBytes,
            ResourceLimit::new(
                limits.http2.max_buffered_bytes,
                "limits.http2.maxBufferedBytes",
            ),
        ),
        (
            ResourceClass::Http2HeaderBytes,
            ResourceLimit::new(limits.http2.max_header_bytes, "limits.http2.maxHeaderBytes"),
        ),
        (
            ResourceClass::Http2DataBytes,
            ResourceLimit::new(limits.http2.max_data_bytes, "limits.http2.maxDataBytes"),
        ),
        (
            ResourceClass::Http2Commands,
            ResourceLimit::new(
                limits.http2.max_pending_commands,
                "limits.http2.maxPendingCommands",
            ),
        ),
        (
            ResourceClass::Http2CommandBytes,
            ResourceLimit::new(
                limits.http2.max_pending_command_bytes,
                "limits.http2.maxPendingCommandBytes",
            ),
        ),
        (
            ResourceClass::Http2Events,
            ResourceLimit::new(
                limits.http2.max_pending_events,
                "limits.http2.maxPendingEvents",
            ),
        ),
        (
            ResourceClass::Http2EventBytes,
            ResourceLimit::new(
                limits.http2.max_pending_event_bytes,
                "limits.http2.maxPendingEventBytes",
            ),
        ),
    ];
    for (resource, child_limit) in &child_limits {
        if let Some(parent_limit) = process.usage(*resource).limit {
            if child_limit.maximum > parent_limit {
                return Err(VmError::InvalidState(format!(
                    "{} ({}) must be <= process {} ({parent_limit})",
                    child_limit.config_path,
                    child_limit.maximum,
                    match resource {
                        ResourceClass::Capabilities => "runtime.resources.maxCapabilities",
                        ResourceClass::ReadyHandles => "runtime.resources.maxReadyHandles",
                        ResourceClass::Sockets => "runtime.resources.maxSockets",
                        ResourceClass::Connections => "runtime.resources.maxConnections",
                        ResourceClass::BufferedBytes => {
                            "runtime.resources.maxSocketBufferedBytes"
                        }
                        ResourceClass::Datagrams => "runtime.resources.maxDatagrams",
                        ResourceClass::HandleCommands => {
                            "runtime.resources.maxHandleCommands"
                        }
                        ResourceClass::HandleCommandBytes => {
                            "runtime.resources.maxHandleCommandBytes"
                        }
                        ResourceClass::BridgeCalls => "runtime.resources.maxBridgeCalls",
                        ResourceClass::BridgeRequestBytes => {
                            "runtime.resources.maxBridgeRequestBytes"
                        }
                        ResourceClass::BridgeResponseBytes => {
                            "runtime.resources.maxBridgeResponseBytes"
                        }
                        ResourceClass::AsyncCompletions => {
                            "runtime.resources.maxAsyncCompletions"
                        }
                        ResourceClass::AsyncCompletionBytes => {
                            "runtime.resources.maxAsyncCompletionBytes"
                        }
                        ResourceClass::UdpDatagrams => "runtime.resources.maxUdpDatagrams",
                        ResourceClass::UdpBytes => "runtime.resources.maxUdpBytes",
                        ResourceClass::TlsBytes => "runtime.resources.maxTlsBytes",
                        ResourceClass::Timers => "runtime.resources.maxTimers",
                        ResourceClass::Tasks => "runtime.resources.maxTasks",
                        ResourceClass::ExecutorSlots => "runtime.blocking.maxJobs",
                        ResourceClass::ExecutorBytes => "runtime.blocking.maxQueuedBytes",
                        ResourceClass::WasmMemoryBytes => {
                            "runtime.resources.maxWasmMemoryBytes"
                        }
                        ResourceClass::WasmThreads => "runtime.resources.maxWasmThreads",
                        ResourceClass::Http2Connections => "limits.http2.maxConnections",
                        ResourceClass::Http2Streams => "limits.http2.maxStreams",
                        ResourceClass::Http2BufferedBytes => "limits.http2.maxBufferedBytes",
                        ResourceClass::Http2HeaderBytes => "limits.http2.maxHeaderBytes",
                        ResourceClass::Http2DataBytes => "limits.http2.maxDataBytes",
                        ResourceClass::Http2Commands => "limits.http2.maxPendingCommands",
                        ResourceClass::Http2CommandBytes => {
                            "limits.http2.maxPendingCommandBytes"
                        }
                        ResourceClass::Http2Events => "limits.http2.maxPendingEvents",
                        ResourceClass::Http2EventBytes => "limits.http2.maxPendingEventBytes",
                    }
                )));
            }
        }
    }
    Ok(ResourceLedger::child(
        format!("vm={vm_id} generation={generation}"),
        child_limits,
        process,
    ))
}

// ---------------------------------------------------------------------------
// Free functions — VM lifecycle helpers
// ---------------------------------------------------------------------------

fn native_root_plugin_from_config(
    config: Option<&vm_config::NativeRootFilesystemConfig>,
) -> Result<Option<NativeRootPluginConfig>, VmError> {
    let Some(config) = config else {
        return Ok(None);
    };
    let plugin_config = serde_json::to_string(&config.plugin.config).map_err(|error| {
        VmError::InvalidState(format!(
            "failed to serialize nativeRoot.plugin.config: {error}"
        ))
    })?;
    Ok(Some(NativeRootPluginConfig {
        plugin: MountPluginDescriptor {
            id: config.plugin.id.clone(),
            config: plugin_config,
        },
        read_only: config.read_only,
    }))
}

fn vm_dns_config_from_config(
    config: Option<&vm_config::VmDnsConfig>,
) -> Result<VmDnsConfig, VmError> {
    let Some(config) = config else {
        return Ok(VmDnsConfig::default());
    };
    let name_servers = config
        .name_servers
        .iter()
        .map(|entry| parse_vm_dns_nameserver(entry))
        .collect::<Result<Vec<_>, _>>()?;
    let mut overrides = BTreeMap::new();
    for (hostname, addresses) in &config.overrides {
        let normalized_hostname = normalize_dns_hostname(hostname)?;
        let parsed_addresses = addresses
            .iter()
            .map(|entry| {
                entry.parse::<IpAddr>().map_err(|error| {
                    VmError::InvalidState(format!(
                        "invalid DNS override {hostname}={entry}: {error}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        overrides.insert(normalized_hostname, parsed_addresses);
    }
    Ok(VmDnsConfig {
        name_servers,
        overrides,
    })
}

fn vm_listen_policy_from_config(
    config: Option<&vm_config::VmListenPolicyConfig>,
) -> Result<VmListenPolicy, VmError> {
    let mut policy = VmListenPolicy::default();
    let Some(config) = config else {
        return Ok(policy);
    };
    if let Some(port_min) = config.port_min {
        policy.port_min = port_min;
    }
    if let Some(port_max) = config.port_max {
        policy.port_max = port_max;
    }
    if policy.port_min > policy.port_max {
        return Err(VmError::InvalidState(format!(
            "invalid listen port range {} exceeds {}",
            policy.port_min, policy.port_max
        )));
    }
    if let Some(allow_privileged) = config.allow_privileged {
        policy.allow_privileged = allow_privileged;
    }
    Ok(policy)
}

#[derive(Debug, Clone)]
struct NativeRootPluginConfig {
    plugin: MountPluginDescriptor,
    read_only: bool,
}

fn build_native_root_mount_table<B>(
    mount_plugins: &agentos_vm_kernel::mount_plugin::FileSystemPluginRegistry<
        MountPluginContext<B>,
    >,
    native_root: &NativeRootPluginConfig,
    descriptor: &RootFilesystemDescriptor,
    context: MountPluginContext<B>,
) -> Result<MountTable, VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    if !descriptor.lowers.is_empty() {
        return Err(VmError::InvalidState(String::from(
            "native root filesystems do not support rootFilesystem.lowers",
        )));
    }

    let config_value: serde_json::Value = serde_json::from_str(&native_root.plugin.config)
        .map_err(|error| {
            VmError::InvalidState(format!(
                "root native plugin config for {} is not valid JSON: {error}",
                native_root.plugin.id
            ))
        })?;
    let mut filesystem = mount_plugins
        .open(
            &native_root.plugin.id,
            OpenFileSystemPluginRequest {
                vm_id: &context.vm_id,
                guest_path: "/",
                read_only: native_root.read_only,
                config: &config_value,
                context: &context,
            },
        )
        .map_err(plugin_error)?;

    bootstrap_native_root_filesystem(filesystem.as_mut(), descriptor)?;

    Ok(MountTable::new_boxed_root(
        filesystem,
        MountOptions::new(native_root.plugin.id.clone()).read_only(native_root.read_only),
    ))
}

fn bootstrap_native_root_filesystem(
    filesystem: &mut dyn MountedFileSystem,
    descriptor: &RootFilesystemDescriptor,
) -> Result<(), VmError> {
    for (guest_path, mode, uid, gid) in ROOT_BOOTSTRAP_DIRS {
        filesystem.mkdir(guest_path, true).map_err(vfs_error)?;
        filesystem.chmod(guest_path, *mode).map_err(vfs_error)?;
        filesystem
            .chown(guest_path, *uid, *gid)
            .map_err(vfs_error)?;
    }

    seed_native_ca_certificates_bundle(filesystem)?;

    for entry in &descriptor.bootstrap_entries {
        apply_native_root_filesystem_entry(filesystem, entry)?;
    }

    Ok(())
}

fn apply_native_root_filesystem_entry(
    filesystem: &mut dyn MountedFileSystem,
    entry: &RootFilesystemEntry,
) -> Result<(), VmError> {
    let snapshot = root_snapshot_from_entries(std::slice::from_ref(entry))?;
    let kernel_entry = snapshot
        .entries
        .into_iter()
        .next()
        .expect("root snapshot from one entry should contain one entry");
    ensure_mounted_parent_directories(filesystem, &kernel_entry.path)?;
    prepare_mounted_destination(filesystem, &kernel_entry.path, &kernel_entry.kind)?;

    match kernel_entry.kind {
        KernelFilesystemEntryKind::Directory => filesystem
            .mkdir(&kernel_entry.path, true)
            .map_err(vfs_error)?,
        KernelFilesystemEntryKind::File => filesystem
            .write_file(&kernel_entry.path, kernel_entry.content.unwrap_or_default())
            .map_err(vfs_error)?,
        KernelFilesystemEntryKind::Symlink => filesystem
            .symlink(
                kernel_entry.target.as_deref().ok_or_else(|| {
                    VmError::InvalidState(format!(
                        "root filesystem bootstrap for symlink {} requires a target",
                        entry.path
                    ))
                })?,
                &kernel_entry.path,
            )
            .map_err(vfs_error)?,
    }

    if !matches!(kernel_entry.kind, KernelFilesystemEntryKind::Symlink) {
        filesystem
            .chmod(&kernel_entry.path, kernel_entry.mode)
            .map_err(vfs_error)?;
        filesystem
            .chown(&kernel_entry.path, kernel_entry.uid, kernel_entry.gid)
            .map_err(vfs_error)?;
    }

    Ok(())
}

fn seed_native_ca_certificates_bundle(
    filesystem: &mut dyn MountedFileSystem,
) -> Result<(), VmError> {
    if CA_CERTIFICATES_BUNDLE.is_empty() {
        return Err(VmError::Io(
            "embedded Mozilla CA certificate bundle is empty".to_string(),
        ));
    }

    if !mounted_entry_exists(filesystem, CA_CERTIFICATES_GUEST_PATH)? {
        ensure_mounted_parent_directories(filesystem, CA_CERTIFICATES_GUEST_PATH)?;
        filesystem
            .write_file(CA_CERTIFICATES_GUEST_PATH, CA_CERTIFICATES_BUNDLE.to_vec())
            .map_err(vfs_error)?;
        filesystem
            .chmod(CA_CERTIFICATES_GUEST_PATH, 0o644)
            .map_err(vfs_error)?;
        filesystem
            .chown(CA_CERTIFICATES_GUEST_PATH, 0, 0)
            .map_err(vfs_error)?;
    }

    if !mounted_entry_exists(filesystem, CA_CERTIFICATES_SYMLINK_PATH)? {
        ensure_mounted_parent_directories(filesystem, CA_CERTIFICATES_SYMLINK_PATH)?;
        filesystem
            .symlink(CA_CERTIFICATES_SYMLINK_TARGET, CA_CERTIFICATES_SYMLINK_PATH)
            .map_err(vfs_error)?;
    }

    Ok(())
}

fn mounted_entry_exists(filesystem: &dyn MountedFileSystem, path: &str) -> Result<bool, VmError> {
    match filesystem.lstat(path) {
        Ok(_) => Ok(true),
        Err(error) if error.code() == "ENOENT" => Ok(false),
        Err(error) => Err(vfs_error(error)),
    }
}

fn prepare_mounted_destination(
    filesystem: &mut dyn MountedFileSystem,
    path: &str,
    desired_kind: &KernelFilesystemEntryKind,
) -> Result<(), VmError> {
    let existing = match filesystem.lstat(path) {
        Ok(existing) => existing,
        Err(error) if error.code() == "ENOENT" => return Ok(()),
        Err(error) => return Err(vfs_error(error)),
    };
    let already_compatible = match desired_kind {
        KernelFilesystemEntryKind::Directory => existing.is_directory && !existing.is_symbolic_link,
        KernelFilesystemEntryKind::File => !existing.is_directory && !existing.is_symbolic_link,
        KernelFilesystemEntryKind::Symlink => false,
    };
    if already_compatible {
        return Ok(());
    }

    if existing.is_directory && !existing.is_symbolic_link {
        filesystem.remove_dir(path).map_err(vfs_error)?;
    } else {
        filesystem.remove_file(path).map_err(vfs_error)?;
    }
    Ok(())
}

fn ensure_mounted_parent_directories(
    filesystem: &mut dyn MountedFileSystem,
    path: &str,
) -> Result<(), VmError> {
    let parent = dirname(path);
    if parent != "/" && !filesystem.exists(&parent) {
        ensure_mounted_parent_directories(filesystem, &parent)?;
        filesystem.mkdir(&parent, true).map_err(vfs_error)?;
    }
    Ok(())
}

fn reconcile_mounts<B>(
    mount_plugins: &agentos_vm_kernel::mount_plugin::FileSystemPluginRegistry<
        MountPluginContext<B>,
    >,
    vm: &mut VmState,
    mounts: &[crate::protocol::MountDescriptor],
    context: MountPluginContext<B>,
) -> Result<(), VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    // Parse and validate the entire desired set before changing the live view.
    let prepared = prepare_mount_descriptors(mount_plugins, mounts)?;
    let mut existing_mounts = vm.configuration.mounts.clone();
    existing_mounts.sort_by_key(|mount| std::cmp::Reverse(mount_path_depth(&mount.guest_path)));
    let mut detached = Vec::with_capacity(existing_mounts.len());
    for mount in &existing_mounts {
        match vm.kernel.detach_filesystem_for_operator(&mount.guest_path) {
            Ok(backend) => detached.push(backend),
            Err(error) => {
                return Err(rollback_mount_reconciliation(
                    vm,
                    &context,
                    Vec::new(),
                    detached,
                    BTreeSet::new(),
                    kernel_error(error),
                ));
            }
        }
    }

    let mut mounted = Vec::with_capacity(prepared.len());
    let mut created_mountpoints = BTreeSet::new();
    if let Err(error) = mount_prepared_descriptors(
        mount_plugins,
        vm,
        prepared,
        &context,
        &mut mounted,
        &mut created_mountpoints,
    ) {
        return Err(rollback_mount_reconciliation(
            vm,
            &context,
            mounted,
            detached,
            created_mountpoints,
            error,
        ));
    }

    // The replacement view is installed. Shutdown is backend cleanup, not a
    // reason to report that configuration failed after its visible commit.
    for (mount, backend) in existing_mounts.iter().zip(detached) {
        if let Err(error) = backend.shutdown() {
            tracing::error!(vm_id = %context.vm_id, guest_path = %mount.guest_path, error = %error, "detached mount backend shutdown failed after reconfiguration");
            let _ = emit_structured_event(
                &context.bridge,
                &context.vm_id,
                "filesystem.mount.shutdown_failed",
                audit_fields([
                    (String::from("guest_path"), mount.guest_path.clone()),
                    (String::from("plugin_id"), mount.plugin.id.clone()),
                    (String::from("phase"), String::from("configure_vm")),
                    (String::from("error"), error.to_string()),
                ]),
            );
        }
        emit_mount_audit_event(&context, mount, "security.mount.unmounted");
    }
    for mount in &mounted {
        emit_mount_audit_event(&context, mount, "security.mount.mounted");
    }
    Ok(())
}

fn rollback_mount_reconciliation<B>(
    vm: &mut VmState,
    context: &MountPluginContext<B>,
    mounted: Vec<MountDescriptor>,
    detached: Vec<DetachedMount>,
    created_mountpoints: BTreeSet<String>,
    original_error: VmError,
) -> VmError
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let mut rollback_errors = Vec::new();
    let mut paths = created_mountpoints
        .iter()
        .map(|path| (path.clone(), false))
        .collect::<BTreeMap<_, _>>();
    for mount in mounted {
        paths.insert(normalize_path(&mount.guest_path), true);
    }
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort_by_key(|(path, _)| std::cmp::Reverse(mount_path_depth(path)));
    // Remove a child's new directory before detaching its temporary parent.
    // Otherwise the same path could resolve to an unrelated, pre-existing
    // empty directory in the underlying filesystem and wrongly delete it.
    for (path, mounted) in paths {
        if mounted {
            if let Err(error) = vm.kernel.unmount_filesystem_for_operator(&path) {
                rollback_errors.push(format!("unmount {path}: {error}"));
                // Shutdown may fail after detachment, but an earlier failure
                // can leave the mount live. Never remove its root directory.
                if vm
                    .kernel
                    .mounted_filesystems()
                    .iter()
                    .any(|mount| mount.path == path)
                {
                    continue;
                }
            }
        }
        if created_mountpoints.contains(&path) {
            let result = vm.kernel.exists_for_operator(&path).and_then(|exists| {
                if exists {
                    vm.kernel.remove_dir_for_operator(&path)
                } else {
                    Ok(())
                }
            });
            if let Err(error) = result {
                rollback_errors.push(format!("remove new mountpoint {path}: {error}"));
            }
        }
    }
    for backend in detached.into_iter().rev() {
        let path = backend.path().to_owned();
        if let Err(error) = vm.kernel.restore_detached_filesystem_for_operator(backend) {
            rollback_errors.push(format!("restore {path}: {error}"));
        }
    }
    if rollback_errors.is_empty() {
        return original_error;
    }
    let detail = rollback_errors.join("; ");
    tracing::error!(vm_id = %context.vm_id, error = %original_error, rollback = %detail, "VM mount rollback failed");
    VmError::InvalidState(format!(
        "{original_error}; VM mount rollback failed: {detail}"
    ))
}

fn emit_mount_audit_event<B>(context: &MountPluginContext<B>, mount: &MountDescriptor, name: &str)
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    emit_security_audit_event(
        &context.bridge,
        &context.vm_id,
        name,
        audit_fields([
            (String::from("guest_path"), mount.guest_path.clone()),
            (String::from("plugin_id"), mount.plugin.id.clone()),
            (String::from("read_only"), mount.read_only.to_string()),
        ]),
    );
}

fn mount_leaf_descriptors<B>(
    mount_plugins: &agentos_vm_kernel::mount_plugin::FileSystemPluginRegistry<
        MountPluginContext<B>,
    >,
    vm: &mut VmState,
    mounts: &[crate::protocol::MountDescriptor],
    context: MountPluginContext<B>,
) -> Result<(), VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let prepared = prepare_mount_descriptors(mount_plugins, mounts)?;
    let mut mounted = Vec::with_capacity(prepared.len());
    let mut created_mountpoints = BTreeSet::new();
    if let Err(error) = mount_prepared_descriptors(
        mount_plugins,
        vm,
        prepared,
        &context,
        &mut mounted,
        &mut created_mountpoints,
    ) {
        return Err(rollback_mount_reconciliation(
            vm,
            &context,
            mounted,
            Vec::new(),
            created_mountpoints,
            error,
        ));
    }
    for mount in &mounted {
        emit_mount_audit_event(&context, mount, "security.mount.mounted");
    }
    Ok(())
}

fn prepare_mount_descriptors<'a, B>(
    mount_plugins: &agentos_vm_kernel::mount_plugin::FileSystemPluginRegistry<
        MountPluginContext<B>,
    >,
    mounts: &'a [crate::protocol::MountDescriptor],
) -> Result<Vec<(&'a crate::protocol::MountDescriptor, serde_json::Value)>, VmError> {
    let registered_plugins = mount_plugins
        .plugin_ids()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut paths = BTreeSet::new();
    let mut prepared = Vec::with_capacity(mounts.len());
    for mount in mounts {
        agentos_vm_kernel::vfs::validate_path(&mount.guest_path).map_err(vfs_error)?;
        let path = normalize_path(&mount.guest_path);
        if path == "/" || !paths.insert(path.clone()) {
            return Err(VmError::InvalidState(format!(
                "invalid or duplicate VM mount path: {}",
                mount.guest_path
            )));
        }
        if !registered_plugins.contains(&mount.plugin.id) {
            return Err(VmError::Plugin(format!(
                "filesystem plugin is not registered: {}",
                mount.plugin.id
            )));
        }
        let config_value = serde_json::from_str(&mount.plugin.config).map_err(|error| {
            VmError::InvalidState(format!(
                "mount plugin config for {} is not valid JSON: {error}",
                mount.plugin.id
            ))
        })?;
        prepared.push((mount, config_value));
    }
    prepared.sort_by_key(|(mount, _)| mount_path_depth(&mount.guest_path));
    Ok(prepared)
}

fn mount_prepared_descriptors<B>(
    mount_plugins: &agentos_vm_kernel::mount_plugin::FileSystemPluginRegistry<
        MountPluginContext<B>,
    >,
    vm: &mut VmState,
    mounts: Vec<(&crate::protocol::MountDescriptor, serde_json::Value)>,
    context: &MountPluginContext<B>,
    mounted: &mut Vec<MountDescriptor>,
    created_mountpoints: &mut BTreeSet<String>,
) -> Result<(), VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let max_filesystem_bytes = vm.kernel.resource_limits().max_filesystem_bytes;
    let max_inode_count = vm.kernel.resource_limits().max_inode_count;
    let mut checked_paths = BTreeSet::new();
    // Mount parents before nested leaves. Configure payload order is not a
    // filesystem invariant, and mounting a parent after one of its children is
    // rejected by the kernel mount table.
    for (mount, config_value) in mounts {
        // mount_boxed may recursively mkdir on the underlying parent, even
        // before a later leaf fails. Record missing ancestors for rollback.
        let mut path = String::new();
        for component in normalize_path(&mount.guest_path)
            .split('/')
            .filter(|part| !part.is_empty())
        {
            path.push('/');
            path.push_str(component);
            if checked_paths.insert(path.clone())
                && !vm.kernel.exists_for_operator(&path).map_err(kernel_error)?
            {
                created_mountpoints.insert(path.clone());
            }
        }
        let filesystem = mount_plugins
            .open(
                &mount.plugin.id,
                OpenFileSystemPluginRequest {
                    vm_id: &context.vm_id,
                    guest_path: &mount.guest_path,
                    read_only: mount.read_only,
                    config: &config_value,
                    context,
                },
            )
            .map_err(plugin_error)?;

        vm.kernel
            .mount_boxed_filesystem_for_operator(
                &mount.guest_path,
                filesystem,
                MountOptions::new(mount.plugin.id.clone())
                    .guest_source(mount.guest_source.clone())
                    .guest_fstype(mount.guest_fstype.clone())
                    .read_only(mount.read_only)
                    .max_bytes(max_filesystem_bytes)
                    .max_inodes(max_inode_count),
            )
            .map_err(kernel_error)?;
        mounted.push(mount.clone());
    }

    Ok(())
}

fn shutdown_configured_mounts<B>(
    vm: &mut VmState,
    context: &MountPluginContext<B>,
    phase: &str,
    continue_on_error: bool,
) -> Result<(), VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    // Nested leaves must be detached before their parents. In particular, npm
    // workspace packages are explicit child mounts below `/node_modules`.
    let mut existing_mounts = vm.configuration.mounts.clone();
    existing_mounts.sort_by_key(|mount| std::cmp::Reverse(mount_path_depth(&mount.guest_path)));
    for existing in existing_mounts {
        match vm
            .kernel
            .unmount_filesystem_for_operator(&existing.guest_path)
        {
            Ok(()) => emit_security_audit_event(
                &context.bridge,
                &context.vm_id,
                "security.mount.unmounted",
                audit_fields([
                    (String::from("guest_path"), existing.guest_path.clone()),
                    (String::from("plugin_id"), existing.plugin.id.clone()),
                    (String::from("read_only"), existing.read_only.to_string()),
                ]),
            ),
            Err(error) if error.code() == "EINVAL" => {}
            Err(error) => {
                if let Err(emit_error) = emit_structured_event(
                    &context.bridge,
                    &context.vm_id,
                    "filesystem.mount.shutdown_failed",
                    audit_fields([
                        (String::from("guest_path"), existing.guest_path.clone()),
                        (String::from("plugin_id"), existing.plugin.id.clone()),
                        (String::from("read_only"), existing.read_only.to_string()),
                        (String::from("phase"), String::from(phase)),
                        (String::from("error_code"), String::from(error.code())),
                        (String::from("error"), error.to_string()),
                    ]),
                ) {
                    eprintln!(
                        "ERR_AGENTOS_DIAGNOSTIC_EMIT: failed to emit mount shutdown failure for VM {} at {}: {emit_error:?}",
                        context.vm_id, existing.guest_path
                    );
                }

                if !continue_on_error {
                    return Err(kernel_error(error));
                }
            }
        }
    }

    Ok(())
}

fn mount_path_depth(path: &str) -> usize {
    normalize_path(path)
        .split('/')
        .filter(|component| !component.is_empty())
        .count()
}

/// Build the `/opt/agentos` package projection for `configure_vm`.
///
/// The projection mounts the package tar directly and serves derived aliases as
/// synthetic symlink leaves. This eliminates extraction and the old host-disk
/// symlink farm: the tar VFS indexes member offsets once and reads mmap-backed
/// byte ranges. Each managed entry is a granular leaf mount, while parent dirs
/// such as `/opt/agentos/bin` and `/opt/agentos/pkgs/<pkg>` remain writable
/// overlay dirs so guest-installed commands can coexist beside managed entries.
fn build_packages_projection(
    _vm_id: &str,
    packages: &[crate::package_projection::PackageDescriptor],
    mount_at: &str,
    max_mounts: usize,
) -> Result<Vec<MountDescriptor>, VmError> {
    Ok(
        crate::package_projection::build_package_leaf_mounts_with_limit(
            packages, mount_at, max_mounts,
        )?
        .into_iter()
        .map(package_leaf_mount_to_descriptor)
        .collect(),
    )
}

fn build_complete_package_projection(
    vm_id: &str,
    package: &crate::package_projection::PackageDescriptor,
    mount_at: &str,
    used: usize,
    max_mounts: usize,
) -> Result<Vec<MountDescriptor>, VmError> {
    let mut mounts = build_packages_projection(
        vm_id,
        std::slice::from_ref(package),
        mount_at,
        max_mounts.saturating_sub(used),
    )
    .map_err(|error| match error {
        VmError::PackageMountLimit { requested, .. } => VmError::PackageMountLimit {
            used,
            requested,
            limit: max_mounts,
        },
        other => other,
    })?;
    append_package_provides_mounts(&mut mounts, std::slice::from_ref(package), used, max_mounts)?;
    Ok(mounts)
}

fn check_package_mount_limit(
    vm_id: &str,
    used: usize,
    requested: usize,
    limit: usize,
) -> Result<(), VmError> {
    let observed = used
        .checked_add(requested)
        .ok_or(VmError::PackageMountLimit {
            used,
            requested,
            limit,
        })?;
    if observed > limit {
        return Err(VmError::PackageMountLimit {
            used,
            requested,
            limit,
        });
    }
    if observed != 0 && observed >= limit - limit / 5 {
        tracing::warn!(
            vm_id,
            observed,
            capacity = limit,
            config_path = "limits.agentosPackages.maxMounts",
            "agentOS package mounts approaching VM limit"
        );
    }
    Ok(())
}

fn normalized_package_mount_root(root: &str) -> String {
    if root.is_empty() {
        crate::package_projection::OPT_AGENTOS_ROOT.to_owned()
    } else {
        normalize_path(root)
    }
}

fn validate_package_names_and_commands<'a>(
    descriptors: impl IntoIterator<Item = &'a crate::package_projection::PackageDescriptor>,
) -> Result<(), VmError> {
    let mut names = BTreeSet::new();
    let mut commands = BTreeSet::new();
    for descriptor in descriptors {
        if !names.insert(&descriptor.name) {
            return Err(VmError::InvalidState(format!(
                "package {:?} is already projected under another identity",
                descriptor.name
            )));
        }
        for target in &descriptor.commands {
            if !commands.insert(&target.command) {
                return Err(VmError::InvalidState(format!(
                    "command {:?} is already provided by another package",
                    target.command
                )));
            }
        }
    }
    Ok(())
}

fn package_mount_root<'a>(vm: &'a VmState, id: &str) -> Result<&'a str, VmError> {
    vm.package_mount_roots
        .get(id)
        .map(String::as_str)
        .ok_or_else(|| {
            VmError::InvalidState(format!(
                "software package {id:?} is missing its projection root"
            ))
        })
}

fn package_command_guest_path(root: &str, command: &str) -> String {
    normalize_path(&format!("{root}/bin/{command}"))
}

fn package_leaf_mount_to_descriptor(
    mount: crate::package_projection::PackageLeafMount,
) -> MountDescriptor {
    match mount {
        crate::package_projection::PackageLeafMount::Tar {
            guest_path,
            tar_path,
            root,
        } => MountDescriptor {
            guest_path,
            guest_source: String::from("agentos_packages"),
            guest_fstype: String::from("agentos_packages"),
            read_only: true,
            plugin: MountPluginDescriptor {
                id: String::from("agentos_packages"),
                config: serde_json::json!({
                    "kind": "tar",
                    "tarPath": tar_path,
                    "root": root,
                    "readOnly": true,
                })
                .to_string(),
            },
        },
        crate::package_projection::PackageLeafMount::HostDir {
            guest_path,
            host_path,
        } => MountDescriptor {
            guest_path,
            guest_source: String::from("agentos_packages"),
            guest_fstype: String::from("agentos_packages"),
            read_only: true,
            plugin: MountPluginDescriptor {
                id: String::from("agentos_packages"),
                config: serde_json::json!({
                    "kind": "hostDir",
                    "hostPath": host_path,
                    "readOnly": true,
                })
                .to_string(),
            },
        },
        crate::package_projection::PackageLeafMount::SingleSymlink { guest_path, target } => {
            MountDescriptor {
                guest_path,
                guest_source: String::from("agentos_packages"),
                guest_fstype: String::from("agentos_packages"),
                read_only: true,
                plugin: MountPluginDescriptor {
                    id: String::from("agentos_packages"),
                    config: serde_json::json!({
                        "kind": "singleSymlink",
                        "target": target,
                        "readOnly": true,
                    })
                    .to_string(),
                },
            }
        }
    }
}

fn package_descriptors_from_wire(
    packages: &[crate::protocol::PackageDescriptor],
) -> Result<Vec<crate::package_projection::PackageDescriptor>, VmError> {
    packages
        .iter()
        .map(|package| crate::package_projection::read_package_manifest_from_path(&package.path))
        .collect()
}

fn apply_package_provides_env(
    guest_env: &mut BTreeMap<String, String>,
    packages: &[crate::package_projection::PackageDescriptor],
) {
    for package in packages {
        let Some(provides) = package.provides.as_ref() else {
            continue;
        };
        for (key, value) in &provides.env {
            guest_env
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
}

fn refresh_package_runtime_state(vm: &mut VmState) -> Result<(), VmError> {
    let descriptors = vm
        .package_descriptors
        .iter()
        .map(|(_, descriptor)| descriptor.clone())
        .collect::<Vec<_>>();
    vm.guest_env = vm.base_guest_env.clone();
    apply_package_provides_env(&mut vm.guest_env, &descriptors);

    vm.command_guest_paths = discover_command_guest_paths(&mut vm.kernel)?;
    let mut provided_commands = BTreeMap::new();
    for (id, descriptor) in &vm.package_descriptors {
        let mount_root = package_mount_root(vm, id)?.to_owned();
        let commands = descriptor
            .commands
            .iter()
            .map(|target| target.command.clone())
            .collect::<Vec<_>>();
        for command in &commands {
            vm.command_guest_paths
                .entry(command.clone())
                .or_insert_with(|| package_command_guest_path(&mount_root, command));
        }
        provided_commands.insert(descriptor.name.clone(), commands);
    }
    vm.provided_commands = provided_commands.clone();
    vm.configuration.provided_commands = provided_commands;

    let command_guest_paths = vm.command_guest_paths.clone();
    refresh_guest_command_path_env_from_map(&mut vm.guest_env, &command_guest_paths);
    let mut execution_commands = default_execution_commands(vm.configuration.defaults_profile);
    execution_commands.extend(vm.command_guest_paths.keys().cloned());
    vm.kernel
        .register_driver_for_operator(CommandDriver::new(
            EXECUTION_DRIVER_NAME,
            execution_commands,
        ))
        .map_err(kernel_error)
}

fn append_package_provides_mounts(
    mounts: &mut Vec<MountDescriptor>,
    packages: &[crate::package_projection::PackageDescriptor],
    used: usize,
    limit: usize,
) -> Result<(), VmError> {
    for package in packages {
        let Some(provides) = package.provides.as_ref() else {
            continue;
        };
        for file in &provides.files {
            match crate::package_projection::package_provides_file_mount(
                package,
                &file.source,
                &file.target,
            )? {
                Some(mount) => {
                    if mounts.len() >= limit.saturating_sub(used) {
                        return Err(VmError::PackageMountLimit {
                            used,
                            requested: mounts.len().saturating_add(1),
                            limit,
                        });
                    }
                    mounts.push(package_leaf_mount_to_descriptor(mount));
                }
                None => {
                    tracing::warn!(
                        package = %package.name,
                        source = %file.source,
                        target = %file.target,
                        "package provides file source is not a directory; skipping"
                    );
                }
            }
        }
    }
    Ok(())
}

fn append_module_access_mount(
    mounts: &mut Vec<MountDescriptor>,
    module_access_cwd: Option<&String>,
) -> Result<(), VmError> {
    if mounts
        .iter()
        .any(|mount| mount.guest_path == "/root/node_modules")
    {
        return Ok(());
    }

    let Some(module_access_cwd) = module_access_cwd else {
        return Ok(());
    };
    let root = resolve_host_path(Some(module_access_cwd))?.join("node_modules");
    if !root.is_dir() {
        return Ok(());
    }

    mounts.push(MountDescriptor {
        guest_path: String::from("/root/node_modules"),
        guest_source: String::from("module_access"),
        guest_fstype: String::from("module_access"),
        read_only: true,
        plugin: MountPluginDescriptor {
            id: String::from("module_access"),
            config: serde_json::json!({
                "hostPath": root,
            })
            .to_string(),
        },
    });
    append_module_access_symlink_mounts(mounts, &root)?;
    Ok(())
}

fn append_module_access_symlink_mounts(
    mounts: &mut Vec<MountDescriptor>,
    node_modules_root: &Path,
) -> Result<(), VmError> {
    for entry in fs::read_dir(node_modules_root)
        .map_err(|error| VmError::Io(format!("failed to read module_access root: {error}")))?
    {
        let entry = entry.map_err(|error| {
            VmError::Io(format!("failed to inspect module_access root: {error}"))
        })?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| VmError::Io(format!("failed to stat module_access entry: {error}")))?;
        if metadata.file_type().is_symlink() {
            append_module_access_symlink_mount(
                mounts,
                &format!("/root/node_modules/{name}"),
                &path,
            )?;
            continue;
        }
        if !metadata.is_dir() || !name.starts_with('@') {
            continue;
        }
        for scoped_entry in fs::read_dir(&path)
            .map_err(|error| VmError::Io(format!("failed to read module_access scope: {error}")))?
        {
            let scoped_entry = scoped_entry.map_err(|error| {
                VmError::Io(format!("failed to inspect module_access scope: {error}"))
            })?;
            let scoped_name = scoped_entry.file_name().to_string_lossy().into_owned();
            if scoped_name.starts_with('.') {
                continue;
            }
            let scoped_path = scoped_entry.path();
            let scoped_metadata = fs::symlink_metadata(&scoped_path).map_err(|error| {
                VmError::Io(format!(
                    "failed to stat module_access scoped entry: {error}"
                ))
            })?;
            if scoped_metadata.file_type().is_symlink() {
                append_module_access_symlink_mount(
                    mounts,
                    &format!("/root/node_modules/{name}/{scoped_name}"),
                    &scoped_path,
                )?;
            }
        }
    }

    Ok(())
}

fn append_module_access_symlink_mount(
    mounts: &mut Vec<MountDescriptor>,
    guest_path: &str,
    symlink_path: &Path,
) -> Result<(), VmError> {
    if mounts.iter().any(|mount| mount.guest_path == guest_path) {
        return Ok(());
    }

    let target = fs::canonicalize(symlink_path).map_err(|error| {
        VmError::Io(format!(
            "failed to resolve module_access package symlink {}: {error}",
            symlink_path.display()
        ))
    })?;
    if !target.is_dir() {
        return Ok(());
    }

    mounts.push(MountDescriptor {
        guest_path: guest_path.to_owned(),
        guest_source: String::from("host_dir"),
        guest_fstype: String::from("host_dir"),
        read_only: true,
        plugin: MountPluginDescriptor {
            id: String::from("host_dir"),
            config: serde_json::json!({
                "hostPath": target,
                "readOnly": true,
            })
            .to_string(),
        },
    });
    Ok(())
}

fn sidecar_core_error(error: crate::core::SidecarCoreError) -> VmError {
    VmError::InvalidState(error.to_string())
}

fn resolve_guest_cwd(value: Option<&String>) -> String {
    value
        .map(|path| normalize_guest_path(path))
        .unwrap_or_else(|| String::from("/workspace"))
}

fn resolve_vm_cwds(
    metadata_cwd: Option<&String>,
    runtime_scratch_root: &Path,
) -> Result<(String, PathBuf), VmError> {
    if let Some(raw_cwd) = metadata_cwd {
        let candidate = PathBuf::from(raw_cwd);
        if candidate.is_absolute() || raw_cwd.starts_with('.') {
            let resolved_host_cwd = resolve_host_path(Some(raw_cwd))?;
            return Ok((String::from("/"), resolved_host_cwd));
        }
    }

    let guest_cwd = resolve_guest_cwd(metadata_cwd);
    let host_cwd = runtime_scratch_path_for_guest(runtime_scratch_root, &guest_cwd);
    Ok((guest_cwd, host_cwd))
}

fn resolve_host_path(value: Option<&String>) -> Result<PathBuf, VmError> {
    match value {
        Some(path) => {
            let cwd = PathBuf::from(path);
            let resolved = if cwd.is_absolute() {
                cwd
            } else {
                std::env::current_dir()
                    .map_err(|error| {
                        VmError::Io(format!("failed to resolve current directory: {error}"))
                    })?
                    .join(cwd)
            };
            Ok(resolved)
        }
        None => std::env::current_dir()
            .map_err(|error| VmError::Io(format!("failed to resolve current directory: {error}"))),
    }
}

fn create_vm_runtime_scratch_root(vm_id: &str) -> Result<PathBuf, VmError> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| VmError::Io(format!("failed to compute scratch-root nonce: {error}")))?
        .as_nanos();
    let root = std::env::temp_dir().join(format!("agentos-vm-runtime-{vm_id}-{nonce}"));
    fs::create_dir_all(&root)
        .map_err(|error| VmError::Io(format!("failed to create VM runtime root: {error}")))?;
    initialize_vm_runtime_scratch_root(root)
}

fn initialize_vm_runtime_scratch_root(root: PathBuf) -> Result<PathBuf, VmError> {
    let cleanup_root = root.clone();
    // macOS: `std::env::temp_dir()` lives under `/var/folders/…`, but `/var` is a
    // symlink to `/private/var`, and macOS fd→path recovery (`fcntl(F_GETPATH)`)
    // reports the resolved `/private/var/…` form. Canonicalize the private
    // runtime root so executor confinement compares the same host path form.
    #[cfg(target_os = "macos")]
    let initialized = fs::canonicalize(&root)
        .map_err(|error| VmError::Io(format!("failed to canonicalize VM runtime root: {error}")));
    #[cfg(not(target_os = "macos"))]
    let initialized: Result<PathBuf, VmError> = Ok(root);

    match initialized {
        Ok(root) => Ok(root),
        Err(error) => match fs::remove_dir_all(&cleanup_root) {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(VmError::Io(format!(
                "{error}; additionally failed to clean runtime root {}: {cleanup_error}",
                cleanup_root.display()
            ))),
        },
    }
}

fn runtime_scratch_path_for_guest(runtime_root: &Path, guest_path: &str) -> PathBuf {
    let relative = normalize_guest_path(guest_path);
    let relative = relative.trim_start_matches('/');
    if relative.is_empty() {
        runtime_root.to_path_buf()
    } else {
        runtime_root.join(relative)
    }
}

fn default_execution_commands(profile: vm_config::VmDefaultsProfile) -> Vec<String> {
    let mut commands = vec![
        String::from(JAVASCRIPT_COMMAND),
        String::from(PYTHON_COMMAND),
        String::from("python3"),
        String::from(WASM_COMMAND),
    ];
    if profile == vm_config::VmDefaultsProfile::AgentOs {
        commands.extend([String::from("npm"), String::from("npx")]);
    }
    commands
}

fn create_vm_environment(
    config: &vm_config::CreateVmConfig,
) -> Result<BTreeMap<String, String>, VmError> {
    if let Some(environment) = &config.env {
        return Ok(environment.clone());
    }
    match config.defaults_profile() {
        vm_config::VmDefaultsProfile::Secure => Ok(BTreeMap::new()),
        vm_config::VmDefaultsProfile::AgentOs => load_bundled_base_environment().map_err(|error| {
            VmError::InvalidState(format!(
                "failed to load sidecar-owned agentOS environment defaults: {error}"
            ))
        }),
    }
}

/// Compare the policy that VM creation actually applies, while retaining a
/// fail-safe structural comparison for fields without a resolved form here.
/// The public comparison request will call this with the serving sidecar's
/// frame limit, so actor clients do not need to copy VM default values.
fn equivalent_vm_creation_config(
    mut before: vm_config::CreateVmConfig,
    mut after: vm_config::CreateVmConfig,
    sidecar_max_frame_bytes: usize,
) -> Result<bool, VmError> {
    let resolve = |config: &mut vm_config::CreateVmConfig| -> Result<_, VmError> {
        config
            .normalize()
            .map_err(|error| VmError::InvalidState(format!("invalid create VM config: {error}")))?;
        config
            .validate(sidecar_max_frame_bytes)
            .map_err(|error| VmError::InvalidState(format!("invalid create VM config: {error}")))?;
        let profile = config.defaults_profile();
        let permissions = resolve_profile_permissions_policy(profile, config.permissions.clone());
        validate_permissions_policy(&permissions)?;
        let limits =
            crate::limits::vm_limits_from_config(config.limits.as_ref(), sidecar_max_frame_bytes)?;
        let environment = create_vm_environment(config)?;
        let mut js_runtime = config.js_runtime.clone().unwrap_or_default();
        js_runtime.high_resolution_time = Some(js_runtime.high_resolution_time.unwrap_or(false));

        // Keep every other field in the structural comparison, including
        // omission-sensitive allow-lists, bootstrap commands, and database
        // descriptors. New fields therefore require replacement by default.
        config.defaults_profile = Some(profile);
        config.permissions = None;
        config.limits = None;
        config.env = None;
        config.js_runtime = None;
        Ok((permissions, limits, environment, js_runtime))
    };

    let before_resolved = resolve(&mut before)?;
    let after_resolved = resolve(&mut after)?;
    Ok(before == after && before_resolved == after_resolved)
}

fn normalize_guest_path(path: &str) -> String {
    let mut segments = Vec::new();
    let absolute = path.starts_with('/');
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }

    if !absolute {
        return format!("/{}", segments.join("/"));
    }
    if segments.is_empty() {
        String::from("/")
    } else {
        format!("/{}", segments.join("/"))
    }
}

fn parse_vm_dns_nameserver(value: &str) -> Result<SocketAddr, VmError> {
    use crate::state::VM_DNS_SERVERS_METADATA_KEY;

    if let Ok(address) = value.parse::<SocketAddr>() {
        return Ok(address);
    }
    if let Ok(ip) = value.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, 53));
    }
    Err(VmError::InvalidState(format!(
        "invalid {} entry {value}; expected IP or IP:port",
        VM_DNS_SERVERS_METADATA_KEY
    )))
}

fn refresh_guest_command_path_env_from_map(
    env: &mut BTreeMap<String, String>,
    commands: &BTreeMap<String, String>,
) {
    let roots = commands
        .values()
        .filter_map(|path| path.rsplit_once('/').map(|(parent, _)| parent.to_owned()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    refresh_guest_command_path_env(env, &roots);
}

fn refresh_guest_command_path_env(
    guest_env: &mut BTreeMap<String, String>,
    command_search_roots: &[String],
) {
    let mut merged = Vec::new();
    let mut seen = BTreeSet::new();

    for root in command_search_roots {
        let normalized = normalize_path(root);
        if normalized == "/" {
            continue;
        }
        if seen.insert(normalized.clone()) {
            merged.push(normalized);
        }
    }

    for segment in DEFAULT_GUEST_PATH_ENV.split(':') {
        let normalized = normalize_path(segment);
        if seen.insert(normalized.clone()) {
            merged.push(normalized);
        }
    }

    // PATH is derived state. Strip roots managed by the command projection
    // before preserving caller-supplied extras, so a removed numeric legacy
    // mount cannot survive forever merely because it appeared in the previous
    // synthesized PATH value.
    if let Some(existing_path) = guest_env.get("PATH") {
        for segment in existing_path.split(':') {
            let trimmed = segment.trim();
            if trimmed.is_empty() {
                continue;
            }
            let normalized = if trimmed.starts_with('/') {
                normalize_path(trimmed)
            } else {
                trimmed.to_owned()
            };
            if is_managed_guest_command_path_segment(&normalized) {
                continue;
            }
            if seen.insert(normalized.clone()) {
                merged.push(normalized);
            }
        }
    }

    guest_env.insert(String::from("PATH"), merged.join(":"));
}

fn is_managed_guest_command_path_segment(segment: &str) -> bool {
    let normalized = if segment.starts_with('/') {
        normalize_path(segment)
    } else {
        segment.to_owned()
    };
    if DEFAULT_GUEST_PATH_ENV
        .split(':')
        .any(|default| normalize_path(default) == normalized)
    {
        return true;
    }
    normalized
        .strip_prefix("/__agentos/commands/")
        .is_some_and(|root| {
            !root.is_empty() && !root.contains('/') && root.chars().all(|ch| ch.is_ascii_digit())
        })
}

pub(crate) fn normalize_dns_hostname(hostname: &str) -> Result<String, VmError> {
    let normalized = hostname.trim().trim_end_matches('.').to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(VmError::InvalidState(String::from(
            "DNS hostname must not be empty",
        )));
    }
    Ok(normalized)
}

// Retained for the native-root command-stub test; `python` is now a real
// command so production no longer prunes `/bin/python`.
#[cfg(test)]
fn prune_kernel_command_stub(
    kernel: &mut KernelVm<agentos_vm_kernel::mount_table::MountTable>,
    path: &str,
) -> Result<(), VmError> {
    if !kernel.exists(path).map_err(kernel_error)? {
        return Ok(());
    }

    let content = kernel.read_file(path).map_err(kernel_error)?;
    if content == KERNEL_COMMAND_STUB {
        kernel.remove_file(path).map_err(kernel_error)?;
    }

    Ok(())
}

fn canonicalize_comparison_mounts<B>(
    mount_plugins: &agentos_vm_kernel::mount_plugin::FileSystemPluginRegistry<
        MountPluginContext<B>,
    >,
    mounts: &mut [crate::protocol::MountDescriptor],
) -> Result<(), VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    prepare_mount_descriptors(mount_plugins, mounts)?;
    for mount in mounts.iter_mut() {
        // Creation resolves paths and parses plugin JSON before applying a
        // mount. Compare that same identity, not spelling/serialization details.
        mount.guest_path = normalize_path(&mount.guest_path);
        let config: serde_json::Value = serde_json::from_str(&mount.plugin.config)
            .map_err(|error| VmError::InvalidState(error.to_string()))?;
        mount.plugin.config = config.to_string();
    }
    mounts.sort_by(|left, right| left.guest_path.cmp(&right.guest_path));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        bootstrap_native_root_filesystem, close_vm_capability_admission,
        create_vm_unix_socket_host_dir, execution_driver_commands, native_root_plugin_from_config,
        projected_commands_from_provided_commands, prune_kernel_command_stub,
        refresh_guest_command_path_env, retire_vm_fairness, vm_quarantine_reason,
        vm_resource_ledger, wait_for_vm_reconciliation, CA_CERTIFICATES_BUNDLE,
        CA_CERTIFICATES_GUEST_PATH, CA_CERTIFICATES_SYMLINK_PATH, DEFAULT_GUEST_PATH_ENV,
        KERNEL_COMMAND_STUB,
    };
    use crate::bootstrap::KernelCommandInventory;
    use crate::bridge::MountPluginContext;
    use crate::plugins::chunked_local::ChunkedLocalMountPlugin;
    use crate::protocol::*;
    use crate::protocol::{RootFilesystemDescriptor, RootFilesystemEntry, RootFilesystemEntryKind};
    use crate::service::VmManager;
    use crate::state::{
        ConnectionState, QuarantinedVmGeneration, SessionState, VmQuarantineReason,
        VmReconciliationSnapshot,
    };
    use agentos_driver_tokio::accounting::{ResourceClass, ResourceLedger, ResourceLimit};
    use agentos_driver_tokio::capability::{CapabilityKind, CapabilityRegistry};
    use agentos_driver_tokio::fairness::FairBudget;
    use agentos_driver_tokio::metrics::ResourceMetricClass;
    use agentos_driver_tokio::{DriverHandle, TaskClass, TokioDriver};
    use agentos_vm_host_interface::LocalVmHost as LocalBridge;
    use agentos_vm_kernel::kernel::{KernelVm, KernelVmConfig};
    use agentos_vm_kernel::mount_plugin::{FileSystemPluginFactory, OpenFileSystemPluginRequest};
    use agentos_vm_kernel::mount_table::{MountOptions, MountTable};
    use agentos_vm_kernel::permissions::Permissions;
    use agentos_vm_kernel::vfs::VirtualFileSystem;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn mount_preflight_orders_normalized_parents_before_children() {
        let registry = super::build_mount_plugin_registry::<LocalBridge>().unwrap();
        let descriptor = |path: &str| crate::protocol::MountDescriptor {
            guest_path: path.into(),
            guest_source: "test".into(),
            guest_fstype: "test".into(),
            read_only: true,
            plugin: crate::protocol::MountPluginDescriptor {
                id: "agentos_packages".into(),
                config: "{}".into(),
            },
        };
        let mounts = [descriptor("/parent/child"), descriptor("/x/../parent")];
        let prepared = super::prepare_mount_descriptors(&registry, &mounts).unwrap();
        assert_eq!(prepared[0].0.guest_path, "/x/../parent");
        assert_eq!(prepared[1].0.guest_path, "/parent/child");
        // Teardown uses the same depth in reverse and must also ignore dots.
        assert_eq!(super::mount_path_depth("/parent/./child/"), 2);
        let duplicates = [descriptor("/parent"), descriptor("/x/../parent")];
        assert!(super::prepare_mount_descriptors(&registry, &duplicates).is_err());
    }

    fn reconciliation_handles(
        generation: u64,
    ) -> (Arc<ResourceLedger>, DriverHandle, CapabilityRegistry) {
        let process = TokioDriver::process(&agentos_driver_tokio::DriverConfig::default())
            .expect("initialize process runtime")
            .handle();
        let resources = Arc::new(ResourceLedger::child(
            format!("teardown-test-vm-generation={generation}"),
            [
                (
                    ResourceClass::Tasks,
                    ResourceLimit::new(4, "limits.reactor.maxTasks"),
                ),
                (
                    ResourceClass::Capabilities,
                    ResourceLimit::new(4, "limits.reactor.maxCapabilities"),
                ),
                (
                    ResourceClass::Sockets,
                    ResourceLimit::new(4, "limits.resources.maxSockets"),
                ),
            ],
            Arc::clone(process.resources()),
        ));
        let runtime_context = process.scoped_for_vm(Arc::clone(&resources), generation);
        let capabilities = CapabilityRegistry::new(generation, Arc::clone(&resources));
        (resources, runtime_context, capabilities)
    }

    #[test]
    fn guest_command_path_rebuild_drops_removed_managed_roots() {
        let mut guest_env = BTreeMap::from([(
            String::from("PATH"),
            format!(
                "/__agentos/commands/001:{DEFAULT_GUEST_PATH_ENV}:/custom/bin:relative:/__agentos/commands/custom"
            ),
        )]);

        refresh_guest_command_path_env(&mut guest_env, &[String::from("/__agentos/commands/002")]);

        assert_eq!(
            guest_env.get("PATH").map(String::as_str),
            Some(
                "/__agentos/commands/002:/usr/local/sbin:/usr/local/bin:/opt/agentos/bin:/usr/sbin:/usr/bin:/sbin:/bin:/custom/bin:relative:/__agentos/commands/custom"
            )
        );
    }

    #[test]
    fn transient_inventory_drives_registration_and_projection_reporting() {
        let kernel_commands = KernelCommandInventory {
            names: BTreeSet::from([String::from("legacy"), String::from("shadowed")]),
            search_roots: vec![String::from("/__agentos/commands/001")],
        };
        let provided_commands = BTreeMap::from([
            (
                String::from("pkg-a"),
                vec![String::from("visible"), String::from("shadowed")],
            ),
            (String::from("pkg-b"), vec![String::from("second")]),
        ]);

        let registered = execution_driver_commands(
            &kernel_commands,
            &provided_commands,
            [String::from("hostFunction"), String::from("visible")],
        );
        assert_eq!(
            registered.iter().collect::<BTreeSet<_>>().len(),
            registered.len()
        );
        for expected in [
            "hostFunction",
            "legacy",
            "node",
            "python",
            "python3",
            "second",
            "shadowed",
            "visible",
            "wasm",
        ] {
            assert!(registered.iter().any(|command| command == expected));
        }

        assert_eq!(
            projected_commands_from_provided_commands(&provided_commands, &kernel_commands),
            vec![
                crate::protocol::ProjectedCommand {
                    name: String::from("second"),
                    guest_path: String::from("/opt/agentos/bin/second"),
                },
                crate::protocol::ProjectedCommand {
                    name: String::from("visible"),
                    guest_path: String::from("/opt/agentos/bin/visible"),
                },
            ]
        );
    }

    #[test]
    fn vm_runtime_bounds_every_resource_class_by_default() {
        let process = TokioDriver::process(&agentos_driver_tokio::DriverConfig::default())
            .expect("initialize process runtime")
            .handle();
        let ledger = vm_resource_ledger(
            "vm-all-resource-limits",
            88_001,
            &crate::limits::VmLimits::default(),
            Arc::clone(process.resources()),
        )
        .expect("construct bounded VM ledger");

        for resource in ResourceClass::ALL {
            let usage = ledger.usage(resource);
            assert_eq!(usage.used, 0, "{} starts charged", resource.name());
            assert!(
                usage.limit.is_some_and(|limit| limit > 0),
                "{} has no positive VM limit",
                resource.name()
            );
        }
        assert_eq!(
            ledger.usage(ResourceClass::WasmMemoryBytes).limit,
            process
                .resources()
                .usage(ResourceClass::WasmMemoryBytes)
                .limit,
            "the VM aggregate Store envelope must inherit the bounded process ceiling"
        );
        assert_ne!(
            ledger.usage(ResourceClass::WasmMemoryBytes).limit,
            crate::limits::VmLimits::default()
                .resources
                .max_wasm_memory_bytes
                .and_then(|value| usize::try_from(value).ok()),
            "the per-memory linear cap must not be reused as Store-overhead admission"
        );
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("teardown test runtime")
            .block_on(future)
    }

    struct PendingCloseDatabase {
        close_started: Arc<AtomicBool>,
    }

    #[async_trait::async_trait]
    impl crate::vm_sqlite::VmSqliteDatabase for PendingCloseDatabase {
        async fn query(
            &self,
            _statement: crate::vm_sqlite::SqlStatement,
        ) -> Result<crate::vm_sqlite::QueryResult, crate::vm_sqlite::VmSqliteError> {
            unreachable!("pending-close test never queries SQLite")
        }

        async fn transaction(
            &self,
            _statements: Vec<crate::vm_sqlite::SqlStatement>,
        ) -> Result<Vec<crate::vm_sqlite::QueryResult>, crate::vm_sqlite::VmSqliteError> {
            unreachable!("pending-close test never transacts SQLite")
        }

        async fn close(&self) -> Result<(), crate::vm_sqlite::VmSqliteError> {
            self.close_started.store(true, Ordering::Release);
            std::future::pending().await
        }
    }

    #[test]
    fn sqlite_close_deadline_polls_close_and_returns_typed_failure() {
        let close_started = Arc::new(AtomicBool::new(false));
        let database = PendingCloseDatabase {
            close_started: Arc::clone(&close_started),
        };
        let deadline = tokio::time::Instant::now() - Duration::from_millis(1);
        let (result, timed_out) = block_on(super::close_vm_database_before_deadline(
            "vm-close-deadline",
            &database,
            deadline,
            5_000,
        ));
        assert!(close_started.load(Ordering::Acquire));
        assert!(timed_out);
        assert!(matches!(
            result,
            Err(crate::VmError::VmTeardownDeadline { message, .. })
                if message.contains("ERR_AGENTOS_VM_TEARDOWN_DEADLINE")
                    && message.contains("phase=sqlite_close")
                    && message.contains("limits.reactor.shutdownDeadlineMs")
        ));
    }

    #[test]
    fn vm_config_comparison_uses_resolved_sidecar_defaults() {
        let baseline = agentos_vm_config::CreateVmConfig {
            defaults_profile: Some(agentos_vm_config::VmDefaultsProfile::AgentOs),
            ..Default::default()
        };
        let mut explicit_defaults = baseline.clone();
        explicit_defaults.limits = Some(agentos_vm_config::VmLimitsConfig::default());
        explicit_defaults.permissions = Some(agentos_vm_config::PermissionsPolicy {
            fs: None,
            network: None,
            child_process: None,
            process: None,
            env: None,
            host_function: None,
        });
        explicit_defaults.js_runtime = Some(agentos_vm_config::JsRuntimeConfig {
            high_resolution_time: Some(false),
            ..Default::default()
        });
        assert!(super::equivalent_vm_creation_config(
            baseline.clone(),
            explicit_defaults,
            crate::protocol::DEFAULT_MAX_FRAME_BYTES,
        )
        .expect("compare resolved defaults"));

        let mut empty_environment = baseline.clone();
        empty_environment.env = Some(Default::default());
        assert!(!super::equivalent_vm_creation_config(
            baseline.clone(),
            empty_environment,
            crate::protocol::DEFAULT_MAX_FRAME_BYTES,
        )
        .expect("explicit empty environment differs from base environment"));

        let mut empty_allowlist = baseline.clone();
        empty_allowlist.js_runtime = Some(agentos_vm_config::JsRuntimeConfig {
            allowed_builtins: Some(Vec::new()),
            ..Default::default()
        });
        assert!(!super::equivalent_vm_creation_config(
            baseline.clone(),
            empty_allowlist,
            crate::protocol::DEFAULT_MAX_FRAME_BYTES,
        )
        .expect("explicit empty allow-list denies builtins"));

        let mut changed_root = baseline.clone();
        changed_root.root_filesystem.mode = agentos_vm_config::RootFilesystemMode::ReadOnly;
        assert!(!super::equivalent_vm_creation_config(
            baseline,
            changed_root,
            crate::protocol::DEFAULT_MAX_FRAME_BYTES,
        )
        .expect("root filesystem changes require replacement"));
    }

    #[test]
    fn vm_lifecycle_wrappers_return_static_futures_without_borrowing_the_sidecar() {
        fn assert_static<F: std::future::Future + 'static>(_: &F) {}

        let mut sidecar = VmManager::new(LocalBridge::default()).expect("test sidecar");
        let request = RequestFrame::new(
            1,
            OwnershipScope::vm("connection-missing", "session-missing", "vm-missing"),
            RequestPayload::CreateLayer(CreateLayerRequest {}),
        );
        let operation = sidecar.create_layer(&request, CreateLayerRequest {});
        assert_static(&operation);

        // This mutation is a compile-time assertion that `operation` did not
        // retain the method receiver's `&mut VmManager` borrow.
        sidecar.next_vm_id = 41;
        let result = block_on(operation);
        assert!(matches!(result, Err(crate::VmError::InvalidState(_))));
        assert_eq!(sidecar.next_vm_id, 41);

        let configure_payload = ConfigureVmRequest {
            mounts: Vec::new(),
            software: Vec::new(),
            permissions: None,
            module_access_cwd: None,
            instructions: Vec::new(),
            projected_modules: Vec::new(),
            command_permissions: std::collections::HashMap::new(),
            loopback_exempt_ports: Vec::new(),
            packages: Vec::new(),
            packages_mount_at: String::new(),
            bootstrap_commands: Vec::new(),
            host_function_shim_commands: Vec::new(),
        };
        let configure_request = RequestFrame::new(
            2,
            OwnershipScope::vm("connection-missing", "session-missing", "vm-missing"),
            RequestPayload::ConfigureVm(configure_payload.clone()),
        );
        let configure_operation = sidecar.configure_vm(&configure_request, configure_payload);
        assert_static(&configure_operation);
        sidecar.next_vm_id = 42;
        let result = block_on(configure_operation);
        assert!(matches!(result, Err(crate::VmError::InvalidState(_))));
        assert_eq!(sidecar.next_vm_id, 42);
    }

    struct AdmissionCheckingDatabase {
        inner: crate::vm_sqlite::SharedVmSqliteDatabase,
        runtime_context: DriverHandle,
        capabilities: CapabilityRegistry,
    }

    #[async_trait::async_trait]
    impl crate::vm_sqlite::VmSqliteDatabase for AdmissionCheckingDatabase {
        async fn query(
            &self,
            statement: crate::vm_sqlite::SqlStatement,
        ) -> Result<crate::vm_sqlite::QueryResult, crate::vm_sqlite::VmSqliteError> {
            self.inner.query(statement).await
        }

        async fn transaction(
            &self,
            statements: Vec<crate::vm_sqlite::SqlStatement>,
        ) -> Result<Vec<crate::vm_sqlite::QueryResult>, crate::vm_sqlite::VmSqliteError> {
            self.inner.transaction(statements).await
        }

        async fn close(&self) -> Result<(), crate::vm_sqlite::VmSqliteError> {
            assert!(
                self.runtime_context.admission_is_open(),
                "SQLite close still needs runtime blocking admission"
            );
            let error = self
                .capabilities
                .reserve(CapabilityKind::UdpSocket)
                .expect_err("guest capability admission must close before SQLite cleanup");
            assert!(error
                .to_string()
                .contains("ERR_AGENTOS_CAPABILITY_REGISTRY_CLOSED"));
            self.inner.close().await
        }
    }

    fn check_database_admission_during_teardown(vm: &mut crate::state::VmState) {
        vm.database = Some(Arc::new(AdmissionCheckingDatabase {
            inner: vm.database.take().expect("real SQLite database"),
            runtime_context: vm.runtime_context.clone(),
            capabilities: vm.capabilities.clone(),
        }));
    }

    #[test]
    fn prepared_creation_preserves_kernel_process_generation() {
        let mut sidecar = VmManager::new(LocalBridge::default()).expect("test sidecar");
        sidecar.connections.insert(
            "identity-connection".into(),
            ConnectionState {
                auth_token: String::new(),
                sessions: BTreeSet::from(["identity-session".into()]),
            },
        );
        sidecar.sessions.insert(
            "identity-session".into(),
            SessionState {
                connection_id: "identity-connection".into(),
                placement: SidecarPlacement::SidecarPlacementShared(SidecarPlacementShared {
                    pool: None,
                }),
                metadata: BTreeMap::new(),
                vm_ids: BTreeSet::new(),
            },
        );
        for request_id in 1..=2 {
            let payload = CreateVmRequest::legacy_test_config(
                GuestRuntimeKind::JavaScript,
                Default::default(),
                Default::default(),
                None,
            );
            let request = RequestFrame::new(
                request_id,
                OwnershipScope::session("identity-connection", "identity-session"),
                RequestPayload::CreateVm(payload.clone()),
            );
            let prepared = sidecar
                .prepare_create_vm(&request, payload)
                .expect("prepare VM");
            let mut completed = block_on(prepared.execute()).expect("create VM");
            let vm = &mut completed.vm;
            vm.kernel
                .register_driver_for_operator(
                    agentos_vm_kernel::command_registry::CommandDriver::new(
                        "identity-test",
                        ["identity-test"],
                    ),
                )
                .expect("register test driver");
            let process = vm
                .kernel
                .spawn_process(
                    "identity-test",
                    Vec::new(),
                    agentos_vm_kernel::kernel::SpawnOptions {
                        requester_driver: Some("identity-test".into()),
                        ..Default::default()
                    },
                )
                .expect("spawn kernel process");
            assert_ne!(vm.generation, 0);
            assert_eq!(
                process.runtime_identity().generation,
                vm.generation,
                "host reply identity must match the admitted VM generation"
            );
            process.finish(0);
        }
    }

    #[test]
    fn create_and_dispose_release_central_state_during_owned_work() {
        let mut sidecar = VmManager::new(LocalBridge::default()).expect("test sidecar");
        let connection_id = String::from("connection-owned-lifecycle");
        let session_id = String::from("session-owned-lifecycle");
        sidecar.connections.insert(
            connection_id.clone(),
            ConnectionState {
                auth_token: String::new(),
                sessions: BTreeSet::from([session_id.clone()]),
            },
        );
        sidecar.sessions.insert(
            session_id.clone(),
            SessionState {
                connection_id: connection_id.clone(),
                placement: crate::protocol::SidecarPlacement::SidecarPlacementShared(
                    crate::protocol::SidecarPlacementShared { pool: None },
                ),
                metadata: BTreeMap::new(),
                vm_ids: BTreeSet::new(),
            },
        );

        let sqlite_dir = tempfile::tempdir().expect("VM SQLite test directory");
        let mut create_payload = CreateVmRequest::legacy_test_config(
            GuestRuntimeKind::JavaScript,
            Default::default(),
            Default::default(),
            None,
        );
        let mut create_config: agentos_vm_config::CreateVmConfig =
            serde_json::from_str(&create_payload.config).expect("decode VM creation config");
        create_config.database = Some(agentos_vm_config::VmSqliteDescriptor::SqliteFile {
            path: sqlite_dir
                .path()
                .join("vm.sqlite")
                .to_string_lossy()
                .into_owned(),
        });
        create_payload.config =
            serde_json::to_string(&create_config).expect("encode VM creation config");
        let create_request = RequestFrame::new(
            100,
            OwnershipScope::session(&connection_id, &session_id),
            RequestPayload::CreateVm(create_payload.clone()),
        );
        let prepared = sidecar
            .prepare_create_vm(&create_request, create_payload.clone())
            .expect("prepare owned VM create");
        assert!(sidecar.vms.is_empty());
        sidecar.next_sidecar_request_id = -41;
        let completed = block_on(prepared.execute()).expect("execute owned VM create");
        assert!(sidecar.vms.is_empty());
        assert_eq!(sidecar.next_sidecar_request_id, -41);
        sidecar
            .complete_create_vm(completed)
            .expect("publish completed VM create");
        let vm_id = sidecar
            .sessions
            .get(&session_id)
            .and_then(|session| session.vm_ids.iter().next())
            .cloned()
            .expect("created VM session membership");
        assert!(sidecar.vms.contains_key(&vm_id));
        check_database_admission_during_teardown(&mut sidecar.vms.get_mut(&vm_id).unwrap());

        let dispose_payload = DisposeVmRequest {
            reason: DisposeReason::Requested,
        };
        let dispose_request = RequestFrame::new(
            101,
            OwnershipScope::vm(&connection_id, &session_id, &vm_id),
            RequestPayload::DisposeVm(dispose_payload.clone()),
        );
        let plan = sidecar
            .prepare_dispose_vm(&dispose_request, dispose_payload)
            .expect("prepare owned VM dispose");
        let prepared = sidecar
            .detach_vm_for_disposal(plan)
            .expect("detach VM after operation drain");
        assert!(!sidecar.vms.contains_key(&vm_id));
        assert!(sidecar
            .sessions
            .get(&session_id)
            .is_some_and(|session| session.vm_ids.contains(&vm_id)));
        sidecar.next_sidecar_request_id = -42;
        let completed = block_on(prepared.execute());
        assert_eq!(sidecar.next_sidecar_request_id, -42);
        sidecar
            .complete_dispose_vm(completed)
            .expect("finalize completed VM dispose");
        assert!(sidecar
            .sessions
            .get(&session_id)
            .is_some_and(|session| !session.vm_ids.contains(&vm_id)));

        // The internal disposal path has separate teardown code and must also
        // close SQLite before closing VM-scoped blocking-job admission.
        let create_request = RequestFrame::new(
            102,
            OwnershipScope::session(&connection_id, &session_id),
            RequestPayload::CreateVm(create_payload.clone()),
        );
        let prepared = sidecar
            .prepare_create_vm(&create_request, create_payload)
            .expect("prepare internally disposed VM");
        let completed = block_on(prepared.execute()).expect("execute internally disposed VM");
        sidecar
            .complete_create_vm(completed)
            .expect("publish internally disposed VM");
        let vm_id = sidecar
            .sessions
            .get(&session_id)
            .and_then(|session| session.vm_ids.iter().next())
            .cloned()
            .expect("internally disposed VM session membership");
        check_database_admission_during_teardown(&mut sidecar.vms.get_mut(&vm_id).unwrap());
        block_on(sidecar.dispose_vm_internal(
            &connection_id,
            &session_id,
            &vm_id,
            DisposeReason::Requested,
        ))
        .expect("dispose internally owned VM with local SQLite");
        assert!(sidecar
            .sessions
            .get(&session_id)
            .is_some_and(|session| !session.vm_ids.contains(&vm_id)));

        // A stalled close must return a typed timeout and quarantine the VM
        // instead of holding the disposal request indefinitely.
        for internal_disposal in [false, true] {
            let create_payload = CreateVmRequest::legacy_test_config(
                GuestRuntimeKind::JavaScript,
                Default::default(),
                Default::default(),
                None,
            );
            let create_request = RequestFrame::new(
                103,
                OwnershipScope::session(&connection_id, &session_id),
                RequestPayload::CreateVm(create_payload.clone()),
            );
            let prepared = sidecar
                .prepare_create_vm(&create_request, create_payload)
                .expect("prepare VM with pending SQLite close");
            let completed = block_on(prepared.execute()).expect("execute pending-close VM create");
            sidecar
                .complete_create_vm(completed)
                .expect("publish pending-close VM");
            let vm_id = sidecar
                .sessions
                .get(&session_id)
                .and_then(|session| session.vm_ids.iter().next())
                .cloned()
                .expect("pending-close VM session membership");
            let close_started = Arc::new(AtomicBool::new(false));
            let generation = {
                let mut vm = sidecar.vms.get_mut(&vm_id).expect("pending-close VM");
                vm.limits.reactor.shutdown_deadline_ms = 10;
                vm.database = Some(Arc::new(PendingCloseDatabase {
                    close_started: Arc::clone(&close_started),
                }));
                vm.generation
            };
            let error = if internal_disposal {
                block_on(sidecar.dispose_vm_internal(
                    &connection_id,
                    &session_id,
                    &vm_id,
                    DisposeReason::Requested,
                ))
                .expect_err("stalled SQLite close must fail internal disposal")
            } else {
                let dispose_payload = DisposeVmRequest {
                    reason: DisposeReason::Requested,
                };
                let dispose_request = RequestFrame::new(
                    104,
                    OwnershipScope::vm(&connection_id, &session_id, &vm_id),
                    RequestPayload::DisposeVm(dispose_payload.clone()),
                );
                let plan = sidecar
                    .prepare_dispose_vm(&dispose_request, dispose_payload)
                    .expect("prepare pending-close VM disposal");
                let prepared = sidecar
                    .detach_vm_for_disposal(plan)
                    .expect("detach pending-close VM");
                let completed = block_on(prepared.execute());
                sidecar
                    .complete_dispose_vm(completed)
                    .expect_err("stalled SQLite close must fail disposal")
            };
            assert!(close_started.load(Ordering::Acquire));
            assert!(matches!(error, crate::VmError::VmTeardownDeadline { .. }));
            assert!(sidecar.quarantined_vms.contains_key(&generation));
            let quarantined = sidecar.quarantined_vms.get(&generation).unwrap();
            assert!(quarantined.sqlite_close_unconfirmed);
            assert!(quarantined.reconciliation_snapshot().ledger_zero);
            sidecar.reap_reconciled_quarantined_vms();
            assert!(
                sidecar.quarantined_vms.contains_key(&generation),
                "local resource counts do not prove canceled SQLite close completion"
            );
        }
    }

    fn active_vm_metric(sidecar: &VmManager<LocalBridge>) -> usize {
        sidecar
            .runtime_context
            .as_ref()
            .expect("process runtime context")
            .metrics()
            .snapshot()
            .resources[ResourceMetricClass::ActiveVms.index()]
        .current
    }

    #[test]
    fn teardown_closes_capabilities_before_executor_admission() {
        let (_resources, runtime_context, capabilities) = reconciliation_handles(70_001);
        let stale_runtime_context = runtime_context.clone();
        close_vm_capability_admission(&capabilities).expect("close VM capability admission");
        let error = capabilities
            .reserve(CapabilityKind::UdpSocket)
            .expect_err("closed VM generation must reject new capabilities");
        assert!(error
            .to_string()
            .contains("ERR_AGENTOS_CAPABILITY_REGISTRY_CLOSED"));
        assert!(runtime_context.admission_is_open());
        assert_eq!(
            block_on(runtime_context.blocking().run(1, || 42)).unwrap(),
            42,
            "trusted SQLite close must still be able to schedule blocking work"
        );
        runtime_context.close_admission();
        let task_error = stale_runtime_context
            .spawn(TaskClass::Vm, async {})
            .expect_err("stale VM runtime clone must reject new executor work");
        assert!(task_error
            .to_string()
            .contains("ERR_AGENTOS_TASK_ADMISSION_CLOSED"));
    }

    #[test]
    fn teardown_fairness_retirement_survives_generation_churn_past_max_vms() {
        let process = TokioDriver::process(&agentos_driver_tokio::DriverConfig::default())
            .expect("initialize process runtime")
            .handle();

        block_on(async {
            let mut first_generation = None;
            for _ in 0..=4_096 {
                let generation = process
                    .allocate_vm_generation()
                    .expect("allocate churn VM generation");
                first_generation.get_or_insert(generation);
                let turn = process
                    .fairness()
                    .acquire(generation, 1, FairBudget::new(1, 1))
                    .await
                    .expect("acquire churn fairness turn");
                turn.complete(FairBudget::new(1, 1), false)
                    .expect("complete churn fairness turn");
                retire_vm_fairness(&process, generation)
                    .expect("teardown must retire churn VM fairness membership");
            }

            let first_generation = first_generation.expect("at least one churn generation");
            let error = process
                .fairness()
                .acquire(first_generation, 2, FairBudget::new(1, 1))
                .await
                .expect_err("retired VM generation must not re-enroll");
            assert!(
                error
                    .to_string()
                    .contains("ERR_AGENTOS_FAIRNESS_CAPABILITY_RETIRED"),
                "{error}"
            );

            let successor = process
                .allocate_vm_generation()
                .expect("allocate post-churn VM generation");
            let turn = process
                .fairness()
                .acquire(successor, 1, FairBudget::new(1, 1))
                .await
                .expect("retirement must reclaim maxVms membership");
            turn.complete(FairBudget::new(1, 1), false)
                .expect("complete post-churn fairness turn");
            retire_vm_fairness(&process, successor)
                .expect("retire post-churn VM fairness membership");
        });
    }

    #[test]
    fn vm_executor_limits_must_fit_process_executor_limits() {
        let limits = crate::limits::VmLimits::default();
        for (resource, maximum, child_path, process_path) in [
            (
                ResourceClass::ExecutorSlots,
                1,
                "limits.reactor.maxBlockingJobs",
                "runtime.blocking.maxJobs",
            ),
            (
                ResourceClass::ExecutorBytes,
                1,
                "limits.reactor.maxBlockingBytes",
                "runtime.blocking.maxQueuedBytes",
            ),
        ] {
            let process = Arc::new(ResourceLedger::root(
                format!("executor-ceiling-test-{resource:?}"),
                [
                    (resource, ResourceLimit::new(maximum, process_path)),
                    (
                        ResourceClass::WasmMemoryBytes,
                        ResourceLimit::new(
                            1024 * 1024 * 1024,
                            "runtime.resources.maxWasmMemoryBytes",
                        ),
                    ),
                ],
            ));
            let error = vm_resource_ledger("vm-test", 70_005, &limits, process)
                .expect_err("VM executor limit must not exceed its process ceiling");
            let diagnostic = error.to_string();
            assert!(diagnostic.contains(child_path), "{diagnostic}");
            assert!(diagnostic.contains(process_path), "{diagnostic}");
        }
    }

    #[test]
    fn empty_vm_generation_reconciles_without_waiting() {
        let (resources, runtime_context, capabilities) = reconciliation_handles(70_002);
        let (snapshot, deadline_expired) = block_on(wait_for_vm_reconciliation(
            resources.as_ref(),
            &runtime_context,
            &capabilities,
            Duration::ZERO,
        ));
        assert!(!deadline_expired);
        assert_eq!(snapshot.active_tasks, 0);
        assert_eq!(snapshot.outstanding_capabilities, 0);
        assert!(snapshot.ledger_zero);
        assert!(snapshot.integrity_ok);
        assert_eq!(vm_quarantine_reason(false, false, snapshot, false), None);
    }

    #[test]
    fn fairness_retirement_failure_is_a_non_reapable_integrity_quarantine() {
        let generation = 70_007;
        let (resources, runtime_context, capabilities) = reconciliation_handles(generation);
        let snapshot = VmReconciliationSnapshot {
            active_tasks: 0,
            outstanding_capabilities: 0,
            ledger_zero: true,
            integrity_ok: true,
        };
        assert_eq!(
            vm_quarantine_reason(false, true, snapshot, false),
            Some(VmQuarantineReason::FairnessIntegrity)
        );
        let quarantined = QuarantinedVmGeneration {
            connection_id: String::from("conn-test"),
            session_id: String::from("session-test"),
            vm_id: String::from("vm-test"),
            generation,
            resources,
            runtime_context,
            capabilities,
            reason: VmQuarantineReason::FairnessIntegrity,
            sqlite_close_unconfirmed: false,
        };
        assert!(quarantined.reconciliation_snapshot().ledger_zero);
        assert!(!quarantined.can_reap());
    }

    #[test]
    fn integrity_quarantine_is_never_reaped_after_counts_reconcile() {
        let generation = 70_006;
        let (resources, runtime_context, capabilities) = reconciliation_handles(generation);
        let quarantined = QuarantinedVmGeneration {
            connection_id: String::from("conn-test"),
            session_id: String::from("session-test"),
            vm_id: String::from("vm-test"),
            generation,
            resources,
            runtime_context,
            capabilities,
            reason: VmQuarantineReason::ResourceIntegrity,
            sqlite_close_unconfirmed: false,
        };
        assert!(quarantined.reconciliation_snapshot().ledger_zero);
        assert!(!quarantined.can_reap());
    }

    #[test]
    fn stuck_supervised_task_enters_quarantine_until_barrier_releases() {
        block_on(async {
            let generation = 70_003;
            let (resources, runtime_context, capabilities) = reconciliation_handles(generation);
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            let task = runtime_context
                .spawn(TaskClass::Vm, async move {
                    let _ = started_tx.send(());
                    let _ = release_rx.await;
                })
                .expect("spawn supervised VM task");
            started_rx
                .await
                .expect("task reached deterministic barrier");

            let (snapshot, deadline_expired) = wait_for_vm_reconciliation(
                resources.as_ref(),
                &runtime_context,
                &capabilities,
                Duration::ZERO,
            )
            .await;
            assert!(deadline_expired);
            assert_eq!(snapshot.active_tasks, 1);
            assert_eq!(
                vm_quarantine_reason(false, false, snapshot, deadline_expired),
                Some(VmQuarantineReason::TeardownDeadline)
            );

            let quarantined = QuarantinedVmGeneration {
                connection_id: String::from("conn-test"),
                session_id: String::from("session-test"),
                vm_id: String::from("vm-test"),
                generation,
                resources: Arc::clone(&resources),
                runtime_context: runtime_context.clone(),
                capabilities: capabilities.clone(),
                reason: VmQuarantineReason::TeardownDeadline,
                sqlite_close_unconfirmed: false,
            };
            assert!(!quarantined.can_reap());

            release_tx.send(()).expect("release task barrier");
            task.await.expect("supervised task joins");
            let (snapshot, deadline_expired) = wait_for_vm_reconciliation(
                resources.as_ref(),
                &runtime_context,
                &capabilities,
                Duration::ZERO,
            )
            .await;
            assert!(!deadline_expired);
            assert!(snapshot.ledger_zero);
            assert!(quarantined.can_reap());
        });
    }

    #[test]
    fn quarantined_generation_is_not_reused_by_successor() {
        let mut sidecar = VmManager::new(LocalBridge::default()).expect("test sidecar");
        sidecar.observe_active_vm_generations();
        let baseline = active_vm_metric(&sidecar);
        let (quarantined_vm_id, generation) =
            sidecar.allocate_vm_identity().expect("allocate generation");
        let (resources, runtime_context, capabilities) = reconciliation_handles(generation);
        let held = resources
            .reserve(ResourceClass::Tasks, 1)
            .expect("hold quarantine accounting open");
        sidecar
            .retain_quarantined_vm(QuarantinedVmGeneration {
                connection_id: String::from("conn-test"),
                session_id: String::from("session-test"),
                vm_id: quarantined_vm_id.clone(),
                generation,
                resources: Arc::clone(&resources),
                runtime_context,
                capabilities,
                reason: VmQuarantineReason::TeardownDeadline,
                sqlite_close_unconfirmed: false,
            })
            .expect("retain quarantined generation");
        assert_eq!(active_vm_metric(&sidecar), baseline + 1);
        sidecar.connections.insert(
            String::from("conn-test"),
            ConnectionState {
                auth_token: String::new(),
                sessions: BTreeSet::from([String::from("session-test")]),
            },
        );
        sidecar.sessions.insert(
            String::from("session-test"),
            SessionState {
                connection_id: String::from("conn-test"),
                placement: crate::protocol::SidecarPlacement::SidecarPlacementShared(
                    crate::protocol::SidecarPlacementShared { pool: None },
                ),
                metadata: BTreeMap::new(),
                vm_ids: BTreeSet::new(),
            },
        );
        let rejected = sidecar
            .require_owned_vm("conn-test", "session-test", &quarantined_vm_id)
            .expect_err("quarantined generation must reject work");
        assert!(rejected.to_string().contains("ERR_AGENTOS_VM_QUARANTINED"));

        let (successor_id, successor_generation) =
            sidecar.allocate_vm_identity().expect("allocate successor");
        assert!(successor_generation > generation);
        assert_ne!(successor_id, quarantined_vm_id);
        assert!(sidecar.quarantined_vms.contains_key(&generation));

        drop(held);
        sidecar.reap_reconciled_quarantined_vms();
        assert!(!sidecar.quarantined_vms.contains_key(&generation));
        assert_eq!(active_vm_metric(&sidecar), baseline);
    }

    #[test]
    fn vm_unix_socket_host_directories_are_private_and_unique() {
        let first = create_vm_unix_socket_host_dir()
            .expect("first private Unix socket namespace should be created");
        let second = create_vm_unix_socket_host_dir()
            .expect("second private Unix socket namespace should be created");

        assert_ne!(first, second, "VMs must not share a Unix socket namespace");
        for path in [&first, &second] {
            let mode = fs::metadata(path)
                .expect("private Unix socket namespace metadata should be readable")
                .permissions()
                .mode()
                & 0o7777;
            assert_eq!(mode, 0o700, "private Unix socket namespace must be 0700");
            fs::remove_dir(path).expect("private Unix socket namespace should be removable");
            assert!(
                !path.exists(),
                "removed Unix socket namespace must stay absent"
            );
        }
    }
    #[test]
    fn native_root_config_opens_chunked_local_as_persistent_root() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let database_path =
            std::env::temp_dir().join(format!("agentos-native-root-{unique}.sqlite"));
        let block_root = std::env::temp_dir().join(format!("agentos-native-root-blocks-{unique}"));
        let native_root =
            native_root_plugin_from_config(Some(&agentos_vm_config::NativeRootFilesystemConfig {
                plugin: agentos_vm_config::MountPluginDescriptor {
                    id: "chunked_local".to_string(),
                    config: serde_json::json!({
                        "metadataPath": database_path.to_string_lossy(),
                        "blockRoot": block_root.to_string_lossy(),
                    }),
                },
                read_only: false,
            }))
            .expect("native root config should parse")
            .expect("native root should be present");
        let config: serde_json::Value =
            serde_json::from_str(&native_root.plugin.config).expect("valid plugin config");
        let sidecar = VmManager::new(LocalBridge::default()).expect("test sidecar");
        let mount_context = MountPluginContext {
            bridge: sidecar.bridge.clone(),
            runtime_context: sidecar
                .runtime_context
                .clone()
                .expect("test sidecar runtime context"),
            connection_id: String::from("connection-test"),
            session_id: String::from("session-test"),
            vm_id: String::from("vm-test"),
            sidecar_requests: sidecar.sidecar_requests.clone(),
            database: None,
            max_pread_bytes: None,
        };
        let plugin = ChunkedLocalMountPlugin;
        let mut filesystem = plugin
            .open(OpenFileSystemPluginRequest {
                vm_id: "vm-test",
                guest_path: "/",
                read_only: false,
                config: &config,
                context: &mount_context,
            })
            .expect("sqlite root should open");
        bootstrap_native_root_filesystem(
            filesystem.as_mut(),
            &RootFilesystemDescriptor {
                bootstrap_entries: vec![
                    RootFilesystemEntry {
                        path: "/etc/agentos/boot.txt".to_string(),
                        kind: RootFilesystemEntryKind::File,
                        content: Some("booted".to_string()),
                        ..Default::default()
                    },
                    RootFilesystemEntry {
                        path: CA_CERTIFICATES_SYMLINK_PATH.to_string(),
                        kind: RootFilesystemEntryKind::File,
                        content: Some("custom native cert.pem\n".to_string()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        )
        .expect("native root should bootstrap");

        let mut mount_table = MountTable::new_boxed_root(
            filesystem,
            MountOptions::new(native_root.plugin.id.clone()),
        );
        let home = mount_table.stat("/home/agentos").expect("stat guest home");
        assert_eq!(home.mode & 0o7777, 0o2755);
        assert_eq!((home.uid, home.gid), (1000, 1000));
        let workspace = mount_table.stat("/workspace").expect("stat workspace");
        assert_eq!(workspace.mode & 0o7777, 0o755);
        assert_eq!((workspace.uid, workspace.gid), (1000, 1000));
        let root_home = mount_table.stat("/root").expect("stat root home");
        assert_eq!(root_home.mode & 0o7777, 0o711);
        assert_eq!((root_home.uid, root_home.gid), (0, 0));
        assert_eq!(
            mount_table
                .read_file("/etc/agentos/boot.txt")
                .expect("bootstrap file should be readable"),
            b"booted".to_vec()
        );
        assert_eq!(
            mount_table
                .read_file(CA_CERTIFICATES_GUEST_PATH)
                .expect("default CA bundle should be readable from native root"),
            CA_CERTIFICATES_BUNDLE
        );
        assert_eq!(
            mount_table
                .read_file(CA_CERTIFICATES_SYMLINK_PATH)
                .expect("custom regular cert.pem should replace the default symlink"),
            b"custom native cert.pem\n".to_vec()
        );
        assert!(
            !mount_table
                .lstat(CA_CERTIFICATES_SYMLINK_PATH)
                .expect("lstat custom native cert.pem")
                .is_symbolic_link
        );
        mount_table
            .write_file("/home/agentos/persist.txt", b"persisted".to_vec())
            .expect("write through sqlite root should succeed");
        let mut kernel_config = KernelVmConfig::new("vm-test");
        kernel_config.permissions = Permissions::allow_all();
        let mut kernel = KernelVm::new(mount_table, kernel_config);
        kernel
            .write_file("/bin/python", KERNEL_COMMAND_STUB.to_vec())
            .expect("command stub should be writable");
        prune_kernel_command_stub(&mut kernel, "/bin/python")
            .expect("command stub prune should support native roots");
        assert!(
            !kernel.exists("/bin/python").expect("exists should succeed"),
            "stub should be pruned through the mounted root"
        );
        drop(kernel);

        let reopened = plugin
            .open(OpenFileSystemPluginRequest {
                vm_id: "vm-test",
                guest_path: "/",
                read_only: false,
                config: &config,
                context: &mount_context,
            })
            .expect("chunked local root should reopen");
        let mut reopened = MountTable::new_boxed_root(reopened, MountOptions::new("chunked_local"));
        assert_eq!(
            reopened
                .read_file("/home/agentos/persist.txt")
                .expect("persisted file should survive reopen"),
            b"persisted".to_vec()
        );

        let _ = fs::remove_file(database_path);
        let _ = fs::remove_dir_all(block_root);
    }
}
