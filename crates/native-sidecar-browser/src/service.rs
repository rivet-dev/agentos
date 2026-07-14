use crate::package_projection::{
    normalize_packages_mount_root, prepare_aospkg_bytes, BrowserPackageProjectionError,
    BrowserProjectedPackage, PreparedBrowserPackage, PreparedBrowserPackageMount,
    DEFAULT_BROWSER_PACKAGES_MOUNT_ROOT, MAX_BROWSER_PROJECTED_PACKAGES_PER_VM,
    MAX_BROWSER_PROJECTED_PACKAGE_BYTES_PER_VM, MAX_BROWSER_PROJECTED_PACKAGE_ENTRIES_PER_VM,
    MAX_BROWSER_PROJECTED_PACKAGE_MATERIALIZED_BYTES_PER_VM,
    MAX_BROWSER_PROJECTED_PACKAGE_MOUNTS_PER_VM,
};
use crate::{
    BrowserSidecarBridge, BrowserWorkerEntrypoint, BrowserWorkerHandle, BrowserWorkerHandleRequest,
    BrowserWorkerOsConfig, BrowserWorkerProcessConfig, BrowserWorkerSpawnRequest,
};
use agentos_bridge::{
    BridgeTypes, CreateJavascriptContextRequest, CreateWasmContextRequest, ExecutionEvent,
    ExecutionHandleRequest, ExecutionSignal, GuestContextHandle, GuestRuntime,
    KillExecutionRequest, LifecycleEventRecord, LifecycleState, PollExecutionEventRequest,
    SignalDispositionAction, SignalHandlerRegistration, StartExecutionRequest, StartedExecution,
    StructuredEventRecord, WriteExecutionStdinRequest,
};
use agentos_kernel::bridge::LifecycleState as KernelLifecycleState;
use agentos_kernel::kernel::{KernelError, KernelVm, KernelVmConfig, VirtualProcessOptions};
use agentos_kernel::mount_table::{MountOptions, MountTable};
use agentos_kernel::poll::{PollTargetEntry, POLLERR, POLLHUP, POLLIN};
use agentos_kernel::process_table::ProcessStatus;
use agentos_kernel::root_fs::{
    RootFilesystemMode as KernelRootFilesystemMode, RootFilesystemSnapshot,
};
use agentos_kernel::socket_table::{
    InetSocketAddress, SocketId, SocketSpec, SocketState, SocketType,
};
use agentos_native_sidecar_core::{
    apply_process_signal_state_update, build_root_mount_table,
    ensure_vm_fetch_raw_response_buffer_within_limit, ensure_vm_fetch_response_within_limit,
    handle_guest_filesystem_call, handle_guest_kernel_call, parse_kernel_http_fetch_response,
    process_snapshot_entry_from_kernel, serialize_kernel_http_fetch_request,
    unsupported_guest_kernel_call_detail, SharedProcessSnapshotEntry, VmLayerStore,
    VM_FETCH_BUFFER_LIMIT_BYTES,
};
use agentos_native_sidecar_core::{
    ensure_toolkit_name_available, ensure_toolkit_registry_capacity, registered_tool_command_names,
    shared_guest_runtime_identity, validate_toolkit_registration,
};
use agentos_sidecar_protocol::protocol::{
    FindBoundUdpRequest, FindListenerRequest, GuestFilesystemCallRequest,
    GuestFilesystemResultResponse, GuestKernelCallRequest, GuestKernelResultResponse,
    RegisterHostCallbacksRequest, RootFilesystemEntry,
    SignalDispositionAction as ProtocolSignalDispositionAction,
    SignalHandlerRegistration as ProtocolSignalHandlerRegistration, SocketStateEntry,
    WasmPermissionTier,
};
use agentos_vm_config::RootFilesystemConfig;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::time::{Duration, Instant};

type BridgeError<B> = <B as BridgeTypes>::Error;
type BrowserKernel = KernelVm<MountTable>;
const BROWSER_WORKER_DRIVER: &str = "browser.worker";
const BROWSER_VM_FETCH_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_MAX_DEFERRED_EXECUTION_EVENTS_PER_VM: usize = 256;
pub const DEFAULT_MAX_PENDING_EXECUTION_CLEANUPS_PER_VM: usize = 256;
pub const DEFAULT_MAX_VMS: usize = 1_024;
pub const DEFAULT_MAX_SESSIONS: usize = 4_096;
pub const MAX_BROWSER_PROJECTED_AGENTS_PER_VM: usize = 4_096;
const DEFERRED_EXECUTION_EVENTS_LIMIT: &str = "max_deferred_execution_events_per_vm";
const PENDING_EXECUTION_CLEANUPS_LIMIT: &str = "max_pending_execution_cleanups_per_vm";
const EXTENSION_EVENTS_LIMIT: &str = "available_extension_event_slots";
const DEFAULT_EXTENSION_EVENT_CAPACITY: usize = 256;
#[cfg(not(target_arch = "wasm32"))]
const BROWSER_VM_FETCH_TIMEOUT_MS_ENV: &str = "AGENTOS_TEST_BROWSER_VM_FETCH_TIMEOUT_MS";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserSidecarConfig {
    pub sidecar_id: String,
    pub max_sessions_per_connection: usize,
    /// Global bound including closing sessions retained for cleanup retry.
    pub max_sessions: usize,
    /// Bound for events temporarily retained while an extension polls output for
    /// one execution from the bridge's VM-global event stream.
    pub max_deferred_execution_events_per_vm: usize,
    /// Bound on active execution reservations plus retained, non-routable
    /// cleanup handles for one VM.
    pub max_pending_execution_cleanups_per_vm: usize,
    /// Bound on active VMs plus their reserved non-routable cleanup state.
    pub max_vms: usize,
}

impl Default for BrowserSidecarConfig {
    fn default() -> Self {
        Self {
            sidecar_id: String::from("agentos-native-sidecar-browser"),
            max_sessions_per_connection:
                agentos_native_sidecar_core::DEFAULT_MAX_SESSIONS_PER_CONNECTION,
            max_sessions: DEFAULT_MAX_SESSIONS,
            max_deferred_execution_events_per_vm: DEFAULT_MAX_DEFERRED_EXECUTION_EVENTS_PER_VM,
            max_pending_execution_cleanups_per_vm: DEFAULT_MAX_PENDING_EXECUTION_CLEANUPS_PER_VM,
            max_vms: DEFAULT_MAX_VMS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserSidecarError {
    InvalidState(String),
    InvalidPackage(String),
    PackageConflict(String),
    PackageMount(String),
    PackageStateCorrupt(String),
    Kernel(String),
    Bridge(String),
    Context {
        context: String,
        source: Box<BrowserSidecarError>,
    },
    Cleanup {
        context: &'static str,
        errors: Vec<BrowserSidecarError>,
    },
    LimitExceeded {
        limit: &'static str,
        capacity: usize,
        how_to_raise: &'static str,
    },
}

impl fmt::Display for BrowserSidecarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState(message)
            | Self::InvalidPackage(message)
            | Self::PackageConflict(message)
            | Self::PackageMount(message)
            | Self::PackageStateCorrupt(message)
            | Self::Kernel(message)
            | Self::Bridge(message) => f.write_str(message),
            Self::Cleanup { context, errors } => {
                write!(f, "{context}")?;
                for (index, error) in errors.iter().enumerate() {
                    write!(f, "; cleanup error {}: {error}", index + 1)?;
                }
                Ok(())
            }
            Self::Context { context, source } => write!(f, "{context}: {source}"),
            Self::LimitExceeded {
                limit,
                capacity,
                how_to_raise,
            } => write!(
                f,
                "browser sidecar limit {limit} reached at configured capacity {capacity}; {how_to_raise}"
            ),
        }
    }
}

impl Error for BrowserSidecarError {}

struct VmState {
    kernel: BrowserKernel,
    guest_cwd: String,
    agent_additional_instructions: Option<String>,
    projected_agent_launch: BTreeMap<String, BrowserProjectedAgentLaunch>,
    projected_package_names: BTreeSet<String>,
    projected_command_names: BTreeSet<String>,
    projected_package_bytes: usize,
    projected_package_entries: usize,
    projected_package_materialized_bytes: usize,
    projected_package_sources: Vec<Vec<u8>>,
    projected_packages: Vec<BrowserProjectedPackage>,
    projected_package_mount_paths: Vec<String>,
    projected_package_mount_root: String,
    projected_package_env: BTreeMap<String, String>,
    provided_commands: BTreeMap<String, Vec<String>>,
    configuration: BrowserVmConfiguration,
    layers: VmLayerStore,
    toolkits: BTreeMap<String, RegisterHostCallbacksRequest>,
    signal_states: BTreeMap<String, BTreeMap<u32, ProtocolSignalHandlerRegistration>>,
    contexts: BTreeSet<String>,
    active_executions: BTreeSet<String>,
    deferred_execution_events: VecDeque<ExecutionEvent>,
    deferred_execution_events_warned: bool,
    pending_execution_cleanups_warned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollExecutionOutputRequest {
    pub vm_id: String,
    pub execution_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionOutput {
    Stdout(agentos_bridge::OutputChunk),
    Stderr(agentos_bridge::OutputChunk),
    Exited(agentos_bridge::ExecutionExited),
}

/// One agent launch surface retained in browser sidecar VM state.
///
/// The catalog is populated only from trusted `.aospkg` bytes decoded by the
/// sidecar. New VMs start empty and never derive metadata from guest files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserProjectedAgentLaunch {
    pub id: String,
    pub adapter_entrypoint: String,
    pub env: BTreeMap<String, String>,
    pub launch_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserProvidedCommands {
    pub package_name: String,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BrowserVmConfiguration {
    command_permissions: BTreeMap<String, WasmPermissionTier>,
    create_loopback_exempt_ports: BTreeSet<u16>,
    loopback_exempt_ports: Vec<u16>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserExecutionOptions {
    pub command_name: Option<String>,
    pub wasm_permission_tier: Option<WasmPermissionTier>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextState {
    vm_id: String,
    runtime: GuestRuntime,
    entrypoint: BrowserWorkerEntrypoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExecutionState {
    vm_id: String,
    context_id: String,
    worker: BrowserWorkerHandle,
    kernel_pid: u32,
    stdin_write_fd: u32,
    cwd: String,
}

#[derive(Debug, Clone)]
struct ExecutionCleanupState {
    execution: ExecutionState,
    event_name: &'static str,
    kernel_reaped: bool,
    worker_terminated: bool,
    structured_event_emitted: bool,
    lifecycle_event_emitted: bool,
}

#[derive(Debug, Clone)]
struct StartupCleanupState {
    vm_id: String,
    execution_id: Option<String>,
    kernel_pid: u32,
    kernel_reaped: bool,
    bridge_killed: bool,
}

pub trait BrowserExtension: Send + Sync {
    fn namespace(&self) -> &str;

    fn handle_request(
        &self,
        context: &mut BrowserExtensionContext<'_>,
        payload: &[u8],
    ) -> Result<Vec<u8>, BrowserSidecarError> {
        let _ = context;
        let _ = payload;
        Err(BrowserSidecarError::InvalidState(format!(
            "browser extension {} does not handle requests",
            self.namespace()
        )))
    }

    fn on_session_disposed(
        &self,
        _context: &mut BrowserExtensionContext<'_>,
        _connection_id: &str,
        _session_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        Ok(())
    }

    fn on_vm_disposed(
        &self,
        _context: &mut BrowserExtensionContext<'_>,
        _connection_id: &str,
        _session_id: &str,
        _vm_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserExtensionRequest {
    pub namespace: String,
    pub payload: Vec<u8>,
    /// VM the request is scoped to (from the wire ownership scope), so extensions
    /// that drive guest executions know which VM to target. `None` for connection/
    /// session-scoped requests that carry no VM.
    pub vm_id: Option<String>,
    /// Owning connection (from the wire ownership scope), for per-connection
    /// ownership enforcement inside extensions.
    pub connection_id: Option<String>,
    /// Owning wire session, paired with connection + VM for complete extension
    /// isolation. `None` only for connection-scoped extension requests.
    pub wire_session_id: Option<String>,
    /// Slots currently available in the wire dispatcher's bounded event queue.
    pub event_capacity: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserExtensionResponse {
    pub namespace: String,
    pub payload: Vec<u8>,
    /// Namespaced extension events produced synchronously by this request. The
    /// wire dispatcher applies the request's exact ownership before delivery.
    pub events: Vec<Vec<u8>>,
}

pub trait BrowserExtensionHost {
    fn resolve_projected_agent(
        &mut self,
        vm_id: &str,
        id: &str,
    ) -> Result<Option<BrowserProjectedAgentLaunch>, BrowserSidecarError>;

    fn list_projected_agents(
        &mut self,
        vm_id: &str,
    ) -> Result<Vec<BrowserProjectedAgentLaunch>, BrowserSidecarError>;

    fn agent_additional_instructions(
        &mut self,
        vm_id: &str,
    ) -> Result<Option<String>, BrowserSidecarError>;

    fn registered_host_tool_reference(
        &mut self,
        vm_id: &str,
    ) -> Result<String, BrowserSidecarError>;

    fn write_file(
        &mut self,
        vm_id: &str,
        path: &str,
        contents: Vec<u8>,
    ) -> Result<(), BrowserSidecarError>;

    fn read_file(&mut self, vm_id: &str, path: &str) -> Result<Vec<u8>, BrowserSidecarError>;

    fn mkdir(
        &mut self,
        vm_id: &str,
        path: &str,
        recursive: bool,
    ) -> Result<(), BrowserSidecarError>;

    fn read_dir(&mut self, vm_id: &str, path: &str) -> Result<Vec<String>, BrowserSidecarError>;

    fn create_javascript_context(
        &mut self,
        request: CreateJavascriptContextRequest,
    ) -> Result<GuestContextHandle, BrowserSidecarError>;

    fn create_wasm_context(
        &mut self,
        request: CreateWasmContextRequest,
    ) -> Result<GuestContextHandle, BrowserSidecarError>;

    fn start_execution(
        &mut self,
        request: StartExecutionRequest,
    ) -> Result<StartedExecution, BrowserSidecarError>;

    fn release_context(&mut self, vm_id: &str, context_id: &str)
        -> Result<(), BrowserSidecarError>;

    fn write_stdin(
        &mut self,
        request: WriteExecutionStdinRequest,
    ) -> Result<(), BrowserSidecarError>;

    fn close_stdin(&mut self, request: ExecutionHandleRequest) -> Result<(), BrowserSidecarError>;

    fn kill_execution(&mut self, request: KillExecutionRequest) -> Result<(), BrowserSidecarError>;

    fn release_execution(&mut self, execution_id: &str) -> Result<(), BrowserSidecarError>;

    fn abort_execution(
        &mut self,
        vm_id: &str,
        execution_id: &str,
    ) -> Result<(), BrowserSidecarError>;

    fn poll_execution_event(
        &mut self,
        request: PollExecutionEventRequest,
    ) -> Result<Option<ExecutionEvent>, BrowserSidecarError>;

    /// Poll output for one execution without consuming events owned by another
    /// execution or by the central guest-request/signal event loop.
    fn poll_execution_output(
        &mut self,
        request: PollExecutionOutputRequest,
    ) -> Result<Option<ExecutionOutput>, BrowserSidecarError>;
}

pub struct BrowserExtensionContext<'a> {
    host: &'a mut dyn BrowserExtensionHost,
    vm_id: Option<String>,
    connection_id: Option<String>,
    wire_session_id: Option<String>,
    events: Vec<Vec<u8>>,
    event_capacity: usize,
}

impl<'a> BrowserExtensionContext<'a> {
    pub fn new(host: &'a mut dyn BrowserExtensionHost) -> Self {
        Self {
            host,
            vm_id: None,
            connection_id: None,
            wire_session_id: None,
            events: Vec::new(),
            event_capacity: DEFAULT_EXTENSION_EVENT_CAPACITY,
        }
    }

    /// Construct with the wire ownership scope threaded in (VM + connection), so
    /// extensions can target the right VM and enforce per-connection ownership.
    pub fn with_ownership(
        host: &'a mut dyn BrowserExtensionHost,
        vm_id: Option<String>,
        connection_id: Option<String>,
        event_capacity: usize,
    ) -> Self {
        Self::with_full_ownership(host, vm_id, connection_id, None, event_capacity)
    }

    pub fn with_full_ownership(
        host: &'a mut dyn BrowserExtensionHost,
        vm_id: Option<String>,
        connection_id: Option<String>,
        wire_session_id: Option<String>,
        event_capacity: usize,
    ) -> Self {
        Self {
            host,
            vm_id,
            connection_id,
            wire_session_id,
            events: Vec::new(),
            event_capacity,
        }
    }

    /// VM this request is scoped to, if any.
    pub fn vm_id(&self) -> Option<&str> {
        self.vm_id.as_deref()
    }

    /// Owning connection of this request, if any.
    pub fn connection_id(&self) -> Option<&str> {
        self.connection_id.as_deref()
    }

    pub fn wire_session_id(&self) -> Option<&str> {
        self.wire_session_id.as_deref()
    }

    /// Queue one event in this extension's namespace. Event ownership is added
    /// by the wire dispatcher from the request that invoked the extension.
    pub fn emit_event(&mut self, payload: Vec<u8>) -> Result<(), BrowserSidecarError> {
        if self.events.len() >= self.event_capacity {
            return Err(BrowserSidecarError::LimitExceeded {
                limit: EXTENSION_EVENTS_LIMIT,
                capacity: self.event_capacity,
                how_to_raise:
                    "drain events with pollEvent before issuing another extension request",
            });
        }
        self.events.push(payload);
        Ok(())
    }

    pub fn event_capacity(&self) -> usize {
        self.event_capacity
    }

    fn take_events(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.events)
    }

    pub fn resolve_projected_agent(
        &mut self,
        vm_id: &str,
        id: &str,
    ) -> Result<Option<BrowserProjectedAgentLaunch>, BrowserSidecarError> {
        self.host.resolve_projected_agent(vm_id, id)
    }

    pub fn list_projected_agents(
        &mut self,
        vm_id: &str,
    ) -> Result<Vec<BrowserProjectedAgentLaunch>, BrowserSidecarError> {
        self.host.list_projected_agents(vm_id)
    }

    pub fn agent_additional_instructions(
        &mut self,
        vm_id: &str,
    ) -> Result<Option<String>, BrowserSidecarError> {
        self.host.agent_additional_instructions(vm_id)
    }

    pub fn registered_host_tool_reference(
        &mut self,
        vm_id: &str,
    ) -> Result<String, BrowserSidecarError> {
        self.host.registered_host_tool_reference(vm_id)
    }

    pub fn write_file(
        &mut self,
        vm_id: &str,
        path: &str,
        contents: impl Into<Vec<u8>>,
    ) -> Result<(), BrowserSidecarError> {
        self.host.write_file(vm_id, path, contents.into())
    }

    pub fn read_file(&mut self, vm_id: &str, path: &str) -> Result<Vec<u8>, BrowserSidecarError> {
        self.host.read_file(vm_id, path)
    }

    pub fn mkdir(
        &mut self,
        vm_id: &str,
        path: &str,
        recursive: bool,
    ) -> Result<(), BrowserSidecarError> {
        self.host.mkdir(vm_id, path, recursive)
    }

    pub fn read_dir(
        &mut self,
        vm_id: &str,
        path: &str,
    ) -> Result<Vec<String>, BrowserSidecarError> {
        self.host.read_dir(vm_id, path)
    }

    pub fn create_javascript_context(
        &mut self,
        request: CreateJavascriptContextRequest,
    ) -> Result<GuestContextHandle, BrowserSidecarError> {
        self.host.create_javascript_context(request)
    }

    pub fn create_wasm_context(
        &mut self,
        request: CreateWasmContextRequest,
    ) -> Result<GuestContextHandle, BrowserSidecarError> {
        self.host.create_wasm_context(request)
    }

    pub fn start_execution(
        &mut self,
        request: StartExecutionRequest,
    ) -> Result<StartedExecution, BrowserSidecarError> {
        self.host.start_execution(request)
    }

    pub fn release_context(
        &mut self,
        vm_id: &str,
        context_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        self.host.release_context(vm_id, context_id)
    }

    pub fn write_stdin(
        &mut self,
        request: WriteExecutionStdinRequest,
    ) -> Result<(), BrowserSidecarError> {
        self.host.write_stdin(request)
    }

    pub fn close_stdin(
        &mut self,
        request: ExecutionHandleRequest,
    ) -> Result<(), BrowserSidecarError> {
        self.host.close_stdin(request)
    }

    pub fn kill_execution(
        &mut self,
        request: KillExecutionRequest,
    ) -> Result<(), BrowserSidecarError> {
        self.host.kill_execution(request)
    }

    pub fn release_execution(&mut self, execution_id: &str) -> Result<(), BrowserSidecarError> {
        self.host.release_execution(execution_id)
    }

    pub fn abort_execution(
        &mut self,
        vm_id: &str,
        execution_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        self.host.abort_execution(vm_id, execution_id)
    }

    pub fn poll_execution_event(
        &mut self,
        request: PollExecutionEventRequest,
    ) -> Result<Option<ExecutionEvent>, BrowserSidecarError> {
        self.host.poll_execution_event(request)
    }

    pub fn poll_execution_output(
        &mut self,
        request: PollExecutionOutputRequest,
    ) -> Result<Option<ExecutionOutput>, BrowserSidecarError> {
        self.host.poll_execution_output(request)
    }
}

pub struct BrowserSidecar<B> {
    bridge: B,
    config: BrowserSidecarConfig,
    vms: BTreeMap<String, VmState>,
    contexts: BTreeMap<String, ContextState>,
    executions: BTreeMap<String, ExecutionState>,
    execution_cleanups: BTreeMap<String, ExecutionCleanupState>,
    startup_cleanups: BTreeMap<String, StartupCleanupState>,
    disposing_vms: BTreeSet<String>,
    extensions: BTreeMap<String, Box<dyn BrowserExtension>>,
    extension_session_cleanups: BTreeMap<(String, String), BTreeSet<String>>,
    extension_vm_cleanups: BTreeMap<(String, String, String), BTreeSet<String>>,
    #[cfg(test)]
    next_kernel_cleanup_error: Option<BrowserSidecarError>,
}

impl<B> BrowserSidecar<B>
where
    B: BrowserSidecarBridge,
    BridgeError<B>: fmt::Debug,
{
    pub fn new(bridge: B, config: BrowserSidecarConfig) -> Self {
        Self::with_extensions(bridge, config, Vec::new())
            .expect("empty browser extension registry should be valid")
    }

    pub fn with_extensions(
        bridge: B,
        config: BrowserSidecarConfig,
        extensions: Vec<Box<dyn BrowserExtension>>,
    ) -> Result<Self, BrowserSidecarError> {
        let mut sidecar = Self {
            bridge,
            config,
            vms: BTreeMap::new(),
            contexts: BTreeMap::new(),
            executions: BTreeMap::new(),
            execution_cleanups: BTreeMap::new(),
            startup_cleanups: BTreeMap::new(),
            disposing_vms: BTreeSet::new(),
            extensions: BTreeMap::new(),
            extension_session_cleanups: BTreeMap::new(),
            extension_vm_cleanups: BTreeMap::new(),
            #[cfg(test)]
            next_kernel_cleanup_error: None,
        };
        for extension in extensions {
            sidecar.register_extension(extension)?;
        }
        Ok(sidecar)
    }

    pub fn register_extension(
        &mut self,
        extension: Box<dyn BrowserExtension>,
    ) -> Result<(), BrowserSidecarError> {
        let namespace = extension.namespace();
        if namespace.is_empty() {
            return Err(BrowserSidecarError::InvalidState(String::from(
                "browser extension namespace must not be empty",
            )));
        }
        if self.extensions.contains_key(namespace) {
            return Err(BrowserSidecarError::InvalidState(format!(
                "browser extension namespace already registered: {namespace}",
            )));
        }
        self.extensions.insert(namespace.to_string(), extension);
        Ok(())
    }

    pub fn extension_count(&self) -> usize {
        self.extensions.len()
    }

    pub fn has_extension(&self, namespace: &str) -> bool {
        self.extensions.contains_key(namespace)
    }

    pub fn dispatch_extension_request(
        &mut self,
        request: BrowserExtensionRequest,
    ) -> Result<BrowserExtensionResponse, BrowserSidecarError> {
        let Some(extension) = self.extensions.remove(&request.namespace) else {
            return Err(BrowserSidecarError::InvalidState(format!(
                "no browser extension registered for namespace {}",
                request.namespace
            )));
        };
        let (payload, events) = {
            let mut context = BrowserExtensionContext::with_full_ownership(
                self,
                request.vm_id.clone(),
                request.connection_id.clone(),
                request.wire_session_id.clone(),
                request.event_capacity,
            );
            let payload = extension.handle_request(&mut context, &request.payload);
            let events = context.take_events();
            (payload, events)
        };
        self.extensions.insert(request.namespace.clone(), extension);
        let payload = payload?;
        Ok(BrowserExtensionResponse {
            namespace: request.namespace,
            payload,
            events,
        })
    }

    pub fn dispose_extension_session_state(
        &mut self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        let cleanup_key = (connection_id.to_string(), session_id.to_string());
        let mut errors = Vec::new();
        let namespaces = self.extensions.keys().cloned().collect::<Vec<_>>();
        for namespace in namespaces {
            if self
                .extension_session_cleanups
                .get(&cleanup_key)
                .is_some_and(|completed| completed.contains(&namespace))
            {
                continue;
            }
            let extension = self
                .extensions
                .remove(&namespace)
                .expect("extension namespace came from registry");
            let result = {
                let mut context = BrowserExtensionContext::with_full_ownership(
                    self,
                    None,
                    Some(connection_id.to_string()),
                    Some(session_id.to_string()),
                    DEFAULT_EXTENSION_EVENT_CAPACITY,
                );
                extension.on_session_disposed(&mut context, connection_id, session_id)
            };
            self.extensions.insert(namespace.clone(), extension);
            match result {
                Ok(()) => {
                    self.extension_session_cleanups
                        .entry(cleanup_key.clone())
                        .or_default()
                        .insert(namespace);
                }
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(BrowserSidecarError::Cleanup {
                context: "failed to dispose browser extension session state completely",
                errors,
            })
        }
    }

    pub fn dispose_extension_vm_state(
        &mut self,
        connection_id: &str,
        session_id: &str,
        vm_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        let cleanup_key = (
            connection_id.to_string(),
            session_id.to_string(),
            vm_id.to_string(),
        );
        let mut errors = Vec::new();
        let namespaces = self.extensions.keys().cloned().collect::<Vec<_>>();
        for namespace in namespaces {
            if self
                .extension_vm_cleanups
                .get(&cleanup_key)
                .is_some_and(|completed| completed.contains(&namespace))
            {
                continue;
            }
            let extension = self
                .extensions
                .remove(&namespace)
                .expect("extension namespace came from registry");
            let result = {
                let mut context = BrowserExtensionContext::with_full_ownership(
                    self,
                    Some(vm_id.to_string()),
                    Some(connection_id.to_string()),
                    Some(session_id.to_string()),
                    DEFAULT_EXTENSION_EVENT_CAPACITY,
                );
                extension.on_vm_disposed(&mut context, connection_id, session_id, vm_id)
            };
            self.extensions.insert(namespace.clone(), extension);
            match result {
                Ok(()) => {
                    self.extension_vm_cleanups
                        .entry(cleanup_key.clone())
                        .or_default()
                        .insert(namespace);
                }
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(BrowserSidecarError::Cleanup {
                context: "failed to dispose browser extension VM state completely",
                errors,
            })
        }
    }

    pub fn finish_extension_vm_cleanup(
        &mut self,
        connection_id: &str,
        session_id: &str,
        vm_id: &str,
    ) {
        self.extension_vm_cleanups.remove(&(
            connection_id.to_string(),
            session_id.to_string(),
            vm_id.to_string(),
        ));
    }

    pub fn finish_extension_session_cleanup(&mut self, connection_id: &str, session_id: &str) {
        self.extension_session_cleanups
            .remove(&(connection_id.to_string(), session_id.to_string()));
        self.extension_vm_cleanups
            .retain(|(owner_connection, owner_session, _), _| {
                owner_connection != connection_id || owner_session != session_id
            });
    }

    pub fn sidecar_id(&self) -> &str {
        &self.config.sidecar_id
    }

    pub fn bridge(&self) -> &B {
        &self.bridge
    }

    pub fn bridge_mut(&mut self) -> &mut B {
        &mut self.bridge
    }

    pub fn into_bridge(self) -> B {
        self.bridge
    }

    pub fn vm_count(&self) -> usize {
        self.vms.len()
    }

    pub fn active_vm_count(&self) -> usize {
        self.vms
            .keys()
            .filter(|vm_id| !self.disposing_vms.contains(*vm_id))
            .count()
    }

    pub fn pending_vm_cleanup_count(&self) -> usize {
        self.disposing_vms.len()
    }

    pub fn context_count(&self, vm_id: &str) -> usize {
        self.vms
            .get(vm_id)
            .map(|vm| vm.contexts.len())
            .unwrap_or_default()
    }

    pub fn active_worker_count(&self, vm_id: &str) -> usize {
        self.vms
            .get(vm_id)
            .map(|vm| vm.active_executions.len())
            .unwrap_or_default()
    }

    pub fn pending_execution_cleanup_count(&self, vm_id: &str) -> usize {
        self.execution_cleanups
            .values()
            .filter(|cleanup| cleanup.execution.vm_id == vm_id)
            .count()
            + self
                .startup_cleanups
                .values()
                .filter(|cleanup| cleanup.vm_id == vm_id)
                .count()
    }

    pub fn guest_cwd(&self, vm_id: &str) -> Result<String, BrowserSidecarError> {
        Ok(self.vm(vm_id)?.guest_cwd.clone())
    }

    pub fn set_agent_additional_instructions(
        &mut self,
        vm_id: &str,
        instructions: Option<String>,
    ) -> Result<(), BrowserSidecarError> {
        self.vm_mut(vm_id)?.agent_additional_instructions = instructions;
        Ok(())
    }

    pub fn agent_additional_instructions(
        &self,
        vm_id: &str,
    ) -> Result<Option<String>, BrowserSidecarError> {
        Ok(self.vm(vm_id)?.agent_additional_instructions.clone())
    }

    pub fn registered_host_tool_reference(
        &self,
        vm_id: &str,
    ) -> Result<String, BrowserSidecarError> {
        agentos_native_sidecar_core::tools::build_host_tool_reference(&self.vm(vm_id)?.toolkits)
            .map_err(|error| BrowserSidecarError::InvalidState(error.to_string()))
    }

    pub fn resolve_projected_agent(
        &self,
        vm_id: &str,
        id: &str,
    ) -> Result<Option<BrowserProjectedAgentLaunch>, BrowserSidecarError> {
        Ok(self.vm(vm_id)?.projected_agent_launch.get(id).cloned())
    }

    pub fn list_projected_agents(
        &self,
        vm_id: &str,
    ) -> Result<Vec<BrowserProjectedAgentLaunch>, BrowserSidecarError> {
        Ok(self
            .vm(vm_id)?
            .projected_agent_launch
            .values()
            .cloned()
            .collect())
    }

    pub fn provided_commands(
        &self,
        vm_id: &str,
    ) -> Result<Vec<BrowserProvidedCommands>, BrowserSidecarError> {
        Ok(self
            .vm(vm_id)?
            .provided_commands
            .iter()
            .map(|(package_name, commands)| BrowserProvidedCommands {
                package_name: package_name.clone(),
                commands: commands.clone(),
            })
            .collect())
    }

    /// Return the authoritative package projection metadata retained by the
    /// browser sidecar. Package manifests are never supplied by the client and
    /// are not read from mutable guest files.
    pub fn projected_packages(
        &self,
        vm_id: &str,
    ) -> Result<Vec<BrowserProjectedPackage>, BrowserSidecarError> {
        Ok(self.vm(vm_id)?.projected_packages.clone())
    }

    /// Decode and atomically add one complete `.aospkg` supplied by trusted
    /// sidecar transport. The client never supplies parsed package metadata.
    pub fn project_aospkg_bytes(
        &mut self,
        vm_id: &str,
        bytes: Vec<u8>,
    ) -> Result<BrowserProjectedPackage, BrowserSidecarError> {
        let mut projected = self.project_aospkg_batch_bytes(vm_id, vec![bytes])?;
        Ok(projected
            .pop()
            .expect("one staged package must produce one projection"))
    }

    /// Atomically add complete `.aospkg` containers at the VM's current package
    /// root. New VMs begin at `/opt/agentos`; an explicit ConfigureVm root stays
    /// authoritative for later dynamic links.
    pub fn project_aospkg_batch_bytes(
        &mut self,
        vm_id: &str,
        packages: Vec<Vec<u8>>,
    ) -> Result<Vec<BrowserProjectedPackage>, BrowserSidecarError> {
        let vm = self.vm(vm_id)?;
        let mount_root = vm.projected_package_mount_root.clone();
        let mut prepared = Self::prepare_package_batch(
            packages,
            &mount_root,
            vm.projected_package_names.len(),
            vm.projected_package_bytes,
            vm.projected_package_entries,
            vm.projected_package_materialized_bytes,
        )?;
        Self::validate_prepared_package_batch(vm, &prepared, false)?;

        let vm = self.vm_mut(vm_id)?;
        let mounted_paths = match mount_prepared_package_batch(&mut vm.kernel, &mut prepared, false)
        {
            Ok(paths) => paths,
            Err(failure) => {
                let rollback = rollback_projected_package_mounts(&mut vm.kernel, &failure.mounted);
                return Err(package_mount_failure(vm_id, failure, rollback));
            }
        };
        Ok(commit_projected_package_batch(
            vm,
            prepared,
            mounted_paths,
            &mount_root,
            false,
        ))
    }

    /// Atomically replace all projected packages. An empty batch clears the
    /// projection; callers preserve it by not invoking this method for an
    /// omitted package field.
    pub fn replace_aospkg_batch_bytes(
        &mut self,
        vm_id: &str,
        packages: Vec<Vec<u8>>,
        mount_root: Option<&str>,
    ) -> Result<Vec<BrowserProjectedPackage>, BrowserSidecarError> {
        let mount_root =
            normalize_packages_mount_root(mount_root).map_err(package_projection_error)?;
        let mut prepared = Self::prepare_package_batch(packages, &mount_root, 0, 0, 0, 0)?;
        let vm = self.vm(vm_id)?;
        Self::validate_prepared_package_batch(vm, &prepared, true)?;

        // Retained opaque source bytes let a failed mount swap reconstruct the
        // exact old read-only projection without guest filesystem access.
        let old_root = vm.projected_package_mount_root.clone();
        let mut old_prepared = vm
            .projected_package_sources
            .clone()
            .into_iter()
            .map(|bytes| prepare_aospkg_bytes(bytes, &old_root))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                BrowserSidecarError::PackageStateCorrupt(format!(
                    "rebuild existing package rollback plan: {error}"
                ))
            })?;
        let old_paths = vm.projected_package_mount_paths.clone();

        let vm = self.vm_mut(vm_id)?;
        if let Err(unmount_error) = rollback_projected_package_mounts(&mut vm.kernel, &old_paths) {
            let restore = mount_prepared_package_batch(&mut vm.kernel, &mut old_prepared, true);
            return Err(match restore {
                Ok(_) => BrowserSidecarError::PackageMount(format!(
                    "unmount old browser package projection: {unmount_error}"
                )),
                Err(restore_error) => BrowserSidecarError::PackageStateCorrupt(format!(
                    "unmount old browser package projection: {unmount_error}; restore failed: {}",
                    restore_error.detail
                )),
            });
        }

        let mounted_paths = match mount_prepared_package_batch(&mut vm.kernel, &mut prepared, false)
        {
            Ok(paths) => paths,
            Err(failure) => {
                let new_rollback =
                    rollback_projected_package_mounts(&mut vm.kernel, &failure.mounted);
                let old_restore =
                    mount_prepared_package_batch(&mut vm.kernel, &mut old_prepared, true);
                if new_rollback.is_err() || old_restore.is_err() {
                    let new_rollback_detail = new_rollback
                        .err()
                        .map(|error| format!("new projection rollback failed: {error}"));
                    let old_restore_detail = old_restore.err().map(|restore_error| {
                        format!("old projection restore failed: {}", restore_error.detail)
                    });
                    let recovery_detail = [new_rollback_detail, old_restore_detail]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join("; ");
                    tracing::error!(vm_id, error = %recovery_detail, "failed to recover package projection after replacement failure");
                    return Err(BrowserSidecarError::PackageStateCorrupt(format!(
                        "replace .aospkg projection failed at {}: {}; {recovery_detail}",
                        failure.path, failure.detail
                    )));
                }
                return Err(BrowserSidecarError::PackageMount(format!(
                    "replace .aospkg projection failed at {}: {}",
                    failure.path, failure.detail
                )));
            }
        };

        Ok(commit_projected_package_batch(
            vm,
            prepared,
            mounted_paths,
            &mount_root,
            true,
        ))
    }

    fn prepare_package_batch(
        packages: Vec<Vec<u8>>,
        mount_root: &str,
        existing_packages: usize,
        existing_bytes: usize,
        existing_entries: usize,
        existing_materialized_bytes: usize,
    ) -> Result<Vec<PreparedBrowserPackage>, BrowserSidecarError> {
        ensure_projected_package_limit(
            "max_projected_packages_per_vm",
            existing_packages,
            packages.len(),
            MAX_BROWSER_PROJECTED_PACKAGES_PER_VM,
            "raise the browser package projection package limit in the sidecar",
        )?;
        let incoming_bytes = packages.iter().try_fold(0usize, |total, package| {
            total
                .checked_add(package.len())
                .ok_or(BrowserSidecarError::LimitExceeded {
                    limit: "max_projected_package_bytes_per_vm",
                    capacity: MAX_BROWSER_PROJECTED_PACKAGE_BYTES_PER_VM,
                    how_to_raise: "raise the browser package projection byte limit in the sidecar",
                })
        })?;
        ensure_projected_package_limit(
            "max_projected_package_bytes_per_vm",
            existing_bytes,
            incoming_bytes,
            MAX_BROWSER_PROJECTED_PACKAGE_BYTES_PER_VM,
            "raise the browser package projection byte limit in the sidecar",
        )?;
        let prepared = packages
            .into_iter()
            .map(|bytes| prepare_aospkg_bytes(bytes, mount_root))
            .collect::<Result<Vec<_>, _>>()
            .map_err(package_projection_error)?;
        let incoming_entries = prepared.iter().try_fold(0usize, |total, package| {
            total
                .checked_add(package.index_entries)
                .ok_or(BrowserSidecarError::LimitExceeded {
                    limit: "max_projected_package_entries_per_vm",
                    capacity: MAX_BROWSER_PROJECTED_PACKAGE_ENTRIES_PER_VM,
                    how_to_raise: "raise the browser package projection entry limit in the sidecar",
                })
        })?;
        ensure_projected_package_limit(
            "max_projected_package_entries_per_vm",
            existing_entries,
            incoming_entries,
            MAX_BROWSER_PROJECTED_PACKAGE_ENTRIES_PER_VM,
            "raise the browser package projection entry limit in the sidecar",
        )?;
        let incoming_materialized_bytes = prepared.iter().try_fold(0usize, |total, package| {
            total
                .checked_add(package.materialized_bytes)
                .ok_or(BrowserSidecarError::LimitExceeded {
                limit: "max_projected_package_materialized_bytes_per_vm",
                capacity: MAX_BROWSER_PROJECTED_PACKAGE_MATERIALIZED_BYTES_PER_VM,
                how_to_raise:
                    "raise the browser package projection materialized-byte limit in the sidecar",
            })
        })?;
        ensure_projected_package_limit(
            "max_projected_package_materialized_bytes_per_vm",
            existing_materialized_bytes,
            incoming_materialized_bytes,
            MAX_BROWSER_PROJECTED_PACKAGE_MATERIALIZED_BYTES_PER_VM,
            "raise the browser package projection materialized-byte limit in the sidecar",
        )?;
        Ok(prepared)
    }

    fn validate_prepared_package_batch(
        vm: &VmState,
        packages: &[PreparedBrowserPackage],
        replacing: bool,
    ) -> Result<(), BrowserSidecarError> {
        let existing_mounts = if replacing {
            0
        } else {
            vm.projected_package_mount_paths.len()
        };
        let incoming_mounts = packages.iter().try_fold(0usize, |total, package| {
            total
                .checked_add(package.mounts.len())
                .ok_or(BrowserSidecarError::LimitExceeded {
                    limit: "max_projected_package_mounts_per_vm",
                    capacity: MAX_BROWSER_PROJECTED_PACKAGE_MOUNTS_PER_VM,
                    how_to_raise: "raise the browser package projection mount limit in the sidecar",
                })
        })?;
        ensure_projected_package_limit(
            "max_projected_package_mounts_per_vm",
            existing_mounts,
            incoming_mounts,
            MAX_BROWSER_PROJECTED_PACKAGE_MOUNTS_PER_VM,
            "raise the browser package projection mount limit in the sidecar",
        )?;
        let mut package_names = if replacing {
            BTreeSet::new()
        } else {
            vm.projected_package_names.clone()
        };
        let mut command_names = if replacing {
            BTreeSet::new()
        } else {
            vm.projected_command_names.clone()
        };
        let mut agent_ids = if replacing {
            BTreeSet::new()
        } else {
            vm.projected_agent_launch
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
        };
        let mut mount_paths = BTreeSet::new();
        for package in packages {
            if !package_names.insert(package.projection.name.clone()) {
                return Err(BrowserSidecarError::PackageConflict(format!(
                    "package {:?} is already projected in this VM",
                    package.projection.name
                )));
            }
            for command in &package.projection.commands {
                if !command_names.insert(command.clone()) {
                    return Err(BrowserSidecarError::PackageConflict(format!(
                        "command {command:?} is already provided by another projected package"
                    )));
                }
            }
            if let Some(agent) = &package.projection.agent {
                if !agent_ids.insert(agent.id.clone()) {
                    return Err(BrowserSidecarError::PackageConflict(format!(
                        "agent {:?} is already provided by another projected package",
                        agent.id
                    )));
                }
            }
            for mount in &package.mounts {
                if !mount_paths.insert(mount.guest_path().to_owned()) {
                    return Err(BrowserSidecarError::PackageConflict(format!(
                        "duplicate projected package mount {:?}",
                        mount.guest_path()
                    )));
                }
            }
        }
        if agent_ids.len() > MAX_BROWSER_PROJECTED_AGENTS_PER_VM {
            return Err(BrowserSidecarError::LimitExceeded {
                limit: "max_projected_agents_per_vm",
                capacity: MAX_BROWSER_PROJECTED_AGENTS_PER_VM,
                how_to_raise: "raise the browser package projection agent limit in the sidecar",
            });
        }
        Ok(())
    }

    pub fn create_vm(&mut self, config: KernelVmConfig) -> Result<(), BrowserSidecarError> {
        self.create_vm_with_root_filesystem(config, RootFilesystemConfig::default())
    }

    pub fn configure_vm(
        &mut self,
        vm_id: &str,
        permissions: Option<agentos_kernel::permissions::Permissions>,
        command_permissions: Option<BTreeMap<String, WasmPermissionTier>>,
        loopback_exempt_ports: Option<Vec<u16>>,
    ) -> Result<(), BrowserSidecarError> {
        let vm = self
            .vms
            .get_mut(vm_id)
            .ok_or_else(|| BrowserSidecarError::InvalidState(format!("unknown VM {vm_id}")))?;
        if let Some(permissions) = permissions {
            vm.kernel.set_permissions(permissions);
        }
        if let Some(command_permissions) = command_permissions {
            vm.configuration.command_permissions = command_permissions;
        }
        if let Some(loopback_exempt_ports) = loopback_exempt_ports {
            vm.configuration.loopback_exempt_ports = loopback_exempt_ports;
        }
        let mut effective_loopback_exempt_ports =
            vm.configuration.create_loopback_exempt_ports.clone();
        effective_loopback_exempt_ports
            .extend(vm.configuration.loopback_exempt_ports.iter().copied());
        vm.kernel
            .set_loopback_exempt_ports(effective_loopback_exempt_ports);
        Ok(())
    }

    pub fn create_vm_with_root_filesystem(
        &mut self,
        config: KernelVmConfig,
        root_filesystem: RootFilesystemConfig,
    ) -> Result<(), BrowserSidecarError> {
        let vm_id = config.vm_id.clone();
        if self.vms.contains_key(&vm_id) {
            return Err(BrowserSidecarError::InvalidState(format!(
                "browser sidecar VM already exists: {vm_id}"
            )));
        }
        if self.vms.len() >= self.config.max_vms {
            return Err(BrowserSidecarError::LimitExceeded {
                limit: "max_vms",
                capacity: self.config.max_vms,
                how_to_raise:
                    "dispose active or pending-cleanup VMs or raise BrowserSidecarConfig::max_vms",
            });
        }
        if self.vms.len().saturating_add(1)
            >= deferred_execution_event_warning_threshold(self.config.max_vms)
        {
            tracing::warn!(
                limit = "max_vms",
                observed = self.vms.len() + 1,
                capacity = self.config.max_vms,
                "browser sidecar VM registry is near its limit; dispose VMs or raise BrowserSidecarConfig::max_vms"
            );
        }

        self.emit_lifecycle(
            &vm_id,
            LifecycleState::Starting,
            Some(String::from(
                "browser sidecar booting kernel on main thread",
            )),
        )?;
        let create_loopback_exempt_ports = config.loopback_exempt_ports.clone();
        let guest_cwd = config.cwd.clone();
        self.vms.insert(
            vm_id.clone(),
            VmState {
                kernel: KernelVm::new(
                    build_root_mount_table(&root_filesystem, &config.resources)
                        .map_err(Self::sidecar_core_error)?,
                    config,
                ),
                guest_cwd,
                agent_additional_instructions: None,
                projected_agent_launch: BTreeMap::new(),
                projected_package_names: BTreeSet::new(),
                projected_command_names: BTreeSet::new(),
                projected_package_bytes: 0,
                projected_package_entries: 0,
                projected_package_materialized_bytes: 0,
                projected_package_sources: Vec::new(),
                projected_packages: Vec::new(),
                projected_package_mount_paths: Vec::new(),
                projected_package_mount_root: String::from(DEFAULT_BROWSER_PACKAGES_MOUNT_ROOT),
                projected_package_env: BTreeMap::new(),
                provided_commands: BTreeMap::new(),
                configuration: BrowserVmConfiguration {
                    create_loopback_exempt_ports,
                    ..BrowserVmConfiguration::default()
                },
                layers: VmLayerStore::default(),
                toolkits: BTreeMap::new(),
                signal_states: BTreeMap::new(),
                contexts: BTreeSet::new(),
                active_executions: BTreeSet::new(),
                deferred_execution_events: VecDeque::new(),
                deferred_execution_events_warned: false,
                pending_execution_cleanups_warned: false,
            },
        );
        if let Some(root) = self
            .vms
            .get_mut(&vm_id)
            .and_then(|vm| vm.kernel.root_filesystem_mut())
        {
            root.finish_bootstrap();
        }
        self.emit_lifecycle(
            &vm_id,
            LifecycleState::Ready,
            Some(String::from(
                "browser sidecar kernel is ready on the main thread",
            )),
        )?;
        Ok(())
    }

    pub fn write_file(
        &mut self,
        vm_id: &str,
        path: &str,
        contents: impl Into<Vec<u8>>,
    ) -> Result<(), BrowserSidecarError> {
        let vm = self.vm_mut(vm_id)?;
        vm.kernel
            .write_file(path, contents)
            .map_err(Self::kernel_error)
    }

    pub fn read_file(&mut self, vm_id: &str, path: &str) -> Result<Vec<u8>, BrowserSidecarError> {
        let vm = self.vm_mut(vm_id)?;
        vm.kernel.read_file(path).map_err(Self::kernel_error)
    }

    pub fn mkdir(
        &mut self,
        vm_id: &str,
        path: &str,
        recursive: bool,
    ) -> Result<(), BrowserSidecarError> {
        let vm = self.vm_mut(vm_id)?;
        vm.kernel.mkdir(path, recursive).map_err(Self::kernel_error)
    }

    pub fn read_dir(
        &mut self,
        vm_id: &str,
        path: &str,
    ) -> Result<Vec<String>, BrowserSidecarError> {
        let vm = self.vm_mut(vm_id)?;
        vm.kernel.read_dir(path).map_err(Self::kernel_error)
    }

    pub fn snapshot_root_filesystem(
        &mut self,
        vm_id: &str,
    ) -> Result<RootFilesystemSnapshot, BrowserSidecarError> {
        let vm = self.vm_mut(vm_id)?;
        vm.kernel
            .snapshot_root_filesystem()
            .map_err(Self::kernel_error)
    }

    pub fn create_layer(&mut self, vm_id: &str) -> Result<String, BrowserSidecarError> {
        self.vm_mut(vm_id)?
            .layers
            .create_writable_layer()
            .map_err(Self::sidecar_core_error)
    }

    pub fn seal_layer(
        &mut self,
        vm_id: &str,
        layer_id: &str,
    ) -> Result<String, BrowserSidecarError> {
        self.vm_mut(vm_id)?
            .layers
            .seal_layer(layer_id)
            .map_err(Self::sidecar_core_error)
    }

    pub fn import_snapshot(
        &mut self,
        vm_id: &str,
        entries: &[RootFilesystemEntry],
    ) -> Result<String, BrowserSidecarError> {
        let snapshot = root_snapshot_from_entries(entries)?;
        self.vm_mut(vm_id)?
            .layers
            .import_snapshot(snapshot)
            .map_err(Self::sidecar_core_error)
    }

    pub fn export_snapshot(
        &mut self,
        vm_id: &str,
        layer_id: &str,
    ) -> Result<RootFilesystemSnapshot, BrowserSidecarError> {
        self.vm_mut(vm_id)?
            .layers
            .export_snapshot(layer_id)
            .map_err(Self::sidecar_core_error)
    }

    pub fn create_overlay(
        &mut self,
        vm_id: &str,
        mode: KernelRootFilesystemMode,
        upper_layer_id: Option<String>,
        lower_layer_ids: Vec<String>,
    ) -> Result<String, BrowserSidecarError> {
        self.vm_mut(vm_id)?
            .layers
            .create_overlay_layer(mode, upper_layer_id, lower_layer_ids)
            .map_err(Self::sidecar_core_error)
    }

    pub fn register_host_callbacks(
        &mut self,
        vm_id: &str,
        payload: RegisterHostCallbacksRequest,
    ) -> Result<(String, u32), BrowserSidecarError> {
        validate_toolkit_registration(&payload)
            .map_err(|error| BrowserSidecarError::InvalidState(error.to_string()))?;
        let vm = self.vm_mut(vm_id)?;
        ensure_toolkit_name_available(&vm.toolkits, &payload.name)
            .map_err(|error| BrowserSidecarError::InvalidState(error.to_string()))?;
        ensure_toolkit_registry_capacity(&vm.toolkits, &payload)
            .map_err(|error| BrowserSidecarError::InvalidState(error.to_string()))?;

        let registration = payload.name.clone();
        vm.toolkits.insert(registration.clone(), payload);
        let command_count = u32::try_from(registered_tool_command_names(&vm.toolkits).len())
            .map_err(|_| {
                BrowserSidecarError::InvalidState(String::from(
                    "registered host callback command count exceeds u32",
                ))
            })?;
        Ok((registration, command_count))
    }

    pub fn bootstrap_root_filesystem_entries(
        &mut self,
        vm_id: &str,
        entries: &[RootFilesystemEntry],
    ) -> Result<u32, BrowserSidecarError> {
        for entry in entries {
            self.apply_root_filesystem_entry(vm_id, entry)?;
        }
        u32::try_from(entries.len()).map_err(|_| {
            BrowserSidecarError::InvalidState(String::from(
                "root filesystem bootstrap entry count exceeds u32",
            ))
        })
    }

    pub fn guest_filesystem_call(
        &mut self,
        vm_id: &str,
        payload: GuestFilesystemCallRequest,
    ) -> Result<GuestFilesystemResultResponse, BrowserSidecarError> {
        let vm = self.vm_mut(vm_id)?;
        let payload =
            agentos_native_sidecar_core::resolve_guest_filesystem_request(&vm.guest_cwd, payload)
                .map_err(Self::sidecar_core_error)?;
        handle_guest_filesystem_call(&mut vm.kernel, payload).map_err(Self::sidecar_core_error)
    }

    pub fn guest_kernel_call(
        &mut self,
        vm_id: &str,
        payload: GuestKernelCallRequest,
    ) -> Result<GuestKernelResultResponse, BrowserSidecarError> {
        let execution = self.ensure_execution_state(vm_id, &payload.execution_id)?;
        let kernel_pid = execution.kernel_pid;
        let vm = self.vm_mut(vm_id)?;
        let response = handle_guest_kernel_call(
            &mut vm.kernel,
            kernel_pid,
            BROWSER_WORKER_DRIVER,
            &payload.operation,
            &payload.payload,
        )
        .map_err(Self::sidecar_core_error)?;
        Ok(GuestKernelResultResponse { payload: response })
    }

    pub fn kernel_state(&self, vm_id: &str) -> Result<LifecycleState, BrowserSidecarError> {
        let vm = self.vm(vm_id)?;
        Ok(match vm.kernel.state() {
            KernelLifecycleState::Starting => LifecycleState::Starting,
            KernelLifecycleState::Ready => LifecycleState::Ready,
            KernelLifecycleState::Busy => LifecycleState::Busy,
            KernelLifecycleState::Terminated => LifecycleState::Terminated,
        })
    }

    pub fn zombie_timer_count(&self, vm_id: &str) -> Result<u64, BrowserSidecarError> {
        let vm = self.vm(vm_id)?;
        Ok(vm.kernel.zombie_timer_count() as u64)
    }

    pub fn process_snapshot_entries(
        &self,
        vm_id: &str,
    ) -> Result<Vec<SharedProcessSnapshotEntry>, BrowserSidecarError> {
        let vm = self.vm(vm_id)?;
        let process_table = vm.kernel.list_processes();
        Ok(self
            .executions
            .iter()
            .filter(|(_, execution)| execution.vm_id == vm_id)
            .filter_map(|(execution_id, execution)| {
                process_table
                    .get(&execution.kernel_pid)
                    .map(|info| process_snapshot_entry_from_kernel(execution_id, info, None))
            })
            .collect())
    }

    pub fn signal_state(
        &self,
        vm_id: &str,
        execution_id: &str,
    ) -> Result<BTreeMap<u32, ProtocolSignalHandlerRegistration>, BrowserSidecarError> {
        let vm = self.vm(vm_id)?;
        Ok(vm
            .signal_states
            .get(execution_id)
            .cloned()
            .unwrap_or_default())
    }

    pub fn find_listener(
        &self,
        vm_id: &str,
        request: &FindListenerRequest,
    ) -> Result<Option<SocketStateEntry>, BrowserSidecarError> {
        let vm = self.vm(vm_id)?;
        for (execution_id, execution) in &self.executions {
            if execution.vm_id != vm_id {
                continue;
            }
            for record in vm.kernel.socket_records_for_pid(execution.kernel_pid) {
                if let Some(path) = request.path.as_deref() {
                    if record.state() == SocketState::Listening
                        && record.spec().socket_type == SocketType::Stream
                        && record.local_unix_path() == Some(path)
                    {
                        return Ok(Some(SocketStateEntry {
                            process_id: execution_id.clone(),
                            host: None,
                            port: None,
                            path: Some(path.to_string()),
                        }));
                    }
                    continue;
                }

                let Some(address) = record.local_address() else {
                    continue;
                };
                if record.state() != SocketState::Listening
                    || record.spec().socket_type != SocketType::Stream
                    || !socket_host_matches(request.host.as_deref(), address.host())
                    || request.port.is_some_and(|port| address.port() != port)
                {
                    continue;
                }
                return Ok(Some(SocketStateEntry {
                    process_id: execution_id.clone(),
                    host: Some(address.host().to_string()),
                    port: Some(address.port()),
                    path: None,
                }));
            }
        }
        Ok(None)
    }

    pub fn find_bound_udp(
        &self,
        vm_id: &str,
        request: &FindBoundUdpRequest,
    ) -> Result<Option<SocketStateEntry>, BrowserSidecarError> {
        let vm = self.vm(vm_id)?;
        for (execution_id, execution) in &self.executions {
            if execution.vm_id != vm_id {
                continue;
            }
            for record in vm.kernel.socket_records_for_pid(execution.kernel_pid) {
                let Some(address) = record.local_address() else {
                    continue;
                };
                if record.state() != SocketState::Bound
                    || record.spec().socket_type != SocketType::Datagram
                    || !socket_host_matches(request.host.as_deref(), address.host())
                    || request.port.is_some_and(|port| address.port() != port)
                {
                    continue;
                }
                return Ok(Some(SocketStateEntry {
                    process_id: execution_id.clone(),
                    host: Some(address.host().to_string()),
                    port: Some(address.port()),
                    path: None,
                }));
            }
        }
        Ok(None)
    }

    pub fn vm_fetch(
        &mut self,
        vm_id: &str,
        request: &agentos_sidecar_protocol::protocol::VmFetchRequest,
    ) -> Result<String, BrowserSidecarError> {
        let target_path = if request.path.starts_with('/') {
            request.path.clone()
        } else {
            format!("/{}", request.path)
        };
        let listener = self
            .find_listener(
                vm_id,
                &FindListenerRequest {
                    host: Some(String::from("127.0.0.1")),
                    port: Some(request.port),
                    path: None,
                },
            )?
            .ok_or_else(|| {
                BrowserSidecarError::InvalidState(format!(
                    "vm.fetch could not find a guest HTTP listener on port {}",
                    request.port
                ))
            })?;
        let target_execution_id = listener.process_id;
        let target = self.ensure_execution_state(vm_id, &target_execution_id)?;
        let request_bytes = serialize_kernel_http_fetch_request(
            request.port,
            &target_path,
            &request.method,
            &request.headers_json,
            request.body.as_deref(),
        )
        .map_err(|error| BrowserSidecarError::InvalidState(error.to_string()))?;
        let url = format!("http://127.0.0.1:{}{}", request.port, target_path);

        let socket_id = {
            let vm = self.vm_mut(vm_id)?;
            let socket_id = vm
                .kernel
                .socket_create(BROWSER_WORKER_DRIVER, target.kernel_pid, SocketSpec::tcp())
                .map_err(Self::kernel_error)?;
            let result = vm
                .kernel
                .socket_connect_inet_loopback(
                    BROWSER_WORKER_DRIVER,
                    target.kernel_pid,
                    socket_id,
                    InetSocketAddress::new("127.0.0.1", request.port),
                )
                .map_err(Self::kernel_error)
                .and_then(|()| {
                    vm.kernel
                        .socket_write(
                            BROWSER_WORKER_DRIVER,
                            target.kernel_pid,
                            socket_id,
                            &request_bytes,
                        )
                        .map(|_| ())
                        .map_err(Self::kernel_error)
                });
            if let Err(error) = result {
                if let Err(close_error) =
                    vm.kernel
                        .socket_close(BROWSER_WORKER_DRIVER, target.kernel_pid, socket_id)
                {
                    tracing::error!(
                        vm_id,
                        socket_id,
                        %close_error,
                        "failed to close browser vm.fetch socket after request setup failure"
                    );
                }
                return Err(error);
            }
            socket_id
        };

        let result = self.read_vm_fetch_response(
            vm_id,
            target.kernel_pid,
            socket_id,
            &url,
            VM_FETCH_BUFFER_LIMIT_BYTES,
        );
        let close_result = {
            let vm = self.vm_mut(vm_id)?;
            vm.kernel
                .socket_close(BROWSER_WORKER_DRIVER, target.kernel_pid, socket_id)
                .map_err(Self::kernel_error)
        };

        match (result, close_result) {
            (Ok(response), Ok(())) => Ok(response),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    fn read_vm_fetch_response(
        &mut self,
        vm_id: &str,
        kernel_pid: u32,
        socket_id: SocketId,
        url: &str,
        max_fetch_response_bytes: usize,
    ) -> Result<String, BrowserSidecarError> {
        let mut response_buffer = Vec::new();
        let mut peer_closed = false;
        let deadline = Instant::now() + browser_vm_fetch_timeout();

        loop {
            if let Some(response) =
                parse_kernel_http_fetch_response(&response_buffer, peer_closed, url)
                    .map_err(|error| BrowserSidecarError::InvalidState(error.to_string()))?
            {
                ensure_vm_fetch_response_within_limit(
                    &response,
                    "vm.fetch",
                    max_fetch_response_bytes,
                )
                .map_err(|error| BrowserSidecarError::InvalidState(error.to_string()))?;
                return Ok(response);
            }
            if Instant::now() >= deadline {
                let preview = String::from_utf8_lossy(&response_buffer);
                return Err(BrowserSidecarError::InvalidState(format!(
                    "vm.fetch timed out waiting for kernel TCP HTTP response ({} buffered bytes: {:?})",
                    response_buffer.len(),
                    preview.chars().take(200).collect::<String>()
                )));
            }

            let poll = {
                let vm = self.vm_mut(vm_id)?;
                vm.kernel
                    .poll_targets(
                        BROWSER_WORKER_DRIVER,
                        kernel_pid,
                        vec![PollTargetEntry::socket(
                            socket_id,
                            POLLIN | POLLHUP | POLLERR,
                        )],
                        5,
                    )
                    .map_err(Self::kernel_error)?
            };
            let revents = poll
                .targets
                .first()
                .map(|entry| entry.revents)
                .unwrap_or_default();
            if revents.intersects(POLLERR) {
                return Err(BrowserSidecarError::InvalidState(String::from(
                    "vm.fetch kernel TCP socket reported POLLERR",
                )));
            }
            if revents.intersects(POLLIN) {
                let read_result = {
                    let vm = self.vm_mut(vm_id)?;
                    vm.kernel
                        .socket_read(BROWSER_WORKER_DRIVER, kernel_pid, socket_id, 64 * 1024)
                };
                match read_result {
                    Ok(Some(bytes)) if !bytes.is_empty() => {
                        response_buffer.extend(bytes);
                        ensure_vm_fetch_raw_response_buffer_within_limit(
                            response_buffer.len(),
                            "vm.fetch",
                        )
                        .map_err(|error| BrowserSidecarError::InvalidState(error.to_string()))?;
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => peer_closed = true,
                    Err(error) if error.code() == "EAGAIN" => {}
                    Err(error) => return Err(Self::kernel_error(error)),
                }
            }
            if revents.intersects(POLLHUP) {
                peer_closed = true;
            }
        }
    }

    pub fn create_kernel_tcp_listener_for_execution(
        &mut self,
        vm_id: &str,
        execution_id: &str,
        host: &str,
        port: u16,
        backlog: usize,
    ) -> Result<SocketId, BrowserSidecarError> {
        let execution = self.ensure_execution_state(vm_id, execution_id)?;
        let vm = self.vm_mut(vm_id)?;
        let socket_id = vm
            .kernel
            .socket_create(
                BROWSER_WORKER_DRIVER,
                execution.kernel_pid,
                SocketSpec::tcp(),
            )
            .map_err(Self::kernel_error)?;
        vm.kernel
            .socket_bind_inet(
                BROWSER_WORKER_DRIVER,
                execution.kernel_pid,
                socket_id,
                InetSocketAddress::new(host, port),
            )
            .map_err(Self::kernel_error)?;
        vm.kernel
            .socket_listen(
                BROWSER_WORKER_DRIVER,
                execution.kernel_pid,
                socket_id,
                backlog,
            )
            .map_err(Self::kernel_error)?;
        Ok(socket_id)
    }

    pub fn create_kernel_bound_udp_for_execution(
        &mut self,
        vm_id: &str,
        execution_id: &str,
        host: &str,
        port: u16,
    ) -> Result<SocketId, BrowserSidecarError> {
        let execution = self.ensure_execution_state(vm_id, execution_id)?;
        let vm = self.vm_mut(vm_id)?;
        let socket_id = vm
            .kernel
            .socket_create(
                BROWSER_WORKER_DRIVER,
                execution.kernel_pid,
                SocketSpec::udp(),
            )
            .map_err(Self::kernel_error)?;
        vm.kernel
            .socket_bind_inet(
                BROWSER_WORKER_DRIVER,
                execution.kernel_pid,
                socket_id,
                InetSocketAddress::new(host, port),
            )
            .map_err(Self::kernel_error)?;
        Ok(socket_id)
    }

    pub fn read_execution_stdin(
        &mut self,
        vm_id: &str,
        execution_id: &str,
        length: usize,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>, BrowserSidecarError> {
        let execution = self.ensure_execution_state(vm_id, execution_id)?;
        let vm = self.vm_mut(vm_id)?;
        vm.kernel
            .read_process_stdin(
                BROWSER_WORKER_DRIVER,
                execution.kernel_pid,
                length,
                Some(timeout),
            )
            .map_err(Self::kernel_error)
    }

    pub fn dispose_vm(&mut self, vm_id: &str) -> Result<(), BrowserSidecarError> {
        self.begin_vm_cleanup(vm_id)?;
        let vm_state = self
            .vms
            .get(vm_id)
            .expect("VM exists after cleanup transition");

        let active_execution_ids = vm_state
            .active_executions
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let cleanup_execution_ids = self
            .execution_cleanups
            .iter()
            .filter(|(_, cleanup)| cleanup.execution.vm_id == vm_id)
            .map(|(execution_id, _)| execution_id.clone())
            .collect::<Vec<_>>();
        let startup_cleanup_ids = self
            .startup_cleanups
            .iter()
            .filter(|(_, cleanup)| cleanup.vm_id == vm_id)
            .map(|(cleanup_id, _)| cleanup_id.clone())
            .collect::<Vec<_>>();
        let mut errors = Vec::new();
        for execution_id in active_execution_ids
            .into_iter()
            .chain(cleanup_execution_ids)
        {
            if let Err(error) = self.release_execution(&execution_id, "browser.worker.disposed") {
                errors.push(error);
            }
        }
        for cleanup_id in startup_cleanup_ids {
            if let Err(error) = self.drive_startup_cleanup(&cleanup_id) {
                errors.push(error);
            }
        }

        let has_pending_executions = self
            .execution_cleanups
            .values()
            .any(|cleanup| cleanup.execution.vm_id == vm_id)
            || self
                .startup_cleanups
                .values()
                .any(|cleanup| cleanup.vm_id == vm_id);
        if !has_pending_executions {
            if let Err(error) = self.emit_lifecycle(
                vm_id,
                LifecycleState::Terminated,
                Some(String::from(
                    "browser sidecar VM disposed on the main thread",
                )),
            ) {
                errors.push(error);
            } else {
                let context_ids = self
                    .vms
                    .get(vm_id)
                    .map(|vm| vm.contexts.iter().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                for context_id in context_ids {
                    self.contexts.remove(&context_id);
                }
                self.vms.remove(vm_id);
                self.disposing_vms.remove(vm_id);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(BrowserSidecarError::Cleanup {
                context: "failed to dispose browser VM completely",
                errors,
            })
        }
    }

    /// Make a VM non-routable before an outer extension/session cleanup begins.
    pub fn begin_vm_cleanup(&mut self, vm_id: &str) -> Result<(), BrowserSidecarError> {
        if !self.vms.contains_key(vm_id) {
            return Err(BrowserSidecarError::InvalidState(format!(
                "unknown browser sidecar VM: {vm_id}"
            )));
        }
        self.disposing_vms.insert(vm_id.to_string());
        Ok(())
    }

    pub fn vm_is_disposing(&self, vm_id: &str) -> bool {
        self.disposing_vms.contains(vm_id)
    }

    pub fn create_javascript_context(
        &mut self,
        request: CreateJavascriptContextRequest,
    ) -> Result<GuestContextHandle, BrowserSidecarError> {
        self.ensure_vm(&request.vm_id)?;

        let vm_id = request.vm_id.clone();
        let entrypoint = BrowserWorkerEntrypoint::JavaScript {
            bootstrap_module: request.bootstrap_module.clone(),
        };
        let handle = self
            .bridge
            .create_javascript_context(request)
            .map_err(Self::bridge_error)?;

        self.register_context(vm_id, handle.clone(), entrypoint)?;
        Ok(handle)
    }

    pub fn create_wasm_context(
        &mut self,
        request: CreateWasmContextRequest,
    ) -> Result<GuestContextHandle, BrowserSidecarError> {
        self.ensure_vm(&request.vm_id)?;

        let vm_id = request.vm_id.clone();
        let entrypoint = BrowserWorkerEntrypoint::WebAssembly {
            module_path: request.module_path.clone(),
        };
        let handle = self
            .bridge
            .create_wasm_context(request)
            .map_err(Self::bridge_error)?;

        self.register_context(vm_id, handle.clone(), entrypoint)?;
        Ok(handle)
    }

    pub fn start_execution(
        &mut self,
        request: StartExecutionRequest,
    ) -> Result<StartedExecution, BrowserSidecarError> {
        self.start_execution_with_options(request, BrowserExecutionOptions::default())
    }

    pub fn start_execution_with_options(
        &mut self,
        request: StartExecutionRequest,
        options: BrowserExecutionOptions,
    ) -> Result<StartedExecution, BrowserSidecarError> {
        self.ensure_execution_admission(&request.vm_id)?;

        let context = self
            .contexts
            .get(&request.context_id)
            .cloned()
            .ok_or_else(|| {
                BrowserSidecarError::InvalidState(format!(
                    "unknown browser sidecar context: {}",
                    request.context_id
                ))
            })?;

        if context.vm_id != request.vm_id {
            return Err(BrowserSidecarError::InvalidState(format!(
                "browser sidecar context {} belongs to vm {}, not {}",
                request.context_id, context.vm_id, request.vm_id
            )));
        }

        let guest_cwd = request.cwd.clone();
        let pending_process = {
            let vm = self.vm_mut(&request.vm_id)?;
            let kernel_handle = vm
                .kernel
                .create_virtual_process(
                    BROWSER_WORKER_DRIVER,
                    BROWSER_WORKER_DRIVER,
                    request
                        .argv
                        .first()
                        .map(String::as_str)
                        .unwrap_or("browser-worker"),
                    request.argv.clone(),
                    VirtualProcessOptions {
                        env: request.env.clone(),
                        cwd: Some(guest_cwd.clone()),
                        ..VirtualProcessOptions::default()
                    },
                )
                .map_err(Self::kernel_error)?;
            let kernel_pid = kernel_handle.pid();
            match Self::configure_process_stdio(&mut vm.kernel, kernel_pid) {
                Ok(stdin_write_fd) => Ok((kernel_pid, stdin_write_fd)),
                Err(error) => {
                    let cleanup = Self::cleanup_pending_kernel_process(&mut vm.kernel, kernel_pid);
                    Err((kernel_pid, error, cleanup.err()))
                }
            }
        };
        let (kernel_pid, stdin_write_fd) = match pending_process {
            Ok(process) => process,
            Err((_kernel_pid, primary, None)) => return Err(primary),
            Err((kernel_pid, primary, Some(cleanup))) => {
                let key = format!("startup-kernel:{}:{kernel_pid}", request.vm_id);
                self.startup_cleanups.insert(
                    key,
                    StartupCleanupState {
                        vm_id: request.vm_id.clone(),
                        execution_id: None,
                        kernel_pid,
                        kernel_reaped: false,
                        bridge_killed: true,
                    },
                );
                return Err(BrowserSidecarError::Cleanup {
                    context: "failed to configure and roll back browser execution stdio",
                    errors: vec![primary, cleanup],
                });
            }
        };

        let (process_config, os_config) = {
            let vm = self.vm(&request.vm_id)?;
            browser_worker_identity(&vm.kernel, &vm.projected_package_env, &request, kernel_pid)
        };
        let wasm_permission_tier = match context.runtime {
            GuestRuntime::JavaScript => None,
            GuestRuntime::WebAssembly => Some(self.resolve_wasm_permission_tier(
                &request.vm_id,
                options.command_name.as_deref(),
                options.wasm_permission_tier,
                &context.entrypoint,
            )?),
        };

        let started = match self.bridge.start_execution(request.clone()) {
            Ok(started) => started,
            Err(error) => {
                let primary = Self::bridge_error(error);
                match self.reap_execution_kernel_process(&request.vm_id, kernel_pid) {
                    Ok(()) => return Err(primary),
                    Err(cleanup) => {
                        let key = format!("startup-kernel:{}:{kernel_pid}", request.vm_id);
                        self.startup_cleanups.insert(
                            key,
                            StartupCleanupState {
                                vm_id: request.vm_id.clone(),
                                execution_id: None,
                                kernel_pid,
                                kernel_reaped: false,
                                bridge_killed: true,
                            },
                        );
                        return Err(BrowserSidecarError::Cleanup {
                            context: "failed to start and roll back browser execution",
                            errors: vec![primary, cleanup],
                        });
                    }
                }
            }
        };

        let worker = match self.bridge.create_worker(BrowserWorkerSpawnRequest {
            vm_id: request.vm_id.clone(),
            context_id: request.context_id.clone(),
            execution_id: started.execution_id.clone(),
            runtime: context.runtime,
            entrypoint: context.entrypoint.clone(),
            wasm_permission_tier,
            process_config,
            os_config,
        }) {
            Ok(worker) => worker,
            Err(error) => {
                let primary = Self::bridge_error(error);
                let cleanup_key = format!("startup-execution:{}", started.execution_id);
                self.startup_cleanups.insert(
                    cleanup_key.clone(),
                    StartupCleanupState {
                        vm_id: request.vm_id.clone(),
                        execution_id: Some(started.execution_id.clone()),
                        kernel_pid,
                        kernel_reaped: false,
                        bridge_killed: false,
                    },
                );
                return match self.drive_startup_cleanup(&cleanup_key) {
                    Ok(()) => Err(primary),
                    Err(cleanup) => Err(BrowserSidecarError::Cleanup {
                        context: "failed to create worker and roll back browser execution",
                        errors: vec![primary, cleanup],
                    }),
                };
            }
        };

        let worker_id = worker.worker_id.clone();
        self.executions.insert(
            started.execution_id.clone(),
            ExecutionState {
                vm_id: request.vm_id.clone(),
                context_id: request.context_id.clone(),
                worker: worker.clone(),
                kernel_pid,
                stdin_write_fd,
                cwd: guest_cwd,
            },
        );
        let vm_state = self
            .vms
            .get_mut(&request.vm_id)
            .expect("VM should exist after validation");
        vm_state
            .active_executions
            .insert(started.execution_id.clone());

        if let Err(error) = self.emit_structured(
            &request.vm_id,
            "browser.worker.spawned",
            BTreeMap::from([
                (String::from("context_id"), request.context_id),
                (String::from("execution_id"), started.execution_id.clone()),
                (
                    String::from("runtime"),
                    runtime_label(context.runtime).to_string(),
                ),
                (String::from("worker_id"), worker_id),
            ]),
        ) {
            tracing::error!(vm_id = %request.vm_id, execution_id = %started.execution_id, %error, "failed to emit browser worker-start diagnostic after committing execution");
        }
        if let Err(error) = self.emit_lifecycle(
            &request.vm_id,
            LifecycleState::Busy,
            Some(String::from(
                "browser sidecar is coordinating guest execution on the main thread",
            )),
        ) {
            tracing::error!(vm_id = %request.vm_id, execution_id = %started.execution_id, %error, "failed to emit browser busy lifecycle after committing execution");
        }

        Ok(started)
    }

    pub fn ensure_execution_admission(&mut self, vm_id: &str) -> Result<(), BrowserSidecarError> {
        self.ensure_vm(vm_id)?;
        self.retry_startup_cleanups_for_vm(vm_id)?;
        self.reserve_execution_cleanup_capacity(vm_id)
    }

    fn resolve_wasm_permission_tier(
        &self,
        vm_id: &str,
        command_name: Option<&str>,
        explicit_tier: Option<WasmPermissionTier>,
        entrypoint: &BrowserWorkerEntrypoint,
    ) -> Result<WasmPermissionTier, BrowserSidecarError> {
        let vm = self.vm(vm_id)?;
        Ok(explicit_tier
            .or_else(|| {
                command_name
                    .and_then(|command| vm.configuration.command_permissions.get(command).copied())
            })
            .or_else(|| {
                let BrowserWorkerEntrypoint::WebAssembly {
                    module_path: Some(module_path),
                } = entrypoint
                else {
                    return None;
                };
                module_path
                    .rsplit('/')
                    .next()
                    .and_then(|name| vm.configuration.command_permissions.get(name).copied())
            })
            .unwrap_or(WasmPermissionTier::Full))
    }

    fn configure_process_stdio(
        kernel: &mut BrowserKernel,
        kernel_pid: u32,
    ) -> Result<u32, BrowserSidecarError> {
        let (stdin_read_fd, stdin_write_fd) = kernel
            .open_pipe(BROWSER_WORKER_DRIVER, kernel_pid)
            .map_err(Self::kernel_error)?;
        kernel
            .fd_dup2(BROWSER_WORKER_DRIVER, kernel_pid, stdin_read_fd, 0)
            .map_err(Self::kernel_error)?;
        let (_stdout_read_fd, stdout_write_fd) = kernel
            .open_pipe(BROWSER_WORKER_DRIVER, kernel_pid)
            .map_err(Self::kernel_error)?;
        kernel
            .fd_dup2(BROWSER_WORKER_DRIVER, kernel_pid, stdout_write_fd, 1)
            .map_err(Self::kernel_error)?;
        let (_stderr_read_fd, stderr_write_fd) = kernel
            .open_pipe(BROWSER_WORKER_DRIVER, kernel_pid)
            .map_err(Self::kernel_error)?;
        kernel
            .fd_dup2(BROWSER_WORKER_DRIVER, kernel_pid, stderr_write_fd, 2)
            .map_err(Self::kernel_error)?;
        Ok(stdin_write_fd)
    }

    fn cleanup_pending_kernel_process(
        kernel: &mut BrowserKernel,
        kernel_pid: u32,
    ) -> Result<(), BrowserSidecarError> {
        kernel
            .exit_process(BROWSER_WORKER_DRIVER, kernel_pid, 1)
            .map_err(Self::kernel_error)?;
        kernel.waitpid(kernel_pid).map_err(Self::kernel_error)?;
        Ok(())
    }

    fn reap_execution_kernel_process(
        &mut self,
        vm_id: &str,
        kernel_pid: u32,
    ) -> Result<(), BrowserSidecarError> {
        #[cfg(test)]
        if let Some(error) = self.next_kernel_cleanup_error.take() {
            return Err(error);
        }

        let Some(vm) = self.vms.get_mut(vm_id) else {
            return Ok(());
        };
        let Some(process) = vm.kernel.list_processes().get(&kernel_pid).cloned() else {
            return Ok(());
        };

        if process.status != ProcessStatus::Exited {
            vm.kernel
                .exit_process(BROWSER_WORKER_DRIVER, kernel_pid, 1)
                .map_err(Self::kernel_error)?;
        }
        vm.kernel.waitpid(kernel_pid).map_err(Self::kernel_error)?;
        Ok(())
    }

    fn retry_startup_cleanups_for_vm(&mut self, vm_id: &str) -> Result<(), BrowserSidecarError> {
        let keys = self
            .startup_cleanups
            .iter()
            .filter(|(_, cleanup)| cleanup.vm_id == vm_id)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let mut errors = Vec::new();
        for key in keys {
            if let Err(error) = self.drive_startup_cleanup(&key) {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(BrowserSidecarError::Cleanup {
                context: "failed to retry browser execution startup cleanup",
                errors,
            })
        }
    }

    fn drive_startup_cleanup(&mut self, key: &str) -> Result<(), BrowserSidecarError> {
        let Some(mut cleanup) = self.startup_cleanups.get(key).cloned() else {
            return Ok(());
        };
        let mut errors = Vec::new();
        if !cleanup.kernel_reaped {
            match self.reap_execution_kernel_process(&cleanup.vm_id, cleanup.kernel_pid) {
                Ok(()) => cleanup.kernel_reaped = true,
                Err(error) => errors.push(error),
            }
        }
        if !cleanup.bridge_killed {
            let execution_id = cleanup
                .execution_id
                .as_ref()
                .expect("bridge cleanup requires an execution id");
            match self
                .bridge
                .kill_execution(KillExecutionRequest {
                    vm_id: cleanup.vm_id.clone(),
                    execution_id: execution_id.clone(),
                    signal: agentos_bridge::ExecutionSignal::Kill,
                })
                .map_err(Self::bridge_error)
            {
                Ok(()) => cleanup.bridge_killed = true,
                Err(error) => errors.push(error),
            }
        }
        self.startup_cleanups
            .insert(key.to_string(), cleanup.clone());
        if cleanup.kernel_reaped && cleanup.bridge_killed {
            self.startup_cleanups.remove(key);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(BrowserSidecarError::Cleanup {
                context: "failed to clean up partial browser execution startup",
                errors,
            })
        }
    }

    pub fn write_stdin(
        &mut self,
        request: WriteExecutionStdinRequest,
    ) -> Result<(), BrowserSidecarError> {
        self.ensure_execution(&request.vm_id, &request.execution_id)?;
        let execution = self.ensure_execution_state(&request.vm_id, &request.execution_id)?;
        {
            let vm = self.vm_mut(&request.vm_id)?;
            vm.kernel
                .fd_write(
                    BROWSER_WORKER_DRIVER,
                    execution.kernel_pid,
                    execution.stdin_write_fd,
                    &request.chunk,
                )
                .map_err(Self::kernel_error)?;
        }
        self.bridge.write_stdin(request).map_err(Self::bridge_error)
    }

    pub fn close_stdin(
        &mut self,
        request: ExecutionHandleRequest,
    ) -> Result<(), BrowserSidecarError> {
        self.ensure_execution(&request.vm_id, &request.execution_id)?;
        let execution = self.ensure_execution_state(&request.vm_id, &request.execution_id)?;
        {
            let vm = self.vm_mut(&request.vm_id)?;
            vm.kernel
                .fd_close(
                    BROWSER_WORKER_DRIVER,
                    execution.kernel_pid,
                    execution.stdin_write_fd,
                )
                .map_err(Self::kernel_error)?;
        }
        self.bridge.close_stdin(request).map_err(Self::bridge_error)
    }

    pub fn kill_execution(
        &mut self,
        request: KillExecutionRequest,
    ) -> Result<(), BrowserSidecarError> {
        self.ensure_execution(&request.vm_id, &request.execution_id)?;
        self.signal_execution_kernel_process(
            &request.vm_id,
            &request.execution_id,
            agentos_native_sidecar_core::execution_signal_to_kernel(request.signal),
        )?;
        self.bridge
            .kill_execution(request)
            .map_err(Self::bridge_error)
    }

    pub fn abort_execution(
        &mut self,
        vm_id: &str,
        execution_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        if !self.executions.contains_key(execution_id)
            && !self.execution_cleanups.contains_key(execution_id)
        {
            return Ok(());
        }
        let mut errors = Vec::new();
        if self.executions.contains_key(execution_id) {
            if let Err(error) = self.kill_execution(KillExecutionRequest {
                vm_id: vm_id.to_string(),
                execution_id: execution_id.to_string(),
                signal: ExecutionSignal::Kill,
            }) {
                errors.push(BrowserSidecarError::InvalidState(format!(
                    "execution {execution_id} abort signal: {error}"
                )));
            }
        }
        if let Err(error) = self.release_execution(execution_id, "browser.worker.acp_aborted") {
            errors.push(error);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(BrowserSidecarError::Cleanup {
                context: "failed to abort browser execution completely",
                errors,
            })
        }
    }

    pub fn signal_execution_kernel_process(
        &mut self,
        vm_id: &str,
        execution_id: &str,
        signal: i32,
    ) -> Result<(), BrowserSidecarError> {
        self.ensure_execution(vm_id, execution_id)?;
        let execution = self.ensure_execution_state(vm_id, execution_id)?;
        {
            let vm = self.vm_mut(vm_id)?;
            vm.kernel
                .kill_process(BROWSER_WORKER_DRIVER, execution.kernel_pid, signal)
                .map_err(Self::kernel_error)?;
        }
        Ok(())
    }

    pub fn poll_execution_event(
        &mut self,
        request: PollExecutionEventRequest,
    ) -> Result<Option<ExecutionEvent>, BrowserSidecarError> {
        self.ensure_vm(&request.vm_id)?;

        let event = match self.take_deferred_execution_event(&request.vm_id)? {
            Some(event) => Some(event),
            None => self
                .bridge
                .poll_execution_event(request)
                .map_err(Self::bridge_error)?,
        };
        if let Some(event) = &event {
            self.apply_execution_event(event)?;
        }
        Ok(event)
    }

    /// Poll only stdout/stderr/exit for one execution. The bridge exposes a
    /// VM-global destructive event stream, so non-matching output and centrally
    /// owned GuestRequest/SignalState events are retained in a bounded per-VM
    /// queue for the ordinary `poll_execution_event` path.
    pub fn poll_execution_output(
        &mut self,
        request: PollExecutionOutputRequest,
    ) -> Result<Option<ExecutionOutput>, BrowserSidecarError> {
        self.ensure_execution(&request.vm_id, &request.execution_id)?;

        if let Some(event) =
            self.take_deferred_execution_output(&request.vm_id, &request.execution_id)?
        {
            self.apply_execution_event(&event)?;
            return Ok(execution_output_from_event(event));
        }

        self.ensure_deferred_execution_event_capacity(&request.vm_id)?;
        let event = self
            .bridge
            .poll_execution_event(PollExecutionEventRequest {
                vm_id: request.vm_id.clone(),
            })
            .map_err(Self::bridge_error)?;
        let Some(event) = event else {
            return Ok(None);
        };
        if execution_output_matches(&event, &request.execution_id) {
            self.apply_execution_event(&event)?;
            return Ok(execution_output_from_event(event));
        }

        self.defer_execution_event(event)?;
        Ok(None)
    }

    fn apply_execution_event(&mut self, event: &ExecutionEvent) -> Result<(), BrowserSidecarError> {
        match event {
            ExecutionEvent::Stdout(chunk) => {
                let execution = self.ensure_execution_state(&chunk.vm_id, &chunk.execution_id)?;
                let vm = self.vm_mut(&chunk.vm_id)?;
                vm.kernel
                    .write_process_stdout(BROWSER_WORKER_DRIVER, execution.kernel_pid, &chunk.chunk)
                    .map_err(Self::kernel_error)?;
            }
            ExecutionEvent::Stderr(chunk) => {
                let execution = self.ensure_execution_state(&chunk.vm_id, &chunk.execution_id)?;
                let vm = self.vm_mut(&chunk.vm_id)?;
                vm.kernel
                    .write_process_stderr(BROWSER_WORKER_DRIVER, execution.kernel_pid, &chunk.chunk)
                    .map_err(Self::kernel_error)?;
            }
            ExecutionEvent::Exited(exited) => {
                let execution = self.ensure_execution_state(&exited.vm_id, &exited.execution_id)?;
                {
                    let vm = self.vm_mut(&exited.vm_id)?;
                    vm.kernel
                        .exit_process(
                            BROWSER_WORKER_DRIVER,
                            execution.kernel_pid,
                            exited.exit_code,
                        )
                        .map_err(Self::kernel_error)?;
                }
                self.release_execution(&exited.execution_id, "browser.worker.reaped")?;
            }
            ExecutionEvent::GuestRequest(call) => {
                let fields = unsupported_guest_kernel_call_detail(
                    None,
                    &call.execution_id,
                    &call.operation,
                    call.payload.len(),
                )
                .into_iter()
                .collect();
                self.emit_structured(
                    &call.vm_id,
                    agentos_native_sidecar_core::UNSUPPORTED_GUEST_KERNEL_CALL_EVENT,
                    fields,
                )?;
            }
            ExecutionEvent::SignalState(state) => {
                self.ensure_execution_state(&state.vm_id, &state.execution_id)?;
                let registration = protocol_signal_registration(&state.registration);
                let vm = self.vm_mut(&state.vm_id)?;
                apply_process_signal_state_update(
                    &mut vm.signal_states,
                    &state.execution_id,
                    state.signal,
                    registration,
                );
            }
        }
        Ok(())
    }

    fn ensure_deferred_execution_event_capacity(
        &self,
        vm_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        let capacity = self.config.max_deferred_execution_events_per_vm;
        if self.vm(vm_id)?.deferred_execution_events.len() >= capacity {
            return Err(BrowserSidecarError::LimitExceeded {
                limit: DEFERRED_EXECUTION_EVENTS_LIMIT,
                capacity,
                how_to_raise: "drain events with poll_execution_event or raise BrowserSidecarConfig::max_deferred_execution_events_per_vm",
            });
        }
        Ok(())
    }

    fn defer_execution_event(&mut self, event: ExecutionEvent) -> Result<(), BrowserSidecarError> {
        let vm_id = execution_event_vm_id(&event).to_string();
        let capacity = self.config.max_deferred_execution_events_per_vm;
        let warning_threshold = deferred_execution_event_warning_threshold(capacity);
        let vm = self.vm_mut(&vm_id)?;
        debug_assert!(vm.deferred_execution_events.len() < capacity);
        vm.deferred_execution_events.push_back(event);
        let observed = vm.deferred_execution_events.len();
        if observed >= warning_threshold && !vm.deferred_execution_events_warned {
            vm.deferred_execution_events_warned = true;
            tracing::warn!(
                limit = DEFERRED_EXECUTION_EVENTS_LIMIT,
                observed,
                capacity,
                "browser sidecar deferred execution event queue is near its limit; drain events with poll_execution_event or raise BrowserSidecarConfig::max_deferred_execution_events_per_vm"
            );
        }
        Ok(())
    }

    fn take_deferred_execution_event(
        &mut self,
        vm_id: &str,
    ) -> Result<Option<ExecutionEvent>, BrowserSidecarError> {
        let capacity = self.config.max_deferred_execution_events_per_vm;
        let warning_threshold = deferred_execution_event_warning_threshold(capacity);
        let vm = self.vm_mut(vm_id)?;
        let event = vm.deferred_execution_events.pop_front();
        if vm.deferred_execution_events.len() < warning_threshold {
            vm.deferred_execution_events_warned = false;
        }
        Ok(event)
    }

    fn take_deferred_execution_output(
        &mut self,
        vm_id: &str,
        execution_id: &str,
    ) -> Result<Option<ExecutionEvent>, BrowserSidecarError> {
        let capacity = self.config.max_deferred_execution_events_per_vm;
        let warning_threshold = deferred_execution_event_warning_threshold(capacity);
        let vm = self.vm_mut(vm_id)?;
        let event = vm
            .deferred_execution_events
            .iter()
            .position(|event| execution_output_matches(event, execution_id))
            .and_then(|index| vm.deferred_execution_events.remove(index));
        if vm.deferred_execution_events.len() < warning_threshold {
            vm.deferred_execution_events_warned = false;
        }
        Ok(event)
    }

    fn register_context(
        &mut self,
        vm_id: String,
        handle: GuestContextHandle,
        entrypoint: BrowserWorkerEntrypoint,
    ) -> Result<(), BrowserSidecarError> {
        self.contexts.insert(
            handle.context_id.clone(),
            ContextState {
                vm_id: vm_id.clone(),
                runtime: handle.runtime,
                entrypoint,
            },
        );
        let vm_state = self
            .vms
            .get_mut(&vm_id)
            .expect("VM should exist while registering a guest context");
        vm_state.contexts.insert(handle.context_id.clone());

        if let Err(error) = self.emit_structured(
            &vm_id,
            "browser.context.created",
            BTreeMap::from([
                (String::from("context_id"), handle.context_id.clone()),
                (
                    String::from("runtime"),
                    runtime_label(handle.runtime).to_string(),
                ),
            ]),
        ) {
            tracing::error!(vm_id, context_id = %handle.context_id, %error, "failed to emit browser context-created diagnostic after committing context");
        }
        Ok(())
    }

    pub fn release_context(
        &mut self,
        vm_id: &str,
        context_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        let Some(context) = self.contexts.get(context_id) else {
            return Ok(());
        };
        if context.vm_id != vm_id {
            return Err(BrowserSidecarError::InvalidState(format!(
                "browser sidecar context {context_id} belongs to vm {}, not {vm_id}",
                context.vm_id
            )));
        }
        if self
            .executions
            .values()
            .any(|execution| execution.context_id == context_id)
        {
            return Err(BrowserSidecarError::InvalidState(format!(
                "browser sidecar context {context_id} still has an active execution"
            )));
        }
        self.contexts.remove(context_id);
        if let Some(vm) = self.vms.get_mut(vm_id) {
            vm.contexts.remove(context_id);
        }
        if let Err(error) = self.emit_structured(
            vm_id,
            "browser.context.released",
            BTreeMap::from([(String::from("context_id"), context_id.to_string())]),
        ) {
            tracing::error!(vm_id, context_id, %error, "failed to emit browser context-release diagnostic after cleanup");
        }
        Ok(())
    }

    pub fn release_execution(
        &mut self,
        execution_id: &str,
        event_name: &'static str,
    ) -> Result<(), BrowserSidecarError> {
        if !self.execution_cleanups.contains_key(execution_id) {
            let Some(execution) = self.executions.remove(execution_id) else {
                return Ok(());
            };
            if let Some(vm_state) = self.vms.get_mut(&execution.vm_id) {
                vm_state.active_executions.remove(execution_id);
                vm_state.signal_states.remove(execution_id);
            }
            self.execution_cleanups.insert(
                execution_id.to_string(),
                ExecutionCleanupState {
                    execution,
                    event_name,
                    kernel_reaped: false,
                    worker_terminated: false,
                    structured_event_emitted: false,
                    lifecycle_event_emitted: false,
                },
            );
        }
        let mut cleanup = self
            .execution_cleanups
            .get(execution_id)
            .cloned()
            .expect("active execution was transitioned to cleanup state");
        let vm_id = cleanup.execution.vm_id.clone();
        let worker_id = cleanup.execution.worker.worker_id.clone();
        let mut errors = Vec::new();
        if !cleanup.kernel_reaped {
            match self.reap_execution_kernel_process(&vm_id, cleanup.execution.kernel_pid) {
                Ok(()) => cleanup.kernel_reaped = true,
                Err(error) => errors.push(BrowserSidecarError::Kernel(format!(
                    "execution {execution_id} kernel reap: {error}"
                ))),
            }
        }
        if !cleanup.worker_terminated {
            match self
                .bridge
                .terminate_worker(BrowserWorkerHandleRequest {
                    vm_id: vm_id.clone(),
                    execution_id: execution_id.to_string(),
                    worker_id: worker_id.clone(),
                })
                .map_err(Self::bridge_error)
            {
                Ok(()) => cleanup.worker_terminated = true,
                Err(error) => errors.push(BrowserSidecarError::Bridge(format!(
                    "execution {execution_id} worker {worker_id} termination: {error}"
                ))),
            }
        }
        if cleanup.kernel_reaped && cleanup.worker_terminated {
            if !cleanup.structured_event_emitted {
                match self.emit_structured(
                    &vm_id,
                    cleanup.event_name,
                    BTreeMap::from([
                        (String::from("execution_id"), execution_id.to_string()),
                        (
                            String::from("runtime"),
                            runtime_label(cleanup.execution.worker.runtime).to_string(),
                        ),
                        (String::from("worker_id"), worker_id.clone()),
                    ]),
                ) {
                    Ok(()) => cleanup.structured_event_emitted = true,
                    Err(error) => errors.push(error),
                }
            }
            if self.disposing_vms.contains(&vm_id) {
                cleanup.lifecycle_event_emitted = true;
            } else if !cleanup.lifecycle_event_emitted {
                let next_state = if self.active_worker_count(&vm_id) == 0 {
                    LifecycleState::Ready
                } else {
                    LifecycleState::Busy
                };
                match self.emit_lifecycle(
                    &vm_id,
                    next_state,
                    Some(String::from(
                        "browser sidecar worker bookkeeping was updated on the main thread",
                    )),
                ) {
                    Ok(()) => cleanup.lifecycle_event_emitted = true,
                    Err(error) => errors.push(error),
                }
            }
        }
        let complete = cleanup.kernel_reaped
            && cleanup.worker_terminated
            && cleanup.structured_event_emitted
            && cleanup.lifecycle_event_emitted;
        if complete {
            self.execution_cleanups.remove(execution_id);
        } else {
            self.execution_cleanups
                .insert(execution_id.to_string(), cleanup);
        }
        self.refresh_execution_cleanup_warning(&vm_id);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(BrowserSidecarError::Cleanup {
                context: "failed to release browser execution completely",
                errors,
            })
        }
    }

    fn ensure_vm(&self, vm_id: &str) -> Result<(), BrowserSidecarError> {
        if self.vms.contains_key(vm_id) && !self.disposing_vms.contains(vm_id) {
            Ok(())
        } else if self.disposing_vms.contains(vm_id) {
            Err(BrowserSidecarError::InvalidState(format!(
                "browser sidecar VM {vm_id} is being disposed"
            )))
        } else {
            Err(BrowserSidecarError::InvalidState(format!(
                "unknown browser sidecar VM: {vm_id}"
            )))
        }
    }

    fn reserve_execution_cleanup_capacity(
        &mut self,
        vm_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        let capacity = self.config.max_pending_execution_cleanups_per_vm;
        let active = self
            .vms
            .get(vm_id)
            .map(|vm| vm.active_executions.len())
            .unwrap_or_default();
        let cleaning = self
            .execution_cleanups
            .values()
            .filter(|cleanup| cleanup.execution.vm_id == vm_id)
            .count();
        let starting_cleanup = self
            .startup_cleanups
            .values()
            .filter(|cleanup| cleanup.vm_id == vm_id)
            .count();
        let observed = active
            .saturating_add(cleaning)
            .saturating_add(starting_cleanup);
        if observed >= capacity {
            return Err(BrowserSidecarError::LimitExceeded {
                limit: PENDING_EXECUTION_CLEANUPS_LIMIT,
                capacity,
                how_to_raise: "finish pending execution cleanup or raise BrowserSidecarConfig::max_pending_execution_cleanups_per_vm",
            });
        }
        let warning_threshold = deferred_execution_event_warning_threshold(capacity);
        if observed.saturating_add(1) >= warning_threshold {
            let vm = self
                .vms
                .get_mut(vm_id)
                .expect("VM exists after execution cleanup reservation validation");
            if !vm.pending_execution_cleanups_warned {
                vm.pending_execution_cleanups_warned = true;
                tracing::warn!(
                    vm_id,
                    limit = PENDING_EXECUTION_CLEANUPS_LIMIT,
                    observed = observed + 1,
                    capacity,
                    "browser sidecar execution cleanup reservations are near their limit; finish cleanup or raise BrowserSidecarConfig::max_pending_execution_cleanups_per_vm"
                );
            }
        }
        Ok(())
    }

    fn refresh_execution_cleanup_warning(&mut self, vm_id: &str) {
        let capacity = self.config.max_pending_execution_cleanups_per_vm;
        let cleaning = self
            .execution_cleanups
            .values()
            .filter(|cleanup| cleanup.execution.vm_id == vm_id)
            .count();
        if let Some(vm) = self.vms.get_mut(vm_id) {
            let observed = vm.active_executions.len().saturating_add(cleaning);
            if observed < deferred_execution_event_warning_threshold(capacity) {
                vm.pending_execution_cleanups_warned = false;
            }
        }
    }

    fn ensure_execution(&self, vm_id: &str, execution_id: &str) -> Result<(), BrowserSidecarError> {
        let execution = self.executions.get(execution_id).ok_or_else(|| {
            BrowserSidecarError::InvalidState(format!(
                "unknown browser sidecar execution: {execution_id}"
            ))
        })?;

        if execution.vm_id == vm_id {
            Ok(())
        } else {
            Err(BrowserSidecarError::InvalidState(format!(
                "browser sidecar execution {execution_id} belongs to vm {}, not {vm_id}",
                execution.vm_id
            )))
        }
    }

    fn ensure_execution_state(
        &self,
        vm_id: &str,
        execution_id: &str,
    ) -> Result<ExecutionState, BrowserSidecarError> {
        let execution = self.executions.get(execution_id).cloned().ok_or_else(|| {
            BrowserSidecarError::InvalidState(format!(
                "unknown browser sidecar execution: {execution_id}"
            ))
        })?;

        if execution.vm_id == vm_id {
            Ok(execution)
        } else {
            Err(BrowserSidecarError::InvalidState(format!(
                "browser sidecar execution {execution_id} belongs to vm {}, not {vm_id}",
                execution.vm_id
            )))
        }
    }

    fn vm(&self, vm_id: &str) -> Result<&VmState, BrowserSidecarError> {
        self.vms.get(vm_id).ok_or_else(|| {
            BrowserSidecarError::InvalidState(format!("unknown browser sidecar VM: {vm_id}"))
        })
    }

    fn vm_mut(&mut self, vm_id: &str) -> Result<&mut VmState, BrowserSidecarError> {
        self.vms.get_mut(vm_id).ok_or_else(|| {
            BrowserSidecarError::InvalidState(format!("unknown browser sidecar VM: {vm_id}"))
        })
    }

    fn emit_lifecycle(
        &mut self,
        vm_id: &str,
        state: LifecycleState,
        detail: Option<String>,
    ) -> Result<(), BrowserSidecarError> {
        self.bridge
            .emit_lifecycle(LifecycleEventRecord {
                vm_id: vm_id.to_string(),
                state,
                detail,
            })
            .map_err(Self::bridge_error)
    }

    fn emit_structured(
        &mut self,
        vm_id: &str,
        name: &str,
        fields: BTreeMap<String, String>,
    ) -> Result<(), BrowserSidecarError> {
        self.bridge
            .emit_structured_event(StructuredEventRecord {
                vm_id: vm_id.to_string(),
                name: name.to_string(),
                fields,
            })
            .map_err(Self::bridge_error)
    }

    fn bridge_error(error: BridgeError<B>) -> BrowserSidecarError {
        BrowserSidecarError::Bridge(format!("{error:?}"))
    }

    fn sidecar_core_error(
        error: agentos_native_sidecar_core::SidecarCoreError,
    ) -> BrowserSidecarError {
        let message = error.to_string();
        if message.split_once(':').is_some_and(|(code, _)| {
            code.len() >= 2
                && code.starts_with('E')
                && code[1..]
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        }) {
            BrowserSidecarError::Kernel(message)
        } else {
            BrowserSidecarError::InvalidState(message)
        }
    }

    fn kernel_error(error: KernelError) -> BrowserSidecarError {
        BrowserSidecarError::Kernel(error.to_string())
    }

    fn apply_root_filesystem_entry(
        &mut self,
        vm_id: &str,
        entry: &RootFilesystemEntry,
    ) -> Result<(), BrowserSidecarError> {
        // Shared with native: write the bootstrap entry through the VM's filesystem
        // (trusted, pre-guest; permissions default to allow at bootstrap time) with
        // deterministic mode/uid/gid defaults — see sidecar-core::apply_root_filesystem_entry.
        let filesystem = self.vm_mut(vm_id)?.kernel.filesystem_mut();
        agentos_native_sidecar_core::apply_root_filesystem_entry(filesystem, entry)
            .map_err(Self::sidecar_core_error)
    }
}

fn root_snapshot_from_entries(
    entries: &[RootFilesystemEntry],
) -> Result<RootFilesystemSnapshot, BrowserSidecarError> {
    // Shared with native (sidecar-core): protocol entries -> kernel snapshot.
    agentos_native_sidecar_core::root_snapshot_from_entries(entries)
        .map_err(|error| BrowserSidecarError::InvalidState(error.to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
fn browser_vm_fetch_timeout() -> Duration {
    std::env::var(BROWSER_VM_FETCH_TIMEOUT_MS_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(BROWSER_VM_FETCH_TIMEOUT)
}

#[cfg(target_arch = "wasm32")]
fn browser_vm_fetch_timeout() -> Duration {
    BROWSER_VM_FETCH_TIMEOUT
}

fn socket_host_matches(requested: Option<&str>, actual: &str) -> bool {
    match requested {
        None => true,
        Some(requested) if requested == actual => true,
        Some(requested)
            if is_unspecified_socket_host(requested) && is_unspecified_socket_host(actual) =>
        {
            true
        }
        Some(requested) if is_unspecified_socket_host(requested) => is_loopback_socket_host(actual),
        Some(requested) if requested.eq_ignore_ascii_case("localhost") => {
            is_loopback_socket_host(actual)
        }
        _ => false,
    }
}

fn is_unspecified_socket_host(host: &str) -> bool {
    host == "0.0.0.0" || host == "::"
}

fn is_loopback_socket_host(host: &str) -> bool {
    host == "127.0.0.1" || host == "::1" || host.eq_ignore_ascii_case("localhost")
}

fn execution_event_vm_id(event: &ExecutionEvent) -> &str {
    match event {
        ExecutionEvent::Stdout(chunk) | ExecutionEvent::Stderr(chunk) => &chunk.vm_id,
        ExecutionEvent::Exited(exited) => &exited.vm_id,
        ExecutionEvent::GuestRequest(call) => &call.vm_id,
        ExecutionEvent::SignalState(state) => &state.vm_id,
    }
}

fn execution_output_matches(event: &ExecutionEvent, execution_id: &str) -> bool {
    match event {
        ExecutionEvent::Stdout(chunk) | ExecutionEvent::Stderr(chunk) => {
            chunk.execution_id == execution_id
        }
        ExecutionEvent::Exited(exited) => exited.execution_id == execution_id,
        ExecutionEvent::GuestRequest(_) | ExecutionEvent::SignalState(_) => false,
    }
}

fn execution_output_from_event(event: ExecutionEvent) -> Option<ExecutionOutput> {
    match event {
        ExecutionEvent::Stdout(chunk) => Some(ExecutionOutput::Stdout(chunk)),
        ExecutionEvent::Stderr(chunk) => Some(ExecutionOutput::Stderr(chunk)),
        ExecutionEvent::Exited(exited) => Some(ExecutionOutput::Exited(exited)),
        ExecutionEvent::GuestRequest(_) | ExecutionEvent::SignalState(_) => None,
    }
}

fn deferred_execution_event_warning_threshold(capacity: usize) -> usize {
    capacity.saturating_mul(8).saturating_add(9) / 10
}

impl<B> BrowserExtensionHost for BrowserSidecar<B>
where
    B: BrowserSidecarBridge,
    BridgeError<B>: fmt::Debug,
{
    fn resolve_projected_agent(
        &mut self,
        vm_id: &str,
        id: &str,
    ) -> Result<Option<BrowserProjectedAgentLaunch>, BrowserSidecarError> {
        BrowserSidecar::resolve_projected_agent(self, vm_id, id)
    }

    fn list_projected_agents(
        &mut self,
        vm_id: &str,
    ) -> Result<Vec<BrowserProjectedAgentLaunch>, BrowserSidecarError> {
        BrowserSidecar::list_projected_agents(self, vm_id)
    }

    fn agent_additional_instructions(
        &mut self,
        vm_id: &str,
    ) -> Result<Option<String>, BrowserSidecarError> {
        BrowserSidecar::agent_additional_instructions(self, vm_id)
    }

    fn registered_host_tool_reference(
        &mut self,
        vm_id: &str,
    ) -> Result<String, BrowserSidecarError> {
        BrowserSidecar::registered_host_tool_reference(self, vm_id)
    }

    fn write_file(
        &mut self,
        vm_id: &str,
        path: &str,
        contents: Vec<u8>,
    ) -> Result<(), BrowserSidecarError> {
        BrowserSidecar::write_file(self, vm_id, path, contents)
    }

    fn read_file(&mut self, vm_id: &str, path: &str) -> Result<Vec<u8>, BrowserSidecarError> {
        BrowserSidecar::read_file(self, vm_id, path)
    }

    fn mkdir(
        &mut self,
        vm_id: &str,
        path: &str,
        recursive: bool,
    ) -> Result<(), BrowserSidecarError> {
        BrowserSidecar::mkdir(self, vm_id, path, recursive)
    }

    fn read_dir(&mut self, vm_id: &str, path: &str) -> Result<Vec<String>, BrowserSidecarError> {
        BrowserSidecar::read_dir(self, vm_id, path)
    }

    fn create_javascript_context(
        &mut self,
        request: CreateJavascriptContextRequest,
    ) -> Result<GuestContextHandle, BrowserSidecarError> {
        BrowserSidecar::create_javascript_context(self, request)
    }

    fn create_wasm_context(
        &mut self,
        request: CreateWasmContextRequest,
    ) -> Result<GuestContextHandle, BrowserSidecarError> {
        BrowserSidecar::create_wasm_context(self, request)
    }

    fn start_execution(
        &mut self,
        request: StartExecutionRequest,
    ) -> Result<StartedExecution, BrowserSidecarError> {
        BrowserSidecar::start_execution(self, request)
    }

    fn release_context(
        &mut self,
        vm_id: &str,
        context_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        BrowserSidecar::release_context(self, vm_id, context_id)
    }

    fn write_stdin(
        &mut self,
        request: WriteExecutionStdinRequest,
    ) -> Result<(), BrowserSidecarError> {
        BrowserSidecar::write_stdin(self, request)
    }

    fn close_stdin(&mut self, request: ExecutionHandleRequest) -> Result<(), BrowserSidecarError> {
        BrowserSidecar::close_stdin(self, request)
    }

    fn kill_execution(&mut self, request: KillExecutionRequest) -> Result<(), BrowserSidecarError> {
        BrowserSidecar::kill_execution(self, request)
    }

    fn release_execution(&mut self, execution_id: &str) -> Result<(), BrowserSidecarError> {
        BrowserSidecar::release_execution(self, execution_id, "browser.worker.acp_released")
    }

    fn abort_execution(
        &mut self,
        vm_id: &str,
        execution_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        BrowserSidecar::abort_execution(self, vm_id, execution_id)
    }

    fn poll_execution_event(
        &mut self,
        request: PollExecutionEventRequest,
    ) -> Result<Option<ExecutionEvent>, BrowserSidecarError> {
        BrowserSidecar::poll_execution_event(self, request)
    }

    fn poll_execution_output(
        &mut self,
        request: PollExecutionOutputRequest,
    ) -> Result<Option<ExecutionOutput>, BrowserSidecarError> {
        BrowserSidecar::poll_execution_output(self, request)
    }
}

fn runtime_label(runtime: GuestRuntime) -> &'static str {
    match runtime {
        GuestRuntime::JavaScript => "javascript",
        GuestRuntime::WebAssembly => "webassembly",
    }
}

fn protocol_signal_registration(
    registration: &SignalHandlerRegistration,
) -> ProtocolSignalHandlerRegistration {
    ProtocolSignalHandlerRegistration {
        action: match registration.action {
            SignalDispositionAction::Default => ProtocolSignalDispositionAction::Default,
            SignalDispositionAction::Ignore => ProtocolSignalDispositionAction::Ignore,
            SignalDispositionAction::User => ProtocolSignalDispositionAction::User,
        },
        mask: registration.mask.clone(),
        flags: registration.flags,
    }
}

#[cfg(test)]
impl<B> BrowserSidecar<B>
where
    B: BrowserSidecarBridge,
    BridgeError<B>: fmt::Debug,
{
    /// Test-only: number of entries still tracked in the global `contexts` map.
    pub(crate) fn test_total_context_count(&self) -> usize {
        self.contexts.len()
    }

    /// Test-only: active plus non-routable cleanup execution records.
    pub(crate) fn test_total_execution_count(&self) -> usize {
        self.executions.len() + self.execution_cleanups.len()
    }

    /// Test-only: inject a context directly into both the global `contexts` map
    /// and the owning VM's context set, bypassing the bridge round-trip so a
    /// dispose-path test can exercise cleanup at the smallest seam.
    pub(crate) fn test_insert_context(&mut self, vm_id: &str, context_id: &str) {
        self.contexts.insert(
            context_id.to_string(),
            ContextState {
                vm_id: vm_id.to_string(),
                runtime: GuestRuntime::JavaScript,
                entrypoint: BrowserWorkerEntrypoint::JavaScript {
                    bootstrap_module: None,
                },
            },
        );
        if let Some(vm) = self.vms.get_mut(vm_id) {
            vm.contexts.insert(context_id.to_string());
        }
    }

    /// Test-only: inject an active execution directly into both the global
    /// `executions` map and the owning VM's active-execution set.
    pub(crate) fn test_insert_execution(&mut self, vm_id: &str, execution_id: &str) {
        self.executions.insert(
            execution_id.to_string(),
            ExecutionState {
                vm_id: vm_id.to_string(),
                context_id: String::new(),
                worker: BrowserWorkerHandle {
                    worker_id: format!("worker-{execution_id}"),
                    runtime: GuestRuntime::JavaScript,
                },
                kernel_pid: 0,
                stdin_write_fd: 0,
                cwd: String::new(),
            },
        );
        if let Some(vm) = self.vms.get_mut(vm_id) {
            vm.active_executions.insert(execution_id.to_string());
        }
    }

    pub(crate) fn test_fail_next_kernel_cleanup(&mut self, message: impl Into<String>) {
        self.next_kernel_cleanup_error = Some(BrowserSidecarError::Kernel(message.into()));
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)] // browser_worker_identity below is shared with the worker shim
mod tests {
    use super::*;
    use agentos_bridge::{
        ChmodRequest, ClockRequest, CommandPermissionRequest, CreateDirRequest, DiagnosticRecord,
        DirectoryEntry, EnvironmentPermissionRequest, ExecutionHandleRequest, FileMetadata,
        FilesystemPermissionRequest, FilesystemSnapshot, FlushFilesystemStateRequest,
        LoadFilesystemStateRequest, LogRecord, NetworkPermissionRequest, PathRequest,
        PermissionDecision, RandomBytesRequest, ReadDirRequest, ReadFileRequest, RenameRequest,
        ScheduleTimerRequest, ScheduledTimer, SymlinkRequest, TruncateRequest, WriteFileRequest,
    };
    use agentos_bridge::{
        ClockBridge, EventBridge, ExecutionBridge, FilesystemBridge, PermissionBridge,
        PersistenceBridge, RandomBridge,
    };
    use agentos_kernel::kernel::KernelVmConfig;
    use std::time::SystemTime;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestBridgeError(String);

    /// Minimal bridge whose `terminate_worker` can be forced to fail, used to
    /// drive a mid-dispose error through `release_execution`.
    #[derive(Default)]
    struct TerminateFailingBridge {
        fail_terminate: bool,
        terminate_requests: Vec<BrowserWorkerHandleRequest>,
    }

    impl BridgeTypes for TerminateFailingBridge {
        type Error = TestBridgeError;
    }

    impl FilesystemBridge for TerminateFailingBridge {
        fn read_file(&mut self, _request: ReadFileRequest) -> Result<Vec<u8>, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn write_file(&mut self, _request: WriteFileRequest) -> Result<(), Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn stat(&mut self, _request: PathRequest) -> Result<FileMetadata, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn lstat(&mut self, _request: PathRequest) -> Result<FileMetadata, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn read_dir(
            &mut self,
            _request: ReadDirRequest,
        ) -> Result<Vec<DirectoryEntry>, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn create_dir(&mut self, _request: CreateDirRequest) -> Result<(), Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn remove_file(&mut self, _request: PathRequest) -> Result<(), Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn remove_dir(&mut self, _request: PathRequest) -> Result<(), Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn rename(&mut self, _request: RenameRequest) -> Result<(), Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn symlink(&mut self, _request: SymlinkRequest) -> Result<(), Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn read_link(&mut self, _request: PathRequest) -> Result<String, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn chmod(&mut self, _request: ChmodRequest) -> Result<(), Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn truncate(&mut self, _request: TruncateRequest) -> Result<(), Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn exists(&mut self, _request: PathRequest) -> Result<bool, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
    }

    impl PermissionBridge for TerminateFailingBridge {
        fn check_filesystem_access(
            &mut self,
            _request: FilesystemPermissionRequest,
        ) -> Result<PermissionDecision, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn check_network_access(
            &mut self,
            _request: NetworkPermissionRequest,
        ) -> Result<PermissionDecision, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn check_command_execution(
            &mut self,
            _request: CommandPermissionRequest,
        ) -> Result<PermissionDecision, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn check_environment_access(
            &mut self,
            _request: EnvironmentPermissionRequest,
        ) -> Result<PermissionDecision, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
    }

    impl PersistenceBridge for TerminateFailingBridge {
        fn load_filesystem_state(
            &mut self,
            _request: LoadFilesystemStateRequest,
        ) -> Result<Option<FilesystemSnapshot>, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn flush_filesystem_state(
            &mut self,
            _request: FlushFilesystemStateRequest,
        ) -> Result<(), Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
    }

    impl ClockBridge for TerminateFailingBridge {
        fn wall_clock(&mut self, _request: ClockRequest) -> Result<SystemTime, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn monotonic_clock(&mut self, _request: ClockRequest) -> Result<Duration, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn schedule_timer(
            &mut self,
            _request: ScheduleTimerRequest,
        ) -> Result<ScheduledTimer, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
    }

    impl RandomBridge for TerminateFailingBridge {
        fn fill_random_bytes(
            &mut self,
            _request: RandomBytesRequest,
        ) -> Result<Vec<u8>, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
    }

    impl EventBridge for TerminateFailingBridge {
        fn emit_structured_event(
            &mut self,
            _event: StructuredEventRecord,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
        fn emit_diagnostic(&mut self, _event: DiagnosticRecord) -> Result<(), Self::Error> {
            Ok(())
        }
        fn emit_log(&mut self, _event: LogRecord) -> Result<(), Self::Error> {
            Ok(())
        }
        fn emit_lifecycle(&mut self, _event: LifecycleEventRecord) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    impl ExecutionBridge for TerminateFailingBridge {
        fn create_javascript_context(
            &mut self,
            _request: CreateJavascriptContextRequest,
        ) -> Result<GuestContextHandle, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn create_wasm_context(
            &mut self,
            _request: CreateWasmContextRequest,
        ) -> Result<GuestContextHandle, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn start_execution(
            &mut self,
            _request: StartExecutionRequest,
        ) -> Result<StartedExecution, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn write_stdin(&mut self, _request: WriteExecutionStdinRequest) -> Result<(), Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn close_stdin(&mut self, _request: ExecutionHandleRequest) -> Result<(), Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn kill_execution(&mut self, _request: KillExecutionRequest) -> Result<(), Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
        fn poll_execution_event(
            &mut self,
            _request: PollExecutionEventRequest,
        ) -> Result<Option<ExecutionEvent>, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }
    }

    impl crate::BrowserWorkerBridge for TerminateFailingBridge {
        fn create_worker(
            &mut self,
            _request: BrowserWorkerSpawnRequest,
        ) -> Result<BrowserWorkerHandle, Self::Error> {
            unimplemented!("not exercised by dispose test")
        }

        fn terminate_worker(
            &mut self,
            request: BrowserWorkerHandleRequest,
        ) -> Result<(), Self::Error> {
            self.terminate_requests.push(request);
            if self.fail_terminate {
                Err(TestBridgeError(String::from("forced terminate failure")))
            } else {
                Ok(())
            }
        }
    }

    // A mid-dispose worker-termination failure keeps only non-routable cleanup
    // ownership, then an exact retry releases the retained worker and VM.
    #[test]
    fn dispose_vm_retries_retained_worker_cleanup_before_releasing_vm() {
        let bridge = TerminateFailingBridge {
            fail_terminate: true,
            ..TerminateFailingBridge::default()
        };
        let mut sidecar = BrowserSidecar::new(bridge, BrowserSidecarConfig::default());

        sidecar
            .create_vm(KernelVmConfig::new("vm-leak"))
            .expect("create vm");
        sidecar.test_insert_context("vm-leak", "ctx-leak");
        sidecar.test_insert_execution("vm-leak", "exec-leak");

        assert_eq!(sidecar.vm_count(), 1);
        assert_eq!(sidecar.test_total_context_count(), 1);
        assert_eq!(sidecar.test_total_execution_count(), 1);

        let result = sidecar.dispose_vm("vm-leak");
        assert!(result.is_err(), "forced terminate failure should surface");

        assert_eq!(
            sidecar.vm_count(),
            1,
            "cleanup retains the VM kernel handle"
        );
        assert_eq!(
            sidecar.test_total_context_count(),
            1,
            "cleanup retains the context only until execution cleanup completes"
        );
        assert_eq!(
            sidecar.test_total_execution_count(),
            1,
            "failed worker handle must remain retryable"
        );
        assert_eq!(sidecar.active_worker_count("vm-leak"), 0);

        sidecar.bridge_mut().fail_terminate = false;
        sidecar
            .dispose_vm("vm-leak")
            .expect("cleanup retry succeeds");
        assert_eq!(sidecar.vm_count(), 0);
        assert_eq!(sidecar.test_total_context_count(), 0);
        assert_eq!(sidecar.test_total_execution_count(), 0);
        assert_eq!(sidecar.bridge().terminate_requests.len(), 2);
    }

    #[test]
    fn retained_vm_cleanup_consumes_vm_capacity_until_retry_succeeds() {
        let bridge = TerminateFailingBridge {
            fail_terminate: true,
            ..TerminateFailingBridge::default()
        };
        let mut sidecar = BrowserSidecar::new(
            bridge,
            BrowserSidecarConfig {
                max_vms: 1,
                ..BrowserSidecarConfig::default()
            },
        );
        sidecar
            .create_vm(KernelVmConfig::new("vm-retained"))
            .expect("first VM fits");
        sidecar.test_insert_execution("vm-retained", "exec-retained");
        sidecar
            .dispose_vm("vm-retained")
            .expect_err("failed worker cleanup retains VM ownership");

        let error = sidecar
            .create_vm(KernelVmConfig::new("vm-blocked"))
            .expect_err("retained cleanup remains charged to max_vms");
        assert!(matches!(
            error,
            BrowserSidecarError::LimitExceeded {
                limit: "max_vms",
                capacity: 1,
                ..
            }
        ));

        sidecar.bridge_mut().fail_terminate = false;
        sidecar
            .dispose_vm("vm-retained")
            .expect("retry releases retained VM capacity");
        sidecar
            .create_vm(KernelVmConfig::new("vm-after-retry"))
            .expect("VM admission succeeds after cleanup");
    }

    #[test]
    fn release_execution_terminates_worker_after_kernel_cleanup_failure() {
        let mut sidecar = BrowserSidecar::new(
            TerminateFailingBridge::default(),
            BrowserSidecarConfig::default(),
        );
        sidecar
            .create_vm(KernelVmConfig::new("vm-cleanup"))
            .expect("create vm");
        sidecar.test_insert_execution("vm-cleanup", "exec-cleanup");
        sidecar.test_fail_next_kernel_cleanup("forced kernel cleanup failure");

        let error = sidecar
            .release_execution("exec-cleanup", "browser.worker.test_released")
            .expect_err("kernel cleanup failure must surface");
        assert!(error.to_string().contains("forced kernel cleanup failure"));
        assert_eq!(sidecar.test_total_execution_count(), 1);
        assert_eq!(sidecar.active_worker_count("vm-cleanup"), 0);
        assert_eq!(
            sidecar.bridge().terminate_requests,
            vec![BrowserWorkerHandleRequest {
                vm_id: String::from("vm-cleanup"),
                execution_id: String::from("exec-cleanup"),
                worker_id: String::from("worker-exec-cleanup"),
            }]
        );
        sidecar
            .release_execution("exec-cleanup", "browser.worker.test_released")
            .expect("retry reaps the kernel without terminating the worker twice");
        assert_eq!(sidecar.test_total_execution_count(), 0);
        assert_eq!(sidecar.bridge().terminate_requests.len(), 1);
    }

    #[test]
    fn release_execution_preserves_both_errors_and_retries_incomplete_phases() {
        let mut sidecar = BrowserSidecar::new(
            TerminateFailingBridge {
                fail_terminate: true,
                ..TerminateFailingBridge::default()
            },
            BrowserSidecarConfig::default(),
        );
        sidecar
            .create_vm(KernelVmConfig::new("vm-cleanup"))
            .expect("create vm");
        sidecar.test_insert_execution("vm-cleanup", "exec-cleanup");
        sidecar.test_fail_next_kernel_cleanup("forced kernel cleanup failure");

        let error = sidecar
            .release_execution("exec-cleanup", "browser.worker.test_released")
            .expect_err("both cleanup failures must surface");
        let message = error.to_string();
        assert!(message.contains("forced kernel cleanup failure"));
        assert!(message.contains("forced terminate failure"));
        assert_eq!(sidecar.test_total_execution_count(), 1);
        assert_eq!(sidecar.active_worker_count("vm-cleanup"), 0);
        assert_eq!(sidecar.bridge().terminate_requests.len(), 1);
        sidecar.bridge_mut().fail_terminate = false;
        sidecar
            .release_execution("exec-cleanup", "browser.worker.test_released")
            .expect("retry completes both failed phases");
        assert_eq!(sidecar.test_total_execution_count(), 0);
        assert_eq!(sidecar.bridge().terminate_requests.len(), 2);
    }
}

fn ensure_projected_package_limit(
    limit: &'static str,
    existing: usize,
    incoming: usize,
    capacity: usize,
    how_to_raise: &'static str,
) -> Result<(), BrowserSidecarError> {
    let observed = existing
        .checked_add(incoming)
        .ok_or(BrowserSidecarError::LimitExceeded {
            limit,
            capacity,
            how_to_raise,
        })?;
    if observed > capacity {
        return Err(BrowserSidecarError::LimitExceeded {
            limit,
            capacity,
            how_to_raise,
        });
    }
    let warning_threshold = capacity.saturating_mul(8) / 10;
    if incoming > 0 && observed >= warning_threshold {
        tracing::warn!(
            limit,
            observed,
            capacity,
            "browser package projection is near its limit"
        );
    }
    Ok(())
}

fn package_projection_error(error: BrowserPackageProjectionError) -> BrowserSidecarError {
    match error {
        BrowserPackageProjectionError::Invalid(message) => {
            BrowserSidecarError::InvalidPackage(message)
        }
        BrowserPackageProjectionError::LimitExceeded {
            limit,
            capacity,
            how_to_raise,
        } => BrowserSidecarError::LimitExceeded {
            limit,
            capacity,
            how_to_raise,
        },
    }
}

struct PackageMountFailure {
    path: String,
    detail: String,
    mounted: Vec<String>,
}

fn mount_prepared_package_batch(
    kernel: &mut BrowserKernel,
    prepared: &mut [PreparedBrowserPackage],
    skip_existing: bool,
) -> Result<Vec<String>, PackageMountFailure> {
    let mut existing_paths = if skip_existing {
        kernel
            .mounted_filesystems()
            .into_iter()
            .map(|mount| mount.path)
            .collect::<BTreeSet<_>>()
    } else {
        BTreeSet::new()
    };
    let mut mounted = Vec::new();
    for package in prepared {
        for mount in std::mem::take(&mut package.mounts) {
            let path = mount.guest_path().to_owned();
            if existing_paths.contains(&path) {
                continue;
            }
            let result = match mount {
                PreparedBrowserPackageMount::Files { filesystem, .. } => {
                    kernel.filesystem_mut().inner_mut().inner_mut().mount(
                        &path,
                        filesystem,
                        MountOptions::new("agentos_package").read_only(true),
                    )
                }
                PreparedBrowserPackageMount::Symlink { filesystem, .. } => {
                    kernel.filesystem_mut().inner_mut().inner_mut().mount(
                        &path,
                        filesystem,
                        MountOptions::new("agentos_package").read_only(true),
                    )
                }
            };
            if let Err(error) = result {
                return Err(PackageMountFailure {
                    path,
                    detail: error.to_string(),
                    mounted,
                });
            }
            existing_paths.insert(path.clone());
            mounted.push(path);
        }
    }
    Ok(mounted)
}

fn package_mount_failure(
    vm_id: &str,
    failure: PackageMountFailure,
    rollback: Result<(), String>,
) -> BrowserSidecarError {
    match rollback {
        Ok(()) => BrowserSidecarError::PackageMount(format!(
            "mount .aospkg projection for VM {vm_id:?} at {}: {}",
            failure.path, failure.detail
        )),
        Err(rollback_error) => BrowserSidecarError::PackageStateCorrupt(format!(
            "mount .aospkg projection for VM {vm_id:?} at {}: {}; rollback failed: {rollback_error}",
            failure.path, failure.detail
        )),
    }
}

fn commit_projected_package_batch(
    vm: &mut VmState,
    prepared: Vec<PreparedBrowserPackage>,
    mounted_paths: Vec<String>,
    mount_root: &str,
    replacing: bool,
) -> Vec<BrowserProjectedPackage> {
    if replacing {
        vm.projected_agent_launch.clear();
        vm.projected_package_names.clear();
        vm.projected_command_names.clear();
        vm.projected_package_bytes = 0;
        vm.projected_package_entries = 0;
        vm.projected_package_materialized_bytes = 0;
        vm.projected_package_sources.clear();
        vm.projected_packages.clear();
        vm.projected_package_mount_paths.clear();
        vm.projected_package_env.clear();
        vm.provided_commands.clear();
    }
    vm.projected_package_mount_root = mount_root.to_owned();
    vm.projected_package_mount_paths.extend(mounted_paths);

    let mut projections = Vec::with_capacity(prepared.len());
    for package in prepared {
        let projection = package.projection;
        vm.projected_package_names.insert(projection.name.clone());
        vm.projected_command_names
            .extend(projection.commands.iter().cloned());
        vm.projected_package_bytes = vm
            .projected_package_bytes
            .checked_add(package.source_bytes.len())
            .expect("validated projected package byte count must fit usize");
        vm.projected_package_entries = vm
            .projected_package_entries
            .checked_add(package.index_entries)
            .expect("validated projected package entry count must fit usize");
        vm.projected_package_materialized_bytes = vm
            .projected_package_materialized_bytes
            .checked_add(package.materialized_bytes)
            .expect("validated projected package materialized byte count must fit usize");
        vm.projected_package_sources.push(package.source_bytes);
        vm.projected_packages.push(projection.clone());
        vm.provided_commands
            .insert(projection.name.clone(), projection.commands.clone());
        for (name, value) in &projection.provided_env {
            vm.projected_package_env
                .entry(name.clone())
                .or_insert_with(|| value.clone());
        }
        if let Some(agent) = &projection.agent {
            vm.projected_agent_launch.insert(
                agent.id.clone(),
                BrowserProjectedAgentLaunch {
                    id: agent.id.clone(),
                    adapter_entrypoint: agent.adapter_entrypoint.clone(),
                    env: agent.env.clone(),
                    launch_args: agent.launch_args.clone(),
                },
            );
        }
        projections.push(projection);
    }
    projections
}

fn rollback_projected_package_mounts(
    kernel: &mut BrowserKernel,
    mounted_paths: &[String],
) -> Result<(), String> {
    let mut failures = Vec::new();
    let mut mounted_paths = mounted_paths.iter().collect::<Vec<_>>();
    mounted_paths.sort_by_key(|path| std::cmp::Reverse(path.matches('/').count()));
    for path in mounted_paths {
        if let Err(error) = kernel
            .filesystem_mut()
            .inner_mut()
            .inner_mut()
            .unmount(path)
        {
            failures.push(format!("unmount {path}: {error}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn browser_worker_identity(
    kernel: &BrowserKernel,
    projected_package_env: &BTreeMap<String, String>,
    request: &StartExecutionRequest,
    kernel_pid: u32,
) -> (BrowserWorkerProcessConfig, BrowserWorkerOsConfig) {
    let mut env = kernel.environment().clone();
    for (name, value) in projected_package_env {
        env.entry(name.clone()).or_insert_with(|| value.clone());
    }
    env.extend(request.env.clone());
    let user = kernel.user_profile();
    let resource_limits = kernel.resource_limits();
    let identity =
        shared_guest_runtime_identity(&user, resource_limits, Some(u64::from(kernel_pid)), Some(0));

    (
        BrowserWorkerProcessConfig {
            cwd: request.cwd.clone(),
            env,
            argv: request.argv.clone(),
            platform: identity.process_platform.clone(),
            arch: identity.process_arch.clone(),
            version: String::from("v22.0.0"),
            pid: kernel_pid,
            ppid: 0,
            uid: identity.virtual_uid as u32,
            gid: identity.virtual_gid as u32,
        },
        BrowserWorkerOsConfig {
            platform: identity.process_platform,
            arch: identity.process_arch.clone(),
            r#type: identity.os_type,
            release: identity.os_release,
            version: identity.os_version,
            cpu_count: identity.os_cpu_count,
            totalmem: identity.os_totalmem,
            freemem: identity.os_freemem,
            hostname: identity.os_hostname,
            homedir: identity.os_homedir,
            tmpdir: identity.os_tmpdir,
            machine: identity.os_machine,
            user: identity.os_user,
            shell: identity.os_shell,
            uid: identity.virtual_uid as u32,
            gid: identity.virtual_gid as u32,
        },
    )
}
