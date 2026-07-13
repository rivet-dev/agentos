use crate::bridge::{build_mount_plugin_registry, MountPluginContext};
pub(crate) use crate::execution::{
    build_javascript_socket_path_context, canonical_signal_name, dispatch_loopback_http_request,
    error_code, format_tcp_resource, ignore_stale_javascript_sync_rpc_response,
    javascript_sync_rpc_arg_i32, javascript_sync_rpc_arg_str, javascript_sync_rpc_arg_u32,
    javascript_sync_rpc_arg_u32_optional, javascript_sync_rpc_arg_u64,
    javascript_sync_rpc_arg_u64_optional, javascript_sync_rpc_bytes_arg,
    javascript_sync_rpc_bytes_value, javascript_sync_rpc_encoding, javascript_sync_rpc_error_code,
    javascript_sync_rpc_option_bool, javascript_sync_rpc_option_u32, kernel_poll_response,
    kernel_stdin_read_response, mark_execute_exit_event_queued, parse_kernel_poll_args,
    parse_kernel_stdin_read_args, parse_signal, record_execute_exit_event_queue_wait,
    record_execute_phase, sanitize_javascript_child_process_internal_bootstrap_env,
    service_javascript_sync_rpc, vm_network_resource_counts, JavascriptSyncRpcServiceRequest,
    LoopbackHttpDispatchRequest,
};
use crate::extension::{
    Extension, ExtensionBufferedProcessOutput, ExtensionContext, ExtensionFuture, ExtensionHost,
    ExtensionSnapshot,
};
use crate::filesystem::guest_filesystem_call as filesystem_guest_filesystem_call;
use crate::limits::DEFAULT_ACP_STDOUT_BUFFER_BYTE_LIMIT;
use crate::protocol::{
    CancelCronJobRequest, CloseStdinRequest, CompleteCronRunRequest, CronAlarm, CronDispatchEvent,
    CronEventRecord, CronRun, DisposeReason, EventFrame, EventPayload, ExecuteRequest, ExtEnvelope,
    GuestFilesystemCallRequest, GuestFilesystemResultResponse, JavascriptChildProcessSpawnOptions,
    JavascriptChildProcessSpawnRequest, KillProcessRequest, OpenSessionRequest, OwnershipScope,
    ProcessKilledResponse, ProcessStartedResponse, PtyResizedResponse, RequestFrame, RequestId,
    RequestPayload, ResizePtyRequest, ResponseFrame, ResponsePayload, ScheduleCronRequest,
    SidecarRequestFrame, SidecarRequestPayload, SidecarResponseFrame, SidecarResponsePayload,
    SidecarResponseTracker, SidecarResponseTrackerError, SignalDispositionAction,
    StdinClosedResponse, StdinWrittenResponse, VmLifecycleState, WakeCronRequest,
    WriteStdinRequest,
};
use crate::state::{
    ActiveExecutionEvent, BridgeError, ConnectionState, EventSinkTransport, JavascriptSocketFamily,
    JavascriptSocketPathContext, ProcessEventEnvelope, SessionState, SharedBridge, SharedEventSink,
    SharedSidecarRequestClient, SidecarRequestTransport, VmState, EXECUTION_DRIVER_NAME,
};
use crate::tools::register_host_callbacks;
use crate::NativeSidecarBridge;
use agentos_bridge::queue_tracker::{register_queue, QueueGauge, TrackedLimit};
use agentos_bridge::{
    CommandPermissionRequest, EnvironmentAccess, EnvironmentPermissionRequest, FilesystemAccess,
    FilesystemPermissionRequest, LifecycleEventRecord, LifecycleState, LogLevel, LogRecord,
    NetworkAccess, NetworkPermissionRequest, StructuredEventRecord,
};
use agentos_execution::{
    record_sync_bridge_request_observed, JavascriptExecutionEngine, JavascriptExecutionError,
    JavascriptSyncRpcRequest, PythonExecutionEngine, PythonExecutionError, WasmExecutionEngine,
    WasmExecutionError,
};
use agentos_kernel::kernel::KernelError;
use agentos_kernel::mount_plugin::{FileSystemPluginRegistry, PluginError};
use agentos_kernel::permissions::{
    CommandAccessRequest, EnvAccessRequest, EnvironmentOperation, NetworkAccessRequest,
    NetworkOperation, PermissionDecision,
};
use agentos_native_sidecar_core::permissions::{
    deny_all_policy, environment_permission_capability, evaluate_permissions_policy,
    filesystem_permission_capability, network_permission_capability,
    permission_mode_to_kernel_decision,
};
use agentos_native_sidecar_core::{
    apply_process_signal_state_update, authenticated_response as shared_authenticated_response,
    parse_process_signal_state_request, reject as shared_reject, request_dispatch_mode,
    respond as shared_respond, route_request_payload, session_opened_response,
    unsupported_host_callback_direction_dispatch, validate_authenticate_versions,
    vm_lifecycle_event as shared_vm_lifecycle_event, AuthenticateVersionError, CronAction,
    CronScheduler, RequestDispatchMode, RequestRoute, MAX_CRON_JOBS, MAX_CRON_STATE_BYTES,
};
use agentos_protocol::generated::v1::{
    AcpCloseSessionRequest, AcpCreateSessionRequest, AcpRequest, AcpResponse, AcpSessionRequest,
};
use agentos_protocol::ACP_EXTENSION_NAMESPACE;
use agentos_vm_config::PermissionsPolicy;
// root_fs types moved to crate::vm
use agentos_kernel::vfs::VfsError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::time;

// Constants and type aliases moved to crate::state

const INTERNAL_JAVASCRIPT_ENTRYPOINT_ENV_KEYS: &[&str] =
    &["AGENTOS_ENTRYPOINT", "AGENTOS_BOOTSTRAP_MODULE"];
const INTERNAL_WASM_ENTRYPOINT_ENV_KEYS: &[&str] =
    &["AGENTOS_WASM_MODULE_PATH", "AGENTOS_WASM_MODULE_BASE64"];
const INTERNAL_PYTHON_ENTRYPOINT_ENV_PREFIXES: &[&str] = &["AGENTOS_PYTHON_"];
pub(crate) const MAX_PROCESS_EVENT_QUEUE: usize = 10_000;
pub(crate) const MAX_PENDING_SIDECAR_RESPONSES: usize = 10_000;
pub(crate) const MAX_OUTBOUND_SIDECAR_REQUESTS: usize = 10_000;
pub(crate) const MAX_COMPLETED_SIDECAR_RESPONSES: usize = 10_000;
static BLOCKING_DISPATCH_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn blocking_dispatch_runtime() -> &'static tokio::runtime::Runtime {
    BLOCKING_DISPATCH_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("sidecar blocking dispatch runtime")
    })
}

pub(crate) fn process_event_queue_overflow_error() -> SidecarError {
    SidecarError::InvalidState(format!(
        "process event queue exceeded {MAX_PROCESS_EVENT_QUEUE} pending events"
    ))
}

fn sidecar_response_pending_overflow_error() -> SidecarError {
    SidecarError::InvalidState(format!(
        "sidecar response tracker exceeded {MAX_PENDING_SIDECAR_RESPONSES} pending responses"
    ))
}

fn outbound_sidecar_request_queue_overflow_error() -> SidecarError {
    SidecarError::InvalidState(format!(
        "outbound sidecar request queue exceeded {MAX_OUTBOUND_SIDECAR_REQUESTS} pending requests"
    ))
}

fn wire_protocol_error(error: crate::wire::ProtocolCodecError) -> SidecarError {
    SidecarError::InvalidState(format!("invalid generated wire protocol frame: {error}"))
}

fn wire_dispatch_result(
    result: DispatchResult,
) -> Result<crate::wire::WireDispatchResult, SidecarError> {
    crate::wire::dispatch_result_from_compat(crate::wire::CompatDispatchResult {
        response: result.response,
        events: result.events,
    })
    .map_err(wire_protocol_error)
}

pub use agentos_native_sidecar_core::DispatchResult;
// NativeSidecarConfig and SidecarError moved to crate::state
pub use crate::state::{NativeSidecarConfig, SidecarError};

// SharedBridge struct and Clone impl moved to crate::state

#[derive(Debug, Default, Deserialize)]
struct LegacyJavascriptChildProcessSpawnOptions {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    input: Option<Value>,
    #[serde(default)]
    shell: bool,
    #[serde(default)]
    detached: bool,
    #[serde(default)]
    stdio: Vec<String>,
    #[serde(default, rename = "maxBuffer")]
    max_buffer: Option<usize>,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default, rename = "killSignal")]
    kill_signal: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JavascriptHttpLoopbackRequest {
    process_id: String,
    server_id: u64,
    host: String,
    port: u16,
    request: String,
}

fn is_javascript_loopback_host(host: &str) -> bool {
    host == "127.0.0.1" || host == "::1" || host.eq_ignore_ascii_case("localhost")
}

pub(crate) fn parse_javascript_child_process_spawn_request(
    vm: &VmState,
    args: &[Value],
) -> Result<(JavascriptChildProcessSpawnRequest, Option<usize>), SidecarError> {
    if let Some(value) = args.first().cloned() {
        if let Ok(request) = serde_json::from_value::<JavascriptChildProcessSpawnRequest>(value) {
            return Ok((request, None));
        }
    }

    let command = javascript_sync_rpc_arg_str(args, 0, "child_process.spawn command")?.to_owned();
    let raw_args = javascript_sync_rpc_arg_str(args, 1, "child_process.spawn args")?;
    let raw_options = javascript_sync_rpc_arg_str(args, 2, "child_process.spawn options")?;

    let parsed_args = serde_json::from_str::<Vec<String>>(raw_args).map_err(|error| {
        SidecarError::InvalidState(format!("invalid child_process.spawn args payload: {error}"))
    })?;
    let parsed_options = serde_json::from_str::<LegacyJavascriptChildProcessSpawnOptions>(
        raw_options,
    )
    .map_err(|error| {
        SidecarError::InvalidState(format!(
            "invalid child_process.spawn options payload: {error}"
        ))
    })?;

    Ok((
        JavascriptChildProcessSpawnRequest {
            command,
            args: parsed_args,
            options: JavascriptChildProcessSpawnOptions {
                cwd: parsed_options.cwd,
                env: parsed_options.env,
                internal_bootstrap_env: sanitize_javascript_child_process_internal_bootstrap_env(
                    &vm.guest_env,
                ),
                input: parsed_options.input,
                shell: parsed_options.shell,
                detached: parsed_options.detached,
                stdio: parsed_options.stdio,
                timeout: parsed_options.timeout,
                kill_signal: parsed_options.kill_signal,
            },
        },
        parsed_options.max_buffer,
    ))
}

impl<B> SharedBridge<B> {
    fn new(bridge: B) -> Self {
        Self {
            inner: Arc::new(Mutex::new(bridge)),
            permissions: Arc::new(Mutex::new(BTreeMap::new())),
            #[cfg(test)]
            set_vm_permissions_outcomes: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

impl<B> SharedBridge<B>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    pub(crate) fn with_mut<T>(
        &self,
        operation: impl FnOnce(&mut B) -> Result<T, BridgeError<B>>,
    ) -> Result<T, SidecarError> {
        let mut bridge = self.inner.lock().map_err(|_| {
            SidecarError::Bridge(String::from("native sidecar bridge lock poisoned"))
        })?;
        operation(&mut bridge).map_err(|error| SidecarError::Bridge(format!("{error:?}")))
    }

    fn inspect<T>(&self, operation: impl FnOnce(&mut B) -> T) -> Result<T, SidecarError> {
        let mut bridge = self.inner.lock().map_err(|_| {
            SidecarError::Bridge(String::from("native sidecar bridge lock poisoned"))
        })?;
        Ok(operation(&mut bridge))
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn queue_set_vm_permissions_result(
        &self,
        result: Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        let mut outcomes = self.set_vm_permissions_outcomes.lock().map_err(|_| {
            SidecarError::Bridge(String::from(
                "native sidecar test set_vm_permissions outcome lock poisoned",
            ))
        })?;
        outcomes.push_back(result.err());
        Ok(())
    }

    pub(crate) fn emit_lifecycle(
        &self,
        vm_id: &str,
        state: LifecycleState,
    ) -> Result<(), SidecarError> {
        self.with_mut(|bridge| {
            bridge.emit_lifecycle(LifecycleEventRecord {
                vm_id: vm_id.to_owned(),
                state,
                detail: None,
            })
        })
    }

    pub(crate) fn emit_log(
        &self,
        vm_id: &str,
        message: impl Into<String>,
    ) -> Result<(), SidecarError> {
        self.with_mut(|bridge| {
            bridge.emit_log(LogRecord {
                vm_id: vm_id.to_owned(),
                level: LogLevel::Info,
                message: message.into(),
            })
        })
    }

    pub(crate) fn filesystem_decision(
        &self,
        vm_id: &str,
        path: &str,
        access: FilesystemAccess,
    ) -> PermissionDecision {
        if let Some(decision) = self.static_permission_decision(
            vm_id,
            filesystem_permission_capability(access),
            "fs",
            Some(path),
        ) {
            return decision;
        }
        match self.with_mut(|bridge| {
            bridge.check_filesystem_access(FilesystemPermissionRequest {
                vm_id: vm_id.to_owned(),
                path: path.to_owned(),
                access,
            })
        }) {
            Ok(decision) => map_bridge_permission(decision),
            Err(error) => PermissionDecision::deny(error.to_string()),
        }
    }

    pub(crate) fn command_decision(
        &self,
        vm_id: &str,
        request: &CommandAccessRequest,
    ) -> PermissionDecision {
        if is_internal_runtime_command_request(request) {
            return PermissionDecision::allow();
        }
        if let Some(decision) = self.static_permission_decision(
            vm_id,
            "child_process.spawn",
            "child_process",
            Some(&request.command),
        ) {
            return decision;
        }
        match self.with_mut(|bridge| {
            bridge.check_command_execution(CommandPermissionRequest {
                vm_id: vm_id.to_owned(),
                command: request.command.clone(),
                args: request.args.clone(),
                cwd: request.cwd.clone(),
                env: request.env.clone(),
            })
        }) {
            Ok(decision) => map_bridge_permission(decision),
            Err(error) => PermissionDecision::deny(error.to_string()),
        }
    }

    pub(crate) fn environment_decision(
        &self,
        vm_id: &str,
        request: &EnvAccessRequest,
    ) -> PermissionDecision {
        if let Some(decision) = self.static_permission_decision(
            vm_id,
            environment_permission_capability(request.op),
            "env",
            Some(&request.key),
        ) {
            return decision;
        }
        match self.with_mut(|bridge| {
            bridge.check_environment_access(EnvironmentPermissionRequest {
                vm_id: vm_id.to_owned(),
                access: match request.op {
                    EnvironmentOperation::Read => EnvironmentAccess::Read,
                    EnvironmentOperation::Write => EnvironmentAccess::Write,
                },
                key: request.key.clone(),
                value: request.value.clone(),
            })
        }) {
            Ok(decision) => map_bridge_permission(decision),
            Err(error) => PermissionDecision::deny(error.to_string()),
        }
    }

    pub(crate) fn network_decision(
        &self,
        vm_id: &str,
        request: &NetworkAccessRequest,
    ) -> PermissionDecision {
        if let Some(decision) = self.static_permission_decision(
            vm_id,
            network_permission_capability(request.op),
            "network",
            Some(&request.resource),
        ) {
            return decision;
        }
        match self.with_mut(|bridge| {
            bridge.check_network_access(NetworkPermissionRequest {
                vm_id: vm_id.to_owned(),
                access: match request.op {
                    NetworkOperation::Fetch => NetworkAccess::Fetch,
                    NetworkOperation::Http => NetworkAccess::Http,
                    NetworkOperation::Dns => NetworkAccess::Dns,
                    NetworkOperation::Listen => NetworkAccess::Listen,
                },
                resource: request.resource.clone(),
            })
        }) {
            Ok(decision) => map_bridge_permission(decision),
            Err(error) => PermissionDecision::deny(error.to_string()),
        }
    }

    pub(crate) fn require_network_access(
        &self,
        vm_id: &str,
        op: NetworkOperation,
        resource: impl Into<String>,
    ) -> Result<(), SidecarError> {
        let resource = resource.into();
        let decision = self.network_decision(
            vm_id,
            &NetworkAccessRequest {
                vm_id: vm_id.to_owned(),
                op,
                resource: resource.clone(),
            },
        );
        if decision.allow {
            return Ok(());
        }

        let message = match decision.reason.as_deref() {
            Some(reason) => format!("EACCES: permission denied, {resource}: {reason}"),
            None => format!("EACCES: permission denied, {resource}"),
        };
        Err(SidecarError::Execution(message))
    }

    pub(crate) fn set_vm_permissions(
        &self,
        vm_id: &str,
        permissions: &PermissionsPolicy,
    ) -> Result<(), SidecarError> {
        #[cfg(test)]
        {
            let mut outcomes = self.set_vm_permissions_outcomes.lock().map_err(|_| {
                SidecarError::Bridge(String::from(
                    "native sidecar test set_vm_permissions outcome lock poisoned",
                ))
            })?;
            if let Some(Some(error)) = outcomes.pop_front() {
                return Err(error);
            }
        }

        let mut stored = self.permissions.lock().map_err(|_| {
            SidecarError::Bridge(String::from(
                "native sidecar permission policy lock poisoned",
            ))
        })?;
        stored.insert(vm_id.to_owned(), permissions.clone());
        Ok(())
    }

    pub(crate) fn restore_vm_permissions_fail_closed(
        &self,
        vm_id: &str,
        original_permissions: &PermissionsPolicy,
        context: &str,
        operation_error: &SidecarError,
    ) -> Result<(), SidecarError> {
        match self.set_vm_permissions(vm_id, original_permissions) {
            Ok(()) => Ok(()),
            Err(restore_error) => {
                let deny_all = deny_all_policy();
                match self.set_vm_permissions(vm_id, &deny_all) {
                    Ok(()) => Err(SidecarError::InvalidState(format!(
                        "{context} failed: {operation_error}; restoring original permissions failed: {restore_error}; applied deny-all fallback"
                    ))),
                    Err(deny_all_error) => panic!(
                        "{context} failed: {operation_error}; restoring original permissions failed: {restore_error}; deny-all fallback failed: {deny_all_error}"
                    ),
                }
            }
        }
    }

    pub(crate) fn clear_vm_permissions(&self, vm_id: &str) -> Result<(), SidecarError> {
        let mut stored = self.permissions.lock().map_err(|_| {
            SidecarError::Bridge(String::from(
                "native sidecar permission policy lock poisoned",
            ))
        })?;
        stored.remove(vm_id);
        Ok(())
    }

    pub(crate) fn static_permission_decision(
        &self,
        vm_id: &str,
        capability: &str,
        domain: &str,
        resource: Option<&str>,
    ) -> Option<PermissionDecision> {
        let stored = self.permissions.lock().ok()?;
        let permissions = stored.get(vm_id)?;
        let mode = evaluate_permissions_policy(permissions, domain, capability, resource);
        Some(permission_mode_to_kernel_decision(mode, capability))
    }
}

pub(crate) fn validate_permissions_policy(
    permissions: &PermissionsPolicy,
) -> Result<(), SidecarError> {
    agentos_native_sidecar_core::permissions::validate_permissions_policy(permissions)
        .map_err(|error| SidecarError::InvalidState(error.to_string()))
}

fn is_internal_runtime_command_request(request: &CommandAccessRequest) -> bool {
    match request.command.as_str() {
        "node" => request
            .env
            .keys()
            .any(|key| INTERNAL_JAVASCRIPT_ENTRYPOINT_ENV_KEYS.contains(&key.as_str())),
        "wasm" => request
            .env
            .keys()
            .any(|key| INTERNAL_WASM_ENTRYPOINT_ENV_KEYS.contains(&key.as_str())),
        "python" => request.env.keys().any(|key| {
            INTERNAL_PYTHON_ENTRYPOINT_ENV_PREFIXES
                .iter()
                .any(|prefix| key.starts_with(prefix))
        }),
        _ => false,
    }
}

fn ownership_matches_process_event(
    ownership: &OwnershipScope,
    event: &ProcessEventEnvelope,
) -> bool {
    match ownership {
        OwnershipScope::ConnectionOwnership(inner) => inner.connection_id == event.connection_id,
        OwnershipScope::SessionOwnership(inner) => {
            inner.connection_id == event.connection_id && inner.session_id == event.session_id
        }
        OwnershipScope::VmOwnership(inner) => {
            inner.connection_id == event.connection_id
                && inner.session_id == event.session_id
                && inner.vm_id == event.vm_id
        }
    }
}

fn public_process_event_matches_ownership<B>(
    sidecar: &NativeSidecar<B>,
    ownership: &OwnershipScope,
    event: &ProcessEventEnvelope,
) -> bool
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    if !ownership_matches_process_event(ownership, event) {
        return false;
    }

    if event.process_id.contains('/') {
        return false;
    }

    // Stale queued events must still be drained through handle_process_event_envelope()
    // so the sidecar can emit the expected fail-closed log when teardown wins the race.
    let _ = sidecar;
    true
}

fn poll_future_once<F: std::future::Future>(future: std::pin::Pin<&mut F>) -> Option<F::Output> {
    let mut context = Context::from_waker(Waker::noop());
    match future.poll(&mut context) {
        Poll::Ready(output) => Some(output),
        Poll::Pending => None,
    }
}

// ConnectionState, SessionState, VmConfiguration, VmState moved to crate::state

// JavascriptSocketPathContext, JavascriptSocketFamily, VmListenPolicy moved to crate::state

impl JavascriptSocketPathContext {
    pub(crate) fn loopback_port_allowed(&self, port: u16) -> bool {
        self.loopback_exempt_ports.contains(&port)
            || self
                .tcp_loopback_guest_to_host_ports
                .keys()
                .any(|(_, guest_port)| *guest_port == port)
    }

    pub(crate) fn translate_tcp_loopback_port(
        &self,
        family: JavascriptSocketFamily,
        port: u16,
    ) -> Option<u16> {
        self.tcp_loopback_guest_to_host_ports
            .get(&(family, port))
            .copied()
    }

    pub(crate) fn http_loopback_target(
        &self,
        family: JavascriptSocketFamily,
        port: u16,
    ) -> Option<&crate::state::JavascriptHttpLoopbackTarget> {
        self.http_loopback_targets.get(&(family, port))
    }

    pub(crate) fn translate_udp_loopback_port(
        &self,
        family: JavascriptSocketFamily,
        port: u16,
    ) -> Option<u16> {
        self.udp_loopback_guest_to_host_ports
            .get(&(family, port))
            .copied()
    }

    pub(crate) fn guest_udp_port_for_host_port(
        &self,
        family: JavascriptSocketFamily,
        port: u16,
    ) -> Option<u16> {
        self.udp_loopback_host_to_guest_ports
            .get(&(family, port))
            .copied()
    }
}

// ActiveProcess, NetworkResourceCounts moved to crate::state

pub struct NativeSidecar<B> {
    pub(crate) config: NativeSidecarConfig,
    pub(crate) bridge: SharedBridge<B>,
    pub(crate) mount_plugins: FileSystemPluginRegistry<MountPluginContext<B>>,
    pub(crate) cache_root: PathBuf,
    pub(crate) javascript_engine: JavascriptExecutionEngine,
    pub(crate) python_engine: PythonExecutionEngine,
    pub(crate) wasm_engine: WasmExecutionEngine,
    pub(crate) next_connection_id: usize,
    pub(crate) next_session_id: usize,
    pub(crate) next_vm_id: usize,
    pub(crate) next_process_id: u64,
    pub(crate) next_sidecar_request_id: RequestId,
    pub(crate) connections: BTreeMap<String, ConnectionState>,
    pub(crate) sessions: BTreeMap<String, SessionState>,
    pub(crate) vms: BTreeMap<String, VmState>,
    pub(crate) cron_schedulers: BTreeMap<String, CronScheduler>,
    pub(crate) cron_process_runs: BTreeMap<(String, String), String>,
    #[allow(dead_code)]
    pub(crate) process_event_sender: Sender<ProcessEventEnvelope>,
    pub(crate) process_event_receiver: Option<Receiver<ProcessEventEnvelope>>,
    pub(crate) pending_process_events: VecDeque<ProcessEventEnvelope>,
    pub(crate) pending_sidecar_responses: SidecarResponseTracker,
    pub(crate) outbound_sidecar_requests: VecDeque<SidecarRequestFrame>,
    pub(crate) completed_sidecar_responses: BTreeMap<RequestId, SidecarResponseFrame>,
    pub(crate) completed_sidecar_response_order: VecDeque<RequestId>,
    pub(crate) completed_sidecar_responses_gauge: Arc<QueueGauge>,
    pub(crate) pending_process_events_gauge: Arc<QueueGauge>,
    pub(crate) pending_sidecar_responses_gauge: Arc<QueueGauge>,
    pub(crate) outbound_sidecar_requests_gauge: Arc<QueueGauge>,
    pub(crate) sidecar_requests: SharedSidecarRequestClient,
    pub(crate) event_sink: SharedEventSink,
    pub(crate) extensions: BTreeMap<String, Arc<dyn Extension>>,
    pub(crate) extension_sessions: BTreeMap<(String, String), ExtensionSessionResources>,
    pub(crate) extension_process_output_buffers:
        BTreeMap<(String, String), ExtensionBufferedProcessOutput>,
    /// Session scopes (connection_id, session_id) disposed since the stdio
    /// transport last drained them. Lets the transport remove dead sessions from
    /// its active-session set instead of iterating them forever (M5).
    pub(crate) disposed_sessions: Vec<(String, String)>,
}

#[derive(Debug)]
pub(crate) struct ExtensionSessionResources {
    pub(crate) ownership: OwnershipScope,
    pub(crate) process_ids: BTreeSet<String>,
    pub(crate) vm_ids: BTreeSet<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CronSessionOptions {
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    mcp_servers: Vec<Value>,
    #[serde(default)]
    skip_os_instructions: bool,
    additional_instructions: Option<String>,
}

impl<B> fmt::Debug for NativeSidecar<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeSidecar")
            .field("config", &self.config)
            .field("cache_root", &self.cache_root)
            .field("next_connection_id", &self.next_connection_id)
            .field("next_session_id", &self.next_session_id)
            .field("next_vm_id", &self.next_vm_id)
            .field("next_process_id", &self.next_process_id)
            .field("connection_count", &self.connections.len())
            .field("session_count", &self.sessions.len())
            .field("vm_count", &self.vms.len())
            .field("cron_scheduler_count", &self.cron_schedulers.len())
            .field("extension_session_count", &self.extension_sessions.len())
            .field(
                "extension_process_output_buffer_count",
                &self.extension_process_output_buffers.len(),
            )
            .finish()
    }
}

impl<B> NativeSidecar<B>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    pub fn new(bridge: B) -> Result<Self, SidecarError> {
        Self::with_config(bridge, NativeSidecarConfig::default())
    }

    pub fn with_config(bridge: B, config: NativeSidecarConfig) -> Result<Self, SidecarError> {
        if matches!(config.expected_auth_token.as_deref(), Some("")) {
            return Err(SidecarError::InvalidState(String::from(
                "native sidecar expected_auth_token must not be empty",
            )));
        }

        let cache_root = config.compile_cache_root.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "{}-{}",
                config.sidecar_id,
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system time before unix epoch")
                    .as_nanos()
            ))
        });
        fs::create_dir_all(&cache_root).map_err(|error| {
            SidecarError::Io(format!("failed to prepare sidecar cache root: {error}"))
        })?;

        let bridge = SharedBridge::new(bridge);
        let mount_plugins = build_mount_plugin_registry::<B>()?;
        let (process_event_sender, process_event_receiver) = channel(MAX_PROCESS_EVENT_QUEUE);

        Ok(Self {
            config,
            bridge,
            mount_plugins,
            cache_root,
            javascript_engine: JavascriptExecutionEngine::default(),
            python_engine: PythonExecutionEngine::default(),
            wasm_engine: WasmExecutionEngine::default(),
            next_connection_id: 0,
            next_session_id: 0,
            next_vm_id: 0,
            next_process_id: 0,
            next_sidecar_request_id: -1,
            connections: BTreeMap::new(),
            sessions: BTreeMap::new(),
            vms: BTreeMap::new(),
            cron_schedulers: BTreeMap::new(),
            cron_process_runs: BTreeMap::new(),
            process_event_sender,
            process_event_receiver: Some(process_event_receiver),
            pending_process_events: VecDeque::new(),
            pending_sidecar_responses: SidecarResponseTracker::default(),
            outbound_sidecar_requests: VecDeque::new(),
            completed_sidecar_responses: BTreeMap::new(),
            completed_sidecar_response_order: VecDeque::new(),
            completed_sidecar_responses_gauge: register_queue(
                TrackedLimit::CompletedSidecarResponses,
                MAX_COMPLETED_SIDECAR_RESPONSES,
            ),
            pending_process_events_gauge: register_queue(
                TrackedLimit::PendingProcessEvents,
                MAX_PROCESS_EVENT_QUEUE,
            ),
            pending_sidecar_responses_gauge: register_queue(
                TrackedLimit::PendingSidecarResponses,
                MAX_PENDING_SIDECAR_RESPONSES,
            ),
            outbound_sidecar_requests_gauge: register_queue(
                TrackedLimit::OutboundSidecarRequests,
                MAX_OUTBOUND_SIDECAR_REQUESTS,
            ),
            sidecar_requests: SharedSidecarRequestClient::default(),
            event_sink: SharedEventSink::default(),
            extensions: BTreeMap::new(),
            extension_sessions: BTreeMap::new(),
            extension_process_output_buffers: BTreeMap::new(),
            disposed_sessions: Vec::new(),
        })
    }

    pub fn with_config_and_extensions(
        bridge: B,
        config: NativeSidecarConfig,
        extensions: Vec<Box<dyn Extension>>,
    ) -> Result<Self, SidecarError> {
        let mut sidecar = Self::with_config(bridge, config)?;
        for extension in extensions {
            sidecar.register_extension(extension)?;
        }
        Ok(sidecar)
    }

    pub(crate) fn prune_extension_process_resource(&mut self, process_id: &str) {
        self.extension_sessions.retain(|_, resources| {
            resources.process_ids.remove(process_id);
            !resources.process_ids.is_empty() || !resources.vm_ids.is_empty()
        });
    }

    pub(crate) fn prune_extension_vm_resource(&mut self, vm_id: &str) {
        self.extension_sessions.retain(|_, resources| {
            if matches!(
                &resources.ownership,
                OwnershipScope::VmOwnership(inner) if inner.vm_id == vm_id
            ) {
                resources.process_ids.clear();
            }
            resources.vm_ids.remove(vm_id);
            !resources.process_ids.is_empty() || !resources.vm_ids.is_empty()
        });
    }

    /// Reclaim every per-VM tracking entry owned by the sidecar for `vm_id`.
    ///
    /// Called unconditionally from `dispose_vm_internal` so that a fallible
    /// teardown step (root-filesystem snapshot/flush, kernel dispose, permission
    /// reset) erroring out with `?` can never strand these maps for the rest of
    /// the process lifetime (H1). This also reclaims the ACP output-buffer map,
    /// which was previously removed only on a successful handoff and leaked on VM
    /// or session disposal (M6).
    pub(crate) fn reclaim_vm_tracking(&mut self, session_id: &str, vm_id: &str) {
        self.javascript_engine.dispose_vm(vm_id);
        self.python_engine.dispose_vm(vm_id);
        self.wasm_engine.dispose_vm(vm_id);
        self.cron_schedulers.remove(vm_id);
        self.cron_process_runs
            .retain(|(process_vm_id, _), _| process_vm_id != vm_id);
        self.prune_extension_vm_resource(vm_id);
        self.extension_process_output_buffers
            .retain(|(buffer_vm_id, _process_id), _| buffer_vm_id != vm_id);
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.vm_ids.remove(vm_id);
        }
    }

    pub(crate) fn capture_extension_process_output_event(
        &mut self,
        vm_id: &str,
        process_id: &str,
        event: &ActiveExecutionEvent,
    ) -> bool {
        let Some(buffer) = self
            .extension_process_output_buffers
            .get_mut(&(vm_id.to_string(), process_id.to_string()))
        else {
            return false;
        };
        match event {
            ActiveExecutionEvent::Stdout(chunk) => {
                buffer.append_stdout(chunk, DEFAULT_ACP_STDOUT_BUFFER_BYTE_LIMIT);
                true
            }
            ActiveExecutionEvent::Stderr(chunk) => {
                buffer.append_stderr(chunk, DEFAULT_ACP_STDOUT_BUFFER_BYTE_LIMIT);
                true
            }
            ActiveExecutionEvent::Exited(exit_code) => {
                buffer.exit_code = Some(*exit_code);
                true
            }
            ActiveExecutionEvent::JavascriptSyncRpcRequest(_)
            | ActiveExecutionEvent::PythonVfsRpcRequest(_)
            | ActiveExecutionEvent::SignalState { .. } => false,
        }
    }

    fn bind_extension_process_resource(
        &mut self,
        ownership: OwnershipScope,
        namespace: String,
        ext_session_id: String,
        process_id: String,
    ) -> Result<(), SidecarError> {
        if ext_session_id.is_empty() {
            return Err(SidecarError::InvalidState(String::from(
                "extension session id must not be empty",
            )));
        }
        let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
        let process_exists = self
            .vms
            .get(&vm_id)
            .is_some_and(|vm| vm.active_processes.contains_key(&process_id));
        if !process_exists {
            return Err(SidecarError::InvalidState(format!(
                "VM {vm_id} has no active process {process_id}"
            )));
        }

        let key = (namespace, ext_session_id);
        if let Some(resources) = self.extension_sessions.get_mut(&key) {
            if resources.ownership != ownership {
                return Err(SidecarError::InvalidState(String::from(
                    "extension session ownership did not match existing resources",
                )));
            }
            resources.process_ids.insert(process_id);
        } else {
            self.extension_sessions.insert(
                key,
                ExtensionSessionResources {
                    ownership,
                    process_ids: BTreeSet::from([process_id]),
                    vm_ids: BTreeSet::new(),
                },
            );
        }
        Ok(())
    }

    fn bind_extension_vm_resource(
        &mut self,
        ownership: OwnershipScope,
        namespace: String,
        ext_session_id: String,
    ) -> Result<(), SidecarError> {
        if ext_session_id.is_empty() {
            return Err(SidecarError::InvalidState(String::from(
                "extension session id must not be empty",
            )));
        }
        let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;

        let key = (namespace, ext_session_id);
        if let Some(resources) = self.extension_sessions.get_mut(&key) {
            if resources.ownership != ownership {
                return Err(SidecarError::InvalidState(String::from(
                    "extension session ownership did not match existing resources",
                )));
            }
            resources.vm_ids.insert(vm_id);
        } else {
            self.extension_sessions.insert(
                key,
                ExtensionSessionResources {
                    ownership,
                    process_ids: BTreeSet::new(),
                    vm_ids: BTreeSet::from([vm_id]),
                },
            );
        }
        Ok(())
    }

    pub fn sidecar_id(&self) -> &str {
        &self.config.sidecar_id
    }

    pub fn with_bridge_mut<T>(
        &self,
        operation: impl FnOnce(&mut B) -> T,
    ) -> Result<T, SidecarError> {
        self.bridge.inspect(operation)
    }

    pub fn set_sidecar_request_transport(&mut self, transport: Arc<dyn SidecarRequestTransport>) {
        self.sidecar_requests.set_transport(transport);
    }

    pub fn set_event_transport(&mut self, transport: Arc<dyn EventSinkTransport>) {
        self.event_sink.set_transport(transport);
    }

    pub fn register_extension(
        &mut self,
        extension: Box<dyn Extension>,
    ) -> Result<(), SidecarError> {
        let namespace = extension.namespace().to_owned();
        if namespace.is_empty() {
            return Err(SidecarError::InvalidState(String::from(
                "extension namespace must not be empty",
            )));
        }
        if self.extensions.contains_key(&namespace) {
            return Err(SidecarError::Conflict(format!(
                "extension namespace {namespace} is already registered",
            )));
        }
        self.extensions.insert(namespace, Arc::from(extension));
        Ok(())
    }

    pub fn set_sidecar_request_handler<F>(&mut self, handler: F)
    where
        F: Fn(SidecarRequestFrame) -> Result<SidecarResponsePayload, SidecarError>
            + Send
            + Sync
            + 'static,
    {
        struct HandlerTransport<F>(F);

        impl<F> SidecarRequestTransport for HandlerTransport<F>
        where
            F: Fn(SidecarRequestFrame) -> Result<SidecarResponsePayload, SidecarError>
                + Send
                + Sync
                + 'static,
        {
            fn send_request(
                &self,
                request: SidecarRequestFrame,
                _timeout: Duration,
            ) -> Result<SidecarResponseFrame, SidecarError> {
                let payload = (self.0)(request.clone())?;
                Ok(SidecarResponseFrame::new(
                    request.request_id,
                    request.ownership,
                    payload,
                ))
            }
        }

        self.set_sidecar_request_transport(Arc::new(HandlerTransport(handler)));
    }

    pub fn set_wire_sidecar_request_handler<F>(&mut self, handler: F)
    where
        F: Fn(
                crate::wire::SidecarRequestFrame,
            ) -> Result<crate::wire::SidecarResponseFrame, SidecarError>
            + Send
            + Sync
            + 'static,
    {
        self.set_sidecar_request_handler(move |request| {
            let request = crate::wire::sidecar_request_frame_from_compat(request)
                .map_err(wire_protocol_error)?;
            let response = handler(request)?;
            let response = crate::wire::sidecar_response_frame_to_compat(response)
                .map_err(wire_protocol_error)?;
            Ok(response.payload)
        });
    }

    pub(crate) fn queue_pending_process_event(
        &mut self,
        envelope: ProcessEventEnvelope,
    ) -> Result<(), SidecarError> {
        if self.pending_process_events.len() >= MAX_PROCESS_EVENT_QUEUE {
            return Err(process_event_queue_overflow_error());
        }
        if matches!(&envelope.event, ActiveExecutionEvent::Exited(_)) {
            mark_execute_exit_event_queued(&envelope.vm_id, &envelope.process_id);
        }
        self.pending_process_events.push_back(envelope);
        self.pending_process_events_gauge
            .observe_depth(self.pending_process_events.len());
        Ok(())
    }

    pub(crate) fn queue_front_pending_process_event(
        &mut self,
        envelope: ProcessEventEnvelope,
    ) -> Result<(), SidecarError> {
        if self.pending_process_events.len() >= MAX_PROCESS_EVENT_QUEUE {
            return Err(process_event_queue_overflow_error());
        }
        if matches!(&envelope.event, ActiveExecutionEvent::Exited(_)) {
            mark_execute_exit_event_queued(&envelope.vm_id, &envelope.process_id);
        }
        self.pending_process_events.push_front(envelope);
        self.pending_process_events_gauge
            .observe_depth(self.pending_process_events.len());
        Ok(())
    }

    pub(crate) fn pending_process_event_capacity(&self) -> usize {
        MAX_PROCESS_EVENT_QUEUE.saturating_sub(self.pending_process_events.len())
    }

    pub fn dispatch_blocking(
        &mut self,
        request: RequestFrame,
    ) -> Result<DispatchResult, SidecarError> {
        let inside_runtime = tokio::runtime::Handle::try_current().is_ok();
        if request_dispatch_mode(&request) == RequestDispatchMode::Async && !inside_runtime {
            return blocking_dispatch_runtime().block_on(self.dispatch(request));
        }

        let mut future = std::pin::pin!(self.dispatch(request));
        match poll_future_once(future.as_mut()) {
            Some(result) => result,
            None if inside_runtime => Err(SidecarError::InvalidState(String::from(
                "dispatch_blocking cannot wait for an async sidecar request inside a Tokio runtime; use dispatch().await",
            ))),
            None => blocking_dispatch_runtime().block_on(future),
        }
    }

    pub fn dispatch_wire_blocking(
        &mut self,
        request: crate::wire::RequestFrame,
    ) -> Result<crate::wire::WireDispatchResult, SidecarError> {
        let request = crate::wire::request_frame_to_compat(request).map_err(wire_protocol_error)?;
        let result = self.dispatch_blocking(request)?;
        wire_dispatch_result(result)
    }

    pub fn poll_event_blocking(
        &mut self,
        ownership: &OwnershipScope,
        timeout: Duration,
    ) -> Result<Option<EventFrame>, SidecarError> {
        blocking_dispatch_runtime().block_on(self.poll_event(ownership, timeout))
    }

    pub fn poll_event_wire_blocking(
        &mut self,
        ownership: &crate::wire::OwnershipScope,
        timeout: Duration,
    ) -> Result<Option<crate::wire::EventFrame>, SidecarError> {
        let ownership = crate::wire::ownership_scope_to_compat(ownership.clone());
        self.poll_event_blocking(&ownership, timeout)?
            .map(crate::wire::event_frame_from_compat)
            .transpose()
            .map_err(wire_protocol_error)
    }

    pub fn close_session_blocking(
        &mut self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<Vec<EventFrame>, SidecarError> {
        blocking_dispatch_runtime().block_on(self.close_session(connection_id, session_id))
    }

    pub fn remove_connection_blocking(
        &mut self,
        connection_id: &str,
    ) -> Result<Vec<EventFrame>, SidecarError> {
        blocking_dispatch_runtime().block_on(self.remove_connection(connection_id))
    }

    pub fn dispose_vm_internal_blocking(
        &mut self,
        connection_id: &str,
        session_id: &str,
        vm_id: &str,
        reason: DisposeReason,
    ) -> Result<Vec<EventFrame>, SidecarError> {
        blocking_dispatch_runtime().block_on(self.dispose_vm_internal(
            connection_id,
            session_id,
            vm_id,
            reason,
        ))
    }

    pub async fn dispatch(
        &mut self,
        request: RequestFrame,
    ) -> Result<DispatchResult, SidecarError> {
        if let Err(error) = self.ensure_request_within_frame_limit(&request) {
            return Ok(DispatchResult {
                response: self.reject(&request, error_code(&error), &error.to_string()),
                events: Vec::new(),
            });
        }

        let result = match route_request_payload(&request) {
            RequestRoute::Authenticate(payload) => {
                self.authenticate_connection(&request, payload).await
            }
            RequestRoute::OpenSession(payload) => self.open_session(&request, payload).await,
            RequestRoute::CreateVm(payload) => self.create_vm(&request, payload).await,
            RequestRoute::InitializeVm(payload) => self.initialize_vm(&request, payload).await,
            RequestRoute::DisposeVm(payload) => self.dispose_vm(&request, payload).await,
            RequestRoute::BootstrapRootFilesystem(payload) => {
                self.bootstrap_root_filesystem(&request, payload.entries)
                    .await
            }
            RequestRoute::ConfigureVm(payload) => self.configure_vm(&request, payload).await,
            RequestRoute::RegisterHostCallbacks(payload) => {
                register_host_callbacks(self, &request, payload)
            }
            RequestRoute::CreateLayer(payload) => self.create_layer(&request, payload).await,
            RequestRoute::SealLayer(payload) => self.seal_layer(&request, payload).await,
            RequestRoute::ImportSnapshot(payload) => self.import_snapshot(&request, payload).await,
            RequestRoute::ExportSnapshot(payload) => self.export_snapshot(&request, payload).await,
            RequestRoute::CreateOverlay(payload) => self.create_overlay(&request, payload).await,
            RequestRoute::GuestFilesystemCall(payload) => {
                self.guest_filesystem_call(&request, payload).await
            }
            RequestRoute::GuestKernelCall(payload) => {
                self.guest_kernel_call(&request, payload).await
            }
            RequestRoute::SnapshotRootFilesystem(payload) => {
                self.snapshot_root_filesystem(&request, payload).await
            }
            RequestRoute::Execute(payload) => self.execute(&request, payload).await,
            RequestRoute::WriteStdin(payload) => self.write_stdin(&request, payload).await,
            RequestRoute::ResizePty(payload) => self.resize_pty(&request, payload).await,
            RequestRoute::CloseStdin(payload) => self.close_stdin(&request, payload).await,
            RequestRoute::KillProcess(payload) => self.kill_process(&request, payload).await,
            RequestRoute::GetProcessSnapshot(payload) => {
                self.get_process_snapshot(&request, payload).await
            }
            RequestRoute::GetResourceSnapshot(payload) => {
                self.get_resource_snapshot(&request, payload).await
            }
            RequestRoute::FindListener(payload) => self.find_listener(&request, payload).await,
            RequestRoute::FindBoundUdp(payload) => self.find_bound_udp(&request, payload).await,
            RequestRoute::VmFetch(payload) => self.vm_fetch(&request, payload).await,
            RequestRoute::GetSignalState(payload) => self.get_signal_state(&request, payload).await,
            RequestRoute::GetZombieTimerCount(payload) => {
                self.get_zombie_timer_count(&request, payload).await
            }
            RequestRoute::LinkPackage(payload) => self.link_package(&request, payload).await,
            RequestRoute::ProvidedCommands(payload) => {
                self.provided_commands(&request, payload).await
            }
            RequestRoute::ScheduleCron(payload) => self.schedule_cron(&request, payload),
            RequestRoute::ListCronJobs(_) => self.list_cron_jobs(&request),
            RequestRoute::CancelCronJob(payload) => self.cancel_cron_job(&request, payload),
            RequestRoute::WakeCron(payload) => self.wake_cron(&request, payload).await,
            RequestRoute::CompleteCronRun(payload) => self.complete_cron_run(&request, payload),
            RequestRoute::ExportCronState(_) => self.export_cron_state(&request),
            RequestRoute::ImportCronState(payload) => self.import_cron_state(&request, payload),
            RequestRoute::UnsupportedHostCallbackDirection => {
                Ok(unsupported_host_callback_direction_dispatch(&request))
            }
            RequestRoute::Ext(payload) => self.dispatch_extension_request(&request, payload).await,
        };

        match result {
            Ok(dispatch) => Ok(dispatch),
            Err(error @ SidecarError::Io(_)) => Err(error),
            Err(error) => Ok(DispatchResult {
                response: self.reject(&request, error_code(&error), &error.to_string()),
                events: Vec::new(),
            }),
        }
    }

    pub async fn dispatch_wire(
        &mut self,
        request: crate::wire::RequestFrame,
    ) -> Result<crate::wire::WireDispatchResult, SidecarError> {
        let request = crate::wire::request_frame_to_compat(request).map_err(wire_protocol_error)?;
        let result = self.dispatch(request).await?;
        wire_dispatch_result(result)
    }

    pub async fn poll_event_wire(
        &mut self,
        ownership: &crate::wire::OwnershipScope,
        timeout: Duration,
    ) -> Result<Option<crate::wire::EventFrame>, SidecarError> {
        let ownership = crate::wire::ownership_scope_to_compat(ownership.clone());
        self.poll_event(&ownership, timeout)
            .await?
            .map(crate::wire::event_frame_from_compat)
            .transpose()
            .map_err(wire_protocol_error)
    }

    async fn dispatch_extension_request(
        &mut self,
        request: &RequestFrame,
        envelope: ExtEnvelope,
    ) -> Result<DispatchResult, SidecarError> {
        let namespace = envelope.namespace;
        let Some(extension) = self.extensions.get(&namespace).cloned() else {
            return Ok(DispatchResult {
                response: self.reject(
                    request,
                    "unknown_extension",
                    &format!("no extension registered for namespace {namespace}"),
                ),
                events: Vec::new(),
            });
        };
        let snapshot = ExtensionSnapshot::new(
            namespace.clone(),
            request.ownership.clone(),
            self.sidecar_requests.clone(),
            self.event_sink.clone(),
        );
        let ctx = ExtensionContext::new(snapshot, self);
        let response = extension.handle_request(ctx, envelope.payload).await?;
        Ok(DispatchResult {
            response: self.respond(
                request,
                ResponsePayload::ExtResult(ExtEnvelope {
                    namespace,
                    payload: response.payload,
                }),
            ),
            events: response.events,
        })
    }

    async fn initialize_vm(
        &mut self,
        request: &RequestFrame,
        payload: crate::protocol::InitializeVmRequest,
    ) -> Result<DispatchResult, SidecarError> {
        let (connection_id, session_id) = self.session_scope_for(&request.ownership)?;
        self.require_owned_session(&connection_id, &session_id)?;

        let created_dispatch = self
            .create_vm(
                request,
                crate::protocol::CreateVmRequest {
                    runtime: payload.runtime,
                    config: payload.config,
                },
            )
            .await?;
        let DispatchResult {
            response: created_response,
            mut events,
        } = created_dispatch;
        let ResponsePayload::VmCreated(created) = created_response.payload else {
            return Err(SidecarError::InvalidState(String::from(
                "initialize_vm create step returned an unexpected response",
            )));
        };
        let vm_id = created.vm_id.clone();
        let ownership = OwnershipScope::vm(&connection_id, &session_id, &vm_id);

        let initialization = async {
            let configure_payload = crate::protocol::ConfigureVmRequest {
                mounts: payload.mounts,
                permissions: None,
                command_permissions: None,
                loopback_exempt_ports: None,
                packages: payload.packages,
                packages_mount_at: payload.packages_mount_at,
            };
            let configure_request = RequestFrame {
                schema: request.schema.clone(),
                request_id: request.request_id,
                ownership: ownership.clone(),
                payload: RequestPayload::ConfigureVm(configure_payload.clone()),
            };
            let configured_dispatch = self
                .configure_vm(&configure_request, configure_payload)
                .await?;
            events.extend(configured_dispatch.events);
            let ResponsePayload::VmConfigured(configured) = configured_dispatch.response.payload
            else {
                return Err(SidecarError::InvalidState(String::from(
                    "initialize_vm configure step returned an unexpected response",
                )));
            };

            let registrations = payload.host_callbacks.unwrap_or_default();
            let mut host_callbacks = Vec::with_capacity(registrations.len());
            for registration in registrations {
                let registration_request = RequestFrame {
                    schema: request.schema.clone(),
                    request_id: request.request_id,
                    ownership: ownership.clone(),
                    payload: RequestPayload::RegisterHostCallbacks(registration.clone()),
                };
                let registered =
                    register_host_callbacks(self, &registration_request, registration)?;
                events.extend(registered.events);
                let ResponsePayload::HostCallbacksRegistered(registered) =
                    registered.response.payload
                else {
                    return Err(SidecarError::InvalidState(String::from(
                        "initialize_vm host-callback step returned an unexpected response",
                    )));
                };
                host_callbacks.push(registered);
            }

            Ok::<_, SidecarError>((configured, host_callbacks))
        }
        .await;

        let (configured, host_callbacks) = match initialization {
            Ok(result) => result,
            Err(error) => {
                if let Err(cleanup_error) = self
                    .dispose_vm_internal(
                        &connection_id,
                        &session_id,
                        &vm_id,
                        DisposeReason::Requested,
                    )
                    .await
                {
                    tracing::error!(
                        vm_id,
                        ?cleanup_error,
                        "failed to clean up partially initialized VM"
                    );
                }
                return Err(error);
            }
        };

        Ok(DispatchResult {
            response: self.respond(
                request,
                ResponsePayload::VmInitialized(crate::protocol::VmInitializedResponse {
                    vm_id,
                    guest_cwd: created.guest_cwd,
                    guest_env: created.guest_env,
                    applied_mounts: configured.applied_mounts,
                    projected_commands: configured.projected_commands,
                    agents: configured.agents,
                    host_callbacks,
                }),
            ),
            events,
        })
    }

    fn schedule_cron(
        &mut self,
        request: &RequestFrame,
        payload: ScheduleCronRequest,
    ) -> Result<DispatchResult, SidecarError> {
        let vm_id = self.require_cron_vm(&request.ownership)?;
        let response = self
            .cron_schedulers
            .entry(vm_id.clone())
            .or_default()
            .schedule(payload, unix_time_ms())
            .map_err(cron_scheduler_error)?;
        let job_count = self
            .cron_schedulers
            .get(&vm_id)
            .map_or(0, CronScheduler::job_count);
        if job_count.saturating_mul(100) >= MAX_CRON_JOBS.saturating_mul(80) {
            tracing::warn!(
                vm_id,
                job_count,
                limit = MAX_CRON_JOBS,
                "cron job registry is near its configured limit"
            );
        }
        let action_bytes = self
            .cron_schedulers
            .get(&vm_id)
            .map_or(0, CronScheduler::total_action_bytes);
        if action_bytes.saturating_mul(100) >= MAX_CRON_STATE_BYTES.saturating_mul(80) {
            tracing::warn!(
                vm_id,
                action_bytes,
                limit = MAX_CRON_STATE_BYTES,
                "cron action registry is near its configured byte limit"
            );
        }
        Ok(DispatchResult {
            response: self.respond(request, ResponsePayload::CronScheduled(response)),
            events: Vec::new(),
        })
    }

    fn list_cron_jobs(&mut self, request: &RequestFrame) -> Result<DispatchResult, SidecarError> {
        let vm_id = self.require_cron_vm(&request.ownership)?;
        let response = self.cron_schedulers.entry(vm_id).or_default().list();
        Ok(DispatchResult {
            response: self.respond(request, ResponsePayload::CronJobs(response)),
            events: Vec::new(),
        })
    }

    fn cancel_cron_job(
        &mut self,
        request: &RequestFrame,
        payload: CancelCronJobRequest,
    ) -> Result<DispatchResult, SidecarError> {
        let vm_id = self.require_cron_vm(&request.ownership)?;
        let response = self
            .cron_schedulers
            .entry(vm_id)
            .or_default()
            .cancel(payload)
            .map_err(cron_scheduler_error)?;
        Ok(DispatchResult {
            response: self.respond(request, ResponsePayload::CronCancelled(response)),
            events: Vec::new(),
        })
    }

    async fn wake_cron(
        &mut self,
        request: &RequestFrame,
        payload: WakeCronRequest,
    ) -> Result<DispatchResult, SidecarError> {
        let vm_id = self.require_cron_vm(&request.ownership)?;
        let mut response = self
            .cron_schedulers
            .entry(vm_id.clone())
            .or_default()
            .wake(payload, unix_time_ms())
            .map_err(cron_scheduler_error)?;
        response.runs = self
            .start_cron_runs(
                request,
                &vm_id,
                response.runs,
                &mut response.alarm,
                &mut response.events,
            )
            .await?;
        Ok(DispatchResult {
            response: self.respond(request, ResponsePayload::CronWake(response)),
            events: Vec::new(),
        })
    }

    /// Start every executable run inside the sidecar. Only opaque callback runs
    /// are returned to the host because their closures cannot cross the protocol.
    /// Immediate launch failures are completed here so overlap queues and alarm
    /// changes remain authoritative.
    async fn start_cron_runs(
        &mut self,
        request: &RequestFrame,
        vm_id: &str,
        runs: Vec<CronRun>,
        alarm: &mut CronAlarm,
        events: &mut Vec<CronEventRecord>,
    ) -> Result<Vec<CronRun>, SidecarError> {
        let mut pending = VecDeque::from(runs);
        let mut host_runs = Vec::new();
        while let Some(run) = pending.pop_front() {
            let action = match agentos_native_sidecar_core::decode_cron_action(&run.action) {
                Ok(action) => action,
                Err(error) => {
                    self.complete_failed_cron_run(
                        vm_id,
                        run.run_id,
                        error.to_string(),
                        alarm,
                        events,
                        &mut pending,
                    )?;
                    continue;
                }
            };
            match action {
                CronAction::Callback { .. } => host_runs.push(run),
                CronAction::Session {
                    agent_type,
                    prompt,
                    options,
                } => {
                    let action_result = self
                        .run_cron_session_action(request, agent_type, prompt, options)
                        .await;
                    let completion = self
                        .cron_schedulers
                        .entry(vm_id.to_string())
                        .or_default()
                        .complete(
                            CompleteCronRunRequest {
                                run_id: run.run_id,
                                error: action_result.err(),
                            },
                            unix_time_ms(),
                        )
                        .map_err(cron_scheduler_error)?;
                    *alarm = completion.alarm;
                    events.extend(completion.events);
                    pending.extend(completion.runs);
                }
                CronAction::Exec { command, args } => {
                    let process_id = format!("cron-run-{}", run.run_id);
                    let payload = ExecuteRequest {
                        process_id: Some(process_id.clone()),
                        command: Some(command),
                        shell_command: None,
                        runtime: None,
                        entrypoint: None,
                        args,
                        env: None,
                        cwd: None,
                        wasm_permission_tier: None,
                        pty: None,
                        keep_stdin_open: None,
                        timeout_ms: None,
                    };
                    let internal = RequestFrame::new(
                        0,
                        request.ownership.clone(),
                        RequestPayload::Execute(payload.clone()),
                    );
                    let launch = self.execute(&internal, payload).await;
                    let launch_error = match launch {
                        Ok(DispatchResult {
                            response:
                                ResponseFrame {
                                    payload: ResponsePayload::ProcessStarted(_),
                                    ..
                                },
                            ..
                        }) => None,
                        Ok(DispatchResult {
                            response:
                                ResponseFrame {
                                    payload: ResponsePayload::Rejected(rejected),
                                    ..
                                },
                            ..
                        }) => Some(format!("{}: {}", rejected.code, rejected.message)),
                        Ok(dispatch) => Some(format!(
                            "cron exec returned unexpected response: {:?}",
                            dispatch.response.payload
                        )),
                        Err(error) => Some(error.to_string()),
                    };
                    if let Some(error) = launch_error {
                        self.complete_failed_cron_run(
                            vm_id,
                            run.run_id,
                            error,
                            alarm,
                            events,
                            &mut pending,
                        )?;
                    } else {
                        self.cron_process_runs
                            .insert((vm_id.to_string(), process_id), run.run_id);
                    }
                }
            }
        }
        Ok(host_runs)
    }

    async fn run_cron_session_action(
        &mut self,
        request: &RequestFrame,
        agent_type: String,
        prompt: String,
        options: Option<Value>,
    ) -> Result<(), String> {
        let options = options
            .map(serde_json::from_value::<CronSessionOptions>)
            .transpose()
            .map_err(|error| format!("invalid cron session options: {error}"))?
            .unwrap_or_default();
        let created = self
            .dispatch_cron_acp_request(
                request,
                AcpRequest::AcpCreateSessionRequest(AcpCreateSessionRequest {
                    agent_type,
                    runtime: None,
                    cwd: options.cwd,
                    args: None,
                    env: (!options.env.is_empty()).then_some(options.env.into_iter().collect()),
                    protocol_version: None,
                    client_capabilities: None,
                    mcp_servers: (!options.mcp_servers.is_empty())
                        .then(|| serde_json::to_string(&options.mcp_servers))
                        .transpose()
                        .map_err(|error| format!("failed to encode cron MCP servers: {error}"))?,
                    skip_os_instructions: options.skip_os_instructions.then_some(true),
                    additional_instructions: options.additional_instructions,
                }),
            )
            .await?;
        let AcpResponse::AcpSessionCreatedResponse(created) = created else {
            return Err(format!(
                "cron ACP create returned unexpected response: {created:?}"
            ));
        };
        let session_id = created.session_id;
        let prompt_result = self
            .dispatch_cron_acp_request(
                request,
                AcpRequest::AcpSessionRequest(AcpSessionRequest {
                    session_id: session_id.clone(),
                    method: String::from("session/prompt"),
                    params: Some(
                        serde_json::to_string(&json!({
                            "prompt": [{ "type": "text", "text": prompt }]
                        }))
                        .map_err(|error| format!("failed to encode cron prompt: {error}"))?,
                    ),
                }),
            )
            .await;
        let close_result = self
            .dispatch_cron_acp_request(
                request,
                AcpRequest::AcpCloseSessionRequest(AcpCloseSessionRequest {
                    session_id: session_id.clone(),
                }),
            )
            .await;
        match (prompt_result, close_result) {
            (
                Ok(AcpResponse::AcpSessionRpcResponse(_)),
                Ok(AcpResponse::AcpSessionClosedResponse(_)),
            ) => Ok(()),
            (Err(prompt_error), Err(close_error)) => Err(format!(
                "{prompt_error}; also failed to close cron ACP session {session_id}: {close_error}"
            )),
            (Err(error), _) | (_, Err(error)) => Err(error),
            (Ok(prompt), Ok(close)) => Err(format!(
                "cron ACP action returned unexpected responses: prompt={prompt:?}, close={close:?}"
            )),
        }
    }

    async fn dispatch_cron_acp_request(
        &mut self,
        request: &RequestFrame,
        acp_request: AcpRequest,
    ) -> Result<AcpResponse, String> {
        let payload = serde_bare::to_vec(&acp_request)
            .map_err(|error| format!("failed to encode cron ACP request: {error}"))?;
        let internal = RequestFrame::new(
            0,
            request.ownership.clone(),
            RequestPayload::Ext(ExtEnvelope {
                namespace: ACP_EXTENSION_NAMESPACE.to_string(),
                payload: payload.clone(),
            }),
        );
        let dispatch = self
            .dispatch_extension_request(
                &internal,
                ExtEnvelope {
                    namespace: ACP_EXTENSION_NAMESPACE.to_string(),
                    payload,
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        let envelope = match dispatch.response.payload {
            ResponsePayload::ExtResult(envelope) => envelope,
            ResponsePayload::Rejected(rejected) => {
                return Err(format!("{}: {}", rejected.code, rejected.message));
            }
            other => return Err(format!("cron ACP returned unexpected response: {other:?}")),
        };
        let response: AcpResponse = serde_bare::from_slice(&envelope.payload)
            .map_err(|error| format!("failed to decode cron ACP response: {error}"))?;
        match response {
            AcpResponse::AcpErrorResponse(error) => {
                Err(format!("{}: {}", error.code, error.message))
            }
            response => Ok(response),
        }
    }

    fn complete_failed_cron_run(
        &mut self,
        vm_id: &str,
        run_id: String,
        error: String,
        alarm: &mut CronAlarm,
        events: &mut Vec<CronEventRecord>,
        pending: &mut VecDeque<CronRun>,
    ) -> Result<(), SidecarError> {
        let completion = self
            .cron_schedulers
            .entry(vm_id.to_string())
            .or_default()
            .complete(
                CompleteCronRunRequest {
                    run_id,
                    error: Some(error),
                },
                unix_time_ms(),
            )
            .map_err(cron_scheduler_error)?;
        *alarm = completion.alarm;
        events.extend(completion.events);
        pending.extend(completion.runs);
        Ok(())
    }

    fn complete_cron_run(
        &mut self,
        request: &RequestFrame,
        payload: CompleteCronRunRequest,
    ) -> Result<DispatchResult, SidecarError> {
        let vm_id = self.require_cron_vm(&request.ownership)?;
        let response = self
            .cron_schedulers
            .entry(vm_id)
            .or_default()
            .complete(payload, unix_time_ms())
            .map_err(cron_scheduler_error)?;
        Ok(DispatchResult {
            response: self.respond(request, ResponsePayload::CronRunCompleted(response)),
            events: Vec::new(),
        })
    }

    fn export_cron_state(
        &mut self,
        request: &RequestFrame,
    ) -> Result<DispatchResult, SidecarError> {
        let vm_id = self.require_cron_vm(&request.ownership)?;
        let state = self
            .cron_schedulers
            .entry(vm_id)
            .or_default()
            .export_state()
            .map_err(cron_scheduler_error)?;
        Ok(DispatchResult {
            response: self.respond(
                request,
                ResponsePayload::CronStateExported(crate::protocol::CronStateExportedResponse {
                    state,
                }),
            ),
            events: Vec::new(),
        })
    }

    fn import_cron_state(
        &mut self,
        request: &RequestFrame,
        payload: crate::protocol::ImportCronStateRequest,
    ) -> Result<DispatchResult, SidecarError> {
        let vm_id = self.require_cron_vm(&request.ownership)?;
        let response = self
            .cron_schedulers
            .entry(vm_id.clone())
            .or_default()
            .import_state(&payload.state)
            .map_err(cron_scheduler_error)?;
        let scheduler = self
            .cron_schedulers
            .get(&vm_id)
            .expect("cron scheduler was inserted above");
        let action_bytes = scheduler.total_action_bytes();
        if action_bytes.saturating_mul(100) >= MAX_CRON_STATE_BYTES.saturating_mul(80) {
            tracing::warn!(
                vm_id,
                action_bytes,
                limit = MAX_CRON_STATE_BYTES,
                "cron action registry is near its configured byte limit"
            );
        }
        Ok(DispatchResult {
            response: self.respond(request, ResponsePayload::CronStateImported(response)),
            events: Vec::new(),
        })
    }

    fn require_cron_vm(&self, ownership: &OwnershipScope) -> Result<String, SidecarError> {
        let (connection_id, session_id, vm_id) = self.vm_scope_for(ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
        Ok(vm_id)
    }

    pub async fn poll_event(
        &mut self,
        ownership: &OwnershipScope,
        timeout: Duration,
    ) -> Result<Option<EventFrame>, SidecarError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(index) = self
                .pending_process_events
                .iter()
                .position(|event| public_process_event_matches_ownership(self, ownership, event))
            {
                let Some(envelope) = self.pending_process_events.remove(index) else {
                    continue;
                };
                if let Some(frame) = self.handle_process_event_envelope(envelope).await? {
                    return Ok(Some(frame));
                }
                continue;
            }

            if !timeout.is_zero() {
                let _ = self.pump_process_events(ownership).await?;
            }

            let queued_envelopes = {
                let pending_capacity = self.pending_process_event_capacity();
                let receiver = self.process_event_receiver.as_mut().ok_or_else(|| {
                    SidecarError::InvalidState(String::from("process event receiver unavailable"))
                })?;
                let mut queued = Vec::new();
                loop {
                    if queued.len() >= pending_capacity {
                        if receiver.is_empty() {
                            break;
                        }
                        return Err(process_event_queue_overflow_error());
                    }
                    match receiver.try_recv() {
                        Ok(envelope) => queued.push(envelope),
                        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                        Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                    }
                }
                queued
            };

            let mut matching_envelope = None;
            for envelope in queued_envelopes {
                if matching_envelope.is_none()
                    && public_process_event_matches_ownership(self, ownership, &envelope)
                {
                    matching_envelope = Some(envelope);
                } else {
                    self.queue_pending_process_event(envelope)?;
                }
            }

            if let Some(envelope) = matching_envelope {
                if let Some(frame) = self.handle_process_event_envelope(envelope).await? {
                    return Ok(Some(frame));
                }
                continue;
            }

            if Instant::now() >= deadline {
                return Ok(None);
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            time::sleep(remaining.min(Duration::from_millis(10))).await;
        }
    }

    pub(crate) async fn handle_process_event_envelope(
        &mut self,
        envelope: ProcessEventEnvelope,
    ) -> Result<Option<EventFrame>, SidecarError> {
        let handle_start = Instant::now();
        let ProcessEventEnvelope {
            connection_id,
            session_id,
            vm_id,
            process_id,
            event,
        } = envelope;

        let is_exit_event = matches!(event, ActiveExecutionEvent::Exited(_));

        if is_exit_event {
            record_execute_exit_event_queue_wait(
                "process_exit_event_queue_wait",
                &vm_id,
                &process_id,
            );
            let mut trailing = Vec::new();
            let mut deferred = VecDeque::new();
            let phase_start = Instant::now();
            while let Some(pending) = self.pending_process_events.pop_front() {
                if pending.vm_id == vm_id
                    && pending.process_id == process_id
                    && !matches!(pending.event, ActiveExecutionEvent::Exited(_))
                {
                    trailing.push(pending.event);
                } else {
                    deferred.push_back(pending);
                }
            }
            self.pending_process_events = deferred;
            record_execute_phase("process_exit_trailing_pending_scan", phase_start.elapsed());
            let drain_limit = self
                .pending_process_event_capacity()
                .saturating_sub(trailing.len().saturating_add(1));
            let phase_start = Instant::now();
            trailing.extend(
                self.drain_process_events_blocking_with_limit(&vm_id, &process_id, drain_limit)?
                    .into_iter()
                    .filter(|event| !matches!(event, ActiveExecutionEvent::Exited(_))),
            );
            record_execute_phase(
                "process_exit_trailing_blocking_drain",
                phase_start.elapsed(),
            );

            if !trailing.is_empty() {
                if self.pending_process_event_capacity() < trailing.len() {
                    return Err(process_event_queue_overflow_error());
                }
                let emit_now = if self.pending_process_event_capacity() == trailing.len() {
                    Some(trailing.remove(0))
                } else {
                    None
                };
                let phase_start = Instant::now();
                mark_execute_exit_event_queued(&vm_id, &process_id);
                self.queue_front_pending_process_event(ProcessEventEnvelope {
                    connection_id: connection_id.clone(),
                    session_id: session_id.clone(),
                    vm_id: vm_id.clone(),
                    process_id: process_id.clone(),
                    event,
                })?;
                for event in trailing.into_iter().rev() {
                    self.queue_front_pending_process_event(ProcessEventEnvelope {
                        connection_id: connection_id.clone(),
                        session_id: session_id.clone(),
                        vm_id: vm_id.clone(),
                        process_id: process_id.clone(),
                        event,
                    })?;
                }
                record_execute_phase("process_exit_trailing_requeue", phase_start.elapsed());
                if let Some(event) = emit_now {
                    let result = self
                        .handle_cron_execution_event(&vm_id, &process_id, event)
                        .await;
                    record_execute_phase(
                        "process_exit_event_handle_envelope_total",
                        handle_start.elapsed(),
                    );
                    return result;
                }
                record_execute_phase(
                    "process_exit_event_handle_envelope_total",
                    handle_start.elapsed(),
                );
                return Ok(None);
            }
        }

        let result = self
            .handle_cron_execution_event(&vm_id, &process_id, event)
            .await;
        if is_exit_event {
            record_execute_phase(
                "process_exit_event_handle_envelope_total",
                handle_start.elapsed(),
            );
        }
        result
    }

    async fn handle_cron_execution_event(
        &mut self,
        vm_id: &str,
        process_id: &str,
        event: ActiveExecutionEvent,
    ) -> Result<Option<EventFrame>, SidecarError> {
        let cron_run_id = self
            .cron_process_runs
            .get(&(vm_id.to_string(), process_id.to_string()))
            .cloned();
        if cron_run_id.is_some()
            && matches!(
                event,
                ActiveExecutionEvent::Stdout(_) | ActiveExecutionEvent::Stderr(_)
            )
        {
            return Ok(None);
        }
        let exit_code = match &event {
            ActiveExecutionEvent::Exited(exit_code) => Some(*exit_code),
            _ => None,
        };
        let ownership = self
            .vms
            .get(vm_id)
            .map(|vm| OwnershipScope::vm(&vm.connection_id, &vm.session_id, vm_id));
        let normal = self.handle_execution_event(vm_id, process_id, event)?;
        let (Some(run_id), Some(exit_code), Some(ownership)) = (cron_run_id, exit_code, ownership)
        else {
            return Ok(normal);
        };
        self.cron_process_runs
            .remove(&(vm_id.to_string(), process_id.to_string()));

        let mut completion = self
            .cron_schedulers
            .entry(vm_id.to_string())
            .or_default()
            .complete(
                CompleteCronRunRequest {
                    run_id,
                    error: (exit_code != 0)
                        .then(|| format!("cron exec exited with status {exit_code}")),
                },
                unix_time_ms(),
            )
            .map_err(cron_scheduler_error)?;
        let internal = RequestFrame::new(
            0,
            ownership.clone(),
            RequestPayload::WakeCron(WakeCronRequest {
                generation: completion.alarm.generation,
            }),
        );
        completion.runs = self
            .start_cron_runs(
                &internal,
                vm_id,
                completion.runs,
                &mut completion.alarm,
                &mut completion.events,
            )
            .await?;
        Ok(Some(EventFrame::new(
            ownership,
            EventPayload::CronDispatch(CronDispatchEvent {
                alarm: completion.alarm,
                runs: completion.runs,
                events: completion.events,
            }),
        )))
    }

    // try_poll_event moved to crate::execution

    pub async fn close_session(
        &mut self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<Vec<EventFrame>, SidecarError> {
        self.dispose_session(connection_id, session_id, DisposeReason::Requested)
            .await
    }

    pub async fn remove_connection(
        &mut self,
        connection_id: &str,
    ) -> Result<Vec<EventFrame>, SidecarError> {
        self.require_authenticated_connection(connection_id)?;

        let session_ids = self
            .connections
            .get(connection_id)
            .expect("authenticated connection should exist")
            .sessions
            .iter()
            .cloned()
            .collect::<Vec<_>>();

        let mut events = Vec::new();
        let mut first_error: Option<SidecarError> = None;
        for session_id in session_ids {
            // Attempt EVERY session; aggregate errors instead of `?`-ing out on
            // the first so one wedged session cannot abandon the rest (H1).
            match self
                .dispose_session(connection_id, &session_id, DisposeReason::ConnectionClosed)
                .await
            {
                Ok(session_events) => events.extend(session_events),
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        self.connections.remove(connection_id);
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(events)
    }

    async fn authenticate_connection(
        &mut self,
        request: &RequestFrame,
        payload: crate::protocol::AuthenticateRequest,
    ) -> Result<DispatchResult, SidecarError> {
        let _ = self.connection_id_for(&request.ownership)?;
        if let Err(error) = self.validate_auth_token(&payload.auth_token) {
            let mut fields = audit_fields([
                (String::from("source"), payload.client_name.clone()),
                (String::from("reason"), error.to_string()),
            ]);
            if let OwnershipScope::ConnectionOwnership(inner) = &request.ownership {
                fields.insert(String::from("connection_id"), inner.connection_id.clone());
            }
            emit_security_audit_event(
                &self.bridge,
                &self.config.sidecar_id,
                "security.auth.failed",
                fields,
            );
            return Err(error);
        }

        if let Err(error) = validate_authenticate_versions(&payload) {
            return Err(match error {
                AuthenticateVersionError::ProtocolVersionMismatch(message) => {
                    SidecarError::ProtocolVersionMismatch(message)
                }
                AuthenticateVersionError::BridgeVersionMismatch(message) => {
                    SidecarError::BridgeVersionMismatch(message)
                }
            });
        }

        let connection_id = self.allocate_connection_id();
        self.connections.insert(
            connection_id.clone(),
            ConnectionState {
                auth_token: payload.auth_token,
                sessions: BTreeSet::new(),
            },
        );

        let response = shared_authenticated_response(
            request.request_id,
            self.config.sidecar_id.clone(),
            connection_id,
            self.config.max_frame_bytes as u32,
        );
        Ok(DispatchResult {
            response,
            events: Vec::new(),
        })
    }

    async fn open_session(
        &mut self,
        request: &RequestFrame,
        payload: OpenSessionRequest,
    ) -> Result<DispatchResult, SidecarError> {
        let connection_id = self.connection_id_for(&request.ownership)?;
        self.require_authenticated_connection(&connection_id)?;

        self.next_session_id += 1;
        let session_id = format!("session-{}", self.next_session_id);
        self.sessions.insert(
            session_id.clone(),
            SessionState {
                connection_id: connection_id.clone(),
                placement: payload.placement,
                vm_ids: BTreeSet::new(),
            },
        );
        self.connections
            .get_mut(&connection_id)
            .expect("authenticated connection should exist")
            .sessions
            .insert(session_id.clone());

        Ok(DispatchResult {
            response: session_opened_response(request.request_id, connection_id, session_id),
            events: Vec::new(),
        })
    }

    // create_vm, dispose_vm, bootstrap_root_filesystem, configure_vm moved to crate::vm

    async fn guest_filesystem_call(
        &mut self,
        request: &RequestFrame,
        payload: GuestFilesystemCallRequest,
    ) -> Result<DispatchResult, SidecarError> {
        filesystem_guest_filesystem_call(self, request, payload).await
    }

    // snapshot_root_filesystem moved to crate::vm

    // execute, write_stdin, close_stdin, kill_process, find_listener, find_bound_udp,
    // get_signal_state, get_zombie_timer_count moved to crate::execution

    async fn dispose_session(
        &mut self,
        connection_id: &str,
        session_id: &str,
        reason: DisposeReason,
    ) -> Result<Vec<EventFrame>, SidecarError> {
        self.require_owned_session(connection_id, session_id)?;

        let vm_ids = self
            .sessions
            .get(session_id)
            .expect("owned session should exist")
            .vm_ids
            .iter()
            .cloned()
            .collect::<Vec<_>>();

        let mut events = Vec::new();
        let mut first_error: Option<SidecarError> = None;
        for vm_id in vm_ids {
            // Attempt EVERY VM; aggregate errors instead of `?`-ing out on the
            // first so one stuck VM cannot strand the remaining VMs' teardown and
            // leave the session permanently un-reclaimed (H1).
            match self
                .dispose_vm_internal(connection_id, session_id, &vm_id, reason.clone())
                .await
            {
                Ok(vm_events) => events.extend(vm_events),
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        // On client disconnect, give every registered extension a chance to free
        // the per-session state it tracks (H4): the host owns the only signal an
        // extension gets that a session has gone away.
        if matches!(reason, DisposeReason::ConnectionClosed) {
            if let Err(error) = self
                .dispose_extension_session_state(connection_id, session_id)
                .await
            {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }

        self.sessions.remove(session_id);
        if let Some(connection) = self.connections.get_mut(connection_id) {
            connection.sessions.remove(session_id);
        }
        // Tell the stdio transport this session is gone so it stops iterating a
        // dead entry every event-pump tick and the set stops growing (M5).
        self.disposed_sessions
            .push((connection_id.to_owned(), session_id.to_owned()));

        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(events)
    }

    /// Invoke each registered extension's per-session teardown hook so it can
    /// release the state it keyed on this host session. Errors are aggregated so
    /// one misbehaving extension cannot prevent the others from cleaning up.
    async fn dispose_extension_session_state(
        &mut self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<(), SidecarError> {
        let ownership = OwnershipScope::session(connection_id, session_id);
        let extensions = self
            .extensions
            .values()
            .cloned()
            .collect::<Vec<Arc<dyn Extension>>>();
        let mut first_error: Option<SidecarError> = None;
        for extension in extensions {
            let snapshot = ExtensionSnapshot::new(
                extension.namespace().to_owned(),
                ownership.clone(),
                self.sidecar_requests.clone(),
                self.event_sink.clone(),
            );
            if let Err(error) = extension.on_session_disposed(snapshot).await {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Drain the session scopes disposed since the last call so the stdio
    /// transport can untrack them from its active-session set (M5).
    pub(crate) fn take_disposed_sessions(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.disposed_sessions)
    }

    // dispose_vm_internal, terminate_vm_processes, wait_for_vm_processes_to_exit moved to crate::vm

    // kill_process_internal, handle_execution_event, handle_python_vfs_rpc_request,
    // resolve_javascript_child_process_execution, spawn_javascript_child_process,
    // poll_javascript_child_process, write_javascript_child_process_stdin,
    // close_javascript_child_process_stdin, kill_javascript_child_process moved to crate::execution

    /// Whether a `__kernel_stdin_read` / `__kernel_poll` RPC may be serviced
    /// via the non-blocking deferral path. Non-TTY JavaScript keeps its
    /// in-process local stdin bridge (serviced inline by the fallback arm).
    fn kernel_wait_rpc_is_deferrable(
        &self,
        vm_id: &str,
        process_id: &str,
        request: &JavascriptSyncRpcRequest,
    ) -> bool {
        let Some(vm) = self.vms.get(vm_id) else {
            return false;
        };
        let Some(process) = vm.active_processes.get(process_id) else {
            return false;
        };
        if request.method == "__kernel_stdin_read"
            && matches!(
                process.execution,
                crate::state::ActiveExecution::Javascript(_)
            )
            && process.tty_master_fd.is_none()
        {
            return false;
        }
        true
    }

    /// Service `__kernel_stdin_read` / `__kernel_poll` without blocking the
    /// dispatch loop. Probes readiness with a zero timeout; when not ready and
    /// the requested timeout has not expired, parks the RPC on the process
    /// (reply-by-token) and spawns a waiter that re-enqueues it as a process
    /// event when kernel poll state changes or the deadline passes. The kernel
    /// waits stay event-driven (PollNotifier), so a host stdin write wakes the
    /// guest immediately instead of after a polling slice.
    ///
    /// Returns `Ok(Some(response))` to reply now, `Ok(None)` when parked.
    fn service_deferrable_kernel_wait_rpc(
        &mut self,
        vm_id: &str,
        process_id: &str,
        request: &JavascriptSyncRpcRequest,
    ) -> Result<Option<crate::execution::JavascriptSyncRpcServiceResponse>, SidecarError> {
        let requested_timeout_ms = match request.method.as_str() {
            "__kernel_stdin_read" => parse_kernel_stdin_read_args(request)?.1,
            _ => u64::try_from(parse_kernel_poll_args(request)?.1).unwrap_or(0),
        };
        let now = Instant::now();

        let Some(vm) = self.vms.get_mut(vm_id) else {
            log_stale_process_event(&self.bridge, vm_id, process_id, "deferred kernel wait RPC");
            return Ok(None);
        };
        let wait_handle = vm.kernel.poll_wait_handle();
        // Snapshot BEFORE the readiness probe: a write landing between the
        // probe and the waiter's wait bumps the generation, so the wait
        // returns immediately instead of losing the wakeup.
        let generation = wait_handle.snapshot();
        let Some(process) = vm.active_processes.get_mut(process_id) else {
            log_stale_process_event(&self.bridge, vm_id, process_id, "deferred kernel wait RPC");
            return Ok(None);
        };
        let kernel_pid = process.kernel_pid;
        let deadline = match &process.deferred_kernel_wait_rpc {
            Some((parked, parked_deadline)) if parked.id == request.id => *parked_deadline,
            _ => now + Duration::from_millis(requested_timeout_ms),
        };
        let probe = match request.method.as_str() {
            "__kernel_stdin_read" => {
                let (max_bytes, _) = parse_kernel_stdin_read_args(request)?;
                kernel_stdin_read_response(&mut vm.kernel, kernel_pid, max_bytes, Duration::ZERO)
            }
            _ => {
                let (fd_requests, _) = parse_kernel_poll_args(request)?;
                kernel_poll_response(&vm.kernel, kernel_pid, &fd_requests, 0)
            }
        };
        let Some(process) = vm.active_processes.get_mut(process_id) else {
            return Ok(None);
        };
        let probe = match probe {
            Ok(value) => value,
            Err(error) => {
                process.deferred_kernel_wait_rpc = None;
                return Err(error);
            }
        };
        let ready = match request.method.as_str() {
            "__kernel_stdin_read" => !probe.is_null(),
            _ => probe.get("readyCount").and_then(Value::as_u64).unwrap_or(0) > 0,
        };
        if ready || requested_timeout_ms == 0 || now >= deadline {
            process.deferred_kernel_wait_rpc = None;
            return Ok(Some(probe.into()));
        }

        let connection_id = vm.connection_id.clone();
        let session_id = vm.session_id.clone();
        let remaining = deadline.saturating_duration_since(now);
        let sender = self.process_event_sender.clone();
        let waiter_request = request.clone();
        let envelope_vm_id = vm_id.to_owned();
        let envelope_process_id = process_id.to_owned();
        let spawned = std::thread::Builder::new()
            .name(String::from("kernel-wait-rpc"))
            .spawn(move || {
                // Wake on any kernel poll-state change or the deadline; either
                // way requeue exactly once — the handler re-probes and either
                // replies or re-parks.
                let _ = wait_handle.wait_for_change(generation, Some(remaining));
                let _ = sender.blocking_send(ProcessEventEnvelope {
                    connection_id,
                    session_id,
                    vm_id: envelope_vm_id,
                    process_id: envelope_process_id,
                    event: ActiveExecutionEvent::JavascriptSyncRpcRequest(waiter_request),
                });
            });
        let Some(vm) = self.vms.get_mut(vm_id) else {
            return Ok(None);
        };
        let Some(process) = vm.active_processes.get_mut(process_id) else {
            return Ok(None);
        };
        if spawned.is_err() {
            // Degrade to pre-deferral behavior: reply not-ready now and let the
            // guest re-issue its bounded wait.
            process.deferred_kernel_wait_rpc = None;
            return Ok(Some(probe.into()));
        }
        process.deferred_kernel_wait_rpc = Some((request.clone(), deadline));
        Ok(None)
    }

    pub(crate) fn handle_javascript_sync_rpc_request(
        &mut self,
        vm_id: &str,
        process_id: &str,
        request: JavascriptSyncRpcRequest,
    ) -> Result<(), SidecarError> {
        record_sync_bridge_request_observed(request.id, &request.method);
        let Some(vm) = self.vms.get(vm_id) else {
            log_stale_process_event(&self.bridge, vm_id, process_id, "javascript sync RPC");
            return Ok(());
        };
        if !vm.active_processes.contains_key(process_id) {
            log_stale_process_event(&self.bridge, vm_id, process_id, "javascript sync RPC");
            return Ok(());
        }

        let response: Result<crate::execution::JavascriptSyncRpcServiceResponse, SidecarError> =
            match request.method.as_str() {
                "child_process.spawn" => {
                    let Some(vm) = self.vms.get(vm_id) else {
                        log_stale_process_event(
                            &self.bridge,
                            vm_id,
                            process_id,
                            "javascript sync RPC child_process.spawn",
                        );
                        return Ok(());
                    };
                    let (payload, _) =
                        parse_javascript_child_process_spawn_request(vm, &request.args)?;
                    self.spawn_javascript_child_process(vm_id, process_id, payload)
                        .map(Into::into)
                }
                "child_process.spawn_sync" => {
                    let Some(vm) = self.vms.get(vm_id) else {
                        log_stale_process_event(
                            &self.bridge,
                            vm_id,
                            process_id,
                            "javascript sync RPC child_process.spawn_sync",
                        );
                        return Ok(());
                    };
                    let (payload, max_buffer) =
                        parse_javascript_child_process_spawn_request(vm, &request.args)?;
                    self.spawn_javascript_child_process_sync(vm_id, process_id, payload, max_buffer)
                        .map(Into::into)
                }
                "child_process.poll" => {
                    let child_process_id = javascript_sync_rpc_arg_str(
                        &request.args,
                        0,
                        "child_process.poll child id",
                    )?;
                    let wait_ms = javascript_sync_rpc_arg_u64_optional(
                        &request.args,
                        1,
                        "child_process.poll wait ms",
                    )?
                    .unwrap_or_default();
                    self.poll_javascript_child_process(vm_id, process_id, child_process_id, wait_ms)
                        .map(Into::into)
                }
                "child_process.write_stdin" => {
                    let child_process_id = javascript_sync_rpc_arg_str(
                        &request.args,
                        0,
                        "child_process.write_stdin child id",
                    )?;
                    let chunk = javascript_sync_rpc_bytes_arg(
                        &request.args,
                        1,
                        "child_process.write_stdin chunk",
                    )?;
                    self.write_javascript_child_process_stdin(
                        vm_id,
                        process_id,
                        child_process_id,
                        &chunk,
                    )?;
                    Ok(Value::Null.into())
                }
                "child_process.close_stdin" => {
                    let child_process_id = javascript_sync_rpc_arg_str(
                        &request.args,
                        0,
                        "child_process.close_stdin child id",
                    )?;
                    self.close_javascript_child_process_stdin(vm_id, process_id, child_process_id)?;
                    Ok(Value::Null.into())
                }
                "child_process.kill" => {
                    let child_process_id = javascript_sync_rpc_arg_str(
                        &request.args,
                        0,
                        "child_process.kill child id",
                    )?;
                    let signal =
                        javascript_sync_rpc_arg_str(&request.args, 1, "child_process.kill signal")?;
                    self.kill_javascript_child_process(
                        vm_id,
                        process_id,
                        child_process_id,
                        signal,
                    )?;
                    Ok(Value::Null.into())
                }
                "process.kill" => {
                    let target_pid =
                        javascript_sync_rpc_arg_i32(&request.args, 0, "process.kill target pid")?;
                    let signal =
                        javascript_sync_rpc_arg_str(&request.args, 1, "process.kill signal")?;
                    let parsed_signal = parse_signal(signal)?;
                    if parsed_signal == 0 {
                        let Some(vm) = self.vms.get(vm_id) else {
                            log_stale_process_event(
                                &self.bridge,
                                vm_id,
                                process_id,
                                "javascript sync RPC process.kill",
                            );
                            return Ok(());
                        };
                        if !vm.active_processes.contains_key(process_id) {
                            log_stale_process_event(
                                &self.bridge,
                                vm_id,
                                process_id,
                                "javascript sync RPC process.kill",
                            );
                            return Ok(());
                        }
                        vm.kernel
                            .signal_process(EXECUTION_DRIVER_NAME, target_pid, parsed_signal)
                            .map(|()| Value::Null.into())
                            .map_err(kernel_error)
                    } else if target_pid < 0 {
                        let caller_kernel_pid = {
                            let Some(vm) = self.vms.get(vm_id) else {
                                log_stale_process_event(
                                    &self.bridge,
                                    vm_id,
                                    process_id,
                                    "javascript sync RPC process.kill",
                                );
                                return Ok(());
                            };
                            let Some(caller) = vm.active_processes.get(process_id) else {
                                log_stale_process_event(
                                    &self.bridge,
                                    vm_id,
                                    process_id,
                                    "javascript sync RPC process.kill",
                                );
                                return Ok(());
                            };
                            caller.kernel_pid
                        };
                        let pgid = target_pid.unsigned_abs();
                        match self.signal_vm_process_group(vm_id, caller_kernel_pid, pgid, signal) {
                            Ok(true) => Ok(self
                                .apply_self_process_kill(vm_id, process_id, parsed_signal)
                                .into()),
                            Ok(false) => Ok(Value::Null.into()),
                            Err(error) => Err(error),
                        }
                    } else {
                        enum ProcessKillTarget {
                            SelfProcess,
                            Child(String),
                            TopLevel(String),
                            KernelPid(u32),
                        }
                        let target = {
                            let Some(vm) = self.vms.get(vm_id) else {
                                log_stale_process_event(
                                    &self.bridge,
                                    vm_id,
                                    process_id,
                                    "javascript sync RPC process.kill",
                                );
                                return Ok(());
                            };
                            let Some(caller) = vm.active_processes.get(process_id) else {
                                log_stale_process_event(
                                    &self.bridge,
                                    vm_id,
                                    process_id,
                                    "javascript sync RPC process.kill",
                                );
                                return Ok(());
                            };
                            let caller_pid = i32::try_from(caller.kernel_pid).map_err(|_| {
                                SidecarError::InvalidState("caller pid exceeds i32".into())
                            })?;
                            if caller_pid == target_pid {
                                ProcessKillTarget::SelfProcess
                            } else if let Some((child_process_id, _)) =
                                caller.child_processes.iter().find(|(_, child)| {
                                    i32::try_from(child.kernel_pid) == Ok(target_pid)
                                })
                            {
                                ProcessKillTarget::Child(child_process_id.clone())
                            } else if let Some((target_process_id, _)) =
                                vm.active_processes.iter().find(|(_, process)| {
                                    i32::try_from(process.kernel_pid) == Ok(target_pid)
                                })
                            {
                                ProcessKillTarget::TopLevel(target_process_id.clone())
                            } else {
                                let target_kernel_pid =
                                    u32::try_from(target_pid).map_err(|_| {
                                        SidecarError::InvalidState(format!(
                                            "EINVAL: invalid process pid {target_pid}"
                                        ))
                                    })?;
                                ProcessKillTarget::KernelPid(target_kernel_pid)
                            }
                        };
                        match target {
                            ProcessKillTarget::SelfProcess => Ok(self
                                .apply_self_process_kill(vm_id, process_id, parsed_signal)
                                .into()),
                            ProcessKillTarget::Child(child_process_id) => {
                                self.kill_javascript_child_process(
                                    vm_id,
                                    process_id,
                                    &child_process_id,
                                    signal,
                                )?;
                                Ok(Value::Null.into())
                            }
                            ProcessKillTarget::TopLevel(target_process_id) => {
                                self.kill_process_internal(vm_id, &target_process_id, signal)?;
                                Ok(Value::Null.into())
                            }
                            ProcessKillTarget::KernelPid(target_kernel_pid) => {
                                // Grandchildren and untracked kernel processes are
                                // resolved VM-wide instead of failing with an
                                // unknown-pid error.
                                self.signal_vm_kernel_pid(vm_id, target_kernel_pid, signal)
                                    .map(|()| Value::Null.into())
                            }
                        }
                    }
                }
                "process.signal_state" => {
                    let (signal, registration) = parse_process_signal_state_request(&request.args)
                        .map_err(|error| SidecarError::InvalidState(error.to_string()))?;
                    let Some(vm) = self.vms.get_mut(vm_id) else {
                        log_stale_process_event(
                            &self.bridge,
                            vm_id,
                            process_id,
                            "javascript sync RPC process.signal_state",
                        );
                        return Ok(());
                    };
                    apply_process_signal_state_update(
                        &mut vm.signal_states,
                        process_id,
                        signal,
                        registration,
                    );
                    Ok(Value::Null.into())
                }
                "net.http_request" => {
                    let payload = request
                        .args
                        .first()
                        .cloned()
                        .ok_or_else(|| {
                            SidecarError::InvalidState(String::from(
                                "net.http_request requires a request payload",
                            ))
                        })
                        .and_then(|value| {
                            serde_json::from_value::<JavascriptHttpLoopbackRequest>(value).map_err(
                                |error| {
                                    SidecarError::InvalidState(format!(
                                        "invalid net.http_request payload: {error}"
                                    ))
                                },
                            )
                        })?;
                    if !is_javascript_loopback_host(&payload.host) {
                        return Err(SidecarError::Execution(format!(
                            "EACCES: HTTP loopback request requires a loopback host, got {}",
                            payload.host
                        )));
                    }
                    self.bridge.require_network_access(
                        vm_id,
                        NetworkOperation::Http,
                        format_tcp_resource(&payload.host, payload.port),
                    )?;
                    let Some(vm) = self.vms.get_mut(vm_id) else {
                        log_stale_process_event(
                            &self.bridge,
                            vm_id,
                            process_id,
                            "javascript sync RPC net.http_request",
                        );
                        return Ok(());
                    };
                    let resource_limits = vm.kernel.resource_limits().clone();
                    let socket_paths = build_javascript_socket_path_context(vm)?;
                    let target_is_current =
                        [JavascriptSocketFamily::Ipv4, JavascriptSocketFamily::Ipv6]
                            .iter()
                            .any(|family| {
                                socket_paths
                                    .http_loopback_target(*family, payload.port)
                                    .is_some_and(|target| {
                                        target.process_id == payload.process_id
                                            && target.server_id == payload.server_id
                                    })
                            });
                    if !target_is_current {
                        return Err(SidecarError::InvalidState(format!(
                            "unknown HTTP loopback target {}:{} for server {} in process {}",
                            payload.host, payload.port, payload.server_id, payload.process_id
                        )));
                    }
                    let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
                    let Some(target_process) = vm.active_processes.get_mut(&payload.process_id)
                    else {
                        return Err(SidecarError::InvalidState(format!(
                            "unknown HTTP loopback process {}",
                            payload.process_id
                        )));
                    };
                    dispatch_loopback_http_request(LoopbackHttpDispatchRequest {
                        bridge: &self.bridge,
                        vm_id,
                        dns: &vm.dns,
                        socket_paths: &socket_paths,
                        kernel: &mut vm.kernel,
                        kernel_readiness,
                        process: target_process,
                        resource_limits: &resource_limits,
                        server_id: payload.server_id,
                        request_json: &payload.request,
                    })
                    .map(Value::String)
                    .map(Into::into)
                }
                "__kernel_stdio_write"
                    if self
                        .vms
                        .get(vm_id)
                        .and_then(|vm| vm.active_processes.get(process_id))
                        .is_some_and(|process| process.tty_master_owner.is_some()) =>
                {
                    let (writer_kernel_pid, owner) = {
                        let process = self
                            .vms
                            .get(vm_id)
                            .and_then(|vm| vm.active_processes.get(process_id))
                            .expect("guarded by match arm");
                        (
                            process.kernel_pid,
                            process.tty_master_owner.expect("guarded by match arm"),
                        )
                    };
                    self.service_shared_tty_stdio_write(vm_id, writer_kernel_pid, owner, &request)
                        .map(Into::into)
                }
                "__kernel_stdin_read" | "__kernel_poll"
                    if self.kernel_wait_rpc_is_deferrable(vm_id, process_id, &request) =>
                {
                    match self.service_deferrable_kernel_wait_rpc(vm_id, process_id, &request) {
                        Ok(Some(response)) => Ok(response),
                        // Parked: an off-loop waiter re-enqueues this request as a
                        // process event when kernel poll state changes.
                        Ok(None) => return Ok(()),
                        Err(error) => Err(error),
                    }
                }
                _ => {
                    let Some(vm) = self.vms.get_mut(vm_id) else {
                        log_stale_process_event(
                            &self.bridge,
                            vm_id,
                            process_id,
                            "javascript sync RPC bridge dispatch",
                        );
                        return Ok(());
                    };
                    let resource_limits = vm.kernel.resource_limits().clone();
                    let network_counts = vm_network_resource_counts(vm);
                    let socket_paths = build_javascript_socket_path_context(vm)?;
                    let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
                    let Some(process) = vm.active_processes.get_mut(process_id) else {
                        log_stale_process_event(
                            &self.bridge,
                            vm_id,
                            process_id,
                            "javascript sync RPC bridge dispatch",
                        );
                        return Ok(());
                    };
                    service_javascript_sync_rpc(JavascriptSyncRpcServiceRequest {
                        bridge: &self.bridge,
                        vm_id,
                        dns: &vm.dns,
                        socket_paths: &socket_paths,
                        kernel: &mut vm.kernel,
                        kernel_readiness,
                        process,
                        sync_request: &request,
                        resource_limits: &resource_limits,
                        network_counts,
                    })
                }
            };

        let Some(vm) = self.vms.get_mut(vm_id) else {
            log_stale_process_event(
                &self.bridge,
                vm_id,
                process_id,
                "javascript sync RPC response delivery",
            );
            return Ok(());
        };
        let shadow_root = vm.cwd.clone();
        let Some(process) = vm.active_processes.get_mut(process_id) else {
            log_stale_process_event(
                &self.bridge,
                vm_id,
                process_id,
                "javascript sync RPC response delivery",
            );
            return Ok(());
        };

        if response.is_ok()
            && matches!(
                request.method.as_str(),
                "fs.chmodSync" | "fs.promises.chmod"
            )
        {
            let guest_path =
                javascript_sync_rpc_arg_str(&request.args, 0, "filesystem chmod path")?;
            let mode =
                javascript_sync_rpc_arg_u32(&request.args, 1, "filesystem chmod mode")? & 0o7777;
            let host_path =
                shadow_host_path_for_process(&shadow_root, &process.guest_cwd, guest_path);
            if host_path.exists() {
                fs::set_permissions(&host_path, fs::Permissions::from_mode(mode)).map_err(
                    |error| {
                        SidecarError::Io(format!(
                            "failed to mirror chmod to shadow path {}: {error}",
                            host_path.display()
                        ))
                    },
                )?;
            }
        }

        match response {
            Ok(result) => process
                .execution
                .respond_javascript_sync_rpc_response(request.id, result)
                .or_else(ignore_stale_javascript_sync_rpc_response),
            Err(error) => process
                .execution
                .respond_javascript_sync_rpc_error(
                    request.id,
                    javascript_sync_rpc_error_code(&error),
                    error.to_string(),
                )
                .or_else(ignore_stale_javascript_sync_rpc_response),
        }
    }

    /// Applies a `process.kill` aimed at the calling process itself and
    /// returns the self-delivery action payload for the bridge.
    fn apply_self_process_kill(
        &mut self,
        vm_id: &str,
        process_id: &str,
        parsed_signal: i32,
    ) -> Value {
        let action = self
            .vms
            .get(vm_id)
            .and_then(|vm| vm.signal_states.get(process_id))
            .and_then(|handlers| handlers.get(&(parsed_signal as u32)))
            .map(|registration| registration.action.clone())
            .unwrap_or(SignalDispositionAction::Default);
        if action == SignalDispositionAction::Default
            && parsed_signal != 0
            && !matches!(
                canonical_signal_name(parsed_signal),
                Some("SIGWINCH" | "SIGCHLD" | "SIGCONT" | "SIGURG")
            )
        {
            if let Some(vm) = self.vms.get_mut(vm_id) {
                if let Some(process) = vm.active_processes.get_mut(process_id) {
                    process.pending_self_signal_exit = Some(parsed_signal);
                }
            }
        }
        json!({
            "self": true,
            "action": match action {
                SignalDispositionAction::Default => "default",
                SignalDispositionAction::Ignore => "ignore",
                SignalDispositionAction::User => "user",
            },
        })
    }

    pub(crate) fn vm_ids_for_scope(
        &self,
        ownership: &OwnershipScope,
    ) -> Result<Vec<String>, SidecarError> {
        match ownership {
            OwnershipScope::SessionOwnership(inner) => {
                self.require_owned_session(&inner.connection_id, &inner.session_id)?;
                Ok(self
                    .sessions
                    .get(&inner.session_id)
                    .expect("owned session should exist")
                    .vm_ids
                    .iter()
                    .cloned()
                    .collect())
            }
            OwnershipScope::VmOwnership(inner) => {
                self.require_owned_vm(&inner.connection_id, &inner.session_id, &inner.vm_id)?;
                Ok(vec![inner.vm_id.clone()])
            }
            OwnershipScope::ConnectionOwnership(..) => Err(SidecarError::InvalidState(
                String::from("event polling requires session or VM ownership scope"),
            )),
        }
    }

    pub(crate) fn vm_ownership(&self, vm_id: &str) -> Result<OwnershipScope, SidecarError> {
        let vm = self
            .vms
            .get(vm_id)
            .ok_or_else(|| SidecarError::InvalidState(format!("unknown sidecar VM {vm_id}")))?;
        Ok(OwnershipScope::vm(&vm.connection_id, &vm.session_id, vm_id))
    }

    pub(crate) fn vm_has_active_processes(&self, vm_id: &str) -> bool {
        self.vms
            .get(vm_id)
            .is_some_and(|vm| !vm.active_processes.is_empty())
    }

    fn require_authenticated_connection(&self, connection_id: &str) -> Result<(), SidecarError> {
        if self.connections.contains_key(connection_id) {
            Ok(())
        } else {
            Err(SidecarError::InvalidState(format!(
                "connection {connection_id} has not authenticated"
            )))
        }
    }

    pub(crate) fn require_owned_session(
        &self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<(), SidecarError> {
        self.require_authenticated_connection(connection_id)?;
        let session = self.sessions.get(session_id).ok_or_else(|| {
            SidecarError::InvalidState(format!("unknown sidecar session {session_id}"))
        })?;
        if session.connection_id == connection_id {
            Ok(())
        } else {
            Err(SidecarError::InvalidState(format!(
                "session {session_id} is not owned by connection {connection_id}"
            )))
        }
    }

    pub(crate) fn require_owned_vm(
        &self,
        connection_id: &str,
        session_id: &str,
        vm_id: &str,
    ) -> Result<(), SidecarError> {
        self.require_owned_session(connection_id, session_id)?;
        let vm = self
            .vms
            .get(vm_id)
            .ok_or_else(|| SidecarError::InvalidState(format!("unknown sidecar VM {vm_id}")))?;
        if vm.connection_id != connection_id || vm.session_id != session_id {
            return Err(SidecarError::InvalidState(format!(
                "VM {vm_id} is not owned by {connection_id}/{session_id}"
            )));
        }
        Ok(())
    }

    fn connection_id_for(&self, ownership: &OwnershipScope) -> Result<String, SidecarError> {
        match ownership {
            OwnershipScope::ConnectionOwnership(inner) => Ok(inner.connection_id.clone()),
            OwnershipScope::SessionOwnership(..) | OwnershipScope::VmOwnership(..) => {
                Err(SidecarError::InvalidState(String::from(
                    "request requires connection ownership scope",
                )))
            }
        }
    }

    fn validate_auth_token(&self, auth_token: &str) -> Result<(), SidecarError> {
        let Some(expected_auth_token) = self.config.expected_auth_token.as_deref() else {
            return Ok(());
        };

        if auth_token == expected_auth_token {
            Ok(())
        } else {
            Err(SidecarError::Unauthorized(String::from(
                "authenticate request provided an invalid auth token",
            )))
        }
    }

    fn allocate_connection_id(&mut self) -> String {
        self.next_connection_id += 1;
        format!("conn-{}", self.next_connection_id)
    }

    fn take_matching_process_event_envelope(
        &mut self,
        vm_id: &str,
        process_id: &str,
    ) -> Result<Option<ProcessEventEnvelope>, SidecarError> {
        if let Some(index) = self
            .pending_process_events
            .iter()
            .position(|event| event.vm_id == vm_id && event.process_id == process_id)
        {
            return Ok(self.pending_process_events.remove(index));
        }

        let mut matching_envelope = None;
        let mut deferred = Vec::new();
        {
            let pending_capacity = self.pending_process_event_capacity();
            let receiver = self.process_event_receiver.as_mut().ok_or_else(|| {
                SidecarError::InvalidState(String::from("process event receiver unavailable"))
            })?;
            loop {
                if deferred.len() >= pending_capacity {
                    if receiver.is_empty() {
                        break;
                    }
                    return Err(process_event_queue_overflow_error());
                }
                let envelope = match receiver.try_recv() {
                    Ok(envelope) => envelope,
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                };
                if matching_envelope.is_none()
                    && envelope.vm_id == vm_id
                    && envelope.process_id == process_id
                {
                    matching_envelope = Some(envelope);
                    break;
                }
                deferred.push(envelope);
            }
        }
        for envelope in deferred {
            self.queue_pending_process_event(envelope)?;
        }

        Ok(matching_envelope)
    }

    fn allocate_sidecar_request_id(&mut self) -> RequestId {
        let request_id = self.next_sidecar_request_id;
        self.next_sidecar_request_id -= 1;
        request_id
    }

    pub(crate) fn session_scope_for(
        &self,
        ownership: &OwnershipScope,
    ) -> Result<(String, String), SidecarError> {
        match ownership {
            OwnershipScope::SessionOwnership(inner) => {
                Ok((inner.connection_id.clone(), inner.session_id.clone()))
            }
            OwnershipScope::ConnectionOwnership(..) | OwnershipScope::VmOwnership(..) => {
                Err(SidecarError::InvalidState(String::from(
                    "request requires session ownership scope",
                )))
            }
        }
    }

    pub(crate) fn vm_scope_for(
        &self,
        ownership: &OwnershipScope,
    ) -> Result<(String, String, String), SidecarError> {
        match ownership {
            OwnershipScope::VmOwnership(inner) => Ok((
                inner.connection_id.clone(),
                inner.session_id.clone(),
                inner.vm_id.clone(),
            )),
            OwnershipScope::ConnectionOwnership(..) | OwnershipScope::SessionOwnership(..) => Err(
                SidecarError::InvalidState(String::from("request requires VM ownership scope")),
            ),
        }
    }

    pub(crate) fn respond(
        &self,
        request: &RequestFrame,
        payload: ResponsePayload,
    ) -> ResponseFrame {
        shared_respond(request, payload)
    }

    fn reject(&self, request: &RequestFrame, code: &str, message: &str) -> ResponseFrame {
        shared_reject(request, code, message)
    }

    pub fn queue_sidecar_request(
        &mut self,
        ownership: OwnershipScope,
        payload: SidecarRequestPayload,
    ) -> Result<RequestId, SidecarError> {
        if self.outbound_sidecar_requests.len() >= MAX_OUTBOUND_SIDECAR_REQUESTS {
            return Err(outbound_sidecar_request_queue_overflow_error());
        }
        if self.pending_sidecar_responses.pending_count() >= MAX_PENDING_SIDECAR_RESPONSES {
            return Err(sidecar_response_pending_overflow_error());
        }
        let request_id = self.allocate_sidecar_request_id();
        let request = SidecarRequestFrame::new(request_id, ownership, payload);
        self.pending_sidecar_responses
            .register_request(&request)
            .map_err(sidecar_response_tracker_error)?;
        self.outbound_sidecar_requests.push_back(request);
        self.outbound_sidecar_requests_gauge
            .observe_depth(self.outbound_sidecar_requests.len());
        self.pending_sidecar_responses_gauge
            .observe_depth(self.pending_sidecar_responses.pending_count());
        Ok(request_id)
    }

    pub fn queue_wire_sidecar_request(
        &mut self,
        ownership: crate::wire::OwnershipScope,
        payload: crate::wire::SidecarRequestPayload,
    ) -> Result<crate::wire::RequestId, SidecarError> {
        let ownership = crate::wire::ownership_scope_to_compat(ownership);
        let payload = crate::wire::sidecar_request_payload_to_compat(&ownership, payload)
            .map_err(wire_protocol_error)?;
        self.queue_sidecar_request(ownership, payload)
    }

    pub fn pop_sidecar_request(&mut self) -> Option<SidecarRequestFrame> {
        let request = self.outbound_sidecar_requests.pop_front();
        self.outbound_sidecar_requests_gauge
            .observe_depth(self.outbound_sidecar_requests.len());
        request
    }

    pub fn pop_wire_sidecar_request(
        &mut self,
    ) -> Result<Option<crate::wire::SidecarRequestFrame>, SidecarError> {
        self.pop_sidecar_request()
            .map(crate::wire::sidecar_request_frame_from_compat)
            .transpose()
            .map_err(wire_protocol_error)
    }

    pub fn accept_sidecar_response(
        &mut self,
        response: SidecarResponseFrame,
    ) -> Result<(), SidecarError> {
        match self.pending_sidecar_responses.accept_response(&response) {
            Ok(()) => {}
            // A response for a request that is no longer pending (its owning VM
            // was disposed, abandoning the in-flight callback) or already
            // completed is a benign late/stale reply on the shared sidecar — a
            // per-VM `sidecar_request` can be answered by the host after that VM
            // has been torn down (multiple VMs share one sidecar process). Drop
            // it instead of failing the whole sidecar over a harmless straggler.
            Err(
                error @ (SidecarResponseTrackerError::UnmatchedResponse { .. }
                | SidecarResponseTrackerError::DuplicateResponse { .. }),
            ) => {
                tracing::warn!(
                    request_id = response.request_id,
                    "dropping stale sidecar response with no matching pending request: {error}"
                );
                return Ok(());
            }
            Err(error) => return Err(sidecar_response_tracker_error(error)),
        }
        self.pending_sidecar_responses_gauge
            .observe_depth(self.pending_sidecar_responses.pending_count());
        self.completed_sidecar_response_order
            .push_back(response.request_id);
        self.completed_sidecar_responses
            .insert(response.request_id, response);
        self.completed_sidecar_responses_gauge
            .observe_depth(self.completed_sidecar_responses.len());
        while self.completed_sidecar_responses.len() > MAX_COMPLETED_SIDECAR_RESPONSES {
            match self.completed_sidecar_response_order.pop_front() {
                // Only a response that was never retrieved is a real loss; an id
                // already taken via take_sidecar_response leaves a stale order
                // entry that removes to None and is not a dropped response.
                Some(evicted) => {
                    if self.completed_sidecar_responses.remove(&evicted).is_some() {
                        tracing::warn!(
                            queue = "completed_sidecar_responses",
                            evicted_request_id = evicted,
                            capacity = MAX_COMPLETED_SIDECAR_RESPONSES,
                            "dropping an unretrieved completed sidecar response to stay within cap; the host can no longer fetch it (response lost)"
                        );
                        self.completed_sidecar_responses_gauge
                            .observe_depth(self.completed_sidecar_responses.len());
                    }
                }
                None => break,
            }
        }
        Ok(())
    }

    pub fn accept_wire_sidecar_response(
        &mut self,
        response: crate::wire::SidecarResponseFrame,
    ) -> Result<(), SidecarError> {
        let response =
            crate::wire::sidecar_response_frame_to_compat(response).map_err(wire_protocol_error)?;
        self.accept_sidecar_response(response)
    }

    pub fn take_sidecar_response(&mut self, request_id: RequestId) -> Option<SidecarResponseFrame> {
        let response = self.completed_sidecar_responses.remove(&request_id);
        if response.is_some() {
            self.completed_sidecar_response_order
                .retain(|completed_id| completed_id != &request_id);
            self.completed_sidecar_responses_gauge
                .observe_depth(self.completed_sidecar_responses.len());
        }
        response
    }

    pub fn take_wire_sidecar_response(
        &mut self,
        request_id: crate::wire::RequestId,
    ) -> Result<Option<crate::wire::SidecarResponseFrame>, SidecarError> {
        self.take_sidecar_response(request_id)
            .map(|response| {
                crate::wire::sidecar_response_frame_from_compat(response)
                    .map_err(wire_protocol_error)
            })
            .transpose()
    }

    pub(crate) fn vm_lifecycle_event(
        &self,
        connection_id: &str,
        session_id: &str,
        vm_id: &str,
        state: VmLifecycleState,
    ) -> EventFrame {
        shared_vm_lifecycle_event(connection_id, session_id, vm_id, state)
    }

    fn ensure_request_within_frame_limit(
        &self,
        request: &RequestFrame,
    ) -> Result<(), SidecarError> {
        let frame = crate::protocol::to_generated_protocol_frame(
            &crate::protocol::ProtocolFrame::Request(request.clone()),
        )
        .map_err(|error| {
            SidecarError::InvalidState(format!("failed to convert request frame: {error}"))
        })?;
        let crate::wire::ProtocolFrame::RequestFrame(_) = &frame else {
            return Err(SidecarError::InvalidState(String::from(
                "request converted to non-request wire frame",
            )));
        };

        crate::wire::WireFrameCodec::new(self.config.max_frame_bytes)
            .encode(&frame)
            .map(|_| ())
            .map_err(|error| SidecarError::FrameTooLarge(error.to_string()))
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn cron_scheduler_error(error: agentos_native_sidecar_core::CronSchedulerError) -> SidecarError {
    SidecarError::InvalidState(error.to_string())
}

impl<B> ExtensionHost for NativeSidecar<B>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    fn registered_host_tool_reference<'a>(
        &'a mut self,
        ownership: OwnershipScope,
    ) -> ExtensionFuture<'a, String> {
        Box::pin(async move {
            let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
            self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
            let vm = self
                .vms
                .get(&vm_id)
                .ok_or_else(|| SidecarError::InvalidState(format!("unknown VM {vm_id}")))?;
            crate::tools::build_host_tool_reference(&vm.toolkits)
        })
    }

    fn spawn_process<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        payload: ExecuteRequest,
    ) -> ExtensionFuture<'a, ProcessStartedResponse> {
        Box::pin(async move {
            let request = RequestFrame::new(0, ownership, RequestPayload::Execute(payload.clone()));
            let dispatch = NativeSidecar::execute(self, &request, payload).await?;
            match dispatch.response.payload {
                ResponsePayload::ProcessStarted(response) => Ok(response),
                other => Err(unexpected_extension_host_response("execute", other)),
            }
        })
    }

    fn write_stdin<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        payload: WriteStdinRequest,
    ) -> ExtensionFuture<'a, StdinWrittenResponse> {
        Box::pin(async move {
            let request =
                RequestFrame::new(0, ownership, RequestPayload::WriteStdin(payload.clone()));
            let dispatch = NativeSidecar::write_stdin(self, &request, payload).await?;
            match dispatch.response.payload {
                ResponsePayload::StdinWritten(response) => Ok(response),
                other => Err(unexpected_extension_host_response("write_stdin", other)),
            }
        })
    }

    fn close_stdin<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        payload: CloseStdinRequest,
    ) -> ExtensionFuture<'a, StdinClosedResponse> {
        Box::pin(async move {
            let request =
                RequestFrame::new(0, ownership, RequestPayload::CloseStdin(payload.clone()));
            let dispatch = NativeSidecar::close_stdin(self, &request, payload).await?;
            match dispatch.response.payload {
                ResponsePayload::StdinClosed(response) => Ok(response),
                other => Err(unexpected_extension_host_response("close_stdin", other)),
            }
        })
    }

    fn kill_process<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        payload: KillProcessRequest,
    ) -> ExtensionFuture<'a, ProcessKilledResponse> {
        Box::pin(async move {
            let request =
                RequestFrame::new(0, ownership, RequestPayload::KillProcess(payload.clone()));
            let dispatch = NativeSidecar::kill_process(self, &request, payload).await?;
            match dispatch.response.payload {
                ResponsePayload::ProcessKilled(response) => Ok(response),
                other => Err(unexpected_extension_host_response("kill_process", other)),
            }
        })
    }

    fn resize_pty<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        payload: ResizePtyRequest,
    ) -> ExtensionFuture<'a, PtyResizedResponse> {
        Box::pin(async move {
            let request =
                RequestFrame::new(0, ownership, RequestPayload::ResizePty(payload.clone()));
            let dispatch = NativeSidecar::resize_pty(self, &request, payload).await?;
            match dispatch.response.payload {
                ResponsePayload::PtyResized(response) => Ok(response),
                other => Err(unexpected_extension_host_response("resize_pty", other)),
            }
        })
    }

    fn poll_event<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        timeout: Duration,
    ) -> ExtensionFuture<'a, Option<EventFrame>> {
        Box::pin(async move { NativeSidecar::poll_event(self, &ownership, timeout).await })
    }

    fn projected_agents<'a>(
        &'a mut self,
        ownership: OwnershipScope,
    ) -> ExtensionFuture<'a, Vec<crate::extension::ProjectedAgentLaunchEntry>> {
        Box::pin(async move {
            let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
            self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
            let vm = self
                .vms
                .get(&vm_id)
                .ok_or_else(|| SidecarError::InvalidState(format!("unknown VM {vm_id}")))?;
            Ok(vm
                .projected_agent_launch
                .iter()
                .map(
                    |(id, launch): (&String, &crate::state::ProjectedAgentLaunch)| {
                        crate::extension::ProjectedAgentLaunchEntry {
                            id: id.clone(),
                            acp_entrypoint: launch.acp_entrypoint.clone(),
                            env: launch.env.clone(),
                            launch_args: launch.launch_args.clone(),
                        }
                    },
                )
                .collect())
        })
    }

    fn agent_additional_instructions<'a>(
        &'a mut self,
        ownership: OwnershipScope,
    ) -> ExtensionFuture<'a, Option<String>> {
        Box::pin(async move {
            let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
            self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
            Ok(self
                .vms
                .get(&vm_id)
                .and_then(|vm| vm.agent_additional_instructions.clone()))
        })
    }

    fn guest_filesystem_call<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        payload: GuestFilesystemCallRequest,
    ) -> ExtensionFuture<'a, GuestFilesystemResultResponse> {
        Box::pin(async move {
            let request = RequestFrame::new(
                0,
                ownership,
                RequestPayload::GuestFilesystemCall(payload.clone()),
            );
            let dispatch = NativeSidecar::guest_filesystem_call(self, &request, payload).await?;
            match dispatch.response.payload {
                ResponsePayload::GuestFilesystemResult(response) => Ok(response),
                other => Err(unexpected_extension_host_response(
                    "guest_filesystem_call",
                    other,
                )),
            }
        })
    }

    fn bind_process_to_session<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        namespace: String,
        ext_session_id: String,
        process_id: String,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            self.bind_extension_process_resource(ownership, namespace, ext_session_id, process_id)
        })
    }

    fn bind_vm_to_session<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        namespace: String,
        ext_session_id: String,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(
            async move { self.bind_extension_vm_resource(ownership, namespace, ext_session_id) },
        )
    }

    fn dispose_session_resources<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        namespace: String,
        ext_session_id: String,
    ) -> ExtensionFuture<'a, Vec<EventFrame>> {
        Box::pin(async move {
            let key = (namespace, ext_session_id);
            let Some(resources) = self.extension_sessions.get(&key) else {
                return Ok(Vec::new());
            };
            if resources.ownership != ownership {
                return Err(SidecarError::InvalidState(String::from(
                    "extension session ownership did not match dispose request",
                )));
            }
            let resources = self
                .extension_sessions
                .remove(&key)
                .expect("extension resources existed before removal");
            let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
            for process_id in resources.process_ids {
                if self
                    .vms
                    .get(&vm_id)
                    .is_some_and(|vm| vm.active_processes.contains_key(&process_id))
                {
                    self.kill_process_internal(&vm_id, &process_id, "SIGTERM")?;
                }
            }
            let mut events = Vec::new();
            for resource_vm_id in resources.vm_ids {
                if self.vms.contains_key(&resource_vm_id) {
                    events.extend(
                        self.dispose_vm_internal(
                            &connection_id,
                            &session_id,
                            &resource_vm_id,
                            DisposeReason::Requested,
                        )
                        .await?,
                    );
                }
            }
            Ok(events)
        })
    }

    fn start_buffering_process_output<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        process_id: String,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
            self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
            let key = (vm_id, process_id);
            if self.extension_process_output_buffers.contains_key(&key) {
                return Err(SidecarError::Conflict(String::from(
                    "extension process output buffering already started",
                )));
            }
            self.extension_process_output_buffers
                .insert(key, ExtensionBufferedProcessOutput::default());
            Ok(())
        })
    }

    fn handoff_buffered_process_output<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        namespace: String,
        ext_session_id: String,
        process_id: String,
        timeout: Duration,
    ) -> ExtensionFuture<'a, ExtensionBufferedProcessOutput> {
        Box::pin(async move {
            let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
            self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
            let key = (vm_id.clone(), process_id.clone());
            let deadline = Instant::now() + timeout;
            loop {
                self.pump_process_events(&ownership).await?;
                while let Some(envelope) =
                    self.take_matching_process_event_envelope(&vm_id, &process_id)?
                {
                    if self.capture_extension_process_output_event(
                        &vm_id,
                        &process_id,
                        &envelope.event,
                    ) {
                        continue;
                    }
                    self.queue_pending_process_event(envelope)?;
                    break;
                }
                let buffered = self
                    .extension_process_output_buffers
                    .get(&key)
                    .is_some_and(|buffer| !buffer.stdout.is_empty() || !buffer.stderr.is_empty());
                if buffered || timeout.is_zero() || Instant::now() >= deadline {
                    break;
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                time::sleep(remaining.min(Duration::from_millis(10))).await;
            }
            self.bind_extension_process_resource(
                ownership,
                namespace,
                ext_session_id,
                process_id.clone(),
            )?;
            self.extension_process_output_buffers
                .remove(&key)
                .ok_or_else(|| {
                    SidecarError::InvalidState(String::from(
                        "extension process output buffering was not started",
                    ))
                })
        })
    }

    fn drain_buffered_process_output<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        process_id: String,
        timeout: Duration,
    ) -> ExtensionFuture<'a, ExtensionBufferedProcessOutput> {
        Box::pin(async move {
            let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
            self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
            let key = (vm_id.clone(), process_id.clone());
            if !self.extension_process_output_buffers.contains_key(&key) {
                return Err(SidecarError::InvalidState(String::from(
                    "extension process output buffering was not started",
                )));
            }
            let deadline = Instant::now() + timeout;
            loop {
                self.pump_process_events(&ownership).await?;
                while let Some(envelope) =
                    self.take_matching_process_event_envelope(&vm_id, &process_id)?
                {
                    if self.capture_extension_process_output_event(
                        &vm_id,
                        &process_id,
                        &envelope.event,
                    ) {
                        continue;
                    }
                    self.queue_pending_process_event(envelope)?;
                    break;
                }
                let ready = self
                    .extension_process_output_buffers
                    .get(&key)
                    .is_some_and(|buffer| {
                        !buffer.stdout.is_empty()
                            || !buffer.stderr.is_empty()
                            || buffer.exit_code.is_some()
                    });
                if ready || timeout.is_zero() || Instant::now() >= deadline {
                    break;
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                time::sleep(remaining.min(Duration::from_millis(10))).await;
            }
            let buffer = self
                .extension_process_output_buffers
                .get_mut(&key)
                .ok_or_else(|| {
                    SidecarError::InvalidState(String::from(
                        "extension process output buffering was not started",
                    ))
                })?;
            Ok(std::mem::take(buffer))
        })
    }

    fn stop_buffering_process_output<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        process_id: String,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
            self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
            self.extension_process_output_buffers
                .remove(&(vm_id, process_id));
            Ok(())
        })
    }
}

fn unexpected_extension_host_response(operation: &str, payload: ResponsePayload) -> SidecarError {
    match payload {
        ResponsePayload::Rejected(response) => SidecarError::InvalidState(format!(
            "extension {operation} rejected with {}: {}",
            response.code, response.message
        )),
        other => SidecarError::InvalidState(format!(
            "extension {operation} returned unexpected response: {other:?}"
        )),
    }
}

fn shadow_host_path_for_process(
    shadow_root: &Path,
    process_guest_cwd: &str,
    guest_path: &str,
) -> PathBuf {
    let normalized_guest_path = if guest_path.starts_with('/') {
        normalize_path(guest_path)
    } else {
        normalize_path(&format!(
            "{}/{}",
            process_guest_cwd.trim_end_matches('/'),
            guest_path
        ))
    };
    if normalized_guest_path == "/" {
        shadow_root.to_path_buf()
    } else {
        shadow_root.join(normalized_guest_path.trim_start_matches('/'))
    }
}

fn sidecar_response_tracker_error(error: SidecarResponseTrackerError) -> SidecarError {
    SidecarError::InvalidState(format!(
        "invalid sidecar response correlation state: {error}"
    ))
}

fn map_bridge_permission(decision: agentos_bridge::PermissionDecision) -> PermissionDecision {
    match decision.verdict {
        agentos_bridge::PermissionVerdict::Allow => PermissionDecision::allow(),
        agentos_bridge::PermissionVerdict::Deny => PermissionDecision::deny(
            decision
                .reason
                .unwrap_or_else(|| String::from("denied by host")),
        ),
        agentos_bridge::PermissionVerdict::Prompt => PermissionDecision::deny(
            decision
                .reason
                .unwrap_or_else(|| String::from("permission prompt required")),
        ),
    }
}

fn audit_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
        .to_string()
}

pub(crate) fn audit_fields<I, K, V>(fields: I) -> BTreeMap<String, String>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let mut mapped = BTreeMap::from([(String::from("timestamp"), audit_timestamp())]);
    for (key, value) in fields {
        mapped.insert(key.into(), value.into());
    }
    mapped
}

pub(crate) fn emit_structured_event<B>(
    bridge: &SharedBridge<B>,
    vm_id: &str,
    name: &str,
    fields: BTreeMap<String, String>,
) -> Result<(), SidecarError>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    bridge.with_mut(|bridge| {
        bridge.emit_structured_event(StructuredEventRecord {
            vm_id: vm_id.to_owned(),
            name: name.to_owned(),
            fields,
        })
    })
}

pub(crate) fn emit_security_audit_event<B>(
    bridge: &SharedBridge<B>,
    vm_id: &str,
    name: &str,
    fields: BTreeMap<String, String>,
) where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let _ = emit_structured_event(bridge, vm_id, name, fields);
}

/// Build a wire `EventFrame` carrying a `StructuredEvent` (name + string-map
/// detail) scoped to a connection. Used to forward limit-registry warnings to the
/// host as `{type:"structured", name:"limit_warning", detail}` events without a
/// protocol schema change. Emitted directly to the host (not via the polled,
/// per-session bridge queue, which is a no-op in the stdio sidecar), so a
/// process-global signal is delivered against the active connection.
pub(crate) fn structured_event_frame(
    connection_id: &str,
    name: &str,
    detail: std::collections::HashMap<String, String>,
) -> Result<crate::wire::EventFrame, SidecarError> {
    let event = EventFrame::new(
        OwnershipScope::connection(connection_id),
        EventPayload::Structured(crate::protocol::StructuredEvent {
            name: name.to_owned(),
            detail,
        }),
    );
    crate::wire::event_frame_from_compat(event).map_err(|error| {
        SidecarError::InvalidState(format!("invalid structured event frame: {error}"))
    })
}

pub(crate) fn log_stale_process_event<B>(
    bridge: &SharedBridge<B>,
    vm_id: &str,
    process_id: &str,
    context: &str,
) where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let _ = bridge.emit_log(
        vm_id,
        format!(
            "Ignoring stale process event during {context}: VM {vm_id} process {process_id} was already reaped"
        ),
    );
}

// filesystem_operation_label moved to crate::vm

pub(crate) fn root_filesystem_error(error: impl std::fmt::Display) -> SidecarError {
    SidecarError::InvalidState(format!("root filesystem: {error}"))
}

pub(crate) fn normalize_path(path: &str) -> String {
    let mut segments = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::RootDir => segments.clear(),
            Component::ParentDir => {
                segments.pop();
            }
            Component::CurDir => {}
            Component::Normal(value) => segments.push(value.to_string_lossy().into_owned()),
            Component::Prefix(prefix) => {
                segments.push(prefix.as_os_str().to_string_lossy().into_owned());
            }
        }
    }

    let normalized = format!("/{}", segments.join("/"));
    if normalized.is_empty() {
        String::from("/")
    } else {
        normalized
    }
}

pub(crate) fn normalize_host_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized != Path::new("/") {
                    normalized.pop();
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    if normalized.as_os_str().is_empty() {
        if path.is_absolute() {
            PathBuf::from("/")
        } else {
            PathBuf::from(".")
        }
    } else {
        normalized
    }
}

pub(crate) fn path_is_within_root(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

pub(crate) fn dirname(path: &str) -> String {
    let normalized = normalize_path(path);
    let parent = Path::new(&normalized)
        .parent()
        .unwrap_or_else(|| Path::new("/"));
    let value = parent.to_string_lossy();
    if value.is_empty() {
        String::from("/")
    } else {
        value.into_owned()
    }
}

pub(crate) fn kernel_error(error: KernelError) -> SidecarError {
    SidecarError::Kernel(error.to_string())
}

pub(crate) fn plugin_error(error: PluginError) -> SidecarError {
    SidecarError::Plugin(error.to_string())
}

pub(crate) fn javascript_error(error: JavascriptExecutionError) -> SidecarError {
    SidecarError::Execution(error.to_string())
}

pub(crate) fn wasm_error(error: WasmExecutionError) -> SidecarError {
    SidecarError::Execution(error.to_string())
}

pub(crate) fn python_error(error: PythonExecutionError) -> SidecarError {
    SidecarError::Execution(error.to_string())
}

pub(crate) fn vfs_error(error: VfsError) -> SidecarError {
    SidecarError::Kernel(error.to_string())
}

/// Actionable guidance shown when guest package resolution fails because the packages live in a
/// non-flat `node_modules` whose package store is not visible in the VM. Mounting host `node_modules`
/// is a bind mount, so symlinked/store layouts
/// do not resolve inside the VM: Node canonicalizes a module to its store
/// realpath (e.g. `node_modules/.pnpm/...`, `.bun/...`, `.store/...`) which lives
/// above the mounted directory and the guest `fs` cannot read. Plug'n'Play
/// (yarn-berry default) has no `node_modules` at all. A flat (hoisted) layout is
/// required. The empirically-supported package managers are captured in
/// `crates/sidecar/tests/module_layout_e2e.rs`.
#[allow(dead_code)]
const HOISTED_NODE_MODULES_GUIDANCE: &str = "secure-exec can't load mounted node_modules: the directory uses a non-flat layout (pnpm / bun / yarn workspaces store, or yarn Plug'n'Play) whose package store isn't visible inside the VM. A flat (hoisted) node_modules is required.\n  - pnpm        -> add `node-linker=hoisted` to .npmrc, then reinstall\n  - yarn berry  -> set `nodeLinker: node-modules` in .yarnrc.yml (not pnp/pnpm)\n  - bun         -> install dependencies outside a workspace (workspaces use a .bun store)\n  - npm / yarn classic -> already flat, no change needed";

/// Detect, from an adapter's captured stderr, a non-flat-`node_modules` failure
/// signature. Returns the actionable guidance to fold into the surfaced error,
/// or `None` when the failure is unrelated.
///
/// Two signatures, both kept specific so they never fire on unrelated crashes:
/// - a missing-file / cannot-resolve error referencing a package STORE path that
///   lives above the mounted project (`.pnpm`, `.bun`, `.store`, PnP `__virtual__`),
/// - a yarn Plug'n'Play fingerprint (`.pnp.cjs`, the zip cache, or PnP's
///   "isn't declared in your dependencies" resolver error).
#[allow(dead_code)]
fn symlinked_node_modules_hint(stderr: &str) -> Option<&'static str> {
    // Package stores that only appear in a path when a non-flat layout is used.
    // pnpm (isolated), bun (workspace), yarn-berry (nodeLinker: pnpm), and PnP
    // virtual instances all keep real package files under these store dirs, which
    // sit above the mounted project node_modules and so are not guest-visible.
    const STORE_MARKERS: &[&str] = &[
        "node_modules/.pnpm/",
        "node_modules/.bun/",
        "node_modules/.store/",
        "/__virtual__/",
    ];
    // Yarn Plug'n'Play has no node_modules at all; resolution fails against the
    // .pnp runtime / zip cache. "isn't declared in your dependencies" is PnP's
    // distinctive resolver error and is specific enough to fire on its own.
    const PNP_STRICT_MARKERS: &[&str] = &["isn't declared in your dependencies"];
    const PNP_PATH_MARKERS: &[&str] = &[".pnp.cjs", ".pnp.loader.mjs", "/.yarn/cache/"];

    if PNP_STRICT_MARKERS.iter().any(|m| stderr.contains(m)) {
        return Some(HOISTED_NODE_MODULES_GUIDANCE);
    }

    let missing = stderr.contains("ENOENT")
        || stderr.contains("no such file or directory")
        || stderr.contains("Cannot find module")
        || stderr.contains("MODULE_NOT_FOUND");
    if !missing {
        return None;
    }
    if STORE_MARKERS.iter().any(|m| stderr.contains(m))
        || PNP_PATH_MARKERS.iter().any(|m| stderr.contains(m))
    {
        return Some(HOISTED_NODE_MODULES_GUIDANCE);
    }
    None
}

#[cfg(test)]
mod symlinked_node_modules_hint_tests {
    use super::symlinked_node_modules_hint;

    // Positive cases: each non-flat package manager's store/PnP signature.
    #[test]
    fn matches_pnpm_store_enoent() {
        // Real pi-coding-agent failure: getPackageDir() falls back to a
        // dist/package.json inside the unreachable .pnpm store.
        let stderr = "Error: ENOENT: no such file or directory, open '/root/node_modules/.pnpm/@mariozechner+pi-coding-agent@0.60.0_x/node_modules/@mariozechner/pi-coding-agent/dist/package.json'";
        let hint = symlinked_node_modules_hint(stderr).expect("expected hoisted guidance");
        assert!(hint.contains("secure-exec can't load mounted node_modules"));
        assert!(!hint.contains("agentos"));
    }

    #[test]
    fn matches_bun_store_enoent() {
        let stderr = "Error: ENOENT: no such file or directory, open '/root/node_modules/.bun/is-odd@3.0.1/node_modules/is-odd/package.json'";
        assert!(symlinked_node_modules_hint(stderr).is_some());
    }

    #[test]
    fn matches_yarn_pnpm_store_enoent() {
        let stderr = "Error: ENOENT: no such file or directory, open '/root/node_modules/.store/is-odd-npm-3.0.1-93c3c3f41b/package/package.json'";
        assert!(symlinked_node_modules_hint(stderr).is_some());
    }

    #[test]
    fn matches_pnp_declared_error() {
        // Yarn PnP's distinctive resolver error (no node_modules at all).
        let stderr = "Error: Your application tried to access is-number, but it isn't declared in your dependencies; this makes the require call ambiguous and unsound.";
        assert!(symlinked_node_modules_hint(stderr).is_some());
    }

    #[test]
    fn matches_pnp_cjs_module_not_found() {
        let stderr = "Error: Cannot find module 'is-odd'\n    at /root/.pnp.cjs:12345:18\n    code: 'MODULE_NOT_FOUND'";
        assert!(symlinked_node_modules_hint(stderr).is_some());
    }

    #[test]
    fn matches_virtual_instance() {
        let stderr = "Error: ENOENT: no such file or directory, open '/root/.yarn/__virtual__/is-odd-abc/1/node_modules/is-odd/package.json'";
        assert!(symlinked_node_modules_hint(stderr).is_some());
    }

    // Negative cases: must not fire.
    #[test]
    fn ignores_enoent_outside_a_store() {
        let stderr = "Error: ENOENT: no such file or directory, open '/tmp/scratch/config.json'";
        assert!(symlinked_node_modules_hint(stderr).is_none());
    }

    #[test]
    fn ignores_store_path_without_missing_file() {
        let stderr =
            "loaded /root/node_modules/.pnpm/some-pkg@1.0.0/node_modules/some-pkg/index.js";
        assert!(symlinked_node_modules_hint(stderr).is_none());
    }

    #[test]
    fn ignores_flat_node_modules_enoent() {
        // npm / yarn-nm / pnpm-hoisted: flat, no store dir in the path.
        let stderr = "Error: ENOENT: no such file or directory, open '/root/node_modules/is-odd/missing-asset.json'";
        assert!(symlinked_node_modules_hint(stderr).is_none());
    }

    #[test]
    fn ignores_unrelated_failure() {
        let stderr = "Error: connect ECONNREFUSED 127.0.0.1:443";
        assert!(symlinked_node_modules_hint(stderr).is_none());
    }
}

#[cfg(test)]
mod structured_event_frame_tests {
    use super::*;

    #[test]
    fn structured_event_frame_round_trips_limit_warning() {
        let mut detail = std::collections::HashMap::new();
        // Pin a real emitted limit name rather than a fictional string.
        let limit_name = TrackedLimit::JavascriptEventChannel.as_str();
        detail.insert(String::from("limit"), String::from(limit_name));
        detail.insert(String::from("fillPercent"), String::from("82"));

        let wire = structured_event_frame("conn-1", "limit_warning", detail)
            .expect("build structured event frame");
        let compat = crate::wire::event_frame_to_compat(wire).expect("convert to compat");

        match compat.payload {
            EventPayload::Structured(event) => {
                assert_eq!(event.name, "limit_warning");
                assert_eq!(
                    event.detail.get("limit").map(String::as_str),
                    Some(limit_name)
                );
                assert_eq!(
                    event.detail.get("fillPercent").map(String::as_str),
                    Some("82")
                );
            }
            other => panic!("expected structured payload, got {other:?}"),
        }
        match compat.ownership {
            OwnershipScope::ConnectionOwnership(inner) => {
                assert_eq!(inner.connection_id, "conn-1");
            }
            other => panic!("expected connection ownership, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod dispose_lifecycle_tests {
    use super::*;
    use crate::extension::ExtensionResponse;
    use crate::stdio::LocalBridge;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("dispose lifecycle test runtime")
            .block_on(future)
    }

    fn test_sidecar() -> NativeSidecar<LocalBridge> {
        NativeSidecar::new(LocalBridge::default()).expect("build test sidecar")
    }

    // Register a connection + session directly so the dispose paths can be
    // exercised without spinning up a V8-backed VM.
    fn insert_session(
        sidecar: &mut NativeSidecar<LocalBridge>,
        connection_id: &str,
        session_id: &str,
        vm_ids: BTreeSet<String>,
    ) {
        sidecar.connections.insert(
            connection_id.to_string(),
            ConnectionState {
                auth_token: String::new(),
                sessions: BTreeSet::from([session_id.to_string()]),
            },
        );
        sidecar.sessions.insert(
            session_id.to_string(),
            SessionState {
                connection_id: connection_id.to_string(),
                placement: crate::protocol::SidecarPlacement::SidecarPlacementShared(
                    crate::protocol::SidecarPlacementShared { pool: None },
                ),
                vm_ids,
            },
        );
    }

    struct RecordingExtension {
        namespace: String,
        session_disposed: Arc<AtomicUsize>,
    }

    impl Extension for RecordingExtension {
        fn namespace(&self) -> &str {
            &self.namespace
        }

        fn handle_request<'a>(
            &'a self,
            _ctx: ExtensionContext<'a>,
            _payload: Vec<u8>,
        ) -> ExtensionFuture<'a, ExtensionResponse> {
            Box::pin(async { Ok(ExtensionResponse::new(Vec::new())) })
        }

        fn on_session_disposed<'a>(&'a self, _ctx: ExtensionSnapshot) -> ExtensionFuture<'a, ()> {
            let counter = self.session_disposed.clone();
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    fn register_recording_extension(sidecar: &mut NativeSidecar<LocalBridge>) -> Arc<AtomicUsize> {
        let counter = Arc::new(AtomicUsize::new(0));
        sidecar
            .register_extension(Box::new(RecordingExtension {
                namespace: String::from("dev.test.dispose"),
                session_disposed: counter.clone(),
            }))
            .expect("register recording extension");
        counter
    }

    // H4: the extension per-session teardown hook fires on ConnectionClosed so an
    // ACP-style extension can release per-session state on client disconnect.
    #[test]
    fn connection_closed_dispose_invokes_extension_session_teardown() {
        let mut sidecar = test_sidecar();
        let counter = register_recording_extension(&mut sidecar);
        insert_session(&mut sidecar, "conn-1", "session-1", BTreeSet::new());

        block_on(sidecar.dispose_session("conn-1", "session-1", DisposeReason::ConnectionClosed))
            .expect("dispose session on connection close");

        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "extension session-teardown hook must fire on ConnectionClosed"
        );
        assert!(
            !sidecar.sessions.contains_key("session-1"),
            "the disposed session must be reclaimed"
        );
    }

    // H4 (negative): a client-requested dispose is not a disconnect, so the
    // teardown hook must not fire.
    #[test]
    fn requested_dispose_does_not_invoke_extension_session_teardown() {
        let mut sidecar = test_sidecar();
        let counter = register_recording_extension(&mut sidecar);
        insert_session(&mut sidecar, "conn-1", "session-1", BTreeSet::new());

        block_on(sidecar.dispose_session("conn-1", "session-1", DisposeReason::Requested))
            .expect("dispose session on request");

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "the teardown hook is reserved for client disconnect"
        );
    }

    // M5: disposing a session records its scope for the stdio transport to drain.
    #[test]
    fn dispose_session_records_disposed_scope() {
        let mut sidecar = test_sidecar();
        insert_session(&mut sidecar, "conn-1", "session-1", BTreeSet::new());

        block_on(sidecar.dispose_session("conn-1", "session-1", DisposeReason::Requested))
            .expect("dispose session");

        assert_eq!(
            sidecar.take_disposed_sessions(),
            vec![(String::from("conn-1"), String::from("session-1"))],
            "dispose must publish the session scope so stdio can untrack it"
        );
    }

    // H1 + M6: every per-VM tracking map is reclaimed for a disposed VM. The
    // output-buffer map (M6) was previously only removed on a successful handoff,
    // and the engine/extension maps (H1) were only reclaimed after the fallible
    // teardown steps' `?`, so any failure stranded them.
    #[test]
    fn reclaim_vm_tracking_clears_every_per_vm_map() {
        let mut sidecar = test_sidecar();
        insert_session(
            &mut sidecar,
            "conn-1",
            "session-1",
            BTreeSet::from([String::from("vm-1")]),
        );
        sidecar.extension_process_output_buffers.insert(
            (String::from("vm-1"), String::from("proc-1")),
            ExtensionBufferedProcessOutput::default(),
        );
        sidecar.extension_sessions.insert(
            (String::from("ns"), String::from("ext-sess-1")),
            ExtensionSessionResources {
                ownership: OwnershipScope::vm("conn-1", "session-1", "vm-1"),
                process_ids: BTreeSet::new(),
                vm_ids: BTreeSet::from([String::from("vm-1")]),
            },
        );

        sidecar.reclaim_vm_tracking("session-1", "vm-1");

        assert!(
            sidecar.extension_process_output_buffers.is_empty(),
            "M6: the output-buffer map must be reclaimed on VM disposal"
        );
        assert!(
            sidecar.extension_sessions.is_empty(),
            "H1: an extension session bound only to the VM must be reclaimed"
        );
        assert!(
            !sidecar
                .sessions
                .get("session-1")
                .expect("session present")
                .vm_ids
                .contains("vm-1"),
            "the VM id must be removed from its session"
        );
    }

    // H1: a failing VM dispose inside the loop must not abandon the session. With
    // unregistered VM ids, `dispose_vm_internal` fails on `require_owned_vm`;
    // pre-fix the loop `?`-ed out and left the session in `self.sessions`.
    #[test]
    fn dispose_session_reclaims_session_even_when_a_vm_dispose_fails() {
        let mut sidecar = test_sidecar();
        insert_session(
            &mut sidecar,
            "conn-1",
            "session-1",
            BTreeSet::from([String::from("vm-a"), String::from("vm-b")]),
        );

        let result =
            block_on(sidecar.dispose_session("conn-1", "session-1", DisposeReason::Requested));

        assert!(
            result.is_err(),
            "a failing VM dispose must still surface an error"
        );
        assert!(
            !sidecar.sessions.contains_key("session-1"),
            "the session must be reclaimed even though VM dispose failed"
        );
    }
}
