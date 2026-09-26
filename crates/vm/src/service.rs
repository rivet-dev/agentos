use crate::bridge::{build_mount_plugin_registry, MountPluginContext};
use crate::core::permissions::{
    deny_all_policy, environment_permission_capability,
    evaluate_matching_pattern_permission_policy, evaluate_permissions_policy,
    filesystem_permission_capability, network_permission_capability,
    permission_mode_to_kernel_decision,
};
use crate::core::{
    authenticated_response as shared_authenticated_response, parse_process_signal_state_request,
    reject as shared_reject, respond as shared_respond, route_request_payload,
    session_opened_response, unsupported_host_callback_direction_dispatch,
    validate_authenticate_versions, vm_lifecycle_event as shared_vm_lifecycle_event,
    AuthenticateVersionError, RequestRoute,
};
pub(crate) use crate::execution::{
    apply_kernel_signal_registration, build_socket_path_context,
    deferred_kernel_wait_request_for_process, dispatch_loopback_http_request_deferred, error_code,
    flush_pending_kernel_stdin, format_tcp_resource, host_bytes_value, host_service_error_code,
    javascript_sync_rpc_arg_i32, javascript_sync_rpc_arg_str, javascript_sync_rpc_arg_u32,
    javascript_sync_rpc_arg_u32_optional, javascript_sync_rpc_arg_u64,
    javascript_sync_rpc_arg_u64_optional, javascript_sync_rpc_bytes_arg,
    javascript_sync_rpc_encoding, javascript_sync_rpc_may_make_fd_readable,
    javascript_sync_rpc_may_make_fd_writable, javascript_sync_rpc_option_bool,
    javascript_sync_rpc_option_u32, kernel_poll_response, kernel_stdin_read_response,
    mark_execute_exit_event_queued, parse_kernel_poll_args, parse_kernel_stdin_read_args,
    parse_signal, record_execute_exit_event_queue_wait, record_execute_phase,
    sanitize_javascript_child_process_internal_bootstrap_env,
    service_javascript_kernel_fd_write_sync_rpc, service_javascript_sync_rpc,
    settle_execution_host_call, HickoryDnsResolver, JavascriptSyncRpcServiceRequest,
    LoopbackHttpDispatchRequest,
};
use crate::executor::backend::ExecutionBackendKind;
use crate::executor::host::{ProcessLaunchOptions, ProcessLaunchRequest};
use crate::executor::record_sync_bridge_request_observed;
#[cfg(feature = "node-v8")]
use crate::executor::{JavascriptExecutionEngine, JavascriptExecutionError};
#[cfg(feature = "python-v8-pyodide")]
use crate::executor::{PythonExecutionEngine, PythonExecutionError};
use crate::executor::{WasmExecutionEngine, WasmExecutionError};
use crate::extension::{
    Extension, ExtensionBufferedProcessOutput, ExtensionContext, ExtensionFuture, ExtensionHost,
    ExtensionServices, ExtensionSnapshot,
};
use crate::filesystem::{
    guest_filesystem_call as filesystem_guest_filesystem_call, guest_filesystem_call_vm,
};
use crate::host_functions::register_host_callbacks;
use crate::limits::DEFAULT_EXTENSION_OUTPUT_BUFFER_BYTE_LIMIT;
use crate::process_event_broker::{
    ProcessEventBroker, ProcessEventBrokerError, ProcessEventIngress, ProcessEventTarget,
};
use crate::protocol::{
    CloseStdinRequest, DisposeReason, EventFrame, EventPayload, ExecuteRequest, ExtEnvelope,
    GuestFilesystemCallRequest, GuestFilesystemResultResponse, KillProcessRequest,
    OpenSessionRequest, OwnershipScope, ProcessKilledResponse, ProcessStartedResponse,
    RejectedResponse, RequestFrame, RequestId, RequestPayload, ResponseFrame, ResponsePayload,
    SidecarRequestFrame, SidecarRequestPayload, SidecarResponseFrame, SidecarResponsePayload,
    SidecarResponseTracker, SidecarResponseTrackerError, StdinClosedResponse, StdinWrittenResponse,
    VmLifecycleState, WriteStdinRequest,
};
use crate::request_operations::OperationCancellationReason;
use crate::state::{
    ActiveExecutionEvent, BridgeError, ConnectionState, EventSinkTransport, ExecutionHostCall,
    ProcessEventEnvelope, QuarantinedVmGeneration, SessionState, SharedBridge, SharedEventSink,
    SharedSidecarRequestClient, SidecarRequestTransport, SocketFamily, SocketPathContext, VmState,
    EXECUTION_DRIVER_NAME,
};
use crate::ExecutorRegistry;
use crate::VmManagerHost;
use agentos_driver_tokio::metrics::ResourceMetricClass;
use agentos_resource_accounting::queue_tracker::{register_queue, QueueGauge, TrackedLimit};
use agentos_vm_config::{FsPermissionScope, PermissionMode, PermissionsPolicy};
use agentos_vm_host_interface::{
    CommandPermissionRequest, EnvironmentAccess, EnvironmentPermissionRequest, FilesystemAccess,
    FilesystemPermissionRequest, LifecycleEventRecord, LifecycleState, LogLevel, LogRecord,
    NetworkAccess, NetworkPermissionRequest, StructuredEventRecord,
};
use agentos_vm_kernel::kernel::KernelError;
use agentos_vm_kernel::mount_plugin::{FileSystemPluginRegistry, PluginError};
use agentos_vm_kernel::permissions::{
    CommandAccessRequest, EnvAccessRequest, EnvironmentOperation, NetworkAccessRequest,
    NetworkOperation, PermissionDecision,
};
// root_fs types moved to crate::vm
use agentos_vm_kernel::vfs::VfsError;
use serde::Deserialize;
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::time;

// Constants and type aliases moved to crate::state

use crate::state::VmRegistry;
use agentos_client::{
    ClientError, PackageResolver, PackageResolverOptions, PackageSource, VerifiedPackage,
};
use std::cell::RefCell;
use std::future::Future;

const INTERNAL_JAVASCRIPT_ENTRYPOINT_ENV_KEYS: &[&str] =
    &["AGENTOS_ENTRYPOINT", "AGENTOS_BOOTSTRAP_MODULE"];
const INTERNAL_WASM_ENTRYPOINT_ENV_KEYS: &[&str] =
    &["AGENTOS_WASM_MODULE_PATH", "AGENTOS_WASM_MODULE_BASE64"];
const INTERNAL_PYTHON_ENTRYPOINT_ENV_PREFIXES: &[&str] = &["AGENTOS_PYTHON_"];
// The integration fixture includes this module as a child and consumes these
// default-limit aliases; the standalone lib-test target does not.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) const MAX_PROCESS_EVENT_QUEUE: usize =
    agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_PROCESS_EVENTS;
#[cfg(test)]
#[allow(dead_code)]
pub(crate) const MAX_PENDING_SIDECAR_RESPONSES: usize =
    agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_PENDING_RESPONSES;
#[cfg(test)]
#[allow(dead_code)]
pub(crate) const MAX_OUTBOUND_SIDECAR_REQUESTS: usize =
    agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_OUTBOUND_REQUESTS;
#[cfg(test)]
#[allow(dead_code)]
pub(crate) const MAX_COMPLETED_SIDECAR_RESPONSES: usize =
    agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_COMPLETED_RESPONSES;
pub(crate) fn process_event_queue_overflow_error(limit: usize) -> VmError {
    VmError::InvalidState(format!(
        "ERR_AGENTOS_PROCESS_EVENT_LIMIT: process event queue exceeded {limit} pending events; raise runtime.protocol.maxProcessEvents"
    ))
}

fn sidecar_response_pending_overflow_error(limit: usize) -> VmError {
    VmError::InvalidState(format!(
        "ERR_AGENTOS_PENDING_RESPONSE_LIMIT: sidecar response tracker exceeded {limit} pending responses; raise runtime.protocol.maxPendingResponses"
    ))
}

fn outbound_sidecar_request_queue_overflow_error(limit: usize) -> VmError {
    VmError::InvalidState(format!(
        "ERR_AGENTOS_OUTBOUND_REQUEST_LIMIT: outbound sidecar request queue exceeded {limit} pending requests; raise runtime.protocol.maxOutboundRequests"
    ))
}

fn wire_protocol_error(error: crate::wire::ProtocolCodecError) -> VmError {
    VmError::InvalidState(format!("invalid generated wire protocol frame: {error}"))
}

pub fn wire_dispatch_result(
    result: DispatchResult,
) -> Result<crate::wire::WireDispatchResult, VmError> {
    crate::wire::dispatch_result_from_compat(crate::wire::CompatDispatchResult {
        response: result.response,
        events: result.events,
    })
    .map_err(wire_protocol_error)
}

pub use crate::core::DispatchResult;
// VmManagerConfig and VmError moved to crate::state
pub use crate::state::{VmError, VmManagerConfig};

fn package_acquisition_rejection(error: &ClientError) -> RejectedResponse {
    if let ClientError::OperationTimedOut { message, details } = error {
        // Retain typed producer/connect/body timeouts, including when they win
        // the race against the outer waiter deadline. Name the wire override
        // at this boundary rather than asking a transport caller to edit Rust options.
        let path = details.configuration_path.as_deref();
        let wire_path = match path {
            Some("PackageResolverOptions.connect_timeout_ms") => {
                Some("AcquirePackageRequest.connectTimeoutMs")
            }
            Some("PackageResolverOptions.download_timeout_ms") => {
                Some("AcquirePackageRequest.downloadTimeoutMs")
            }
            _ => path,
        };
        let message = match (path, wire_path) {
            (Some(from), Some(to)) => message.replace(from, to),
            _ => message.clone(),
        };
        return RejectedResponse {
            code: "timeout".into(),
            message,
            limit_name: details.limit_name.clone(),
            configured_limit: details.configured_limit,
            current_usage: details.current_usage,
            requested: details.requested,
            unit: details.unit.clone(),
            scope: details.scope.clone(),
            vm_id: details.vm_id.clone(),
            session_generation: details.session_generation,
            capability_id: details.capability_id,
            operation: details.operation.clone(),
            configuration_path: wire_path.map(str::to_owned),
            retryable: details.retryable,
            errno: details.errno.clone(),
        };
    }
    let (code, errno) = match error {
        ClientError::InvalidPackageSource(_) => ("invalid_package_source", "EINVAL"),
        ClientError::InvalidPackageFormat(_) => ("invalid_package_format", "EINVAL"),
        ClientError::PackageDigestMismatch { .. } => ("package_digest_mismatch", "EINVAL"),
        ClientError::PackageDownload(_) => ("package_download_failed", "EIO"),
        ClientError::PackageIo(_) => ("package_io_failed", "EIO"),
        ClientError::PackageCacheConfiguration(_) => ("package_cache_configuration", "EINVAL"),
        ClientError::PackageTooLarge { .. }
        | ClientError::PackageCacheCapacity { .. }
        | ClientError::PackageCacheEntryCapacity { .. }
        | ClientError::PackageCachePendingLimit { .. } => ("ERR_AGENTOS_RESOURCE_LIMIT", "ENOSPC"),
        _ => ("package_acquisition_failed", "EIO"),
    };
    let mut rejection = RejectedResponse {
        code: code.into(),
        message: error.to_string(),
        limit_name: None,
        configured_limit: None,
        current_usage: None,
        requested: None,
        unit: None,
        scope: Some("session".into()),
        vm_id: None,
        session_generation: None,
        capability_id: None,
        operation: Some("package.acquire".into()),
        configuration_path: None,
        retryable: Some(false),
        errno: Some(errno.into()),
    };
    let limit = match error {
        ClientError::PackageTooLarge { observed, limit } => Some((
            "packageBytes",
            "AcquirePackageRequest.maxPackageBytes",
            "bytes",
            "session",
            *limit,
            None,
            Some(*observed),
        )),
        ClientError::PackageCacheCapacity {
            requested,
            current,
            limit,
        } => Some((
            "packageCacheBytes",
            "ProcessPackageCacheOptions.max_bytes",
            "bytes",
            "process",
            *limit,
            Some(*current),
            Some(*requested),
        )),
        ClientError::PackageCacheEntryCapacity { current, limit } => Some((
            "packageCacheEntries",
            "ProcessPackageCacheOptions.max_entries",
            "entries",
            "process",
            *limit as u64,
            Some(*current as u64),
            Some(1),
        )),
        ClientError::PackageCachePendingLimit { limit } => Some((
            "packageCachePendingAcquisitions",
            "ProcessPackageCacheOptions.max_pending_acquisitions",
            "acquisitions",
            "process",
            *limit as u64,
            None,
            Some(1),
        )),
        _ => None,
    };
    if let Some((name, path, unit, scope, limit, current, requested)) = limit {
        rejection.limit_name = Some(name.into());
        rejection.configuration_path = Some(path.into());
        rejection.unit = Some(unit.into());
        rejection.scope = Some(scope.into());
        rejection.configured_limit = Some(limit);
        rejection.current_usage = current;
        rejection.requested = requested;
        rejection.message.push_str(&format!("; raise {path}"));
    }
    rejection
}

// Bound the waiter, including joins to existing flights. The resolver separately
// caps each producer. Dropping this future drops its cache waiter; it does not
// assert that already-admitted blocking file validation has synchronously stopped.
// Preserve the full typed wire rejection, including resource-limit metadata.
#[allow(clippy::result_large_err)]
async fn await_package_acquisition<T>(
    timeout_ms: Option<u64>,
    operator_cap_ms: u64,
    operation: impl std::future::Future<Output = Result<T, ClientError>>,
) -> Result<T, RejectedResponse> {
    if timeout_ms == Some(0) {
        return Err(package_acquisition_rejection(
            &ClientError::InvalidPackageSource("timeoutMs must be greater than zero".into()),
        ));
    }
    let deadline_ms = timeout_ms.unwrap_or(operator_cap_ms).min(operator_cap_ms);
    match tokio::time::timeout(Duration::from_millis(deadline_ms), operation).await {
        Ok(result) => result.map_err(|error| package_acquisition_rejection(&error)),
        Err(_) => {
            let path = if deadline_ms < operator_cap_ms {
                "AcquirePackageRequest.timeoutMs"
            } else {
                "ProcessPackageCacheOptions.acquisition_timeout_ms"
            };
            let mut rejection = package_acquisition_rejection(&ClientError::PackageDownload(
                format!("acquisition waiter exceeded {deadline_ms}ms; raise {path}; completion is unconfirmed"),
            ));
            rejection.code = "timeout".into();
            rejection.limit_name = Some("packageAcquisitionWaitMs".into());
            rejection.configured_limit = Some(deadline_ms);
            rejection.configuration_path = Some(path.into());
            rejection.unit = Some("milliseconds".into());
            rejection.errno = Some("ETIMEDOUT".into());
            Err(rejection)
        }
    }
}

async fn acquire_package_owned(
    request: RequestFrame,
    payload: crate::protocol::AcquirePackageRequest,
) -> Result<DispatchResult, VmError> {
    let package = match resolve_package_owned(payload).await {
        Ok(package) => package,
        Err(rejection) => {
            return Ok(DispatchResult {
                response: shared_respond(&request, ResponsePayload::Rejected(rejection)),
                events: Vec::new(),
            })
        }
    };
    Ok(DispatchResult {
        response: shared_respond(
            &request,
            ResponsePayload::PackageAcquired(package_acquired_response(&package)),
        ),
        events: Vec::new(),
    })
}

async fn package_cache_stats_owned(request: RequestFrame) -> Result<DispatchResult, VmError> {
    let stats = match agentos_client::process_package_cache_stats().await {
        Ok(stats) => stats,
        Err(error) => {
            let mut rejection = package_acquisition_rejection(&error);
            rejection.operation = Some("package.cache_stats".into());
            return Ok(DispatchResult {
                response: shared_respond(&request, ResponsePayload::Rejected(rejection)),
                events: Vec::new(),
            });
        }
    };
    let count = |value: usize| u64::try_from(value).unwrap_or(u64::MAX);
    Ok(DispatchResult {
        response: shared_respond(
            &request,
            ResponsePayload::PackageCacheStats(crate::protocol::PackageCacheStatsResponse {
                entries: count(stats.entries),
                source_entries: count(stats.source_entries),
                bytes: stats.bytes,
                pinned_entries: count(stats.pinned_entries),
                pending_acquisitions: count(stats.pending_acquisitions),
                hits: stats.hits,
                misses: stats.misses,
                coalesced_waiters: stats.coalesced_waiters,
                acquisitions: stats.acquisitions,
                evictions: stats.evictions,
                capacity_failures: stats.capacity_failures,
                cancelled_acquisitions: stats.cancelled_acquisitions,
            }),
        ),
        events: Vec::new(),
    })
}

fn package_acquired_response(
    package: &VerifiedPackage,
) -> crate::protocol::PackageAcquiredResponse {
    crate::protocol::PackageAcquiredResponse {
        package_id: package.package_id.clone(),
        digest: package.digest.clone(),
        size: package.size,
        package_name: package.manifest.name.clone(),
        version: package.manifest.version.clone(),
        commands: package.manifest.commands.clone(),
    }
}

// Preserve the full typed wire rejection, including resource-limit metadata.
#[allow(clippy::result_large_err)]
async fn resolve_package_owned(
    payload: crate::protocol::AcquirePackageRequest,
) -> Result<VerifiedPackage, RejectedResponse> {
    let mut options = PackageResolverOptions::default();
    if let Some(value) = payload.max_package_bytes {
        options.max_package_bytes = value;
    }
    if let Some(value) = payload.download_timeout_ms {
        options.download_timeout_ms = value;
    }
    if let Some(value) = payload.connect_timeout_ms {
        options.connect_timeout_ms = value;
    }
    if let Some(value) = payload.max_redirects {
        options.max_redirects = value as usize;
    }
    options.allow_insecure_local_http = payload.allow_insecure_local_http;
    let source = match payload.source {
        crate::protocol::PackageAcquisitionSource::PackageUrlSource(source) => PackageSource::Url {
            url: source.url,
            expected_digest: source.expected_digest,
        },
        crate::protocol::PackageAcquisitionSource::PackagePathSource(source) => {
            PackageSource::Path {
                path: source.path,
                expected_digest: source.expected_digest,
            }
        }
    };
    let result = match PackageResolver::new(options) {
        Ok(resolver) => {
            await_package_acquisition(
                payload.timeout_ms,
                agentos_client::sidecar_internals::package_acquisition_timeout_ms(&resolver),
                async {
                    if payload.advisory {
                        resolver.preload(source).await
                    } else {
                        resolver.resolve(source).await
                    }
                },
            )
            .await
        }
        Err(error) => Err(package_acquisition_rejection(&error)),
    };
    result
}

pub(crate) async fn install_package_owned<B>(
    request: RequestFrame,
    input: Result<crate::vm::LinkPackageOwnedInput<B>, VmError>,
    payload: crate::protocol::InstallPackageRequest,
) -> Result<DispatchResult, VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    // Validate VM ownership before opening a URL or trusted local path.
    let input = input?;
    if payload.acquisition.advisory {
        let rejection = package_acquisition_rejection(&ClientError::InvalidPackageSource(
            "InstallPackageRequest.acquisition.advisory must be false".into(),
        ));
        return Ok(DispatchResult {
            response: shared_respond(&request, ResponsePayload::Rejected(rejection)),
            events: Vec::new(),
        });
    }
    let package = match resolve_package_owned(payload.acquisition).await {
        Ok(package) => package,
        Err(rejection) => {
            return Ok(DispatchResult {
                response: shared_respond(&request, ResponsePayload::Rejected(rejection)),
                events: Vec::new(),
            })
        }
    };
    let metadata = package_acquired_response(&package);
    let path = package
        .path()
        .to_str()
        .ok_or_else(|| VmError::InvalidState("verified package path is not valid UTF-8".into()))?;
    let linked = crate::vm::link_verified_package_owned(
        input,
        crate::protocol::LinkPackageRequest {
            package: crate::protocol::PackageDescriptor { path: path.into() },
            package_id: package.package_id.clone(),
        },
        package,
    )
    .await?;
    let ResponsePayload::PackageLinked(linked_response) = linked.response.payload else {
        return Err(VmError::InvalidState(
            "verified package link returned an unexpected response".into(),
        ));
    };
    Ok(DispatchResult {
        response: shared_respond(
            &request,
            ResponsePayload::PackageInstalled(crate::protocol::PackageInstalledResponse {
                package: metadata,
                projected_commands: linked_response.projected_commands,
            }),
        ),
        events: linked.events,
    })
}

/// An extension request detached from the mutable sidecar coordinator.
///
/// The transport prepares this record while it has short-lived access to the
/// process registry, then executes it as an independently tracked task. Any
/// sidecar operation the extension needs is performed through the cloneable
/// service backend in its context rather than by retaining `&mut VmManager`.
pub struct PreparedExtensionRequest {
    request: RequestFrame,
    namespace: String,
    payload: Vec<u8>,
    extension: Arc<dyn Extension>,
    services: Arc<dyn ExtensionServices>,
    snapshot: ExtensionSnapshot,
}

pub struct CompletedExtensionRequest {
    request: RequestFrame,
    namespace: String,
    result: Result<crate::extension::ExtensionResponse, VmError>,
}

impl CompletedExtensionRequest {
    pub fn failed(&self) -> bool {
        self.result.is_err()
    }
}

impl PreparedExtensionRequest {
    pub async fn execute(self) -> CompletedExtensionRequest {
        let PreparedExtensionRequest {
            request,
            namespace,
            payload,
            extension,
            services,
            snapshot,
        } = self;
        let ctx = ExtensionContext::with_services(snapshot, services);
        let result = {
            let request_future = extension.handle_request(ctx, payload);
            request_future.await
        };
        CompletedExtensionRequest {
            request,
            namespace,
            result,
        }
    }
}

type PreparedRequestFuture =
    Pin<Box<dyn Future<Output = Result<DispatchResult, VmError>> + 'static>>;

/// Bounded coordinator-side effects produced by one detached request.
///
/// The owned operation records effects without retaining the central sidecar.
/// Completion drains them while the coordinator is briefly available, before
/// the terminal response is made visible. `Rc<RefCell<_>>` is intentional: the
/// unified protocol loop executes these non-`Send` futures on one local task.
#[derive(Clone)]
pub(crate) struct RequestCompletionEffects {
    state: Rc<RefCell<RequestCompletionEffectState>>,
}

struct RequestCompletionEffectState {
    exited_process_ids: BTreeMap<String, Vec<String>>,
    exited_process_limit: usize,
    exited_process_limit_path: &'static str,
    warned: bool,
}

impl Default for RequestCompletionEffects {
    fn default() -> Self {
        Self::new(1, "limits.execution.maxCompletedExecutions")
    }
}

impl RequestCompletionEffects {
    fn new(exited_process_limit: usize, exited_process_limit_path: &'static str) -> Self {
        Self {
            state: Rc::new(RefCell::new(RequestCompletionEffectState {
                exited_process_ids: BTreeMap::new(),
                exited_process_limit: exited_process_limit.max(1),
                exited_process_limit_path,
                warned: false,
            })),
        }
    }

    #[cfg(test)]
    pub(crate) fn record_exited_process(&self, process_id: &str) -> Result<(), VmError> {
        self.record_process_exit(process_id, Vec::new())
    }

    pub(crate) fn record_process_exit(
        &self,
        process_id: &str,
        detached_process_ids: Vec<String>,
    ) -> Result<(), VmError> {
        let mut state = self.state.borrow_mut();
        if state.exited_process_ids.contains_key(process_id) {
            return Ok(());
        }
        let observed = state.exited_process_ids.len().saturating_add(1);
        if observed > state.exited_process_limit {
            return Err(VmError::InvalidState(format!(
                "ERR_AGENTOS_REQUEST_COMPLETION_EFFECT_LIMIT: exited-process completion effects observed={observed} limit={} limit_path={}; raise {}",
                state.exited_process_limit,
                state.exited_process_limit_path,
                state.exited_process_limit_path,
            )));
        }
        state
            .exited_process_ids
            .insert(process_id.to_owned(), detached_process_ids);
        let warning_threshold = state
            .exited_process_limit
            .saturating_mul(3)
            .div_ceil(4)
            .max(1);
        if !state.warned && state.exited_process_ids.len() >= warning_threshold {
            state.warned = true;
            tracing::warn!(
                limit = state.exited_process_limit,
                limit_path = state.exited_process_limit_path,
                used = state.exited_process_ids.len(),
                "request completion exited-process effects are near their bound"
            );
        }
        Ok(())
    }

    pub(crate) fn remaining_exited_process_capacity(&self, reserved: usize) -> usize {
        let state = self.state.borrow();
        state
            .exited_process_limit
            .saturating_sub(state.exited_process_ids.len())
            .saturating_sub(reserved)
    }

    fn take_exited_processes(&self) -> BTreeMap<String, Vec<String>> {
        std::mem::take(&mut self.state.borrow_mut().exited_process_ids)
    }
}

/// A non-extension request that no longer borrows the process coordinator.
///
/// Preparation performs only the short ownership/state critical section needed
/// to clone or snapshot request inputs. The owned future may then run under the
/// request supervisor without an `Arc<Mutex<VmManager<_>>>` or another
/// whole-sidecar lease.
#[derive(Clone)]
pub enum PreparedMembershipCommit {
    Connection {
        connection_id: String,
        auth_token: String,
    },
    Session {
        connection_id: String,
        session_id: String,
        placement: crate::protocol::SidecarPlacement,
        metadata: BTreeMap<String, String>,
    },
}

pub struct PreparedRequest {
    request: RequestFrame,
    operation: PreparedRequestFuture,
    effects: RequestCompletionEffects,
    committed_membership: Option<PreparedMembershipCommit>,
}

/// Terminal value returned by an independently executing [`PreparedRequest`].
/// The original request is retained so success and failure both preserve the
/// request ID and ownership scope.
pub struct CompletedRequest {
    request: RequestFrame,
    result: Result<DispatchResult, VmError>,
    effects: RequestCompletionEffects,
    committed_membership: Option<PreparedMembershipCommit>,
}

impl PreparedRequest {
    #[cfg(feature = "test-support")]
    pub fn for_test_future<F>(request: RequestFrame, operation: F) -> Self
    where
        F: Future<Output = Result<DispatchResult, VmError>> + 'static,
    {
        Self::from_future(request, operation)
    }

    pub(crate) fn from_future<F>(request: RequestFrame, operation: F) -> Self
    where
        F: Future<Output = Result<DispatchResult, VmError>> + 'static,
    {
        Self::from_future_with_effects(request, operation, RequestCompletionEffects::default())
    }

    fn from_future_with_effects<F>(
        request: RequestFrame,
        operation: F,
        effects: RequestCompletionEffects,
    ) -> Self
    where
        F: Future<Output = Result<DispatchResult, VmError>> + 'static,
    {
        Self {
            request,
            operation: Box::pin(operation),
            effects,
            committed_membership: None,
        }
    }

    fn from_vm_command<F>(request: RequestFrame, operation: F) -> Self
    where
        F: Future<Output = Result<DispatchResult, VmError>> + 'static,
    {
        Self::from_future(request, operation)
    }

    fn failed(request: RequestFrame, error: VmError) -> Self {
        Self::from_future(request, async move { Err(error) })
    }

    fn from_future_with_membership<F>(
        request: RequestFrame,
        operation: F,
        committed_membership: PreparedMembershipCommit,
    ) -> Self
    where
        F: Future<Output = Result<DispatchResult, VmError>> + 'static,
    {
        let mut prepared = Self::from_future(request, operation);
        prepared.committed_membership = Some(committed_membership);
        prepared
    }

    pub fn committed_membership(&self) -> Option<&PreparedMembershipCommit> {
        self.committed_membership.as_ref()
    }

    pub async fn execute(self) -> CompletedRequest {
        let PreparedRequest {
            request,
            operation,
            effects,
            committed_membership,
        } = self;
        CompletedRequest {
            request,
            result: operation.await,
            effects,
            committed_membership,
        }
    }
}

impl CompletedRequest {
    pub fn failed(&self) -> bool {
        self.result.is_err()
    }
}

// SharedBridge struct and Clone impl moved to crate::state

#[derive(Debug, Default, Deserialize)]
struct LegacyProcessLaunchOptions {
    // The V8 sync host host_function still carries command/argv/options as three
    // strings. Flatten the canonical options object here so every newly added
    // field crosses that compatibility bridge automatically; keeping a second
    // hand-copied field list previously dropped POSIX spawn attributes and fd
    // mappings without an error.
    #[serde(flatten)]
    options: ProcessLaunchOptions,
    #[serde(default, rename = "maxBuffer")]
    max_buffer: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JavascriptHttpLoopbackRequest {
    pub(crate) process_id: String,
    pub(crate) server_id: u64,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) request: String,
}

pub(crate) fn is_javascript_loopback_host(host: &str) -> bool {
    host == "127.0.0.1" || host == "::1" || host.eq_ignore_ascii_case("localhost")
}

pub(crate) fn parse_javascript_child_process_spawn_request(
    vm: &VmState,
    args: &[Value],
) -> Result<(ProcessLaunchRequest, Option<usize>), VmError> {
    if let Some(value) = args.first().cloned() {
        if let Ok(request) = serde_json::from_value::<ProcessLaunchRequest>(value) {
            return Ok((request, None));
        }
    }

    let command = javascript_sync_rpc_arg_str(args, 0, "child_process.spawn command")?.to_owned();
    let raw_args = javascript_sync_rpc_arg_str(args, 1, "child_process.spawn args")?;
    let raw_options = javascript_sync_rpc_arg_str(args, 2, "child_process.spawn options")?;

    let parsed_args = serde_json::from_str::<Vec<String>>(raw_args).map_err(|error| {
        VmError::InvalidState(format!("invalid child_process.spawn args payload: {error}"))
    })?;
    let parsed_options =
        parse_legacy_javascript_child_process_spawn_options(&vm.guest_env, raw_options)?;
    let max_buffer = parsed_options.max_buffer;
    let options = parsed_options.options;

    Ok((
        ProcessLaunchRequest {
            command,
            args: parsed_args,
            options,
        },
        max_buffer,
    ))
}

fn parse_legacy_javascript_child_process_spawn_options(
    vm_guest_env: &BTreeMap<String, String>,
    raw_options: &str,
) -> Result<LegacyProcessLaunchOptions, VmError> {
    let mut parsed =
        serde_json::from_str::<LegacyProcessLaunchOptions>(raw_options).map_err(|error| {
            VmError::InvalidState(format!(
                "invalid child_process.spawn options payload: {error}"
            ))
        })?;
    let mut internal_bootstrap_env =
        sanitize_javascript_child_process_internal_bootstrap_env(vm_guest_env);
    internal_bootstrap_env.extend(sanitize_javascript_child_process_internal_bootstrap_env(
        &parsed.options.internal_bootstrap_env,
    ));
    parsed.options.internal_bootstrap_env = internal_bootstrap_env;
    Ok(parsed)
}

impl<B> SharedBridge<B> {
    fn new(bridge: B) -> Self {
        Self {
            inner: Arc::new(Mutex::new(bridge)),
            permissions: Arc::new(Mutex::new(BTreeMap::new())),
            #[cfg(test)]
            set_vm_permissions_outcomes: Arc::new(Mutex::new(VecDeque::new())),
            #[cfg(test)]
            set_vm_permissions_history: Arc::new(Mutex::new(Vec::new())),
            #[cfg(test)]
            emit_lifecycle_outcomes: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

impl<B> SharedBridge<B>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    pub(crate) fn with_mut<T>(
        &self,
        operation: impl FnOnce(&mut B) -> Result<T, BridgeError<B>>,
    ) -> Result<T, VmError> {
        let mut bridge = self
            .inner
            .lock()
            .map_err(|_| VmError::Bridge(String::from("native sidecar bridge lock poisoned")))?;
        operation(&mut bridge).map_err(|error| VmError::Bridge(format!("{error:?}")))
    }

    fn inspect<T>(&self, operation: impl FnOnce(&mut B) -> T) -> Result<T, VmError> {
        let mut bridge = self
            .inner
            .lock()
            .map_err(|_| VmError::Bridge(String::from("native sidecar bridge lock poisoned")))?;
        Ok(operation(&mut bridge))
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn queue_set_vm_permissions_result(
        &self,
        result: Result<(), VmError>,
    ) -> Result<(), VmError> {
        let mut outcomes = self.set_vm_permissions_outcomes.lock().map_err(|_| {
            VmError::Bridge(String::from(
                "native sidecar test set_vm_permissions outcome lock poisoned",
            ))
        })?;
        outcomes.push_back(result.err());
        Ok(())
    }

    #[cfg(test)]
    // Called by the integration suite that includes this module as source.
    #[allow(dead_code)]
    pub(crate) fn queue_emit_lifecycle_result(
        &self,
        result: Result<(), VmError>,
    ) -> Result<(), VmError> {
        self.emit_lifecycle_outcomes
            .lock()
            .map_err(|_| VmError::Bridge("test lifecycle outcome lock poisoned".into()))?
            .push_back(result.err());
        Ok(())
    }

    pub(crate) fn emit_lifecycle(&self, vm_id: &str, state: LifecycleState) -> Result<(), VmError> {
        #[cfg(test)]
        if let Some(Some(error)) = self
            .emit_lifecycle_outcomes
            .lock()
            .map_err(|_| VmError::Bridge("test lifecycle outcome lock poisoned".into()))?
            .pop_front()
        {
            return Err(error);
        }
        self.with_mut(|bridge| {
            bridge.emit_lifecycle(LifecycleEventRecord {
                vm_id: vm_id.to_owned(),
                state,
                detail: None,
            })
        })
    }

    pub(crate) fn emit_log(&self, vm_id: &str, message: impl Into<String>) -> Result<(), VmError> {
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
    ) -> Result<(), VmError> {
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
            Some(reason) => format!("permission denied, {resource}: {reason}"),
            None => format!("permission denied, {resource}"),
        };
        Err(VmError::host("EACCES", message))
    }

    /// Revalidate an authority-expanding network operation against both the
    /// requested name and the complete DNS answer immediately before use.
    ///
    /// The requested resource uses normal policy semantics. For a stored rule
    /// set, resolved addresses add restrictions only when an address rule
    /// explicitly matches; this preserves hostname allowlists and literal-IP
    /// policies. Dynamic bridge policies are asked about every resource.
    pub(crate) fn require_resolved_network_access(
        &self,
        vm_id: &str,
        op: NetworkOperation,
        requested_resource: &str,
        resolved_resources: &[String],
    ) -> Result<(), VmError> {
        let capability = network_permission_capability(op);
        let permissions = self
            .permissions
            .lock()
            .map_err(|_| {
                VmError::Bridge(String::from(
                    "native sidecar permission policy lock poisoned",
                ))
            })?
            .get(vm_id)
            .cloned();

        let require = |resource: &str, decision: PermissionDecision| {
            if decision.allow {
                return Ok(());
            }
            let message = match decision.reason.as_deref() {
                Some(reason) => format!("permission denied, {resource}: {reason}"),
                None => format!("permission denied, {resource}"),
            };
            Err(VmError::host("EACCES", message))
        };

        if let Some(permissions) = permissions {
            let requested_mode = evaluate_permissions_policy(
                &permissions,
                "network",
                capability,
                Some(requested_resource),
            );
            require(
                requested_resource,
                permission_mode_to_kernel_decision(requested_mode, capability),
            )?;

            let mut checked = BTreeSet::new();
            for resource in resolved_resources {
                if resource == requested_resource || !checked.insert(resource.as_str()) {
                    continue;
                }
                let Some(mode) = evaluate_matching_pattern_permission_policy(
                    &permissions,
                    "network",
                    capability,
                    Some(resource),
                ) else {
                    continue;
                };
                require(
                    resource,
                    permission_mode_to_kernel_decision(mode, capability),
                )?;
            }
            return Ok(());
        }

        self.require_network_access(vm_id, op, requested_resource.to_owned())?;
        let mut checked = BTreeSet::new();
        for resource in resolved_resources {
            if resource != requested_resource && checked.insert(resource.as_str()) {
                self.require_network_access(vm_id, op, resource.clone())?;
            }
        }
        Ok(())
    }

    pub(crate) fn set_vm_permissions(
        &self,
        vm_id: &str,
        permissions: &PermissionsPolicy,
    ) -> Result<(), VmError> {
        #[cfg(test)]
        {
            let mut outcomes = self.set_vm_permissions_outcomes.lock().map_err(|_| {
                VmError::Bridge(String::from(
                    "native sidecar test set_vm_permissions outcome lock poisoned",
                ))
            })?;
            if let Some(Some(error)) = outcomes.pop_front() {
                return Err(error);
            }
        }

        let mut stored = self.permissions.lock().map_err(|_| {
            VmError::Bridge(String::from(
                "native sidecar permission policy lock poisoned",
            ))
        })?;
        stored.insert(vm_id.to_owned(), permissions.clone());
        #[cfg(test)]
        self.set_vm_permissions_history
            .lock()
            .map_err(|_| {
                VmError::Bridge(String::from(
                    "native sidecar test permission history lock poisoned",
                ))
            })?
            .push((vm_id.to_owned(), permissions.clone()));
        Ok(())
    }

    pub(crate) fn restore_vm_permissions_fail_closed(
        &self,
        vm_id: &str,
        original_permissions: &PermissionsPolicy,
        context: &str,
        operation_error: &VmError,
    ) -> Result<(), VmError> {
        match self.set_vm_permissions(vm_id, original_permissions) {
            Ok(()) => Ok(()),
            Err(restore_error) => {
                let deny_all = deny_all_policy();
                match self.set_vm_permissions(vm_id, &deny_all) {
                    Ok(()) => Err(VmError::InvalidState(format!(
                        "{context} failed: {operation_error}; restoring original permissions failed: {restore_error}; applied deny-all fallback"
                    ))),
                    Err(deny_all_error) => panic!(
                        "{context} failed: {operation_error}; restoring original permissions failed: {restore_error}; deny-all fallback failed: {deny_all_error}"
                    ),
                }
            }
        }
    }

    pub(crate) fn clear_vm_permissions(&self, vm_id: &str) -> Result<(), VmError> {
        let mut stored = self.permissions.lock().map_err(|_| {
            VmError::Bridge(String::from(
                "native sidecar permission policy lock poisoned",
            ))
        })?;
        stored.remove(vm_id);
        Ok(())
    }

    pub(crate) fn filesystem_unrestricted(&self, vm_id: &str) -> bool {
        let Ok(stored) = self.permissions.lock() else {
            return false;
        };
        matches!(
            stored.get(vm_id).and_then(|policy| policy.fs.as_ref()),
            Some(FsPermissionScope::Mode(PermissionMode::Allow))
        )
    }

    pub(crate) fn static_permission_decision(
        &self,
        vm_id: &str,
        capability: &str,
        domain: &str,
        resource: Option<&str>,
    ) -> Option<PermissionDecision> {
        let stored = match self.permissions.lock() {
            Ok(stored) => stored,
            Err(error) => {
                tracing::error!(%error, vm_id, "native sidecar permission policy lock poisoned");
                return Some(PermissionDecision::deny(
                    "native sidecar permission policy lock poisoned",
                ));
            }
        };
        let permissions = stored.get(vm_id)?;
        let mode = evaluate_permissions_policy(permissions, domain, capability, resource);
        Some(permission_mode_to_kernel_decision(mode, capability))
    }
}

pub(crate) fn validate_permissions_policy(permissions: &PermissionsPolicy) -> Result<(), VmError> {
    crate::core::permissions::validate_permissions_policy(permissions)
        .map_err(|error| VmError::InvalidState(error.to_string()))
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
    sidecar: &VmManager<B>,
    ownership: &OwnershipScope,
    event: &ProcessEventEnvelope,
) -> bool
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    if !ownership_matches_process_event(ownership, event) {
        return false;
    }

    if event.process_id.contains('/') {
        return false;
    }

    let target = match ProcessEventTarget::new(
        event.connection_id.clone(),
        event.session_id.clone(),
        event.vm_id.clone(),
        event.process_id.clone(),
    ) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("ERR_AGENTOS_PROCESS_EVENT_TARGET: {error}");
            return false;
        }
    };
    match sidecar.process_event_broker.target_is_claimed(&target) {
        Ok(true) => return false,
        Ok(false) => {}
        Err(error) => {
            eprintln!("ERR_AGENTOS_PROCESS_EVENT_CLAIM_CHECK: {error}");
            return false;
        }
    }

    // Stale unclaimed events must still be drained through
    // handle_process_event_envelope() so the sidecar can emit the expected
    // fail-closed log when teardown wins the race.
    true
}

fn poll_future_once<F: std::future::Future + ?Sized>(
    future: std::pin::Pin<&mut F>,
) -> Option<F::Output> {
    let mut context = Context::from_waker(Waker::noop());
    match future.poll(&mut context) {
        Poll::Ready(output) => Some(output),
        Poll::Pending => None,
    }
}

// ConnectionState, SessionState, VmConfiguration, VmState moved to crate::state

// SocketPathContext, SocketFamily, VmListenPolicy moved to crate::state

impl SocketPathContext {
    pub(crate) fn loopback_port_allowed(&self, port: u16) -> bool {
        self.loopback_exempt_ports.contains(&port)
            || self
                .tcp_loopback_guest_to_host_ports
                .keys()
                .any(|(_, guest_port)| *guest_port == port)
            || self
                .udp_loopback_guest_to_host_ports
                .keys()
                .any(|(_, guest_port)| *guest_port == port)
    }

    pub(crate) fn translate_tcp_loopback_port(
        &self,
        family: SocketFamily,
        port: u16,
    ) -> Option<u16> {
        self.tcp_loopback_guest_to_host_ports
            .get(&(family, port))
            .copied()
    }

    pub(crate) fn http_loopback_target(
        &self,
        family: SocketFamily,
        port: u16,
    ) -> Option<&crate::state::HttpLoopbackTarget> {
        self.http_loopback_targets.get(&(family, port))
    }

    pub(crate) fn translate_udp_loopback_port(
        &self,
        family: SocketFamily,
        port: u16,
    ) -> Option<u16> {
        self.udp_loopback_guest_to_host_ports
            .get(&(family, port))
            .copied()
    }

    pub(crate) fn guest_udp_port_for_host_port(
        &self,
        family: SocketFamily,
        port: u16,
    ) -> Option<u16> {
        self.udp_loopback_host_to_guest_ports
            .get(&(family, port))
            .copied()
    }
}

// ActiveProcess, NetworkResourceCounts moved to crate::state

/// Direct Rust callers do not have the sidecar transport supervisor. Retain
/// their claimed work across calls so a completed fetch cannot cancel a
/// server's next read, accept, or child operation.
struct InProcessEventService {
    vm_id: String,
    future: Pin<Box<dyn std::future::Future<Output = Result<(), VmError>>>>,
    reply: Option<crate::executor::backend::DirectHostReplyHandle>,
    completed: bool,
    ready: Arc<std::sync::atomic::AtomicBool>,
}

impl InProcessEventService {
    fn finish(&mut self, result: Result<(), VmError>) {
        if let Err(error) = result {
            if let Some(reply) = &self.reply {
                if !reply.is_terminal() {
                    let failure = match &error {
                        VmError::Host(error) => error.clone(),
                        _ => crate::executor::backend::HostServiceError::new(
                            error.code().unwrap_or("EIO"),
                            error.to_string(),
                        ),
                    };
                    if let Err(delivery) = reply.fail(failure) {
                        tracing::error!(%delivery, "in-process host failure reply could not be delivered");
                    }
                }
            }
            tracing::error!(vm_id = %self.vm_id, %error, "in-process event service failed");
        }
        self.completed = true;
    }
}

impl Drop for InProcessEventService {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        if let Some(reply) = &self.reply {
            if !reply.is_terminal() {
                if let Err(error) = reply.fail(crate::executor::backend::HostServiceError::new(
                    "ECANCELED",
                    "VM disposal cancelled an in-process host service",
                )) {
                    tracing::error!(vm_id = %self.vm_id, %error, "in-process cancellation reply could not be delivered");
                }
            }
        } else {
            tracing::warn!(vm_id = %self.vm_id, "VM disposal cancelled a pending in-process child bridge relay");
        }
    }
}

struct InProcessEventWake {
    notify: Arc<tokio::sync::Notify>,
    ready: Arc<std::sync::atomic::AtomicBool>,
}
impl std::task::Wake for InProcessEventWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, std::sync::atomic::Ordering::Release);
        self.notify.notify_one();
    }
}

pub struct VmManager<B> {
    pub(crate) config: VmManagerConfig,
    in_process_event_services: Vec<InProcessEventService>,
    in_process_event_cursor: usize,
    in_process_event_limit_warned: bool,
    pub(crate) runtime_context: Option<agentos_driver_tokio::DriverHandle>,
    pub(crate) executors: ExecutorRegistry,
    pub(crate) dns_resolver: agentos_vm_kernel::dns::SharedDnsResolver,
    pub(crate) bridge: SharedBridge<B>,
    pub(crate) mount_plugins: FileSystemPluginRegistry<MountPluginContext<B>>,
    pub(crate) cache_root: PathBuf,
    #[cfg(feature = "node-v8")]
    pub(crate) javascript_engine: JavascriptExecutionEngine,
    #[cfg(feature = "python-v8-pyodide")]
    pub(crate) python_engine: PythonExecutionEngine,
    pub(crate) wasm_engine: WasmExecutionEngine,
    pub(crate) next_connection_id: usize,
    pub(crate) next_session_id: usize,
    pub(crate) next_vm_id: usize,
    pub(crate) next_sidecar_request_id: RequestId,
    pub(crate) connections: BTreeMap<String, ConnectionState>,
    pub(crate) sessions: BTreeMap<String, SessionState>,
    pub(crate) vms: VmRegistry,
    /// Detached generations whose asynchronous ownership has not reconciled.
    /// The combined active + quarantined generation count is admitted against
    /// `runtime.resources.maxCapabilities`, keeping this collection bounded.
    pub(crate) quarantined_vms: BTreeMap<u64, QuarantinedVmGeneration>,
    #[allow(dead_code)]
    pub(crate) process_event_sender: Sender<ProcessEventEnvelope>,
    pub(crate) process_event_receiver: Option<Receiver<ProcessEventEnvelope>>,
    pub(crate) process_event_notify: Arc<tokio::sync::Notify>,
    /// Ownership-aware handoff for process-targeted consumers. The legacy raw
    /// receiver remains the compatibility source while callers migrate; a
    /// short coordinator command transfers one matching envelope into this
    /// broker before the caller performs its long wait independently.
    pub(crate) process_event_broker: ProcessEventBroker,
    pub(crate) process_event_ingress: ProcessEventIngress,
    /// The single process-level deadline task that wakes cooperative kernel
    /// zombie reaping. It is replaced only when a genuinely earlier deadline
    /// appears; never one task or OS thread per process.
    pub(crate) kernel_reaper_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) kernel_reaper_deadline: Option<Instant>,
    /// One receiver envelope whose temporary public-queue admission failed.
    /// It is retried before reading the channel, preserving exact FIFO order.
    pub(crate) deferred_process_event_envelope: Option<ProcessEventEnvelope>,
    pub(crate) pending_process_events: VecDeque<ProcessEventEnvelope>,
    pub(crate) pending_sidecar_responses: SidecarResponseTracker,
    pub(crate) outbound_sidecar_requests: VecDeque<SidecarRequestFrame>,
    pub(crate) completed_sidecar_responses: BTreeMap<RequestId, SidecarResponseFrame>,
    pub(crate) completed_sidecar_response_order: VecDeque<RequestId>,
    pub(crate) completed_sidecar_responses_gauge: Arc<QueueGauge>,
    pub(crate) pending_process_events_gauge: Arc<QueueGauge>,
    pub(crate) pending_process_event_bytes_gauge: Arc<QueueGauge>,
    pub(crate) pending_sidecar_responses_gauge: Arc<QueueGauge>,
    pub(crate) outbound_sidecar_requests_gauge: Arc<QueueGauge>,
    pub(crate) sidecar_requests: SharedSidecarRequestClient,
    pub(crate) event_sink: SharedEventSink,
    pub(crate) extensions: BTreeMap<String, Arc<dyn Extension>>,
    /// Cloneable transport-agnostic services used by direct extension
    /// dispatch. Extension futures never receive or retain `&mut VmManager`.
    pub(crate) extension_services: Option<Arc<dyn ExtensionServices>>,
    pub(crate) extension_sessions: BTreeMap<(String, String), ExtensionSessionResources>,
    pub(crate) extension_process_output_buffers:
        BTreeMap<(String, String), ExtensionBufferedProcessOutput>,
    #[cfg(test)]
    pub(crate) fail_next_exec_start_after_commit: bool,
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

struct GuestLimitDiagnostic {
    scope: &'static str,
    current_usage: Option<u64>,
    message: String,
}

fn guest_limit_diagnostic(
    limit: &agentos_driver_tokio::accounting::LimitError,
) -> GuestLimitDiagnostic {
    if limit.scope.starts_with("vm=") {
        return GuestLimitDiagnostic {
            scope: "vm",
            current_usage: Some(u64::try_from(limit.used).unwrap_or(u64::MAX)),
            message: limit.to_string(),
        };
    }

    GuestLimitDiagnostic {
        scope: "process",
        current_usage: None,
        message: crate::state::guest_limit_message(limit),
    }
}

impl<B> fmt::Debug for VmManager<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VmManager")
            .field("config", &self.config)
            .field("cache_root", &self.cache_root)
            .field("next_connection_id", &self.next_connection_id)
            .field("next_session_id", &self.next_session_id)
            .field("next_vm_id", &self.next_vm_id)
            .field("connection_count", &self.connections.len())
            .field("session_count", &self.sessions.len())
            .field("vm_count", &self.vms.len())
            .field("quarantined_vm_count", &self.quarantined_vms.len())
            .field("extension_session_count", &self.extension_sessions.len())
            .field(
                "extension_process_output_buffer_count",
                &self.extension_process_output_buffers.len(),
            )
            .finish()
    }
}

impl<B> VmManager<B>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    pub fn with_driver_and_executors(
        bridge: B,
        config: VmManagerConfig,
        runtime_context: agentos_driver_tokio::DriverHandle,
        executors: ExecutorRegistry,
    ) -> Result<Self, VmError> {
        if matches!(config.expected_auth_token.as_deref(), Some("")) {
            return Err(VmError::InvalidState(String::from(
                "sidecar expected_auth_token must not be empty",
            )));
        }
        let dns_resolver: agentos_vm_kernel::dns::SharedDnsResolver =
            Arc::new(HickoryDnsResolver::new(runtime_context.clone()));

        let cache_root = config.compile_cache_root.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "{}-{}",
                config.instance_id,
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system time before unix epoch")
                    .as_nanos()
            ))
        });
        fs::create_dir_all(&cache_root).map_err(|error| {
            VmError::Io(format!("failed to prepare sidecar cache root: {error}"))
        })?;

        let bridge = SharedBridge::new(bridge);
        let mount_plugins = build_mount_plugin_registry::<B>()?;
        let protocol_limits = config.runtime.protocol.clone();
        let (process_event_sender, process_event_receiver) =
            channel(protocol_limits.max_process_events);
        let process_event_notify = Arc::new(tokio::sync::Notify::new());
        #[cfg(feature = "node-v8")]
        let mut javascript_engine = JavascriptExecutionEngine::new(runtime_context.clone());
        #[cfg(feature = "node-v8")]
        javascript_engine.set_event_notify(Some(Arc::clone(&process_event_notify)));
        #[cfg(feature = "python-v8-pyodide")]
        let mut python_engine = PythonExecutionEngine::new(runtime_context.clone());
        #[cfg(feature = "python-v8-pyodide")]
        python_engine.set_event_notify(Some(Arc::clone(&process_event_notify)));
        let mut wasm_engine = WasmExecutionEngine::new(runtime_context.clone());
        wasm_engine.set_event_notify(Some(Arc::clone(&process_event_notify)));

        let (process_event_broker, process_event_ingress, process_event_broker_driver) =
            ProcessEventBroker::new(&config.runtime).map_err(|error| {
                VmError::InvalidState(format!(
                    "failed to initialize process event broker: {error}"
                ))
            })?;
        runtime_context
            .spawn(agentos_driver_tokio::TaskClass::Runtime, async move {
                if let Err(error) = process_event_broker_driver.run().await {
                    eprintln!("ERR_AGENTOS_PROCESS_EVENT_BROKER_DRIVER: {error}");
                }
            })
            .map_err(VmError::from)?;
        Ok(Self {
            in_process_event_services: Vec::new(),
            in_process_event_cursor: 0,
            in_process_event_limit_warned: false,
            config,
            runtime_context: Some(runtime_context),
            executors,
            dns_resolver,
            bridge,
            mount_plugins,
            cache_root,
            #[cfg(feature = "node-v8")]
            javascript_engine,
            #[cfg(feature = "python-v8-pyodide")]
            python_engine,
            wasm_engine,
            next_connection_id: 0,
            next_session_id: 0,
            next_vm_id: 0,
            next_sidecar_request_id: -1,
            connections: BTreeMap::new(),
            sessions: BTreeMap::new(),
            vms: VmRegistry::default(),
            quarantined_vms: BTreeMap::new(),
            process_event_sender,
            process_event_receiver: Some(process_event_receiver),
            process_event_notify,
            process_event_broker,
            process_event_ingress,
            deferred_process_event_envelope: None,
            kernel_reaper_task: None,
            kernel_reaper_deadline: None,
            pending_process_events: VecDeque::new(),
            pending_sidecar_responses: SidecarResponseTracker::default(),
            outbound_sidecar_requests: VecDeque::new(),
            completed_sidecar_responses: BTreeMap::new(),
            completed_sidecar_response_order: VecDeque::new(),
            completed_sidecar_responses_gauge: register_queue(
                TrackedLimit::CompletedSidecarResponses,
                protocol_limits.max_completed_responses,
            ),
            pending_process_events_gauge: register_queue(
                TrackedLimit::PendingProcessEvents,
                protocol_limits.max_process_events,
            ),
            pending_process_event_bytes_gauge: register_queue(
                TrackedLimit::PendingProcessEventBytes,
                crate::core::limits::DEFAULT_PROCESS_PENDING_EVENT_BYTES,
            ),
            pending_sidecar_responses_gauge: register_queue(
                TrackedLimit::PendingSidecarResponses,
                protocol_limits.max_pending_responses,
            ),
            outbound_sidecar_requests_gauge: register_queue(
                TrackedLimit::OutboundSidecarRequests,
                protocol_limits.max_outbound_requests,
            ),
            sidecar_requests: SharedSidecarRequestClient::default(),
            event_sink: SharedEventSink::default(),
            extensions: BTreeMap::new(),
            extension_services: None,
            extension_sessions: BTreeMap::new(),
            extension_process_output_buffers: BTreeMap::new(),
            #[cfg(test)]
            fail_next_exec_start_after_commit: false,
            disposed_sessions: Vec::new(),
        })
    }

    pub fn with_config_extensions_driver_and_executors(
        bridge: B,
        config: VmManagerConfig,
        extensions: Vec<Box<dyn Extension>>,
        runtime_context: agentos_driver_tokio::DriverHandle,
        executors: ExecutorRegistry,
    ) -> Result<Self, VmError> {
        let mut sidecar =
            Self::with_driver_and_executors(bridge, config, runtime_context, executors)?;
        for extension in extensions {
            sidecar.register_extension(extension)?;
        }
        Ok(sidecar)
    }

    pub(crate) fn transfer_extension_process_resource(
        &mut self,
        process_id: &str,
        detached_process_ids: &[String],
    ) {
        self.extension_sessions.retain(|_, resources| {
            if resources.process_ids.remove(process_id) {
                resources
                    .process_ids
                    .extend(detached_process_ids.iter().cloned());
            }
            !resources.process_ids.is_empty() || !resources.vm_ids.is_empty()
        });
    }

    pub fn instance_id(&self) -> &str {
        &self.config.instance_id
    }

    pub fn process_event_notify(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.process_event_notify)
    }

    pub fn clear_vm_permissions(&self, vm_id: &str) -> Result<(), VmError> {
        self.bridge.clear_vm_permissions(vm_id)
    }

    /// Snapshot transport membership without exposing mutable VM state.
    pub fn membership_snapshot(&self) -> (Vec<String>, Vec<(String, String)>) {
        (
            self.connections.keys().cloned().collect(),
            self.sessions
                .iter()
                .map(|(id, session)| (session.connection_id.clone(), id.clone()))
                .collect(),
        )
    }

    pub fn extension_routes(&self) -> &BTreeMap<String, Arc<dyn Extension>> {
        &self.extensions
    }

    pub fn extension(&self, namespace: &str) -> Option<Arc<dyn Extension>> {
        self.extensions.get(namespace).cloned()
    }

    pub fn new(bridge: B) -> Result<Self, VmError> {
        Self::with_config(bridge, VmManagerConfig::default())
    }

    pub fn with_config(bridge: B, config: VmManagerConfig) -> Result<Self, VmError> {
        let runtime_context = agentos_driver_tokio::TokioDriver::process(&config.runtime)
            .map_err(|error| VmError::InvalidState(error.to_string()))?
            .handle();
        Self::with_runtime_context(bridge, config, runtime_context)
    }

    fn with_runtime_context(
        bridge: B,
        config: VmManagerConfig,
        runtime_context: agentos_driver_tokio::DriverHandle,
    ) -> Result<Self, VmError> {
        Self::with_driver_and_executors(bridge, config, runtime_context, ExecutorRegistry::empty())
    }

    pub fn with_config_and_extensions(
        bridge: B,
        config: VmManagerConfig,
        extensions: Vec<Box<dyn Extension>>,
    ) -> Result<Self, VmError> {
        let mut sidecar = Self::with_config(bridge, config)?;
        for extension in extensions {
            sidecar.register_extension(extension)?;
        }
        Ok(sidecar)
    }

    pub fn with_config_extensions_and_runtime(
        bridge: B,
        config: VmManagerConfig,
        extensions: Vec<Box<dyn Extension>>,
        runtime_context: agentos_driver_tokio::DriverHandle,
    ) -> Result<Self, VmError> {
        Self::with_config_extensions_driver_and_executors(
            bridge,
            config,
            extensions,
            runtime_context,
            ExecutorRegistry::empty(),
        )
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
    /// the process lifetime (H1). This also reclaims extension output buffers,
    /// which was previously removed only on a successful handoff and leaked on VM
    /// or session disposal (M6).
    pub fn reclaim_vm_tracking(&mut self, session_id: &str, vm_id: &str) {
        #[cfg(feature = "node-v8")]
        self.javascript_engine.dispose_vm(vm_id);
        #[cfg(feature = "python-v8-pyodide")]
        self.python_engine.dispose_vm(vm_id);
        self.wasm_engine.dispose_vm(vm_id);
        self.prune_extension_vm_resource(vm_id);
        self.extension_process_output_buffers
            .retain(|(buffer_vm_id, _process_id), _| buffer_vm_id != vm_id);
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.vm_ids.remove(vm_id);
        }
    }

    /// Probe buffered extension process output without retaining the sidecar
    /// coordinator across the caller's wait. When `finalize_if_empty` is false,
    /// `None` means the caller should wait on `process_event_notify` and probe
    /// again. The final probe binds the process resource and transfers ownership
    /// of the buffer even when no output arrived before the deadline.
    pub(crate) async fn probe_extension_process_output_handoff(
        &mut self,
        ownership: OwnershipScope,
        namespace: String,
        ext_session_id: String,
        process_id: String,
        finalize_if_empty: bool,
    ) -> Result<Option<ExtensionBufferedProcessOutput>, VmError> {
        let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
        let key = (vm_id.clone(), process_id.clone());
        self.pump_process_events(&ownership).await?;
        while let Some(envelope) = self.take_matching_process_event_envelope(&vm_id, &process_id)? {
            if self.capture_extension_process_output_event(&vm_id, &process_id, &envelope.event) {
                continue;
            }
            self.queue_pending_process_event(envelope)?;
            break;
        }
        let buffered = self
            .extension_process_output_buffers
            .get(&key)
            .is_some_and(|buffer| !buffer.stdout.is_empty() || !buffer.stderr.is_empty());
        if !buffered && !finalize_if_empty {
            return Ok(None);
        }
        self.bind_extension_process_resource(ownership, namespace, ext_session_id, process_id)?;
        self.extension_process_output_buffers
            .remove(&key)
            .map(Some)
            .ok_or_else(|| {
                VmError::InvalidState(String::from(
                    "extension process output buffering was not started",
                ))
            })
    }

    pub(crate) fn probe_extension_process_output_handoff_nowait(
        &mut self,
        ownership: OwnershipScope,
        namespace: String,
        ext_session_id: String,
        process_id: String,
        finalize_if_empty: bool,
    ) -> Result<Option<ExtensionBufferedProcessOutput>, VmError> {
        let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
        let key = (vm_id.clone(), process_id.clone());
        while let Some(envelope) = self.take_matching_process_event_envelope(&vm_id, &process_id)? {
            if self.capture_extension_process_output_event(&vm_id, &process_id, &envelope.event) {
                continue;
            }
            self.queue_pending_process_event(envelope)?;
            break;
        }
        let buffered = self
            .extension_process_output_buffers
            .get(&key)
            .is_some_and(|buffer| !buffer.stdout.is_empty() || !buffer.stderr.is_empty());
        if !buffered && !finalize_if_empty {
            return Ok(None);
        }
        self.bind_extension_process_resource(ownership, namespace, ext_session_id, process_id)?;
        self.extension_process_output_buffers
            .remove(&key)
            .map(Some)
            .ok_or_else(|| {
                VmError::InvalidState(String::from(
                    "extension process output buffering was not started",
                ))
            })
    }

    pub(crate) fn reap_reconciled_quarantined_vms(&mut self) {
        let before = self.quarantined_vms.len();
        let mut reaped = Vec::new();
        self.quarantined_vms.retain(|generation, quarantined| {
            if quarantined.can_reap() {
                reaped.push((*generation, quarantined.vm_id.clone()));
                false
            } else {
                true
            }
        });
        for (generation, vm_id) in reaped {
            eprintln!("INFO_AGENTOS_VM_QUARANTINE_REAPED: vm_id={vm_id} generation={generation}");
        }
        if self.quarantined_vms.len() != before {
            self.observe_active_vm_generations();
        }
    }

    pub(crate) fn observe_active_vm_generations(&self) {
        if let Some(runtime_context) = self.runtime_context.as_ref() {
            runtime_context.metrics().observe_resource(
                ResourceMetricClass::ActiveVms,
                self.vms.len().saturating_add(self.quarantined_vms.len()),
            );
        }
    }

    pub(crate) fn ensure_vm_generation_capacity(&self) -> Result<(), VmError> {
        let limit = self.config.runtime.resources.max_capabilities;
        let used = self.vms.len().saturating_add(self.quarantined_vms.len());
        if used >= limit {
            return Err(VmError::host(
                "ERR_AGENTOS_VM_GENERATION_LIMIT",
                format!("tracked={used} limit={limit}; raise runtime.resources.maxCapabilities"),
            ));
        }
        Ok(())
    }

    pub(crate) fn retain_quarantined_vm(
        &mut self,
        quarantined: QuarantinedVmGeneration,
    ) -> Result<(), VmError> {
        let generation = quarantined.generation;
        if self.quarantined_vms.contains_key(&generation) {
            return Err(VmError::Conflict(format!(
                "ERR_AGENTOS_VM_GENERATION_DUPLICATE: generation={generation} is already quarantined"
            )));
        }
        let limit = self.config.runtime.resources.max_capabilities;
        if self.quarantined_vms.len() >= limit {
            return Err(VmError::host(
                "ERR_AGENTOS_VM_QUARANTINE_LIMIT",
                format!(
                    "quarantined={} limit={limit}; raise runtime.resources.maxCapabilities",
                    self.quarantined_vms.len()
                ),
            ));
        }
        self.quarantined_vms.insert(generation, quarantined);
        self.observe_active_vm_generations();
        Ok(())
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
            ActiveExecutionEvent::Common(crate::executor::backend::ExecutionEvent::Output {
                stream,
                bytes,
            }) => {
                match stream {
                    crate::executor::backend::OutputStream::Stdout => buffer.append_stdout(
                        bytes.as_slice(),
                        DEFAULT_EXTENSION_OUTPUT_BUFFER_BYTE_LIMIT,
                    ),
                    crate::executor::backend::OutputStream::Stderr => buffer.append_stderr(
                        bytes.as_slice(),
                        DEFAULT_EXTENSION_OUTPUT_BUFFER_BYTE_LIMIT,
                    ),
                }
                true
            }
            ActiveExecutionEvent::Common(_) => false,
            ActiveExecutionEvent::Stdout(chunk) => {
                buffer.append_stdout(chunk, DEFAULT_EXTENSION_OUTPUT_BUFFER_BYTE_LIMIT);
                true
            }
            ActiveExecutionEvent::Stderr(chunk) => {
                buffer.append_stderr(chunk, DEFAULT_EXTENSION_OUTPUT_BUFFER_BYTE_LIMIT);
                true
            }
            ActiveExecutionEvent::HostRpcRequest(_)
            | ActiveExecutionEvent::HostCallCompletion(_)
            | ActiveExecutionEvent::DeferredPosixPollWake
            | ActiveExecutionEvent::ManagedStreamReadRecheck(_)
            | ActiveExecutionEvent::ManagedUdpPollRecheck(_)
            | ActiveExecutionEvent::SignalState { .. }
            | ActiveExecutionEvent::Exited(_) => false,
        }
    }

    pub(crate) fn bind_extension_process_resource(
        &mut self,
        ownership: OwnershipScope,
        namespace: String,
        ext_session_id: String,
        process_id: String,
    ) -> Result<(), VmError> {
        if ext_session_id.is_empty() {
            return Err(VmError::InvalidState(String::from(
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
            return Err(VmError::InvalidState(format!(
                "VM {vm_id} has no active process {process_id}"
            )));
        }

        let key = (namespace, ext_session_id);
        if let Some(resources) = self.extension_sessions.get_mut(&key) {
            if resources.ownership != ownership {
                return Err(VmError::InvalidState(String::from(
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

    pub(crate) fn bind_extension_vm_resource(
        &mut self,
        ownership: OwnershipScope,
        namespace: String,
        ext_session_id: String,
    ) -> Result<(), VmError> {
        if ext_session_id.is_empty() {
            return Err(VmError::InvalidState(String::from(
                "extension session id must not be empty",
            )));
        }
        let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;

        let key = (namespace, ext_session_id);
        if let Some(resources) = self.extension_sessions.get_mut(&key) {
            if resources.ownership != ownership {
                return Err(VmError::InvalidState(String::from(
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

    pub(crate) fn start_buffering_process_output_nowait(
        &mut self,
        ownership: OwnershipScope,
        process_id: String,
    ) -> Result<(), VmError> {
        let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
        let key = (vm_id, process_id);
        if self.extension_process_output_buffers.contains_key(&key) {
            return Err(VmError::Conflict(String::from(
                "extension process output buffering already started",
            )));
        }
        self.extension_process_output_buffers
            .insert(key, ExtensionBufferedProcessOutput::default());
        Ok(())
    }

    pub(crate) fn detach_extension_session_resources_for_owned_disposal(
        &mut self,
        ownership: OwnershipScope,
        namespace: String,
        ext_session_id: String,
    ) -> Result<Vec<String>, VmError> {
        let key = (namespace, ext_session_id);
        let Some(resources) = self.extension_sessions.get(&key) else {
            return Ok(Vec::new());
        };
        if resources.ownership != ownership {
            return Err(VmError::InvalidState(String::from(
                "extension session ownership did not match dispose request",
            )));
        }
        let resources = self
            .extension_sessions
            .remove(&key)
            .expect("extension resources existed before removal");
        let (_, _, vm_id) = self.vm_scope_for(&ownership)?;
        for process_id in resources.process_ids {
            if self
                .vms
                .get(&vm_id)
                .is_some_and(|vm| vm.active_processes.contains_key(&process_id))
            {
                self.kill_process_internal(&vm_id, &process_id, "SIGTERM")?;
            }
        }
        Ok(resources.vm_ids.into_iter().collect())
    }

    pub fn sidecar_id(&self) -> &str {
        &self.config.instance_id
    }

    pub fn with_bridge_mut<T>(&self, operation: impl FnOnce(&mut B) -> T) -> Result<T, VmError> {
        self.bridge.inspect(operation)
    }

    pub fn set_sidecar_request_transport(&mut self, transport: Arc<dyn SidecarRequestTransport>) {
        self.sidecar_requests.set_transport(transport);
    }

    pub fn set_event_transport(&mut self, transport: Arc<dyn EventSinkTransport>) {
        self.event_sink.set_transport(transport);
    }

    pub fn set_extension_services(&mut self, services: Arc<dyn ExtensionServices>) {
        self.extension_services = Some(services);
    }

    pub fn register_extension(&mut self, extension: Box<dyn Extension>) -> Result<(), VmError> {
        let namespace = extension.namespace().to_owned();
        if namespace.is_empty() {
            return Err(VmError::InvalidState(String::from(
                "extension namespace must not be empty",
            )));
        }
        if self.extensions.contains_key(&namespace) {
            return Err(VmError::Conflict(format!(
                "extension namespace {namespace} is already registered",
            )));
        }
        self.extensions.insert(namespace, Arc::from(extension));
        Ok(())
    }

    pub fn set_sidecar_request_handler<F>(&mut self, handler: F)
    where
        F: Fn(SidecarRequestFrame) -> Result<SidecarResponsePayload, VmError>
            + Send
            + Sync
            + 'static,
    {
        struct HandlerTransport<F>(F);

        impl<F> SidecarRequestTransport for HandlerTransport<F>
        where
            F: Fn(SidecarRequestFrame) -> Result<SidecarResponsePayload, VmError>
                + Send
                + Sync
                + 'static,
        {
            fn send_request(
                &self,
                request: SidecarRequestFrame,
                _timeout: Duration,
            ) -> Result<SidecarResponseFrame, VmError> {
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
            ) -> Result<crate::wire::SidecarResponseFrame, VmError>
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
    ) -> Result<(), VmError> {
        self.try_queue_pending_process_event(envelope)
            .map_err(|(error, _envelope)| error)
    }

    // Preserve the rejected envelope so callers can requeue it without losing
    // its retained-byte reservation or delivery ordering.
    #[allow(clippy::result_large_err)]
    pub(crate) fn try_queue_pending_process_event(
        &mut self,
        envelope: ProcessEventEnvelope,
    ) -> Result<(), (VmError, ProcessEventEnvelope)> {
        if let Err(error) = self.check_pending_process_event_capacity(&envelope) {
            return Err((error, envelope));
        }
        if matches!(&envelope.event, ActiveExecutionEvent::Exited(_)) {
            mark_execute_exit_event_queued(&envelope.vm_id, &envelope.process_id);
        }
        self.pending_process_events.push_back(envelope);
        self.observe_pending_process_event_depth();
        Ok(())
    }

    pub fn process_event_broker(&self) -> ProcessEventBroker {
        self.process_event_broker.clone()
    }

    /// Transfer at most one raw event for an ownership-validated process into
    /// the independent broker. Pumping and registry access are short-lived;
    /// the caller awaits its broker lease after this method releases `&mut
    /// VmManager`.
    // TODO(clippy-1.98): unused; wire up or remove.
    #[allow(dead_code)]
    pub(crate) async fn route_owned_process_event_to_broker(
        &mut self,
        ownership: OwnershipScope,
        target: ProcessEventTarget,
    ) -> Result<bool, VmError> {
        let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
        if target.connection_id != connection_id
            || target.session_id != session_id
            || target.vm_id != vm_id
        {
            return Err(VmError::InvalidState(format!(
                "ERR_AGENTOS_PROCESS_EVENT_OWNERSHIP: target {}/{}/{} is not owned by {connection_id}/{session_id}/{vm_id}",
                target.connection_id, target.session_id, target.vm_id
            )));
        }
        self.process_event_broker
            .claim_target(&ownership, target.clone())
            .map_err(|error| VmError::InvalidState(error.to_string()))?;
        self.pump_process_events(&ownership).await?;
        let Some(envelope) =
            self.take_matching_process_event_envelope(&target.vm_id, &target.process_id)?
        else {
            return Ok(false);
        };
        match self.process_event_ingress.try_publish(envelope) {
            Ok(()) => Ok(true),
            Err(failure) => {
                // Preserve ordering and ownership if broker admission loses a
                // race with another target. The caller gets the typed limit and
                // can retry after capacity is released.
                self.queue_front_pending_process_event(failure.envelope)?;
                Err(VmError::InvalidState(failure.error.to_string()))
            }
        }
    }

    /// One bounded broker handoff without polling or awaiting a runtime. The
    /// coalesced process-event supervisor owns runtime polling; extension
    /// waiters merely route already-durable events here.
    pub(crate) fn route_owned_process_event_to_broker_nowait(
        &mut self,
        ownership: &OwnershipScope,
        target: &ProcessEventTarget,
    ) -> Result<bool, VmError> {
        let (connection_id, session_id, vm_id) = self.vm_scope_for(ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
        if target.connection_id != connection_id
            || target.session_id != session_id
            || target.vm_id != vm_id
        {
            return Err(VmError::InvalidState(format!(
                "ERR_AGENTOS_PROCESS_EVENT_OWNERSHIP: target {}/{}/{} is not owned by {connection_id}/{session_id}/{vm_id}",
                target.connection_id, target.session_id, target.vm_id
            )));
        }
        self.process_event_broker
            .claim_target(ownership, target.clone())
            .map_err(|error| VmError::InvalidState(error.to_string()))?;
        let Some(envelope) =
            self.take_matching_process_event_envelope(&target.vm_id, &target.process_id)?
        else {
            return Ok(false);
        };
        match self.process_event_ingress.try_publish(envelope) {
            Ok(()) => Ok(true),
            Err(failure) => {
                self.queue_front_pending_process_event(failure.envelope)?;
                Err(VmError::InvalidState(failure.error.to_string()))
            }
        }
    }

    /// Move every currently retained claimed-target envelope into the broker in
    /// the same coordinator turn that drained its producer. This is the
    /// durable handoff for the competing stdio wake consumer: whichever branch
    /// receives `process_event_notify` routes claimed adapter output and wakes
    /// the exact broker waiter.
    pub(crate) fn route_claimed_pending_process_events(&mut self) -> Result<usize, VmError> {
        let mut routed = 0usize;
        let mut index = 0usize;
        while index < self.pending_process_events.len() {
            let claimed = {
                let envelope = &self.pending_process_events[index];
                let target = ProcessEventTarget::new(
                    envelope.connection_id.clone(),
                    envelope.session_id.clone(),
                    envelope.vm_id.clone(),
                    envelope.process_id.clone(),
                )
                .map_err(|error| VmError::InvalidState(error.to_string()))?;
                self.process_event_broker
                    .target_is_claimed(&target)
                    .map_err(|error| VmError::InvalidState(error.to_string()))?
            };
            if !claimed {
                index += 1;
                continue;
            }
            let envelope = self
                .pending_process_events
                .remove(index)
                .expect("claimed pending process event index");
            match self.process_event_ingress.try_publish(envelope) {
                Ok(()) => routed = routed.saturating_add(1),
                Err(failure) => {
                    self.pending_process_events.insert(index, failure.envelope);
                    self.observe_pending_process_event_depth();
                    return match failure.error {
                        ProcessEventBrokerError::Limit { .. } => Ok(routed),
                        error => Err(VmError::InvalidState(error.to_string())),
                    };
                }
            }
        }
        if routed > 0 {
            self.observe_pending_process_event_depth();
            self.rearm_deferred_process_event_after_capacity_release();
        }
        Ok(routed)
    }

    pub(crate) fn queue_front_pending_process_event(
        &mut self,
        envelope: ProcessEventEnvelope,
    ) -> Result<(), VmError> {
        self.check_pending_process_event_capacity(&envelope)?;
        if matches!(&envelope.event, ActiveExecutionEvent::Exited(_)) {
            mark_execute_exit_event_queued(&envelope.vm_id, &envelope.process_id);
        }
        self.pending_process_events.push_front(envelope);
        self.observe_pending_process_event_depth();
        Ok(())
    }

    pub(crate) fn pending_process_event_capacity(&self) -> usize {
        self.config
            .runtime
            .protocol
            .max_process_events
            .saturating_sub(
                self.pending_process_events
                    .len()
                    .saturating_add(usize::from(self.deferred_process_event_envelope.is_some())),
            )
    }

    pub(crate) fn check_pending_process_event_capacity(
        &self,
        envelope: &ProcessEventEnvelope,
    ) -> Result<(), VmError> {
        self.validate_process_event_envelope_locator(envelope)?;
        let global_limit = self.config.runtime.protocol.max_process_events;
        if self
            .pending_process_events
            .len()
            .saturating_add(usize::from(self.deferred_process_event_envelope.is_some()))
            >= global_limit
        {
            return Err(process_event_queue_overflow_error(global_limit));
        }
        let limits = self
            .vms
            .get(&envelope.vm_id)
            .map(|vm| vm.limits.process.clone())
            .unwrap_or_default();
        let mut vm_count = 0usize;
        let mut vm_bytes = 0usize;
        for pending in self
            .pending_process_events
            .iter()
            .chain(self.deferred_process_event_envelope.iter())
            .filter(|pending| pending.vm_id == envelope.vm_id)
        {
            vm_count = vm_count.saturating_add(1);
            vm_bytes = vm_bytes.saturating_add(pending.retained_bytes());
        }
        if vm_count >= limits.pending_event_count {
            return Err(VmError::host_resource_limit(
                "limits.process.pendingEventCount",
                limits.pending_event_count,
                vm_count.saturating_add(1),
                format!(
                    "VM {} process event queue exceeded {} events; raise limits.process.pendingEventCount",
                    envelope.vm_id, limits.pending_event_count
                ),
            ));
        }
        let next_bytes = vm_bytes.saturating_add(envelope.retained_bytes());
        if next_bytes > limits.pending_event_bytes {
            return Err(VmError::host_resource_limit(
                "limits.process.pendingEventBytes",
                limits.pending_event_bytes,
                next_bytes,
                format!(
                    "VM {} process event queue exceeded {} retained bytes; raise limits.process.pendingEventBytes",
                    envelope.vm_id, limits.pending_event_bytes
                ),
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_process_event_envelope_locator(
        &self,
        envelope: &ProcessEventEnvelope,
    ) -> Result<(), VmError> {
        let global_limit = self.config.runtime.protocol.max_process_events;
        if envelope.child_path.len() > global_limit {
            return Err(VmError::InvalidState(format!(
                "ERR_AGENTOS_PROCESS_EVENT_PATH_LIMIT: process-event child path has {} segments, exceeding runtime.protocol.maxProcessEvents ({global_limit}); raise runtime.protocol.maxProcessEvents",
                envelope.child_path.len()
            )));
        }
        let child_path_bytes = envelope
            .child_path
            .iter()
            .try_fold(0usize, |total, segment| {
                if segment.len() > self.config.max_frame_bytes {
                    return Err(VmError::InvalidState(format!(
                        "ERR_AGENTOS_PROCESS_EVENT_PATH_LIMIT: process-event child path segment has {} bytes, exceeding maxFrameBytes ({}); raise maxFrameBytes",
                        segment.len(), self.config.max_frame_bytes
                    )));
                }
                Ok(total.saturating_add(segment.len()))
            })?;
        if child_path_bytes > self.config.max_frame_bytes {
            return Err(VmError::InvalidState(format!(
                "ERR_AGENTOS_PROCESS_EVENT_PATH_LIMIT: process-event child path has {child_path_bytes} bytes, exceeding maxFrameBytes ({}); raise maxFrameBytes",
                self.config.max_frame_bytes
            )));
        }
        Ok(())
    }

    pub(crate) fn observe_pending_process_event_depth(&self) {
        self.pending_process_events_gauge
            .observe_depth(self.pending_process_events.len());
        self.pending_process_event_bytes_gauge.observe_depth(
            self.pending_process_events
                .iter()
                .chain(self.deferred_process_event_envelope.iter())
                .fold(0usize, |bytes, event| {
                    bytes.saturating_add(event.retained_bytes())
                }),
        );
    }

    /// A deferred receiver envelope is retried only after a consumer releases
    /// public queue capacity. Never call this when initially staging it: that
    /// would turn a stable full queue into a notification hot-spin.
    pub(crate) fn rearm_deferred_process_event_after_capacity_release(&self) {
        if self.deferred_process_event_envelope.is_some() {
            self.process_event_notify.notify_one();
        }
    }

    pub fn dispatch_blocking(&mut self, request: RequestFrame) -> Result<DispatchResult, VmError> {
        let inside_runtime = tokio::runtime::Handle::try_current().is_ok();
        if !inside_runtime {
            let handle = self.process_runtime_handle()?;
            return handle.block_on(self.dispatch(request));
        }

        let mut future = std::pin::pin!(self.dispatch(request));
        match poll_future_once(future.as_mut()) {
            Some(result) => result,
            None => Err(VmError::InvalidState(String::from(
                "dispatch_blocking cannot wait for an async sidecar request inside a Tokio runtime; use dispatch().await",
            ))),
        }
    }

    pub fn dispatch_wire_blocking(
        &mut self,
        request: crate::wire::RequestFrame,
    ) -> Result<crate::wire::WireDispatchResult, VmError> {
        let request = crate::wire::request_frame_to_compat(request).map_err(wire_protocol_error)?;
        let result = self.dispatch_blocking(request)?;
        wire_dispatch_result(result)
    }

    pub fn poll_event_blocking(
        &mut self,
        ownership: &OwnershipScope,
        timeout: Duration,
    ) -> Result<Option<EventFrame>, VmError> {
        let handle = self.process_runtime_handle()?;
        handle.block_on(self.poll_event(ownership, timeout))
    }

    pub fn poll_event_wire_blocking(
        &mut self,
        ownership: &crate::wire::OwnershipScope,
        timeout: Duration,
    ) -> Result<Option<crate::wire::EventFrame>, VmError> {
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
    ) -> Result<Vec<EventFrame>, VmError> {
        let handle = self.process_runtime_handle()?;
        handle.block_on(self.close_session(connection_id, session_id))
    }

    pub fn remove_connection_blocking(
        &mut self,
        connection_id: &str,
    ) -> Result<Vec<EventFrame>, VmError> {
        let handle = self.process_runtime_handle()?;
        handle.block_on(self.remove_connection(connection_id))
    }

    pub fn dispose_vm_internal_blocking(
        &mut self,
        connection_id: &str,
        session_id: &str,
        vm_id: &str,
        reason: DisposeReason,
    ) -> Result<Vec<EventFrame>, VmError> {
        let handle = self.process_runtime_handle()?;
        handle.block_on(self.dispose_vm_internal(connection_id, session_id, vm_id, reason))
    }

    fn process_runtime_handle(&self) -> Result<tokio::runtime::Handle, VmError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(VmError::InvalidState(String::from(
                "blocking sidecar API cannot run on a Tokio worker; use the async API",
            )));
        }
        self.runtime_context
            .as_ref()
            .map(|context| context.tokio_handle().clone())
            .ok_or_else(|| {
                VmError::InvalidState(String::from(
                    "blocking sidecar API requires the process DriverHandle; construct with with_config_extensions_and_runtime or use the async API",
                ))
            })
    }

    pub(crate) fn cancel_in_process_services(&mut self, vm_id: &str) {
        self.in_process_event_services
            .retain(|service| service.vm_id != vm_id);
    }

    pub(crate) fn poll_in_process_event_services_nowait(&mut self) -> bool {
        use std::sync::atomic::Ordering;
        let visits = self.in_process_event_services.len();
        let quantum = self.config.runtime.fairness.vm_quantum_operations.max(1);
        let mut polls = 0;
        let mut progressed = false;
        for _ in 0..visits {
            if self.in_process_event_services.is_empty() || polls >= quantum {
                break;
            }
            self.in_process_event_cursor %= self.in_process_event_services.len();
            let index = self.in_process_event_cursor;
            let service = &mut self.in_process_event_services[index];
            if !service.ready.swap(false, Ordering::AcqRel) {
                self.in_process_event_cursor += 1;
                continue;
            }
            polls += 1;
            let waker = Waker::from(Arc::new(InProcessEventWake {
                notify: self.process_event_notify.clone(),
                ready: service.ready.clone(),
            }));
            let mut context = Context::from_waker(&waker);
            match service.future.as_mut().poll(&mut context) {
                Poll::Ready(result) => {
                    let mut service = self.in_process_event_services.swap_remove(index);
                    service.finish(result);
                    progressed = true;
                }
                Poll::Pending => self.in_process_event_cursor += 1,
            }
        }
        // A quantum boundary must not strand work that has not yet registered
        // a waker. Parked futures do not trigger this continuation wake.
        if self
            .in_process_event_services
            .iter()
            .any(|service| service.ready.load(Ordering::Acquire))
        {
            self.process_event_notify.notify_one();
        }
        progressed
    }

    pub(crate) fn in_process_event_service_slots(&self) -> usize {
        self.config
            .runtime
            .protocol
            .max_process_events
            .max(1)
            .saturating_sub(self.in_process_event_services.len())
    }

    pub(crate) fn retain_in_process_event_turn(
        &mut self,
        turn: crate::execution::ProcessEventPumpTurn,
    ) {
        debug_assert!(
            turn.host_services.len() + turn.child_bridge_services.len()
                <= self.in_process_event_service_slots()
        );
        for target in turn.host_services {
            let vm_id = target.vm_id.clone();
            let reply = crate::execution::internal_event_reply(&target.event);
            let future = self.prepare_owned_host_event_service(target);
            self.in_process_event_services.push(InProcessEventService {
                vm_id,
                future,
                reply,
                completed: false,
                ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            });
        }
        for target in turn.child_bridge_services {
            let vm_id = target.vm_id().to_owned();
            self.in_process_event_services.push(InProcessEventService {
                vm_id,
                future: Box::pin(crate::execution::service_owned_child_bridge_event(target)),
                reply: None,
                completed: false,
                ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            });
        }
    }

    async fn drive_direct_request_events(
        &mut self,
        request: &RequestFrame,
        target_process_id: Option<String>,
        mut operation: crate::execution::OwnedVmRouteFuture,
    ) -> Result<DispatchResult, VmError> {
        let (_, _, vm_id) = self.vm_scope_for(&request.ownership)?;
        let capacity = self.config.runtime.protocol.max_process_events.max(1);
        let notify = self.process_event_notify.clone();
        let mut events = Vec::new();
        let mut retained_event_bytes = 0usize;
        let event_byte_limit = self
            .vms
            .get(&vm_id)
            .map(|vm| vm.limits.process.pending_event_bytes)
            .unwrap_or(self.config.max_frame_bytes);
        loop {
            let ready = notify.notified();
            tokio::pin!(ready);
            ready.as_mut().enable();
            self.poll_in_process_event_services_nowait();
            let available = capacity.saturating_sub(self.in_process_event_services.len());
            if available > 0 {
                let turn = self.pump_process_events_nowait(&request.ownership, available)?;
                self.retain_in_process_event_turn(turn);
                let used = self.in_process_event_services.len();
                let near_limit = used.saturating_mul(5) >= capacity.saturating_mul(4);
                if near_limit && !self.in_process_event_limit_warned {
                    tracing::warn!(
                        used,
                        limit = capacity,
                        configuration_path = "runtime.protocol.maxProcessEvents",
                        "in-process event services approach their bound; raise runtime.protocol.maxProcessEvents to admit more concurrent services"
                    );
                }
                self.in_process_event_limit_warned = near_limit;
                // Register durable wakeups for newly claimed work before
                // waiting. Completed requests leave pending work in the manager.
                self.poll_in_process_event_services_nowait();
            }
            // A complete response wins if it and the target exit arrive in the
            // same turn. Otherwise, deliver that process's public events here:
            // the normal poll_event consumer cannot run while dispatch awaits.
            if let Some(result) = poll_future_once(operation.as_mut()) {
                return result.map(|mut dispatch| {
                    dispatch.events.extend(events);
                    dispatch
                });
            }
            if let Some(target_process_id) = target_process_id.as_deref() {
                if let Some(index) = self.pending_process_events.iter().position(|envelope| {
                    envelope.vm_id == vm_id && envelope.process_id == target_process_id
                }) {
                    let envelope = self
                        .pending_process_events
                        .remove(index)
                        .expect("matching target process event");
                    let exit_code = match &envelope.event {
                        ActiveExecutionEvent::Exited(code) => Some(*code),
                        _ => None,
                    };
                    self.observe_pending_process_event_depth();
                    self.rearm_deferred_process_event_after_capacity_release();
                    if let Some(frame) =
                        self.handle_public_process_event_envelope_nowait(envelope)?
                    {
                        let wire = crate::wire::event_frame_from_compat(frame)
                            .map_err(wire_protocol_error)?;
                        if let Some(wire) = self.event_sink.try_emit(wire)? {
                            let frame = crate::wire::event_frame_to_compat(wire)
                                .map_err(wire_protocol_error)?;
                            let bytes = serde_json::to_vec(&frame)
                                .map_err(|error| VmError::InvalidState(error.to_string()))?
                                .len();
                            if events.len() >= capacity
                                || retained_event_bytes.saturating_add(bytes) > event_byte_limit
                            {
                                let error = VmError::InvalidState(format!(
                                    "ERR_AGENTOS_VM_FETCH_EVENT_LIMIT: vm.fetch retained events exceeded runtime.protocol.maxProcessEvents ({capacity}) or limits.process.pendingEventBytes ({event_byte_limit}); raise the corresponding limit"
                                ));
                                return Ok(DispatchResult {
                                    response: self.reject_error(request, &error),
                                    events,
                                });
                            }
                            retained_event_bytes = retained_event_bytes.saturating_add(bytes);
                            events.push(frame);
                        }
                    }
                    if let Some(exit_code) = exit_code {
                        let still_active = self
                            .vms
                            .get(&vm_id)
                            .is_some_and(|vm| vm.active_processes.contains_key(target_process_id));
                        if !still_active {
                            let error = VmError::Execution(format!(
                                "vm.fetch target exited before responding (exit code {exit_code})"
                            ));
                            return Ok(DispatchResult {
                                response: self.reject_error(request, &error),
                                events,
                            });
                        }
                    }
                    continue;
                }
                // A separate claimed-event waiter may have consumed the exit
                // envelope and completed teardown already. The fetch must not
                // wait for an envelope that no longer belongs to this queue.
                if self
                    .vms
                    .get(&vm_id)
                    .is_none_or(|vm| !vm.active_processes.contains_key(target_process_id))
                {
                    let error = VmError::Execution(String::from(
                        "vm.fetch target exited before responding",
                    ));
                    return Ok(DispatchResult {
                        response: self.reject_error(request, &error),
                        events,
                    });
                }
            }
            tokio::select! {
                biased;
                result = &mut operation => return result.map(|mut dispatch| {
                    dispatch.events.extend(events);
                    dispatch
                }),
                () = &mut ready => {},
            }
        }
    }

    pub async fn dispatch(&mut self, request: RequestFrame) -> Result<DispatchResult, VmError> {
        self.poll_in_process_event_services_nowait();
        self.reap_reconciled_quarantined_vms();
        if let Err(error) = self.ensure_request_within_frame_limit(&request) {
            return Ok(DispatchResult {
                response: self.reject_error(&request, &error),
                events: Vec::new(),
            });
        }

        let route = route_request_payload(&request);
        if !matches!(&route, RequestRoute::DisposeVm(_)) {
            if let OwnershipScope::VmOwnership(ownership) = &request.ownership {
                if let Some(report) = self
                    .vms
                    .get(&ownership.vm_id)
                    .and_then(|vm| vm.runtime_context.terminal_failure())
                {
                    let error = VmError::Execution(format!(
                        "ERR_AGENTOS_VM_TASK_FAILED: vm_id={} class={:?} owner={} reason={:?}; dispose and recreate this VM generation",
                        ownership.vm_id, report.class, report.owner, report.reason
                    ));
                    return Ok(DispatchResult {
                        response: self.reject_error(&request, &error),
                        events: Vec::new(),
                    });
                }
            }
        }

        let result = match route {
            RequestRoute::Authenticate(payload) => self.authenticate_connection(&request, payload),
            RequestRoute::OpenSession(payload) => self.open_session(&request, payload),
            RequestRoute::CreateVm(payload) => self.create_vm(&request, payload).await,
            RequestRoute::CompareVmConfig(payload) => self.compare_vm_config(&request, payload),
            RequestRoute::DisposeVm(payload) => self.dispose_vm(&request, payload).await,
            RequestRoute::BootstrapRootFilesystem(payload) => {
                self.bootstrap_root_filesystem(&request, payload.entries)
                    .await
            }
            RequestRoute::ConfigureVm(payload) => self.configure_vm(&request, payload).await,
            RequestRoute::RegisterHostCallbacks(payload) => {
                register_host_callbacks(self, &request, payload).await
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
            RequestRoute::ListMounts(payload) => self.list_mounts(&request, payload).await,
            RequestRoute::ExecutionOperation(payload) => {
                let effects = self.request_completion_effects(&request);
                let result = self
                    .execute_language_operation(&request, payload, effects.clone())
                    .await;
                self.apply_request_completion_effects(&effects);
                result
            }
            RequestRoute::ExecutionLifecycle(payload) => {
                let effects = self.request_completion_effects(&request);
                let result = self
                    .handle_execution_lifecycle(&request, payload, effects.clone())
                    .await;
                self.apply_request_completion_effects(&effects);
                result
            }
            RequestRoute::Execute(payload) => self.execute(&request, payload).await,
            RequestRoute::WriteStdin(payload) => self.write_stdin(&request, payload).await,
            RequestRoute::ResizePty(payload) => self.resize_pty(&request, payload).await,
            RequestRoute::CloseStdin(payload) => self.close_stdin(&request, payload).await,
            RequestRoute::KillProcess(payload) => self.kill_process(&request, payload).await,
            RequestRoute::ReadProcessOutput(payload) => {
                self.read_process_output(&request, payload).await
            }
            RequestRoute::GetProcessSnapshot(payload) => {
                self.get_process_snapshot(&request, payload).await
            }
            RequestRoute::GetResourceSnapshot(payload) => {
                self.get_resource_snapshot(&request, payload).await
            }
            RequestRoute::FindListener(payload) => self.find_listener(&request, payload).await,
            RequestRoute::FindBoundUdp(payload) => self.find_bound_udp(&request, payload).await,
            RequestRoute::VmFetch(payload) => {
                let (_, _, vm_id) = self.vm_scope_for(&request.ownership)?;
                let target_process_id = self.vms.get(&vm_id).and_then(|vm| {
                    if payload.stream_operation.as_deref() == Some("read") {
                        payload.stream_id.as_ref().and_then(|stream_id| {
                            vm.vm_fetch_streams
                                .get(stream_id)
                                .map(|stream| stream.target_process_id.clone())
                        })
                    } else if payload.stream_operation.as_deref() == Some("cancel") {
                        None
                    } else {
                        crate::execution::find_vm_fetch_target_process(&vm, payload.port)
                    }
                });
                let future = self.vm_fetch(&request, payload);
                self.drive_direct_request_events(&request, target_process_id, future)
                    .await
            }
            RequestRoute::GetSignalState(payload) => self.get_signal_state(&request, payload).await,
            RequestRoute::GetZombieTimerCount(payload) => {
                self.get_zombie_timer_count(&request, payload).await
            }
            RequestRoute::LinkPackage(payload) => self.link_package(&request, payload).await,
            RequestRoute::InstallPackage(payload) => self.install_package(&request, payload).await,
            RequestRoute::GetPackageCacheStats(_) => {
                let result = self.session_scope_for(&request.ownership).and_then(
                    |(connection_id, session_id)| {
                        self.require_owned_session(&connection_id, &session_id)
                    },
                );
                result?;
                package_cache_stats_owned(request.clone()).await
            }
            RequestRoute::UnlinkPackage(payload) => self.unlink_package(&request, payload).await,
            RequestRoute::AcquirePackage(payload) => {
                let result = self.session_scope_for(&request.ownership).and_then(
                    |(connection_id, session_id)| {
                        self.require_owned_session(&connection_id, &session_id)
                    },
                );
                match result {
                    Ok(()) => acquire_package_owned(request.clone(), payload).await,
                    Err(error) => Err(error),
                }
            }
            RequestRoute::ProvidedCommands(payload) => {
                self.provided_commands(&request, payload).await
            }
            RequestRoute::UnsupportedHostCallbackDirection => {
                Ok(unsupported_host_callback_direction_dispatch(&request))
            }
            RequestRoute::Ext(payload) => self.dispatch_extension_request(&request, payload).await,
        };

        match result {
            Ok(dispatch) => Ok(dispatch),
            Err(error @ VmError::Io(_)) => Err(error),
            Err(error) => Ok(DispatchResult {
                response: self.reject_error(&request, &error),
                events: Vec::new(),
            }),
        }
    }

    pub async fn dispatch_wire(
        &mut self,
        request: crate::wire::RequestFrame,
    ) -> Result<crate::wire::WireDispatchResult, VmError> {
        let request = crate::wire::request_frame_to_compat(request).map_err(wire_protocol_error)?;
        let result = self.dispatch(request).await?;
        wire_dispatch_result(result)
    }

    /// Prepare a non-extension request that can finish without retaining the
    /// process coordinator. Returns `Ok(None)` when the route still requires
    /// entity-owned mutable state and must use the coordinated dispatch path.
    ///
    /// Immutable queries are snapshotted synchronously. Mutable VM filesystem
    /// and kernel calls clone only their VM handle and execute under that VM's
    /// short critical section, so neither route retains the process
    /// coordinator or blocks operations owned by another VM.
    pub fn prepare_request_wire(
        &mut self,
        request: crate::wire::RequestFrame,
    ) -> Result<Option<PreparedRequest>, VmError> {
        let request = crate::wire::request_frame_to_compat(request).map_err(wire_protocol_error)?;
        let route = route_request_payload(&request);
        let detachable = matches!(
            &route,
            RequestRoute::Authenticate(_)
                | RequestRoute::OpenSession(_)
                | RequestRoute::RegisterHostCallbacks(_)
                | RequestRoute::ReadProcessOutput(_)
                | RequestRoute::GetProcessSnapshot(_)
                | RequestRoute::GetResourceSnapshot(_)
                | RequestRoute::GetZombieTimerCount(_)
                | RequestRoute::ProvidedCommands(_)
                | RequestRoute::ListMounts(_)
                | RequestRoute::GuestFilesystemCall(_)
                | RequestRoute::GuestKernelCall(_)
                | RequestRoute::BootstrapRootFilesystem(_)
                | RequestRoute::ConfigureVm(_)
                | RequestRoute::CreateLayer(_)
                | RequestRoute::SealLayer(_)
                | RequestRoute::ImportSnapshot(_)
                | RequestRoute::ExportSnapshot(_)
                | RequestRoute::CreateOverlay(_)
                | RequestRoute::SnapshotRootFilesystem(_)
                | RequestRoute::LinkPackage(_)
                | RequestRoute::InstallPackage(_)
                | RequestRoute::GetPackageCacheStats(_)
                | RequestRoute::UnlinkPackage(_)
                | RequestRoute::AcquirePackage(_)
                | RequestRoute::Execute(_)
                | RequestRoute::ExecutionOperation(_)
                | RequestRoute::ExecutionLifecycle(_)
                | RequestRoute::WriteStdin(_)
                | RequestRoute::ResizePty(_)
                | RequestRoute::CloseStdin(_)
                | RequestRoute::KillProcess(_)
                | RequestRoute::FindListener(_)
                | RequestRoute::FindBoundUdp(_)
                | RequestRoute::VmFetch(_)
                | RequestRoute::GetSignalState(_)
                | RequestRoute::UnsupportedHostCallbackDirection
        ) || matches!(
            &route,
            RequestRoute::Ext(envelope) if !self.extensions.contains_key(&envelope.namespace)
        );
        if !detachable {
            return Ok(None);
        }

        self.reap_reconciled_quarantined_vms();
        let preparation = (|| {
            self.ensure_request_within_frame_limit(&request)?;
            if let OwnershipScope::VmOwnership(ownership) = &request.ownership {
                if let Some(report) = self
                    .vms
                    .get(&ownership.vm_id)
                    .and_then(|vm| vm.runtime_context.terminal_failure())
                {
                    return Err(VmError::Execution(format!(
                        "ERR_AGENTOS_VM_TASK_FAILED: vm_id={} class={:?} owner={} reason={:?}; dispose and recreate this VM generation",
                        ownership.vm_id, report.class, report.owner, report.reason
                    )));
                }
            }
            Ok(())
        })();
        if let Err(error) = preparation {
            return Ok(Some(PreparedRequest::from_future(request, async move {
                Err(error)
            })));
        }

        match route {
            RequestRoute::Authenticate(payload) => {
                if let Err(error) = self.connection_id_for(&request.ownership) {
                    return Ok(Some(PreparedRequest::failed(request, error)));
                }
                if let Err(error) = self.validate_auth_token(&payload.auth_token) {
                    let bridge = self.bridge.clone();
                    let sidecar_id = self.config.instance_id.clone();
                    let client_name = payload.client_name;
                    let ownership = request.ownership.clone();
                    return Ok(Some(PreparedRequest::from_future(request, async move {
                        let mut fields = audit_fields([
                            (String::from("source"), client_name),
                            (String::from("reason"), error.to_string()),
                        ]);
                        if let OwnershipScope::ConnectionOwnership(inner) = ownership {
                            fields.insert(String::from("connection_id"), inner.connection_id);
                        }
                        emit_security_audit_event(
                            &bridge,
                            &sidecar_id,
                            "security.auth.failed",
                            fields,
                        );
                        Err(error)
                    })));
                }
                if let Err(error) =
                    validate_authenticate_versions(&payload).map_err(|error| match error {
                        AuthenticateVersionError::ProtocolVersionMismatch(message) => {
                            VmError::ProtocolVersionMismatch(message)
                        }
                        AuthenticateVersionError::BridgeVersionMismatch(message) => {
                            VmError::BridgeVersionMismatch(message)
                        }
                    })
                {
                    return Ok(Some(PreparedRequest::failed(request, error)));
                }
                let Some(connection_number) = self.next_connection_id.checked_add(1) else {
                    return Ok(Some(PreparedRequest::failed(
                        request,
                        VmError::InvalidState(String::from(
                            "ERR_AGENTOS_CONNECTION_ID_EXHAUSTED: connection identifier space exhausted",
                        )),
                    )));
                };
                let connection_id = format!("conn-{connection_number}");
                let response = shared_authenticated_response(
                    request.request_id,
                    self.config.instance_id.clone(),
                    connection_id.clone(),
                    self.config.max_frame_bytes as u32,
                );
                let membership = PreparedMembershipCommit::Connection {
                    connection_id,
                    auth_token: payload.auth_token,
                };
                Ok(Some(PreparedRequest::from_future_with_membership(
                    request,
                    async move {
                        Ok(DispatchResult {
                            response,
                            events: Vec::new(),
                        })
                    },
                    membership,
                )))
            }
            RequestRoute::OpenSession(payload) => {
                let connection_id = match self.connection_id_for(&request.ownership) {
                    Ok(connection_id) => connection_id,
                    Err(error) => return Ok(Some(PreparedRequest::failed(request, error))),
                };
                if let Err(error) = self.require_authenticated_connection(&connection_id) {
                    return Ok(Some(PreparedRequest::failed(request, error)));
                }
                let Some(session_number) = self.next_session_id.checked_add(1) else {
                    return Ok(Some(PreparedRequest::failed(
                        request,
                        VmError::InvalidState(String::from(
                            "ERR_AGENTOS_SESSION_ID_EXHAUSTED: session identifier space exhausted",
                        )),
                    )));
                };
                let session_id = format!("session-{session_number}");
                let response = session_opened_response(
                    request.request_id,
                    connection_id.clone(),
                    session_id.clone(),
                );
                let membership = PreparedMembershipCommit::Session {
                    connection_id,
                    session_id,
                    placement: payload.placement,
                    metadata: payload.metadata.into_iter().collect(),
                };
                Ok(Some(PreparedRequest::from_future_with_membership(
                    request,
                    async move {
                        Ok(DispatchResult {
                            response,
                            events: Vec::new(),
                        })
                    },
                    membership,
                )))
            }
            RequestRoute::RegisterHostCallbacks(payload) => {
                let future = register_host_callbacks(self, &request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::ReadProcessOutput(payload) => {
                let future = self.read_process_output(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::GetProcessSnapshot(payload) => {
                let future = self.get_process_snapshot(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::GetResourceSnapshot(payload) => {
                let future = self.get_resource_snapshot(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::GetZombieTimerCount(payload) => {
                let future = self.get_zombie_timer_count(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::ProvidedCommands(payload) => {
                let future = self.provided_commands(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::ListMounts(payload) => {
                let future = self.list_mounts(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::BootstrapRootFilesystem(payload) => {
                let future = self.bootstrap_root_filesystem(&request, payload.entries);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::ConfigureVm(payload) => {
                let future = self.configure_vm(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::CreateLayer(payload) => {
                let future = self.create_layer(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::SealLayer(payload) => {
                let future = self.seal_layer(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::ImportSnapshot(payload) => {
                let future = self.import_snapshot(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::ExportSnapshot(payload) => {
                let future = self.export_snapshot(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::CreateOverlay(payload) => {
                let future = self.create_overlay(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::SnapshotRootFilesystem(payload) => {
                let future = self.snapshot_root_filesystem(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::LinkPackage(payload) => {
                let future = self.link_package(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::InstallPackage(payload) => {
                let future = self.install_package(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::UnlinkPackage(payload) => {
                let future = self.unlink_package(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::AcquirePackage(payload) => {
                let ownership = self.session_scope_for(&request.ownership).and_then(
                    |(connection_id, session_id)| {
                        self.require_owned_session(&connection_id, &session_id)
                    },
                );
                if let Err(error) = ownership {
                    return Ok(Some(PreparedRequest::failed(request, error)));
                }
                let operation = acquire_package_owned(request.clone(), payload);
                Ok(Some(PreparedRequest::from_future(request, operation)))
            }
            RequestRoute::GetPackageCacheStats(_) => {
                let ownership = self.session_scope_for(&request.ownership).and_then(
                    |(connection_id, session_id)| {
                        self.require_owned_session(&connection_id, &session_id)
                    },
                );
                if let Err(error) = ownership {
                    return Ok(Some(PreparedRequest::failed(request, error)));
                }
                let operation = package_cache_stats_owned(request.clone());
                Ok(Some(PreparedRequest::from_future(request, operation)))
            }
            RequestRoute::Execute(payload) => {
                let future = self.execute(&request, payload);
                Ok(Some(PreparedRequest::from_future(request, future)))
            }
            RequestRoute::ExecutionOperation(payload) => {
                let effects = self.request_completion_effects(&request);
                let future = self.execute_language_operation(&request, payload, effects.clone());
                Ok(Some(PreparedRequest::from_future_with_effects(
                    request, future, effects,
                )))
            }
            RequestRoute::ExecutionLifecycle(payload) => {
                let effects = self.request_completion_effects(&request);
                let future = self.handle_execution_lifecycle(&request, payload, effects.clone());
                Ok(Some(PreparedRequest::from_future_with_effects(
                    request, future, effects,
                )))
            }
            RequestRoute::WriteStdin(payload) => {
                let future = self.write_stdin(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::ResizePty(payload) => {
                let future = self.resize_pty(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::CloseStdin(payload) => {
                let future = self.close_stdin(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::KillProcess(payload) => {
                let future = self.kill_process(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::FindListener(payload) => {
                let future = self.find_listener(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::FindBoundUdp(payload) => {
                let future = self.find_bound_udp(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::VmFetch(payload) => {
                let future = self.vm_fetch(&request, payload);
                Ok(Some(PreparedRequest::from_future(request, future)))
            }
            RequestRoute::GetSignalState(payload) => {
                let future = self.get_signal_state(&request, payload);
                Ok(Some(PreparedRequest::from_vm_command(request, future)))
            }
            RequestRoute::GuestFilesystemCall(payload) => {
                let (connection_id, session_id, vm_id) = match self.vm_scope_for(&request.ownership)
                {
                    Ok(scope) => scope,
                    Err(error) => return Ok(Some(PreparedRequest::failed(request, error))),
                };
                if let Err(error) = self.require_owned_vm(&connection_id, &session_id, &vm_id) {
                    return Ok(Some(PreparedRequest::failed(request, error)));
                }
                let Some(handle) = self.vms.handle(&vm_id) else {
                    return Ok(Some(PreparedRequest::failed(
                        request,
                        VmError::InvalidState(format!(
                            "VM {vm_id} no longer exists for guest filesystem call"
                        )),
                    )));
                };
                let response_request = request.clone();
                Ok(Some(PreparedRequest::from_vm_command(
                    request,
                    async move {
                        let response = handle.try_command("guest filesystem call", |vm| {
                            guest_filesystem_call_vm(vm, &payload)
                        })?;
                        Ok(DispatchResult {
                            response: shared_respond(
                                &response_request,
                                ResponsePayload::GuestFilesystemResult(response),
                            ),
                            events: Vec::new(),
                        })
                    },
                )))
            }
            RequestRoute::GuestKernelCall(payload) => {
                let (connection_id, session_id, vm_id) = match self.vm_scope_for(&request.ownership)
                {
                    Ok(scope) => scope,
                    Err(error) => return Ok(Some(PreparedRequest::failed(request, error))),
                };
                if let Err(error) = self.require_owned_vm(&connection_id, &session_id, &vm_id) {
                    return Ok(Some(PreparedRequest::failed(request, error)));
                }
                let Some(handle) = self.vms.handle(&vm_id) else {
                    return Ok(Some(PreparedRequest::failed(
                        request,
                        VmError::InvalidState(format!(
                            "VM {vm_id} no longer exists for guest kernel call"
                        )),
                    )));
                };
                let response_request = request.clone();
                Ok(Some(PreparedRequest::from_vm_command(
                    request,
                    async move {
                        let response = handle.try_command("guest kernel call", |vm| {
                            let kernel_pid = vm
                                .active_processes
                                .get(&payload.execution_id)
                                .map(|process| process.kernel_pid)
                                .ok_or_else(|| {
                                    VmError::InvalidState(format!(
                                        "VM {vm_id} has no active process {} for guest kernel call",
                                        payload.execution_id
                                    ))
                                })?;
                            crate::core::handle_guest_kernel_call(
                                &mut vm.kernel,
                                kernel_pid,
                                EXECUTION_DRIVER_NAME,
                                &payload.operation,
                                &payload.payload,
                            )
                            .map_err(crate::execution::guest_kernel_core_error)
                        })?;
                        Ok(DispatchResult {
                            response: shared_respond(
                                &response_request,
                                ResponsePayload::GuestKernelResult(
                                    crate::protocol::GuestKernelResultResponse {
                                        payload: response,
                                    },
                                ),
                            ),
                            events: Vec::new(),
                        })
                    },
                )))
            }
            RequestRoute::UnsupportedHostCallbackDirection => {
                let response_request = request.clone();
                Ok(Some(PreparedRequest::from_future(request, async move {
                    Ok(unsupported_host_callback_direction_dispatch(
                        &response_request,
                    ))
                })))
            }
            RequestRoute::Ext(envelope) => {
                let response_request = request.clone();
                Ok(Some(PreparedRequest::from_future(request, async move {
                    Ok(DispatchResult {
                        response: shared_reject(
                            &response_request,
                            "unknown_extension",
                            &format!(
                                "no extension registered for namespace {}",
                                envelope.namespace
                            ),
                        ),
                        events: Vec::new(),
                    })
                })))
            }
            RequestRoute::CreateVm(_)
            | RequestRoute::CompareVmConfig(_)
            | RequestRoute::DisposeVm(_) => {
                unreachable!("VM creation and disposal use dedicated prepared routes")
            }
        }
    }

    /// Finalize a detached non-extension request without awaiting business work.
    fn request_completion_effects(&self, request: &RequestFrame) -> RequestCompletionEffects {
        let limit = match &request.ownership {
            OwnershipScope::VmOwnership(ownership) => self
                .vms
                .get(&ownership.vm_id)
                .map(|vm| {
                    vm.limits
                        .execution
                        .max_completed_executions
                        .saturating_add(1)
                })
                .unwrap_or(1),
            OwnershipScope::ConnectionOwnership(_) | OwnershipScope::SessionOwnership(_) => 1,
        };
        RequestCompletionEffects::new(limit, "limits.execution.maxCompletedExecutions")
    }

    fn apply_request_completion_effects(&mut self, effects: &RequestCompletionEffects) {
        for (process_id, detached_process_ids) in effects.take_exited_processes() {
            self.transfer_extension_process_resource(&process_id, &detached_process_ids);
        }
    }

    pub fn complete_request(
        &mut self,
        completed: CompletedRequest,
    ) -> Result<crate::wire::WireDispatchResult, VmError> {
        let CompletedRequest {
            request,
            result,
            effects,
            committed_membership,
        } = completed;
        if result.is_ok() {
            if let Some(membership) = committed_membership.as_ref() {
                self.commit_prepared_membership(membership)?;
            }
        }
        self.apply_request_completion_effects(&effects);
        let dispatch = match result {
            Ok(dispatch) => dispatch,
            Err(error @ VmError::Io(_)) => return Err(error),
            Err(error) => DispatchResult {
                response: self.reject_error(&request, &error),
                events: Vec::new(),
            },
        };
        wire_dispatch_result(dispatch)
    }

    pub async fn poll_event_wire(
        &mut self,
        ownership: &crate::wire::OwnershipScope,
        timeout: Duration,
    ) -> Result<Option<crate::wire::EventFrame>, VmError> {
        let ownership = crate::wire::ownership_scope_to_compat(ownership.clone());
        self.poll_event(&ownership, timeout)
            .await?
            .map(crate::wire::event_frame_from_compat)
            .transpose()
            .map_err(wire_protocol_error)
    }

    /// Detach an extension request from the process coordinator so its long
    /// wait can run under the request supervisor. Returns `Ok(None)` for
    /// non-extension and unknown-extension requests; those retain the normal
    /// dispatch path, including its canonical unknown-extension rejection.
    pub fn prepare_extension_request_wire(
        &self,
        request: crate::wire::RequestFrame,
        services: Arc<dyn ExtensionServices>,
    ) -> Result<Option<PreparedExtensionRequest>, VmError> {
        let request = crate::wire::request_frame_to_compat(request).map_err(wire_protocol_error)?;
        let RequestPayload::Ext(envelope) = &request.payload else {
            return Ok(None);
        };
        let Some(extension) = self.extensions.get(&envelope.namespace).cloned() else {
            return Ok(None);
        };
        let namespace = envelope.namespace.clone();
        let payload = envelope.payload.clone();
        let snapshot = ExtensionSnapshot::new(
            namespace.clone(),
            request.ownership.clone(),
            self.sidecar_requests.clone(),
            self.event_sink.clone(),
        );
        Ok(Some(PreparedExtensionRequest {
            request,
            namespace,
            payload,
            extension,
            services,
            snapshot,
        }))
    }

    /// Convert a detached extension task's result into the same canonical wire
    /// response used by inline dispatch. This is deliberately short and never
    /// waits on extension work or output capacity.
    pub fn complete_extension_request(
        &self,
        completed: CompletedExtensionRequest,
    ) -> Result<crate::wire::WireDispatchResult, VmError> {
        let CompletedExtensionRequest {
            request,
            namespace,
            result,
        } = completed;
        let dispatch = match result {
            Ok(response) => DispatchResult {
                response: self.respond(
                    &request,
                    ResponsePayload::ExtResult(ExtEnvelope {
                        namespace,
                        payload: response.payload,
                    }),
                ),
                events: response.events,
            },
            Err(error @ VmError::Io(_)) => return Err(error),
            Err(error) => DispatchResult {
                response: self.reject_error(&request, &error),
                events: Vec::new(),
            },
        };
        wire_dispatch_result(dispatch)
    }

    pub fn reject_wire_request_error(
        &self,
        request: crate::wire::RequestFrame,
        error: &VmError,
    ) -> Result<crate::wire::WireDispatchResult, VmError> {
        let request = crate::wire::request_frame_to_compat(request).map_err(wire_protocol_error)?;
        wire_dispatch_result(DispatchResult {
            response: self.reject_error(&request, error),
            events: Vec::new(),
        })
    }

    async fn dispatch_extension_request(
        &mut self,
        request: &RequestFrame,
        envelope: ExtEnvelope,
    ) -> Result<DispatchResult, VmError> {
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
        let services = self.extension_services.clone().ok_or_else(|| {
            VmError::InvalidState(String::from(
                "ERR_AGENTOS_EXTENSION_SERVICES_UNAVAILABLE: extension dispatch requires cloneable owned services",
            ))
        })?;
        let ctx = ExtensionContext::with_services(snapshot, services);
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

    pub async fn poll_event(
        &mut self,
        ownership: &OwnershipScope,
        timeout: Duration,
    ) -> Result<Option<EventFrame>, VmError> {
        let deadline = Instant::now() + timeout;
        let process_event_notify = Arc::clone(&self.process_event_notify);
        loop {
            // Register before probing durable queues so a producer racing the
            // probe cannot lose its edge between the empty check and await.
            let notified = process_event_notify.notified();
            if let Some(index) = self
                .pending_process_events
                .iter()
                .position(|event| public_process_event_matches_ownership(self, ownership, event))
            {
                let Some(envelope) = self.pending_process_events.remove(index) else {
                    continue;
                };
                self.observe_pending_process_event_depth();
                self.rearm_deferred_process_event_after_capacity_release();
                if let Some(frame) = self.handle_process_event_envelope(envelope).await? {
                    return Ok(Some(frame));
                }
                continue;
            }

            if !timeout.is_zero() && self.pump_process_events(ownership).await? {
                // The pump moves execution events into durable sidecar queues.
                // Re-probe those queues before waiting for another edge: the
                // notification that brought us here may be the only edge for
                // this event, and waiting now would strand it until unrelated
                // later activity.
                continue;
            }

            // Runtime producers share this channel with public output. Always
            // classify internal work before publishing any envelope.
            if self.drain_runtime_process_event_channel_nowait()? {
                continue;
            }

            if Instant::now() >= deadline {
                return Ok(None);
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            tokio::select! {
                _ = notified => {}
                _ = time::sleep(remaining) => return Ok(None),
            }
        }
    }

    /// Probe durable public event queues once without awaiting runtime work.
    /// Runtime polling is owned by the coalesced process-event supervisor.
    pub fn poll_event_nowait(
        &mut self,
        ownership: &OwnershipScope,
    ) -> Result<Option<EventFrame>, VmError> {
        let mut drained_channel = false;
        loop {
            if let Some(index) = self
                .pending_process_events
                .iter()
                .position(|event| public_process_event_matches_ownership(self, ownership, event))
            {
                let Some(envelope) = self.pending_process_events.remove(index) else {
                    continue;
                };
                self.observe_pending_process_event_depth();
                self.rearm_deferred_process_event_after_capacity_release();
                if let Some(frame) = self.handle_public_process_event_envelope_nowait(envelope)? {
                    return Ok(Some(frame));
                }
                continue;
            }

            if drained_channel {
                return Ok(None);
            }
            drained_channel = true;
            if !self.drain_runtime_process_event_channel_nowait()? {
                return Ok(None);
            }
        }
    }

    /// Borrowed-host compatibility path for process-targeted polling. The stdio
    /// extension path uses the broker-backed owned service instead, so its
    /// timeout never retains the sidecar coordinator.
    pub async fn poll_process_event(
        &mut self,
        ownership: &OwnershipScope,
        process_id: &str,
        timeout: Duration,
    ) -> Result<Option<EventFrame>, VmError> {
        let target = ProcessEventTarget::for_owned_process(ownership, process_id)
            .map_err(|error| VmError::InvalidState(error.to_string()))?;
        let (connection_id, session_id, vm_id) = self.vm_scope_for(ownership)?;
        self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
        let deadline = Instant::now() + timeout;
        let process_event_notify = Arc::clone(&self.process_event_notify);
        loop {
            let notified = process_event_notify.notified();
            self.pump_process_events(ownership).await?;
            while let Some(envelope) =
                self.take_matching_process_event_envelope(&target.vm_id, &target.process_id)?
            {
                if let Some(frame) = self.handle_process_event_envelope(envelope).await? {
                    return Ok(Some(frame));
                }
            }
            if timeout.is_zero() || Instant::now() >= deadline {
                return Ok(None);
            }
            tokio::select! {
                _ = notified => {}
                _ = time::sleep(deadline.saturating_duration_since(Instant::now())) => {
                    return Ok(None);
                }
            }
        }
    }

    pub(crate) async fn handle_process_event_envelope(
        &mut self,
        envelope: ProcessEventEnvelope,
    ) -> Result<Option<EventFrame>, VmError> {
        let handle_start = Instant::now();
        let ProcessEventEnvelope {
            connection_id,
            session_id,
            vm_id,
            child_path,
            process_id,
            event,
        } = envelope;

        let is_exit_event = Self::terminal_execution_event(&event);

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
                    && !Self::terminal_execution_event(&pending.event)
                {
                    trailing.push(pending.event);
                } else {
                    deferred.push_back(pending);
                }
            }
            self.pending_process_events = deferred;
            self.observe_pending_process_event_depth();
            self.rearm_deferred_process_event_after_capacity_release();
            record_execute_phase("process_exit_trailing_pending_scan", phase_start.elapsed());
            if !trailing.is_empty() {
                if self.pending_process_event_capacity() < trailing.len() {
                    return Err(process_event_queue_overflow_error(
                        self.config.runtime.protocol.max_process_events,
                    ));
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
                    child_path: child_path.clone(),
                    process_id: process_id.clone(),
                    event,
                })?;
                for event in trailing.into_iter().rev() {
                    self.queue_front_pending_process_event(ProcessEventEnvelope {
                        connection_id: connection_id.clone(),
                        session_id: session_id.clone(),
                        vm_id: vm_id.clone(),
                        child_path: child_path.clone(),
                        process_id: process_id.clone(),
                        event,
                    })?;
                }
                record_execute_phase("process_exit_trailing_requeue", phase_start.elapsed());
                if let Some(event) = emit_now {
                    let result = self
                        .handle_execution_event(&vm_id, &process_id, event)
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
            .handle_execution_event(&vm_id, &process_id, event)
            .await;
        if is_exit_event {
            let target = ProcessEventTarget::new(
                connection_id,
                session_id,
                vm_id.clone(),
                process_id.clone(),
            )
            .map_err(|error| VmError::InvalidState(error.to_string()))?;
            if let Err(error) = self
                .process_event_broker
                .dispose_process(&target, OperationCancellationReason::Explicit)
            {
                eprintln!(
                    "ERR_AGENTOS_PROCESS_EVENT_PROCESS_DISPOSAL: vm_id={vm_id} process_id={process_id} error={error}"
                );
            }
            record_execute_phase(
                "process_exit_event_handle_envelope_total",
                handle_start.elapsed(),
            );
        }
        result
    }

    /// Synchronous completion-side form used by the detached extension/event
    /// supervisors. Broker envelopes contain only public events; internal RPC
    /// work is serviced before publication.
    pub(crate) fn handle_public_process_event_envelope_nowait(
        &mut self,
        envelope: ProcessEventEnvelope,
    ) -> Result<Option<EventFrame>, VmError> {
        let ProcessEventEnvelope {
            connection_id,
            session_id,
            vm_id,
            child_path,
            process_id,
            event,
        } = envelope;
        let is_exit_event = Self::terminal_execution_event(&event);

        if is_exit_event {
            record_execute_exit_event_queue_wait(
                "process_exit_event_queue_wait",
                &vm_id,
                &process_id,
            );
            let mut trailing = Vec::new();
            let mut deferred = VecDeque::new();
            while let Some(pending) = self.pending_process_events.pop_front() {
                if pending.vm_id == vm_id
                    && pending.process_id == process_id
                    && !Self::terminal_execution_event(&pending.event)
                {
                    trailing.push(pending.event);
                } else {
                    deferred.push_back(pending);
                }
            }
            self.pending_process_events = deferred;
            self.observe_pending_process_event_depth();
            self.rearm_deferred_process_event_after_capacity_release();
            if !trailing.is_empty() {
                if self.pending_process_event_capacity() < trailing.len() {
                    return Err(process_event_queue_overflow_error(
                        self.config.runtime.protocol.max_process_events,
                    ));
                }
                let emit_now = if self.pending_process_event_capacity() == trailing.len() {
                    Some(trailing.remove(0))
                } else {
                    None
                };
                mark_execute_exit_event_queued(&vm_id, &process_id);
                self.queue_front_pending_process_event(ProcessEventEnvelope {
                    connection_id: connection_id.clone(),
                    session_id: session_id.clone(),
                    vm_id: vm_id.clone(),
                    child_path: child_path.clone(),
                    process_id: process_id.clone(),
                    event,
                })?;
                for event in trailing.into_iter().rev() {
                    self.queue_front_pending_process_event(ProcessEventEnvelope {
                        connection_id: connection_id.clone(),
                        session_id: session_id.clone(),
                        vm_id: vm_id.clone(),
                        child_path: child_path.clone(),
                        process_id: process_id.clone(),
                        event,
                    })?;
                }
                return emit_now.map_or(Ok(None), |event| {
                    self.handle_public_execution_event_nowait(&vm_id, &process_id, event)
                });
            }
        }

        let result = self.handle_public_execution_event_nowait(&vm_id, &process_id, event);
        if is_exit_event {
            let target = ProcessEventTarget::new(
                connection_id,
                session_id,
                vm_id.clone(),
                process_id.clone(),
            )
            .map_err(|error| VmError::InvalidState(error.to_string()))?;
            if let Err(error) = self
                .process_event_broker
                .dispose_process(&target, OperationCancellationReason::Explicit)
            {
                eprintln!(
                    "ERR_AGENTOS_PROCESS_EVENT_PROCESS_DISPOSAL: vm_id={vm_id} process_id={process_id} error={error}"
                );
            }
        }
        result
    }

    // try_poll_event moved to crate::execution

    pub async fn close_session(
        &mut self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<Vec<EventFrame>, VmError> {
        self.dispose_session(connection_id, session_id, DisposeReason::Requested)
            .await
    }

    pub async fn remove_connection(
        &mut self,
        connection_id: &str,
    ) -> Result<Vec<EventFrame>, VmError> {
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
        let mut first_error: Option<VmError> = None;
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

        if let Err(error) = self
            .process_event_broker
            .dispose_connection(connection_id, OperationCancellationReason::ConnectionClosed)
        {
            eprintln!(
                "ERR_AGENTOS_PROCESS_EVENT_CONNECTION_DISPOSAL: connection_id={connection_id} error={error}"
            );
        }

        self.connections.remove(connection_id);
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(events)
    }

    /// Apply the bounded central-state portion of a prepared membership request.
    ///
    /// The stdio router calls this immediately after preparation so pipelined
    /// dependent frames observe membership without waiting for terminal output.
    /// `complete_request` calls it again for in-process users, so the operation
    /// is deliberately idempotent.
    pub fn commit_prepared_membership(
        &mut self,
        membership: &PreparedMembershipCommit,
    ) -> Result<(), VmError> {
        match membership {
            PreparedMembershipCommit::Connection {
                connection_id,
                auth_token,
            } => {
                if let Some(existing) = self.connections.get(connection_id) {
                    if existing.auth_token != *auth_token {
                        return Err(VmError::InvalidState(format!(
                            "ERR_AGENTOS_CONNECTION_COMMIT_CONFLICT: {connection_id} already has different authentication state"
                        )));
                    }
                    return Ok(());
                }
                let expected = format!("conn-{}", self.next_connection_id.saturating_add(1));
                if connection_id != &expected {
                    return Err(VmError::InvalidState(format!(
                        "ERR_AGENTOS_CONNECTION_COMMIT_ORDER: prepared {connection_id}, expected {expected}"
                    )));
                }
                self.next_connection_id = self.next_connection_id.saturating_add(1);
                self.connections.insert(
                    connection_id.clone(),
                    ConnectionState {
                        auth_token: auth_token.clone(),
                        sessions: BTreeSet::new(),
                    },
                );
            }
            PreparedMembershipCommit::Session {
                connection_id,
                session_id,
                placement,
                metadata,
            } => {
                self.require_authenticated_connection(connection_id)?;
                if let Some(existing) = self.sessions.get(session_id) {
                    if existing.connection_id != *connection_id {
                        return Err(VmError::InvalidState(format!(
                            "ERR_AGENTOS_SESSION_COMMIT_CONFLICT: {session_id} is already owned by {}",
                            existing.connection_id
                        )));
                    }
                    return Ok(());
                }
                let expected = format!("session-{}", self.next_session_id.saturating_add(1));
                if session_id != &expected {
                    return Err(VmError::InvalidState(format!(
                        "ERR_AGENTOS_SESSION_COMMIT_ORDER: prepared {session_id}, expected {expected}"
                    )));
                }
                self.next_session_id = self.next_session_id.saturating_add(1);
                self.sessions.insert(
                    session_id.clone(),
                    SessionState {
                        connection_id: connection_id.clone(),
                        placement: placement.clone(),
                        metadata: metadata.clone(),
                        vm_ids: BTreeSet::new(),
                    },
                );
                self.connections
                    .get_mut(connection_id)
                    .expect("authenticated connection checked before session commit")
                    .sessions
                    .insert(session_id.clone());
            }
        }
        Ok(())
    }

    fn authenticate_connection(
        &mut self,
        request: &RequestFrame,
        payload: crate::protocol::AuthenticateRequest,
    ) -> Result<DispatchResult, VmError> {
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
                &self.config.instance_id,
                "security.auth.failed",
                fields,
            );
            return Err(error);
        }

        if let Err(error) = validate_authenticate_versions(&payload) {
            return Err(match error {
                AuthenticateVersionError::ProtocolVersionMismatch(message) => {
                    VmError::ProtocolVersionMismatch(message)
                }
                AuthenticateVersionError::BridgeVersionMismatch(message) => {
                    VmError::BridgeVersionMismatch(message)
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
            self.config.instance_id.clone(),
            connection_id,
            self.config.max_frame_bytes as u32,
        );
        Ok(DispatchResult {
            response,
            events: Vec::new(),
        })
    }

    fn open_session(
        &mut self,
        request: &RequestFrame,
        payload: OpenSessionRequest,
    ) -> Result<DispatchResult, VmError> {
        let connection_id = self.connection_id_for(&request.ownership)?;
        self.require_authenticated_connection(&connection_id)?;

        self.next_session_id += 1;
        let session_id = format!("session-{}", self.next_session_id);
        self.sessions.insert(
            session_id.clone(),
            SessionState {
                connection_id: connection_id.clone(),
                placement: payload.placement,
                metadata: payload.metadata.into_iter().collect(),
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
    ) -> Result<DispatchResult, VmError> {
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
    ) -> Result<Vec<EventFrame>, VmError> {
        self.require_owned_session(connection_id, session_id)?;

        let cancellation_reason = match &reason {
            DisposeReason::Requested => OperationCancellationReason::Explicit,
            DisposeReason::ConnectionClosed => OperationCancellationReason::ConnectionClosed,
            DisposeReason::HostShutdown => OperationCancellationReason::Shutdown,
        };
        if let Err(error) = self.process_event_broker.dispose_session(
            connection_id,
            session_id,
            cancellation_reason,
        ) {
            eprintln!(
                "ERR_AGENTOS_PROCESS_EVENT_SESSION_DISPOSAL: connection_id={connection_id} session_id={session_id} error={error}"
            );
        }

        let vm_ids = self
            .sessions
            .get(session_id)
            .expect("owned session should exist")
            .vm_ids
            .iter()
            .cloned()
            .collect::<Vec<_>>();

        let mut events = Vec::new();
        let mut first_error: Option<VmError> = None;
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
    ) -> Result<(), VmError> {
        let ownership = OwnershipScope::session(connection_id, session_id);
        let extensions = self
            .extensions
            .values()
            .cloned()
            .collect::<Vec<Arc<dyn Extension>>>();
        let mut first_error: Option<VmError> = None;
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
    pub fn take_disposed_sessions(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.disposed_sessions)
    }

    // dispose_vm_internal, terminate_vm_processes, wait_for_vm_processes_to_exit moved to crate::vm

    // kill_process_internal, handle_execution_event, handle_python_vfs_rpc_request,
    // resolve_javascript_child_process_execution, spawn_javascript_child_process,
    // poll_javascript_child_process, write_javascript_child_process_stdin,
    // close_javascript_child_process_stdin, kill_javascript_child_process moved to crate::execution

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
        call: &ExecutionHostCall,
    ) -> Result<Option<crate::execution::HostServiceResponse>, VmError> {
        let request = &call.request;
        let requested_timeout_ms = match request.method.as_str() {
            "process.fd_write" => None,
            "__kernel_stdin_read" => parse_kernel_stdin_read_args(request)?.1,
            _ => {
                let timeout_ms = parse_kernel_poll_args(request)?.1;
                (timeout_ms >= 0).then_some(timeout_ms as u64)
            }
        };
        let now = Instant::now();

        let Some(mut vm) = self.vms.get_mut(vm_id) else {
            log_stale_process_event(&self.bridge, vm_id, process_id, "deferred kernel wait RPC");
            return Ok(None);
        };
        let vm_state = &mut *vm;
        let vm = vm_state;
        let wait_handle = vm.kernel.poll_wait_handle();
        // Snapshot BEFORE the readiness probe: a write landing between the
        // probe and the waiter's wait bumps the generation, so the wait
        // returns immediately instead of losing the wakeup.
        let generation = wait_handle.snapshot();
        let Some(process) = vm.active_processes.get_mut(process_id) else {
            log_stale_process_event(&self.bridge, vm_id, process_id, "deferred kernel wait RPC");
            return Ok(None);
        };
        let requested_timeout_ms = if request.method == "process.fd_write" {
            Some(vm.limits.reactor.operation_deadline_ms)
        } else {
            requested_timeout_ms
        };
        let requested_deadline = requested_timeout_ms
            .map(crate::execution::checked_deferred_guest_wait_deadline)
            .transpose()
            .map_err(VmError::from)?;
        // Reading from the pipe frees capacity. Top it off before every root
        // process read/poll probe, matching the descendant-process path, and
        // deliver a deferred close only after all accepted bytes are written.
        flush_pending_kernel_stdin(&mut vm.kernel, process)?;
        let kernel_pid = process.kernel_pid;
        let kernel_stdin_reader_fd = process.kernel_stdin_reader_fd;
        let same_parked_call = process
            .deferred_kernel_wait_rpc
            .as_ref()
            .is_some_and(|(parked, _)| parked.id == request.id);
        if !same_parked_call {
            process.deferred_kernel_wait_deadline_warned = false;
        }
        let deadline = if same_parked_call {
            process
                .deferred_kernel_wait_rpc
                .as_ref()
                .and_then(|(_, deadline)| *deadline)
        } else {
            requested_deadline
        };
        let probe = match request.method.as_str() {
            "process.fd_write" => {
                service_javascript_kernel_fd_write_sync_rpc(&mut vm.kernel, process, request)
            }
            "__kernel_stdin_read" => {
                let (max_bytes, _) = parse_kernel_stdin_read_args(request)?;
                kernel_stdin_read_response(
                    &mut vm.kernel,
                    kernel_pid,
                    kernel_stdin_reader_fd,
                    max_bytes,
                    Duration::ZERO,
                )
            }
            _ => {
                let (fd_requests, _) = parse_kernel_poll_args(request)?;
                kernel_poll_response(&vm.kernel, kernel_pid, &fd_requests, 0)
            }
        };
        let Some(process) = vm.active_processes.get_mut(process_id) else {
            return Ok(None);
        };
        let (probe, ready) = match probe {
            Ok(value) => {
                let ready = match request.method.as_str() {
                    "process.fd_write" => true,
                    "__kernel_stdin_read" => !value.is_null(),
                    _ => value.get("readyCount").and_then(Value::as_u64).unwrap_or(0) > 0,
                };
                (value, ready)
            }
            Err(error)
                if request.method == "process.fd_write"
                    && host_service_error_code(&error) == "EAGAIN" =>
            {
                (Value::Null, false)
            }
            Err(error) => {
                process.clear_deferred_kernel_wait_rpc();
                return Err(error);
            }
        };
        let mut operation_deadline = if request.method == "process.fd_write" {
            deadline.map(|deadline| {
                crate::execution::OperationDeadlineTracker::from_deadline(
                    deadline,
                    Duration::from_millis(vm.limits.reactor.operation_deadline_ms),
                    process.deferred_kernel_wait_deadline_warned,
                )
            })
        } else {
            None
        };
        if !ready {
            if let Some(deadline) = operation_deadline.as_mut() {
                deadline.observe("deferred root-process fd write");
                process.deferred_kernel_wait_deadline_warned = deadline.warning_emitted();
            }
        }
        if request.method == "process.fd_write"
            && !ready
            && deadline.is_some_and(|deadline| now >= deadline)
        {
            process.clear_deferred_kernel_wait_rpc();
            return Err(VmError::host("ETIMEDOUT", format!("pipe write exceeded limits.reactor.operationDeadlineMs ({} ms); raise that limit for slower readers",
                vm.limits.reactor.operation_deadline_ms
            )));
        }
        if ready
            || requested_timeout_ms == Some(0)
            || deadline.is_some_and(|deadline| now >= deadline)
        {
            process.clear_deferred_kernel_wait_rpc();
            return Ok(Some(probe.into()));
        }

        let connection_id = vm.connection_id.clone();
        let session_id = vm.session_id.clone();
        let runtime = vm.runtime_context.clone();
        let remaining = operation_deadline
            .as_ref()
            .map(crate::execution::OperationDeadlineTracker::remaining_until_next_edge)
            .or_else(|| deadline.map(|deadline| deadline.saturating_duration_since(now)));
        let sender = self.process_event_sender.clone();
        let event_notify = Arc::clone(&self.process_event_notify);
        let waiter_request = call.clone();
        let envelope_vm_id = vm_id.to_owned();
        let envelope_process_id = process_id.to_owned();
        let wake_task = runtime
            .spawn(agentos_driver_tokio::TaskClass::Vm, async move {
            // Wake on any kernel poll-state change or the deadline; either way
            // requeue exactly once. The handler re-probes and either replies or
            // re-parks without dedicating an OS thread to this wait.
            if let Some(remaining) = remaining {
                tokio::select! {
                    _ = wait_handle.wait_for_change_async(generation) => {}
                    _ = tokio::time::sleep(remaining) => {}
                }
            } else {
                wait_handle.wait_for_change_async(generation).await;
            }
            if sender
                .send(ProcessEventEnvelope {
                    connection_id,
                    session_id,
                    vm_id: envelope_vm_id,
                    process_id: envelope_process_id,
                            child_path: Vec::new(),
                    event: ActiveExecutionEvent::HostRpcRequest(waiter_request),
                })
                .await
                .is_err()
            {
                eprintln!(
                    "ERR_AGENTOS_PROCESS_EVENT_CHANNEL_CLOSED: deferred kernel wait completion could not be delivered"
                );
            } else {
                event_notify.notify_one();
            }
            })
            .map_err(VmError::from)?;
        let Some(process) = vm.active_processes.get_mut(process_id) else {
            return Ok(None);
        };
        process.deferred_kernel_wait_rpc = Some((call.clone(), deadline));
        process.deferred_kernel_wait_task = Some(wake_task);
        Ok(None)
    }

    // TODO(clippy-1.98): release the VM/engine RefCell borrow before awaiting; holding it can panic with "already borrowed".
    #[allow(clippy::await_holding_refcell_ref)]
    pub(crate) async fn handle_javascript_sync_rpc_request(
        &mut self,
        vm_id: &str,
        process_id: &str,
        call: ExecutionHostCall,
    ) -> Result<(), VmError> {
        let request = &call.request;
        record_sync_bridge_request_observed(request.id, &request.method);
        if call.reply.is_terminal() {
            eprintln!(
                "INFO_AGENTOS_STALE_KERNEL_WAIT_RETRY: dropping settled host call {} ({})",
                request.id, request.method
            );
            return Ok(());
        }
        let Some(vm) = self.vms.get(vm_id) else {
            log_stale_process_event(&self.bridge, vm_id, process_id, "javascript sync RPC");
            return Ok(());
        };
        if !vm.active_processes.contains_key(process_id) {
            log_stale_process_event(&self.bridge, vm_id, process_id, "javascript sync RPC");
            return Ok(());
        }

        drop(vm);

        let deferrable_fd_write = {
            let vm = self.vms.get(vm_id).expect("VM existence checked above");
            let process = vm
                .active_processes
                .get(process_id)
                .expect("process existence checked above");
            deferred_kernel_wait_request_for_process(request, &vm.kernel, process)?
                .filter(|request| request.method == "process.fd_write")
        };

        let response: Result<crate::execution::HostServiceResponse, VmError> = match request
            .method
            .as_str()
        {
            _ if deferrable_fd_write.is_some() => {
                let normalized = deferrable_fd_write
                    .as_ref()
                    .expect("guarded deferred fd_write request");
                let normalized_call = ExecutionHostCall {
                    request: normalized.clone(),
                    reply: call.reply.clone(),
                };
                match self.service_deferrable_kernel_wait_rpc(vm_id, process_id, &normalized_call) {
                    Ok(Some(response)) => Ok(response),
                    Ok(None) => return Ok(()),
                    Err(error) => Err(error),
                }
            }
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
                    parse_javascript_child_process_spawn_request(&vm, &request.args)?;
                drop(vm);
                self.spawn_child_process(vm_id, process_id, payload)
                    .await
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
                    parse_javascript_child_process_spawn_request(&vm, &request.args)?;
                drop(vm);
                self.defer_javascript_child_process_sync(vm_id, process_id, payload, max_buffer)
                    .await
            }
            "child_process.poll" => {
                let child_process_id =
                    javascript_sync_rpc_arg_str(&request.args, 0, "child_process.poll child id")?;
                let wait_ms = javascript_sync_rpc_arg_u64_optional(
                    &request.args,
                    1,
                    "child_process.poll wait ms",
                )?
                .unwrap_or_default();
                self.poll_child_process(vm_id, process_id, child_process_id, wait_ms)
                    .await
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
                self.write_child_process_stdin(vm_id, process_id, child_process_id, &chunk)?;
                Ok(Value::Null.into())
            }
            "child_process.close_stdin" => {
                let child_process_id = javascript_sync_rpc_arg_str(
                    &request.args,
                    0,
                    "child_process.close_stdin child id",
                )?;
                self.close_child_process_stdin(vm_id, process_id, child_process_id)?;
                Ok(Value::Null.into())
            }
            "child_process.kill" => {
                let child_process_id =
                    javascript_sync_rpc_arg_str(&request.args, 0, "child_process.kill child id")?;
                let signal =
                    javascript_sync_rpc_arg_str(&request.args, 1, "child_process.kill signal")?;
                self.kill_javascript_child_process(vm_id, process_id, child_process_id, signal)?;
                Ok(Value::Null.into())
            }
            "process.kill" => {
                let target_pid =
                    javascript_sync_rpc_arg_i32(&request.args, 0, "process.kill target pid")?;
                let signal = javascript_sync_rpc_arg_str(&request.args, 1, "process.kill signal")?;
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
                        Ok(true) => self
                            .apply_self_process_kill(vm_id, process_id, parsed_signal)
                            .map(Into::into),
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
                        let caller_pid = i32::try_from(caller.kernel_pid)
                            .map_err(|_| VmError::InvalidState("caller pid exceeds i32".into()))?;
                        if caller_pid == target_pid {
                            ProcessKillTarget::SelfProcess
                        } else if let Some((child_process_id, _)) = caller
                            .child_processes
                            .iter()
                            .find(|(_, child)| i32::try_from(child.kernel_pid) == Ok(target_pid))
                        {
                            ProcessKillTarget::Child(child_process_id.clone())
                        } else if let Some((target_process_id, _)) =
                            vm.active_processes.iter().find(|(_, process)| {
                                i32::try_from(process.kernel_pid) == Ok(target_pid)
                            })
                        {
                            ProcessKillTarget::TopLevel(target_process_id.clone())
                        } else {
                            let target_kernel_pid = u32::try_from(target_pid).map_err(|_| {
                                VmError::host("EINVAL", format!("invalid process pid {target_pid}"))
                            })?;
                            ProcessKillTarget::KernelPid(target_kernel_pid)
                        }
                    };
                    match target {
                        ProcessKillTarget::SelfProcess => self
                            .apply_self_process_kill(vm_id, process_id, parsed_signal)
                            .map(Into::into),
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
                let (signal, registration) =
                    parse_process_signal_state_request(&request.args).map_err(VmError::from)?;
                let Some(vm) = self.vms.get_mut(vm_id) else {
                    log_stale_process_event(
                        &self.bridge,
                        vm_id,
                        process_id,
                        "javascript sync RPC process.signal_state",
                    );
                    return Ok(());
                };
                let process = vm.active_processes.get(process_id).ok_or_else(|| {
                    VmError::InvalidState(format!("VM {vm_id} has no active process {process_id}"))
                })?;
                apply_kernel_signal_registration(process, signal, &registration)?;
                Ok(Value::Null.into())
            }
            "net.http_request" => {
                let payload = request
                    .args
                    .first()
                    .cloned()
                    .ok_or_else(|| {
                        VmError::InvalidState(String::from(
                            "net.http_request requires a request payload",
                        ))
                    })
                    .and_then(|value| {
                        serde_json::from_value::<JavascriptHttpLoopbackRequest>(value).map_err(
                            |error| {
                                VmError::InvalidState(format!(
                                    "invalid net.http_request payload: {error}"
                                ))
                            },
                        )
                    })?;
                if !is_javascript_loopback_host(&payload.host) {
                    return Err(VmError::host(
                        "EACCES",
                        format!(
                            "HTTP loopback request requires a loopback host, got {}",
                            payload.host
                        ),
                    ));
                }
                self.bridge.require_network_access(
                    vm_id,
                    NetworkOperation::Http,
                    format_tcp_resource(&payload.host, payload.port),
                )?;
                let Some(mut vm) = self.vms.get_mut(vm_id) else {
                    log_stale_process_event(
                        &self.bridge,
                        vm_id,
                        process_id,
                        "javascript sync RPC net.http_request",
                    );
                    return Ok(());
                };
                let vm = &mut *vm;
                let socket_paths = build_socket_path_context(vm)?;
                let target_is_current =
                    [SocketFamily::Ipv4, SocketFamily::Ipv6]
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
                    return Err(VmError::InvalidState(format!(
                        "unknown HTTP loopback target {}:{} for server {} in process {}",
                        payload.host, payload.port, payload.server_id, payload.process_id
                    )));
                }
                let Some(target_process) = vm.active_processes.get_mut(&payload.process_id) else {
                    return Err(VmError::InvalidState(format!(
                        "unknown HTTP loopback process {}",
                        payload.process_id
                    )));
                };
                dispatch_loopback_http_request_deferred(LoopbackHttpDispatchRequest {
                    process: target_process,
                    server_id: payload.server_id,
                    request_json: &payload.request,
                })
            }
            "__kernel_stdio_write"
                if self.vms.get(vm_id).is_some_and(|vm| {
                    vm.active_processes
                        .get(process_id)
                        .is_some_and(|process| process.tty_master_owner.is_some())
                }) =>
            {
                let (writer_kernel_pid, owner) = {
                    let vm = self.vms.get(vm_id).expect("guarded by match arm");
                    let process = vm
                        .active_processes
                        .get(process_id)
                        .expect("guarded by match arm");
                    (
                        process.kernel_pid,
                        process.tty_master_owner.expect("guarded by match arm"),
                    )
                };
                self.service_shared_tty_stdio_write(vm_id, writer_kernel_pid, owner, request)
                    .map(Into::into)
            }
            "__kernel_stdin_read" | "__kernel_poll" => {
                match self.service_deferrable_kernel_wait_rpc(vm_id, process_id, &call) {
                    Ok(Some(response)) => Ok(response),
                    // Parked: an off-loop waiter re-enqueues this request as a
                    // process event when kernel poll state changes.
                    Ok(None) => return Ok(()),
                    Err(error) => Err(error),
                }
            }
            _ => {
                let Some(mut vm) = self.vms.get_mut(vm_id) else {
                    log_stale_process_event(
                        &self.bridge,
                        vm_id,
                        process_id,
                        "javascript sync RPC bridge dispatch",
                    );
                    return Ok(());
                };
                let vm = &mut *vm;
                let socket_paths = build_socket_path_context(vm)?;
                let kernel_readiness = Arc::clone(&vm.kernel_socket_readiness);
                let capabilities = vm.capabilities.clone();
                let managed_descriptions = Arc::clone(&vm.managed_host_net_descriptions);
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
                    sync_request: request,
                    capabilities,
                    managed_descriptions: Some(managed_descriptions),
                })
                .await
            }
        };

        let response = match response {
            Ok(crate::execution::HostServiceResponse::Deferred {
                receiver,
                timeout,
                task_class,
            }) => {
                let Some(vm) = self.vms.get(vm_id) else {
                    log_stale_process_event(
                        &self.bridge,
                        vm_id,
                        process_id,
                        "deferred sync RPC response admission",
                    );
                    return Ok(());
                };
                let runtime = vm.runtime_context.clone();
                let connection_id = vm.connection_id.clone();
                let session_id = vm.session_id.clone();
                let sender = self.process_event_sender.clone();
                let event_notify = Arc::clone(&self.process_event_notify);
                let envelope_vm_id = vm_id.to_owned();
                let envelope_process_id = process_id.to_owned();
                let reply = call.reply.clone();
                let method = request.method.clone();
                runtime
                .spawn(task_class, async move {
                    let receive = async {
                        receiver.await.unwrap_or_else(|_| {
                            Err(crate::state::DeferredRpcError {
                                code: String::from(
                                    "ERR_AGENTOS_DEFERRED_RPC_RESPONSE_CHANNEL_CLOSED",
                                ),
                                message: format!(
                                    "deferred sync RPC response channel closed for {method}"
                                ),
                                details: None,
                            })
                        })
                    };
                    let result = match timeout {
                        Some(timeout) => match crate::execution::operation_deadline_timeout(
                            &method,
                            timeout,
                            receive,
                        )
                        .await
                        {
                            Ok(result) => result,
                            Err(_) => Err(crate::state::DeferredRpcError {
                                code: String::from("ERR_AGENTOS_DEFERRED_RPC_TIMEOUT"),
                                message: format!(
                                    "{method} exceeded limits.reactor.operationDeadlineMs ({} ms); raise that limit for slower peers",
                                    timeout.as_millis()
                                ),
                                details: None,
                            }),
                        },
                        None => receive.await,
                    };
                    if sender
                        .send(ProcessEventEnvelope {
                            connection_id,
                            session_id,
                            vm_id: envelope_vm_id,
                            process_id: envelope_process_id,
                            child_path: Vec::new(),
                            event: ActiveExecutionEvent::HostCallCompletion(
                                crate::state::HostCallCompletion { reply, result },
                            ),
                        })
                        .await
                        .is_err()
                    {
                        eprintln!(
                            "ERR_AGENTOS_PROCESS_EVENT_CHANNEL_CLOSED: deferred sync RPC completion could not be delivered"
                        );
                    } else {
                        event_notify.notify_one();
                    }
                })
                .map_err(VmError::from)?;
                return Ok(());
            }
            other => other,
        };

        if response.is_ok() && javascript_sync_rpc_may_make_fd_readable(request) {
            if let Some(mut vm) = self.vms.get_mut(vm_id) {
                Self::wake_ready_deferred_fd_reads(&mut vm)?;
            }
        }
        if response.is_ok() && javascript_sync_rpc_may_make_fd_writable(request) {
            if let Some(mut vm) = self.vms.get_mut(vm_id) {
                Self::wake_ready_deferred_fd_writes(&mut vm)?;
            }
        }

        let Some(vm) = self.vms.get_mut(vm_id) else {
            log_stale_process_event(
                &self.bridge,
                vm_id,
                process_id,
                "javascript sync RPC response delivery",
            );
            return Ok(());
        };
        if !vm.active_processes.contains_key(process_id) {
            log_stale_process_event(
                &self.bridge,
                vm_id,
                process_id,
                "javascript sync RPC response delivery",
            );
            return Ok(());
        }

        if let Err(error) = &response {
            tracing::warn!(
                method = %call.request.method,
                error = %error,
                "executor host RPC failed"
            );
        }
        settle_execution_host_call(&call.reply, response)
    }

    /// Applies a `process.kill` aimed at the calling process itself and
    /// returns the self-delivery action payload for the bridge.
    fn apply_self_process_kill(
        &mut self,
        vm_id: &str,
        process_id: &str,
        parsed_signal: i32,
    ) -> Result<Value, VmError> {
        self.kill_process_internal(vm_id, process_id, &parsed_signal.to_string())?;
        Ok(Value::Null)
    }

    pub(crate) fn vm_ids_for_scope(
        &self,
        ownership: &OwnershipScope,
    ) -> Result<Vec<String>, VmError> {
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
            OwnershipScope::ConnectionOwnership(..) => Err(VmError::InvalidState(String::from(
                "event polling requires session or VM ownership scope",
            ))),
        }
    }

    pub(crate) fn vm_ownership(&self, vm_id: &str) -> Result<OwnershipScope, VmError> {
        let vm = self
            .vms
            .get(vm_id)
            .ok_or_else(|| VmError::InvalidState(format!("unknown sidecar VM {vm_id}")))?;
        Ok(OwnershipScope::vm(&vm.connection_id, &vm.session_id, vm_id))
    }

    pub(crate) fn vm_has_active_processes(&self, vm_id: &str) -> bool {
        self.vms
            .get(vm_id)
            .is_some_and(|vm| !vm.active_processes.is_empty())
    }

    fn require_authenticated_connection(&self, connection_id: &str) -> Result<(), VmError> {
        if self.connections.contains_key(connection_id) {
            Ok(())
        } else {
            Err(VmError::InvalidState(format!(
                "connection {connection_id} has not authenticated"
            )))
        }
    }

    pub(crate) fn require_owned_session(
        &self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<(), VmError> {
        self.require_authenticated_connection(connection_id)?;
        let session = self.sessions.get(session_id).ok_or_else(|| {
            VmError::InvalidState(format!("unknown sidecar session {session_id}"))
        })?;
        if session.connection_id == connection_id {
            Ok(())
        } else {
            Err(VmError::InvalidState(format!(
                "session {session_id} is not owned by connection {connection_id}"
            )))
        }
    }

    pub(crate) fn require_owned_vm(
        &self,
        connection_id: &str,
        session_id: &str,
        vm_id: &str,
    ) -> Result<(), VmError> {
        self.require_owned_session(connection_id, session_id)?;
        if let Some(quarantined) = self.quarantined_vms.values().find(|quarantined| {
            quarantined.vm_id == vm_id
                && quarantined.connection_id == connection_id
                && quarantined.session_id == session_id
        }) {
            let snapshot = quarantined.reconciliation_snapshot();
            return Err(VmError::host("ERR_AGENTOS_VM_QUARANTINED", format!("vm_id={vm_id} generation={} reason={:?} active_tasks={} outstanding_capabilities={} ledger_zero={} integrity_ok={}",
                quarantined.generation,
                quarantined.reason,
                snapshot.active_tasks,
                snapshot.outstanding_capabilities,
                snapshot.ledger_zero,
                snapshot.integrity_ok
            )));
        }
        let vm = self
            .vms
            .get(vm_id)
            .ok_or_else(|| VmError::InvalidState(format!("unknown sidecar VM {vm_id}")))?;
        if vm.connection_id != connection_id || vm.session_id != session_id {
            return Err(VmError::InvalidState(format!(
                "VM {vm_id} is not owned by {connection_id}/{session_id}"
            )));
        }
        Ok(())
    }

    fn connection_id_for(&self, ownership: &OwnershipScope) -> Result<String, VmError> {
        match ownership {
            OwnershipScope::ConnectionOwnership(inner) => Ok(inner.connection_id.clone()),
            OwnershipScope::SessionOwnership(..) | OwnershipScope::VmOwnership(..) => Err(
                VmError::InvalidState(String::from("request requires connection ownership scope")),
            ),
        }
    }

    fn validate_auth_token(&self, auth_token: &str) -> Result<(), VmError> {
        let Some(expected_auth_token) = self.config.expected_auth_token.as_deref() else {
            return Ok(());
        };

        if auth_token == expected_auth_token {
            Ok(())
        } else {
            Err(VmError::Unauthorized(String::from(
                "authenticate request provided an invalid auth token",
            )))
        }
    }

    fn allocate_connection_id(&mut self) -> String {
        self.next_connection_id += 1;
        format!("conn-{}", self.next_connection_id)
    }

    pub(crate) fn take_matching_process_event_envelope(
        &mut self,
        vm_id: &str,
        process_id: &str,
    ) -> Result<Option<ProcessEventEnvelope>, VmError> {
        // Preserve queued-public priority, then classify one bounded channel
        // batch and probe again. Internal events belong to the owned supervisor.
        for probe in 0..2 {
            if let Some(index) = self
                .pending_process_events
                .iter()
                .position(|event| event.vm_id == vm_id && event.process_id == process_id)
            {
                let envelope = self.pending_process_events.remove(index);
                self.observe_pending_process_event_depth();
                self.rearm_deferred_process_event_after_capacity_release();
                return Ok(envelope);
            }
            if probe == 0 {
                // Targeted polling historically reports admission failure to
                // its caller. Check before draining so the queued event remains
                // available for retry; the background pump uses backpressure.
                if self.pending_process_event_capacity() == 0
                    && self
                        .process_event_receiver
                        .as_ref()
                        .is_some_and(|receiver| !receiver.is_empty())
                {
                    return Err(process_event_queue_overflow_error(
                        self.config.runtime.protocol.max_process_events,
                    ));
                }
                self.drain_runtime_process_event_channel_nowait()?;
            }
        }
        Ok(None)
    }

    fn allocate_sidecar_request_id(&mut self) -> RequestId {
        let request_id = self.next_sidecar_request_id;
        self.next_sidecar_request_id -= 1;
        request_id
    }

    pub(crate) fn session_scope_for(
        &self,
        ownership: &OwnershipScope,
    ) -> Result<(String, String), VmError> {
        match ownership {
            OwnershipScope::SessionOwnership(inner) => {
                Ok((inner.connection_id.clone(), inner.session_id.clone()))
            }
            OwnershipScope::ConnectionOwnership(..) | OwnershipScope::VmOwnership(..) => Err(
                VmError::InvalidState(String::from("request requires session ownership scope")),
            ),
        }
    }

    pub(crate) fn vm_scope_for(
        &self,
        ownership: &OwnershipScope,
    ) -> Result<(String, String, String), VmError> {
        match ownership {
            OwnershipScope::VmOwnership(inner) => Ok((
                inner.connection_id.clone(),
                inner.session_id.clone(),
                inner.vm_id.clone(),
            )),
            OwnershipScope::ConnectionOwnership(..) | OwnershipScope::SessionOwnership(..) => Err(
                VmError::InvalidState(String::from("request requires VM ownership scope")),
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

    pub(crate) fn reject(
        &self,
        request: &RequestFrame,
        code: &str,
        message: &str,
    ) -> ResponseFrame {
        shared_reject(request, code, message)
    }

    pub(crate) fn reject_error(&self, request: &RequestFrame, error: &VmError) -> ResponseFrame {
        if let VmError::Host(host_error) = error {
            if host_error.code == "ERR_AGENTOS_RESOURCE_LIMIT" {
                let details = host_error.details.as_ref();
                let limit_name = details
                    .and_then(|value| value.get("limitName"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let configured_limit = details
                    .and_then(|value| value.get("limit"))
                    .and_then(Value::as_u64);
                let requested = details
                    .and_then(|value| value.get("observed"))
                    .and_then(Value::as_u64);
                let configuration_path = details
                    .and_then(|value| value.get("configPath"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| limit_name.clone());
                let vm_id = match &request.ownership {
                    OwnershipScope::VmOwnership(owner) => Some(owner.vm_id.clone()),
                    OwnershipScope::ConnectionOwnership(_)
                    | OwnershipScope::SessionOwnership(_) => None,
                };
                let session_generation = vm_id
                    .as_ref()
                    .and_then(|vm_id| self.vms.get(vm_id))
                    .map(|vm| vm.generation);
                return self.respond(
                    request,
                    ResponsePayload::Rejected(RejectedResponse {
                        code: host_error.code.clone(),
                        message: host_error.message.clone(),
                        limit_name,
                        configured_limit,
                        current_usage: None,
                        requested,
                        unit: Some(String::from("items")),
                        scope: Some(if vm_id.is_some() {
                            String::from("vm")
                        } else {
                            String::from("process")
                        }),
                        vm_id,
                        session_generation,
                        capability_id: None,
                        operation: None,
                        configuration_path,
                        retryable: Some(false),
                        errno: Some(String::from("ENOBUFS")),
                    }),
                );
            }
        }
        if let VmError::VmTeardownDeadline {
            vm_id, deadline_ms, ..
        } = error
        {
            return self.respond(
                request,
                ResponsePayload::Rejected(RejectedResponse {
                    code: String::from("timeout"),
                    message: error.to_string(),
                    limit_name: Some(String::from("reactor.shutdownDeadlineMs")),
                    configured_limit: Some(*deadline_ms),
                    current_usage: None,
                    requested: None,
                    unit: Some(String::from("milliseconds")),
                    scope: Some(String::from("vm")),
                    vm_id: Some(vm_id.clone()),
                    session_generation: None,
                    capability_id: None,
                    operation: Some(String::from("vm.dispose")),
                    configuration_path: Some(String::from("limits.reactor.shutdownDeadlineMs")),
                    retryable: Some(false),
                    errno: Some(String::from("ETIMEDOUT")),
                }),
            );
        }
        if let VmError::PackageMountLimit {
            used,
            requested,
            limit,
        } = error
        {
            let vm_id = match &request.ownership {
                OwnershipScope::VmOwnership(owner) => Some(owner.vm_id.clone()),
                OwnershipScope::ConnectionOwnership(_) | OwnershipScope::SessionOwnership(_) => {
                    None
                }
            };
            return self.respond(
                request,
                ResponsePayload::Rejected(RejectedResponse {
                    code: String::from("ERR_AGENTOS_RESOURCE_LIMIT"),
                    message: error.to_string(),
                    limit_name: Some(String::from("packageMounts")),
                    configured_limit: Some(u64::try_from(*limit).unwrap_or(u64::MAX)),
                    current_usage: Some(u64::try_from(*used).unwrap_or(u64::MAX)),
                    requested: Some(u64::try_from(*requested).unwrap_or(u64::MAX)),
                    unit: Some(String::from("mounts")),
                    scope: Some(String::from("vm")),
                    vm_id,
                    session_generation: None,
                    capability_id: None,
                    operation: Some(String::from("vm.packageProjection")),
                    configuration_path: Some(String::from("limits.agentosPackages.maxMounts")),
                    retryable: Some(false),
                    errno: Some(String::from("ENOSPC")),
                }),
            );
        }
        if let VmError::RequestAdmission {
            code,
            message,
            configuration_path,
            retryable,
            errno,
        } = error
        {
            let vm_id = match &request.ownership {
                OwnershipScope::VmOwnership(owner) => Some(owner.vm_id.clone()),
                OwnershipScope::ConnectionOwnership(_) | OwnershipScope::SessionOwnership(_) => {
                    None
                }
            };
            return self.respond(
                request,
                ResponsePayload::Rejected(RejectedResponse {
                    code: (*code).to_owned(),
                    message: message.clone(),
                    limit_name: None,
                    configured_limit: None,
                    current_usage: None,
                    requested: Some(1),
                    unit: Some(String::from("requests")),
                    scope: Some(String::from("vm")),
                    vm_id,
                    session_generation: None,
                    capability_id: None,
                    operation: Some(String::from("vm.lifecycleAdmission")),
                    configuration_path: configuration_path.map(str::to_owned),
                    retryable: Some(*retryable),
                    errno: Some((*errno).to_owned()),
                }),
            );
        }
        let VmError::ResourceLimit(limit) = error else {
            return self.reject(request, error_code(error), &error.to_string());
        };
        use agentos_driver_tokio::accounting::ResourceClass;

        // A child VM ledger can fail because its process parent is full. Do not
        // return that parent ledger's exact occupancy to an untrusted guest:
        // it would be a cross-VM resource-usage oracle. VM-local usage remains
        // useful and safe to report; process pressure is identified by scope,
        // limit and configuration path without the aggregate `used` value.
        let guest_limit = guest_limit_diagnostic(limit);

        let vm_id = match &request.ownership {
            OwnershipScope::VmOwnership(owner) => Some(owner.vm_id.clone()),
            OwnershipScope::ConnectionOwnership(_) | OwnershipScope::SessionOwnership(_) => None,
        };
        let session_generation = vm_id
            .as_ref()
            .and_then(|vm_id| self.vms.get(vm_id))
            .map(|vm| vm.generation);
        let unit = match limit.resource {
            ResourceClass::BufferedBytes
            | ResourceClass::HandleCommandBytes
            | ResourceClass::BridgeRequestBytes
            | ResourceClass::BridgeResponseBytes
            | ResourceClass::AsyncCompletionBytes
            | ResourceClass::UdpBytes
            | ResourceClass::TlsBytes
            | ResourceClass::WasmMemoryBytes
            | ResourceClass::ExecutorBytes
            | ResourceClass::Http2BufferedBytes
            | ResourceClass::Http2HeaderBytes
            | ResourceClass::Http2DataBytes
            | ResourceClass::Http2CommandBytes
            | ResourceClass::Http2EventBytes => "bytes",
            ResourceClass::Tasks => "tasks",
            ResourceClass::Timers => "timers",
            ResourceClass::WasmThreads => "threads",
            ResourceClass::Connections | ResourceClass::Http2Connections => "connections",
            ResourceClass::Http2Streams => "streams",
            ResourceClass::ExecutorSlots => "workers",
            ResourceClass::Capabilities
            | ResourceClass::ReadyHandles
            | ResourceClass::Sockets
            | ResourceClass::Datagrams
            | ResourceClass::HandleCommands
            | ResourceClass::BridgeCalls
            | ResourceClass::AsyncCompletions
            | ResourceClass::UdpDatagrams
            | ResourceClass::Http2Commands
            | ResourceClass::Http2Events => "items",
        };
        let errno = match limit.resource {
            ResourceClass::Capabilities | ResourceClass::Sockets => "EMFILE",
            _ => "ENOBUFS",
        };
        self.respond(
            request,
            ResponsePayload::Rejected(RejectedResponse {
                code: String::from("ERR_AGENTOS_RESOURCE_LIMIT"),
                message: guest_limit.message,
                limit_name: Some(limit.resource.name().to_owned()),
                configured_limit: Some(u64::try_from(limit.limit).unwrap_or(u64::MAX)),
                current_usage: guest_limit.current_usage,
                requested: Some(u64::try_from(limit.requested).unwrap_or(u64::MAX)),
                unit: Some(unit.to_owned()),
                scope: Some(String::from(guest_limit.scope)),
                vm_id,
                session_generation,
                capability_id: None,
                operation: None,
                configuration_path: Some(limit.config_path.clone()),
                retryable: Some(false),
                errno: Some(errno.to_owned()),
            }),
        )
    }

    pub fn queue_sidecar_request(
        &mut self,
        ownership: OwnershipScope,
        payload: SidecarRequestPayload,
    ) -> Result<RequestId, VmError> {
        let outbound_limit = self.config.runtime.protocol.max_outbound_requests;
        if self.outbound_sidecar_requests.len() >= outbound_limit {
            return Err(outbound_sidecar_request_queue_overflow_error(
                outbound_limit,
            ));
        }
        let pending_limit = self.config.runtime.protocol.max_pending_responses;
        if self.pending_sidecar_responses.pending_count() >= pending_limit {
            return Err(sidecar_response_pending_overflow_error(pending_limit));
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
    ) -> Result<crate::wire::RequestId, VmError> {
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
    ) -> Result<Option<crate::wire::SidecarRequestFrame>, VmError> {
        self.pop_sidecar_request()
            .map(crate::wire::sidecar_request_frame_from_compat)
            .transpose()
            .map_err(wire_protocol_error)
    }

    pub fn accept_sidecar_response(
        &mut self,
        response: SidecarResponseFrame,
    ) -> Result<(), VmError> {
        let completed_limit = self.config.runtime.protocol.max_completed_responses;
        if self.completed_sidecar_responses.len() >= completed_limit {
            return Err(VmError::host_resource_limit(
                "runtime.protocol.maxCompletedResponses",
                completed_limit,
                self.completed_sidecar_responses.len().saturating_add(1),
                format!(
                    "completed sidecar response queue reached {completed_limit} retained responses; drain responses or raise runtime.protocol.maxCompletedResponses"
                ),
            ));
        }
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
        Ok(())
    }

    pub fn accept_wire_sidecar_response(
        &mut self,
        response: crate::wire::SidecarResponseFrame,
    ) -> Result<(), VmError> {
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
    ) -> Result<Option<crate::wire::SidecarResponseFrame>, VmError> {
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

    fn ensure_request_within_frame_limit(&self, request: &RequestFrame) -> Result<(), VmError> {
        let frame = crate::protocol::to_generated_protocol_frame(
            &crate::protocol::ProtocolFrame::Request(request.clone()),
        )
        .map_err(|error| {
            VmError::InvalidState(format!("failed to convert request frame: {error}"))
        })?;
        let crate::wire::ProtocolFrame::RequestFrame(_) = &frame else {
            return Err(VmError::InvalidState(String::from(
                "request converted to non-request wire frame",
            )));
        };

        crate::wire::WireFrameCodec::new(self.config.max_frame_bytes)
            .encode(&frame)
            .map(|_| ())
            .map_err(|error| VmError::FrameTooLarge(error.to_string()))
    }
}

impl<B> Drop for VmManager<B> {
    fn drop(&mut self) {
        self.in_process_event_services.clear();
        fn request_shutdown(process: &mut crate::state::ActiveProcess) {
            for child in process.child_processes.values_mut() {
                request_shutdown(child);
            }
            if let Err(error) = process.execution.terminate() {
                eprintln!(
                    "ERR_AGENTOS_PROCESS_DROP_SHUTDOWN: failed to request shutdown for kernel pid {}: {error}",
                    process.kernel_pid
                );
            }
        }

        // Execution engines are declared before `vms`, so Rust's default field
        // drop order would tear the engines down while VM-owned execution
        // handles are still live. Request backend shutdown and release every VM
        // generation first so sessions can drain against a live engine/runtime.
        for mut vm in self.vms.values_mut() {
            for process in vm.active_processes.values_mut() {
                request_shutdown(process);
            }
        }
        if let Err(error) = self
            .process_event_broker
            .shutdown(OperationCancellationReason::Shutdown)
        {
            eprintln!("ERR_AGENTOS_PROCESS_EVENT_BROKER_SHUTDOWN: {error}");
        }
        if let Err(error) = self.vms.clear() {
            eprintln!("ERR_AGENTOS_VM_REGISTRY_SHUTDOWN: {error}");
        }
        self.quarantined_vms.clear();
        if let Some(task) = self.kernel_reaper_task.take() {
            task.abort();
        }
    }
}

impl<B> ExtensionHost for VmManager<B>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    fn vm_database<'a>(
        &'a mut self,
        ownership: OwnershipScope,
    ) -> ExtensionFuture<'a, Option<crate::vm_sqlite::SharedVmSqliteDatabase>> {
        Box::pin(async move {
            let (connection_id, session_id, vm_id) = self.vm_scope_for(&ownership)?;
            self.require_owned_vm(&connection_id, &session_id, &vm_id)?;
            Ok(self.vms.get(&vm_id).and_then(|vm| vm.database.clone()))
        })
    }

    fn spawn_process<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        payload: ExecuteRequest,
    ) -> ExtensionFuture<'a, ProcessStartedResponse> {
        Box::pin(async move {
            let request = RequestFrame::new(0, ownership, RequestPayload::Execute(payload.clone()));
            let dispatch = VmManager::execute(self, &request, payload).await?;
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
            let dispatch = VmManager::write_stdin(self, &request, payload).await?;
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
            let dispatch = VmManager::close_stdin(self, &request, payload).await?;
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
            let dispatch = VmManager::kill_process(self, &request, payload).await?;
            match dispatch.response.payload {
                ResponsePayload::ProcessKilled(response) => Ok(response),
                other => Err(unexpected_extension_host_response("kill_process", other)),
            }
        })
    }

    fn poll_event<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        timeout: Duration,
    ) -> ExtensionFuture<'a, Option<EventFrame>> {
        Box::pin(async move { VmManager::poll_event(self, &ownership, timeout).await })
    }

    fn poll_process_event<'a>(
        &'a mut self,
        ownership: OwnershipScope,
        process_id: String,
        timeout: Duration,
    ) -> ExtensionFuture<'a, Option<EventFrame>> {
        Box::pin(async move {
            VmManager::poll_process_event(self, &ownership, &process_id, timeout).await
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
            let dispatch = VmManager::guest_filesystem_call(self, &request, payload).await?;
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
                return Err(VmError::InvalidState(String::from(
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
                return Err(VmError::Conflict(String::from(
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
            let deadline = Instant::now() + timeout;
            let process_event_notify = Arc::clone(&self.process_event_notify);
            loop {
                let notified = process_event_notify.notified();
                let finalize = timeout.is_zero() || Instant::now() >= deadline;
                if let Some(buffer) = self
                    .probe_extension_process_output_handoff(
                        ownership.clone(),
                        namespace.clone(),
                        ext_session_id.clone(),
                        process_id.clone(),
                        finalize,
                    )
                    .await?
                {
                    return Ok(buffer);
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                tokio::select! {
                    _ = notified => {}
                    _ = time::sleep(remaining) => {}
                }
            }
        })
    }
}

fn unexpected_extension_host_response(operation: &str, payload: ResponsePayload) -> VmError {
    match payload {
        ResponsePayload::Rejected(response) => VmError::InvalidState(format!(
            "extension {operation} rejected with {}: {}",
            response.code, response.message
        )),
        other => VmError::InvalidState(format!(
            "extension {operation} returned unexpected response: {other:?}"
        )),
    }
}

fn sidecar_response_tracker_error(error: SidecarResponseTrackerError) -> VmError {
    VmError::InvalidState(format!(
        "invalid sidecar response correlation state: {error}"
    ))
}

fn map_bridge_permission(
    decision: agentos_vm_host_interface::PermissionDecision,
) -> PermissionDecision {
    match decision.verdict {
        agentos_vm_host_interface::PermissionVerdict::Allow => PermissionDecision::allow(),
        agentos_vm_host_interface::PermissionVerdict::Deny => PermissionDecision::deny(
            decision
                .reason
                .unwrap_or_else(|| String::from("denied by host")),
        ),
        agentos_vm_host_interface::PermissionVerdict::Prompt => PermissionDecision::deny(
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
) -> Result<(), VmError>
where
    B: VmManagerHost + Send + 'static,
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
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    emit_structured_event_or_stderr(bridge, vm_id, name, fields);
}

pub(crate) fn emit_structured_event_or_stderr<B>(
    bridge: &SharedBridge<B>,
    vm_id: &str,
    name: &str,
    fields: BTreeMap<String, String>,
) where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    if let Err(error) = emit_structured_event(bridge, vm_id, name, fields) {
        // This fallback must remain independent of bridge telemetry: routing
        // the failure through the same bridge can recurse or hide it again.
        eprintln!(
            "ERR_AGENTOS_STRUCTURED_EVENT: vm_id={vm_id} event={name} delivery failed: {error}"
        );
    }
}

/// Build a wire `EventFrame` carrying a `StructuredEvent` (name + string-map
/// detail) scoped to a connection. Used to forward limit-registry warnings to the
/// host as `{type:"structured", name:"limit_warning", detail}` events without a
/// protocol schema change. Emitted directly to the host (not via the polled,
/// per-session bridge queue, which is a no-op in the stdio sidecar), so a
/// process-global signal is delivered against the active connection.
pub fn structured_event_frame(
    connection_id: &str,
    name: &str,
    detail: std::collections::HashMap<String, String>,
) -> Result<crate::wire::EventFrame, VmError> {
    let event = EventFrame::new(
        OwnershipScope::connection(connection_id),
        EventPayload::Structured(crate::protocol::StructuredEvent {
            name: name.to_owned(),
            detail,
        }),
    );
    crate::wire::event_frame_from_compat(event)
        .map_err(|error| VmError::InvalidState(format!("invalid structured event frame: {error}")))
}

pub(crate) fn log_stale_process_event<B>(
    bridge: &SharedBridge<B>,
    vm_id: &str,
    process_id: &str,
    context: &str,
) where
    B: VmManagerHost + Send + 'static,
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

pub(crate) fn root_filesystem_error(error: impl std::fmt::Display) -> VmError {
    VmError::InvalidState(format!("root filesystem: {error}"))
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

pub(crate) fn kernel_error(error: KernelError) -> VmError {
    VmError::Host(crate::executor::backend::HostServiceError::new(
        error.code(),
        error.to_string(),
    ))
}

pub(crate) fn plugin_error(error: PluginError) -> VmError {
    VmError::Plugin(error.to_string())
}

#[cfg(feature = "node-v8")]
pub(crate) fn javascript_error(error: JavascriptExecutionError) -> VmError {
    match error {
        JavascriptExecutionError::EventChannelClosed => VmError::ExecutionEventChannelClosed {
            backend: ExecutionBackendKind::Javascript,
        },
        other => VmError::Execution(other.to_string()),
    }
}

pub(crate) fn wasm_error(error: WasmExecutionError) -> VmError {
    let message = error.to_string();
    match error {
        WasmExecutionError::EventChannelClosed => VmError::ExecutionEventChannelClosed {
            backend: ExecutionBackendKind::WebAssembly,
        },
        WasmExecutionError::Host(error) => VmError::Host(error),
        WasmExecutionError::NativeBinaryNotSupported { .. } => {
            VmError::host("ERR_NATIVE_BINARY_NOT_SUPPORTED", message)
        }
        WasmExecutionError::DeterministicFuelUnsupported { .. } => {
            VmError::host("ENOTSUP", message)
        }
        _ => VmError::Execution(message),
    }
}

#[cfg(test)]
mod execution_error_tests {
    use super::*;
    use crate::executor::NativeBinaryFormat;

    #[test]
    fn native_binary_rejection_preserves_its_guest_error_code() {
        let error = wasm_error(WasmExecutionError::NativeBinaryNotSupported {
            path: PathBuf::from("/tmp/fake-rg"),
            header: vec![0x7f, b'E', b'L', b'F'],
            format: NativeBinaryFormat::Elf,
        });

        assert_eq!(error.code(), Some("ERR_NATIVE_BINARY_NOT_SUPPORTED"));
    }

    #[test]
    fn deterministic_fuel_rejection_preserves_enotsup() {
        let error = wasm_error(WasmExecutionError::DeterministicFuelUnsupported { fuel: 42 });

        assert_eq!(error.code(), Some("ENOTSUP"));
        assert!(error.to_string().contains("deterministic WebAssembly fuel"));
    }

    #[test]
    fn wasmtime_host_errors_preserve_their_typed_code_and_details() {
        let error = wasm_error(WasmExecutionError::Host(
            crate::executor::backend::HostServiceError::new(
                "ERR_AGENTOS_VM_EXECUTOR_LIMIT",
                "executor saturated",
            )
            .with_details(serde_json::json!({
                "limitName": "runtime.executor.maxActiveVms",
                "limit": 6,
            })),
        ));

        assert_eq!(error.code(), Some("ERR_AGENTOS_VM_EXECUTOR_LIMIT"));
        let VmError::Host(error) = error else {
            panic!("typed Wasmtime host error must remain a host error");
        };
        assert_eq!(
            error.details,
            Some(serde_json::json!({
                "limitName": "runtime.executor.maxActiveVms",
                "limit": 6,
            }))
        );
    }

    #[cfg(all(feature = "node-v8", feature = "python-v8-pyodide"))]
    #[test]
    fn closed_execution_channels_preserve_the_backend_kind() {
        assert_eq!(
            javascript_error(JavascriptExecutionError::EventChannelClosed),
            VmError::ExecutionEventChannelClosed {
                backend: ExecutionBackendKind::Javascript,
            }
        );
        assert_eq!(
            python_error(PythonExecutionError::EventChannelClosed),
            VmError::ExecutionEventChannelClosed {
                backend: ExecutionBackendKind::Python,
            }
        );
        assert_eq!(
            wasm_error(WasmExecutionError::EventChannelClosed),
            VmError::ExecutionEventChannelClosed {
                backend: ExecutionBackendKind::WebAssembly,
            }
        );
    }
}

#[cfg(feature = "python-v8-pyodide")]
pub(crate) fn python_error(error: PythonExecutionError) -> VmError {
    match error {
        PythonExecutionError::EventChannelClosed => VmError::ExecutionEventChannelClosed {
            backend: ExecutionBackendKind::Python,
        },
        other => VmError::Execution(other.to_string()),
    }
}

pub(crate) fn vfs_error(error: VfsError) -> VmError {
    VmError::Kernel(error.to_string())
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
const HOISTED_NODE_MODULES_GUIDANCE: &str = "agentos can't load mounted node_modules: the directory uses a non-flat layout (pnpm / bun / yarn workspaces store, or yarn Plug'n'Play) whose package store isn't visible inside the VM. A flat (hoisted) node_modules is required.\n  - pnpm        -> add `node-linker=hoisted` to .npmrc, then reinstall\n  - yarn berry  -> set `nodeLinker: node-modules` in .yarnrc.yml (not pnp/pnpm)\n  - bun         -> install dependencies outside a workspace (workspaces use a .bun store)\n  - npm / yarn classic -> already flat, no change needed";

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
mod prepared_request_tests {
    use super::*;
    use agentos_vm_host_interface::LocalVmHost as LocalBridge;
    use agentos_vm_kernel::vfs::VirtualFileSystem;

    fn test_sidecar() -> VmManager<LocalBridge> {
        VmManager::new(LocalBridge::default()).expect("build test sidecar")
    }

    #[test]
    fn poisoned_static_permissions_fail_closed() {
        let bridge = SharedBridge::new(LocalBridge::default());
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = bridge.permissions.lock().unwrap();
            panic!("poison permission policy for regression test");
        }));
        assert!(poisoned.is_err());
        let decision = bridge
            .static_permission_decision("vm", "read", "fs", Some("/secret"))
            .expect("poisoned policy must not fall through to the host permission callback");
        assert!(!decision.allow);
        assert!(decision
            .reason
            .unwrap()
            .contains("permission policy lock poisoned"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pending_process_event_limits_preserve_typed_details() {
        let mut manager = test_sidecar();
        let vm_id = create_test_vms(&mut manager, 1).await.remove(0);
        let envelope = || ProcessEventEnvelope {
            connection_id: "vm-handle-test-connection".into(),
            session_id: "vm-handle-test-session".into(),
            vm_id: vm_id.clone(),
            process_id: "limit-fixture".into(),
            child_path: Vec::new(),
            event: ActiveExecutionEvent::Stdout("payload".into()),
        };
        for (name, limit, observed) in [
            ("limits.process.pendingEventCount", 1, 2),
            (
                "limits.process.pendingEventBytes",
                envelope().retained_bytes(),
                envelope().retained_bytes() * 2,
            ),
        ] {
            manager.pending_process_events.clear();
            {
                let mut vm = manager.vms.get_mut(&vm_id).unwrap();
                vm.limits.process.pending_event_count =
                    if name.ends_with("Count") { limit } else { 8 };
                vm.limits.process.pending_event_bytes = if name.ends_with("Bytes") {
                    limit
                } else {
                    usize::MAX
                };
            }
            manager
                .check_pending_process_event_capacity(&envelope())
                .expect("first event fits");
            manager.pending_process_events.push_back(envelope());
            let error = manager
                .check_pending_process_event_capacity(&envelope())
                .unwrap_err();
            assert_eq!(error.code(), Some("ERR_AGENTOS_RESOURCE_LIMIT"));
            assert!(error.to_string().contains(&format!("raise {name}")));
            let VmError::Host(error) = error else {
                panic!("expected typed host error")
            };
            assert_eq!(
                error.details,
                Some(serde_json::json!({
                    "limitName": name, "configPath": name, "limit": limit, "observed": observed,
                }))
            );
        }
    }

    #[test]
    fn in_process_continuations_rearm_at_quantum_and_park_without_spinning() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let mut manager = test_sidecar();
        manager.config.runtime.fairness.vm_quantum_operations = 1;
        let polls = Arc::new(AtomicUsize::new(0));
        for _ in 0..2 {
            let polls = polls.clone();
            manager
                .in_process_event_services
                .push(InProcessEventService {
                    vm_id: "vm-fixture".into(),
                    reply: None,
                    completed: false,
                    ready: Arc::new(AtomicBool::new(true)),
                    future: Box::pin(std::future::poll_fn(move |_| {
                        polls.fetch_add(1, Ordering::AcqRel);
                        Poll::<Result<(), VmError>>::Pending
                    })),
                });
        }
        manager.poll_in_process_event_services_nowait();
        assert_eq!(polls.load(Ordering::Acquire), 1);
        {
            let notified = manager.process_event_notify.notified();
            tokio::pin!(notified);
            assert!(notified
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_ready());
        }
        manager.poll_in_process_event_services_nowait();
        assert_eq!(
            polls.load(Ordering::Acquire),
            2,
            "continuation reaches the next ready service"
        );
        manager.poll_in_process_event_services_nowait();
        assert_eq!(
            polls.load(Ordering::Acquire),
            2,
            "parked services require a real wake"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn direct_request_retains_owned_waits_across_calls_and_cancels_on_disposal() {
        use crate::executor::backend::{
            DirectHostReplyHandle, DirectHostReplyTarget, HostCallIdentity, HostCallReply,
            HostServiceError,
        };
        struct Reply(Mutex<Vec<Result<HostCallReply, HostServiceError>>>);
        impl DirectHostReplyTarget for Reply {
            fn claim(&self, _: u64) -> Result<bool, HostServiceError> {
                Ok(true)
            }
            fn respond(
                &self,
                _: u64,
                _: bool,
                result: Result<HostCallReply, HostServiceError>,
            ) -> Result<(), HostServiceError> {
                self.0.lock().expect("replies").push(result);
                Ok(())
            }
        }
        let mut manager = test_sidecar();
        let vm_id = create_test_vms(&mut manager, 1).await.remove(0);
        let ownership = OwnershipScope::vm(
            "vm-handle-test-connection",
            "vm-handle-test-session",
            &vm_id,
        );
        let request = RequestFrame::new(
            800,
            ownership.clone(),
            RequestPayload::DisposeVm(crate::protocol::DisposeVmRequest {
                reason: DisposeReason::Requested,
            }),
        );
        let ready_request = || {
            let request = request.clone();
            Box::pin(async move {
                Ok(DispatchResult {
                    response: crate::core::respond(
                        &request,
                        ResponsePayload::VmFetchResult(crate::protocol::VmFetchResponse {
                            response_json: "{}".into(),
                        }),
                    ),
                    events: Vec::new(),
                })
            }) as crate::execution::OwnedVmRouteFuture
        };
        let replies = Arc::new(Reply(Mutex::new(Vec::new())));
        let reply = DirectHostReplyHandle::new(
            HostCallIdentity {
                generation: 1,
                pid: 1,
                call_id: 1,
            },
            replies.clone(),
            4096,
        )
        .expect("reply");
        let (release, released) = tokio::sync::oneshot::channel();
        let completion_reply = reply.clone();
        manager
            .in_process_event_services
            .push(InProcessEventService {
                vm_id: vm_id.clone(),
                reply: Some(reply.clone()),
                completed: false,
                ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
                future: Box::pin(async move {
                    released
                        .await
                        .map_err(|error| VmError::Execution(error.to_string()))?;
                    completion_reply
                        .succeed_json(Value::Null)
                        .map_err(VmError::from)
                }),
            });
        manager
            .drive_direct_request_events(&request, None, ready_request())
            .await
            .expect("first request");
        assert_eq!(manager.in_process_event_services.len(), 1);
        assert!(
            !reply.is_terminal(),
            "returning a request must not cancel an unrelated accept/read"
        );
        release.send(()).expect("release owned wait");
        manager
            .drive_direct_request_events(&request, None, ready_request())
            .await
            .expect("next request");
        assert!(manager.in_process_event_services.is_empty());
        assert!(reply.is_terminal());
        assert!(replies.0.lock().expect("replies")[0].is_ok());

        let pending_reply = DirectHostReplyHandle::new(
            HostCallIdentity {
                generation: 1,
                pid: 1,
                call_id: 2,
            },
            replies.clone(),
            4096,
        )
        .expect("pending reply");
        manager
            .in_process_event_services
            .push(InProcessEventService {
                vm_id: vm_id.clone(),
                reply: Some(pending_reply),
                completed: false,
                ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
                future: Box::pin(std::future::pending()),
            });
        manager.dispatch(request).await.expect("dispose request");
        assert!(manager.in_process_event_services.is_empty());
        assert_eq!(
            replies.0.lock().expect("replies")[1]
                .as_ref()
                .expect_err("disposal cancels owned work")
                .code,
            "ECANCELED"
        );
    }

    #[test]
    fn backend_resource_limit_survives_service_wire_rejection() {
        let sidecar = test_sidecar();
        let request = RequestFrame::new(
            1,
            OwnershipScope::vm("connection", "session", "vm-limit"),
            RequestPayload::DisposeVm(crate::protocol::DisposeVmRequest {
                reason: crate::protocol::DisposeReason::Requested,
            }),
        );
        let error = VmError::Host(
            crate::executor::backend::HostServiceError::new(
                "ERR_AGENTOS_RESOURCE_LIMIT",
                "WASM memory limit exceeded",
            )
            .with_details(serde_json::json!({
                "limitName": "wasmMemoryBytes", "limit": 65536,
                "observed": 131072, "configPath": "limits.wasmMemoryBytes"
            })),
        );
        let response = sidecar.reject_error(&request, &error);
        let wire = crate::protocol::to_generated_protocol_frame(
            &crate::protocol::ProtocolFrame::Response(response),
        )
        .expect("encode rejection");
        let crate::protocol::ProtocolFrame::Response(response) =
            crate::protocol::from_generated_protocol_frame(wire).expect("decode rejection")
        else {
            panic!("expected response");
        };
        let ResponsePayload::Rejected(rejected) = response.payload else {
            panic!("expected typed resource rejection");
        };
        assert_eq!(rejected.code, "ERR_AGENTOS_RESOURCE_LIMIT");
        assert_eq!(rejected.limit_name.as_deref(), Some("wasmMemoryBytes"));
        assert_eq!(rejected.configured_limit, Some(65536));
        assert_eq!(rejected.requested, Some(131072));
        assert_eq!(
            rejected.configuration_path.as_deref(),
            Some("limits.wasmMemoryBytes")
        );
        assert_eq!(rejected.vm_id.as_deref(), Some("vm-limit"));
    }

    #[test]
    fn teardown_deadline_rejection_names_timeout_and_limit() {
        let sidecar = test_sidecar();
        let request = RequestFrame::new(
            1,
            OwnershipScope::vm("connection", "session", "vm-deadline"),
            RequestPayload::DisposeVm(crate::protocol::DisposeVmRequest {
                reason: crate::protocol::DisposeReason::Requested,
            }),
        );
        let error = VmError::VmTeardownDeadline {
            message: String::from("VM SQLite close exceeded its deadline"),
            vm_id: String::from("vm-deadline"),
            deadline_ms: 5_000,
        };
        let response = sidecar.reject_error(&request, &error);
        let wire = crate::protocol::to_generated_protocol_frame(
            &crate::protocol::ProtocolFrame::Response(response),
        )
        .expect("encode typed timeout fields");
        let crate::protocol::ProtocolFrame::Response(response) =
            crate::protocol::from_generated_protocol_frame(wire)
                .expect("decode typed timeout fields")
        else {
            panic!("expected a response frame");
        };
        let ResponsePayload::Rejected(rejected) = response.payload else {
            panic!("expected structured VM teardown rejection");
        };
        assert_eq!(rejected.code, "timeout");
        assert_eq!(
            rejected.limit_name.as_deref(),
            Some("reactor.shutdownDeadlineMs")
        );
        assert_eq!(rejected.retryable, Some(false));
        assert_eq!(rejected.vm_id.as_deref(), Some("vm-deadline"));
        assert_eq!(rejected.configured_limit, Some(5_000));
        assert_eq!(rejected.unit.as_deref(), Some("milliseconds"));
        assert_eq!(rejected.operation.as_deref(), Some("vm.dispose"));
        assert_eq!(
            rejected.configuration_path.as_deref(),
            Some("limits.reactor.shutdownDeadlineMs")
        );
        assert_eq!(rejected.errno.as_deref(), Some("ETIMEDOUT"));
    }

    fn wire_request(request: RequestFrame) -> crate::wire::RequestFrame {
        match crate::protocol::to_generated_protocol_frame(
            &crate::protocol::ProtocolFrame::Request(request),
        )
        .expect("encode request")
        {
            crate::wire::ProtocolFrame::RequestFrame(request) => request,
            _ => panic!("request encoded as another frame class"),
        }
    }

    fn compat_response(dispatch: crate::wire::WireDispatchResult) -> ResponseFrame {
        match crate::protocol::from_generated_protocol_frame(
            crate::wire::ProtocolFrame::ResponseFrame(dispatch.response),
        )
        .expect("decode response")
        {
            crate::protocol::ProtocolFrame::Response(response) => response,
            _ => panic!("response decoded as another frame class"),
        }
    }

    async fn create_test_vms(sidecar: &mut VmManager<LocalBridge>, count: usize) -> Vec<String> {
        let connection_id = "vm-handle-test-connection";
        let session_id = "vm-handle-test-session";
        sidecar.connections.insert(
            connection_id.to_owned(),
            ConnectionState {
                auth_token: String::new(),
                sessions: BTreeSet::from([session_id.to_owned()]),
            },
        );
        sidecar.sessions.insert(
            session_id.to_owned(),
            SessionState {
                connection_id: connection_id.to_owned(),
                placement: crate::protocol::SidecarPlacement::SidecarPlacementShared(
                    crate::protocol::SidecarPlacementShared { pool: None },
                ),
                metadata: BTreeMap::new(),
                vm_ids: BTreeSet::new(),
            },
        );

        let mut vm_ids = Vec::with_capacity(count);
        for request_id in 1..=count {
            let request = RequestFrame::new(
                request_id as RequestId,
                OwnershipScope::session(connection_id, session_id),
                RequestPayload::CreateVm(crate::protocol::CreateVmRequest::legacy_test_config(
                    crate::protocol::GuestRuntimeKind::JavaScript,
                    std::collections::HashMap::new(),
                    Default::default(),
                    Some(crate::wire::PermissionsPolicy::allow_all()),
                )),
            );
            let RequestPayload::CreateVm(payload) = request.payload.clone() else {
                unreachable!("test request is create VM");
            };
            let response = sidecar
                .create_vm(&request, payload)
                .await
                .expect("create test VM");
            let ResponsePayload::VmCreated(created) = response.response.payload else {
                panic!("test VM creation returned a different response");
            };
            vm_ids.push(created.vm_id);
        }
        vm_ids
    }

    fn cleanup_test_ownership(vm_id: &str) -> OwnershipScope {
        OwnershipScope::vm("vm-handle-test-connection", "vm-handle-test-session", vm_id)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn public_poll_routes_racing_internal_wakes_to_owned_service() {
        for reader in ["nowait", "async", "process"] {
            let mut manager = test_sidecar();
            let vm_id = create_test_vms(&mut manager, 1).await.remove(0);
            let process_id = "racing-wake";
            let (connection_id, session_id) = {
                let mut vm = manager.vms.get_mut(&vm_id).unwrap();
                let handle = vm
                    .kernel
                    .create_virtual_process(
                        EXECUTION_DRIVER_NAME,
                        EXECUTION_DRIVER_NAME,
                        crate::state::JAVASCRIPT_COMMAND,
                        Vec::new(),
                        Default::default(),
                    )
                    .unwrap();
                let process = crate::state::ActiveProcess::new(
                    handle.pid(),
                    handle,
                    vm.runtime_context.clone(),
                    vm.limits.clone(),
                    8,
                    crate::protocol::GuestRuntimeKind::JavaScript,
                    crate::state::ActiveExecution::HostFunction(
                        crate::state::HostFunctionExecution::default(),
                    ),
                );
                vm.active_processes.insert(process_id.into(), process);
                (vm.connection_id.clone(), vm.session_id.clone())
            };
            let ownership = OwnershipScope::vm(&connection_id, &session_id, &vm_id);
            let envelope = |event| ProcessEventEnvelope {
                connection_id: connection_id.clone(),
                session_id: session_id.clone(),
                vm_id: vm_id.clone(),
                process_id: process_id.into(),
                child_path: Vec::new(),
                event,
            };
            // A runtime timer can publish after the supervisor turn but before
            // the public consumer probes the shared ingress channel.
            manager
                .process_event_sender
                .try_send(envelope(ActiveExecutionEvent::DeferredPosixPollWake))
                .unwrap();
            match reader {
                "nowait" => assert!(manager
                    .poll_event_nowait(&ownership)
                    .expect("internal wake is not public output")
                    .is_none()),
                "async" => assert!(manager
                    .poll_event(&ownership, Duration::ZERO)
                    .await
                    .expect("zero-timeout poll classifies ingress")
                    .is_none()),
                _ => assert!(manager
                    .take_matching_process_event_envelope(&vm_id, process_id)
                    .expect("targeted reader classifies ingress")
                    .is_none()),
            }
            assert!(
                manager.pending_process_events.is_empty(),
                "{reader}: no internal event enters the public broker"
            );
            {
                let vm = manager.vms.get(&vm_id).unwrap();
                let pending = &vm.active_processes[process_id].pending_execution_events;
                assert_eq!(pending.len(), 1, "{reader}: wake remains durable");
            }
            let turn = manager.pump_process_events_nowait(&ownership, 1).unwrap();
            assert_eq!(
                turn.host_services.len(),
                1,
                "{reader}: supervisor claims the wake"
            );
            for service in turn.host_services {
                manager
                    .prepare_owned_host_event_service(service)
                    .await
                    .unwrap();
            }
            manager
                .process_event_sender
                .try_send(envelope(ActiveExecutionEvent::Stdout(
                    b"after-wake".to_vec(),
                )))
                .unwrap();
            let output = manager
                .poll_event_nowait(&ownership)
                .unwrap()
                .expect("public output still flows");
            assert!(
                matches!(output.payload, EventPayload::ProcessOutput(output) if output.chunk == b"after-wake")
            );
            manager
                .process_event_sender
                .try_send(envelope(ActiveExecutionEvent::Exited(0)))
                .unwrap();
            let exit = manager
                .poll_event_nowait(&ownership)
                .unwrap()
                .expect("process still retires");
            assert!(
                matches!(exit.payload, EventPayload::ProcessExited(exit) if exit.exit_code == 0)
            );
            assert!(!manager
                .vms
                .get(&vm_id)
                .unwrap()
                .active_processes
                .contains_key(process_id));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pending_process_replay_reservations_are_bounded_and_cancel_safe() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let vm = sidecar.vms.handle(&vm_id).unwrap();
        vm.borrow_mut().limits.process.max_output_replays = 1;
        let launch_vm = vm.clone();
        let mut pending_launch = Box::pin(async move {
            let _reservation = launch_vm.reserve_process_output_replay("pending")?;
            std::future::pending::<()>().await;
            Ok::<(), VmError>(())
        });
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(pending_launch.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert_eq!(vm.borrow().process_output_replays.len(), 1);
        assert!(matches!(
            vm.reserve_process_output_replay("concurrent"),
            Err(VmError::RequestAdmission {
                configuration_path: Some("limits.process.maxOutputReplays"),
                ..
            })
        ));
        drop(pending_launch);
        assert!(vm.borrow().process_output_replays.is_empty());
        assert!(vm.borrow().process_output_replay_order.is_empty());
        let retry = vm.reserve_process_output_replay("retry").unwrap();
        drop(retry);
        assert!(vm.borrow().process_output_replays.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn process_replay_omitted_bounds_use_vm_defaults_and_explicit_excess_rejects() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        {
            let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
            vm.limits.process.output_replay_page_events = 1;
            vm.limits.process.output_replay_page_bytes = 3;
            vm.prepare_process_output_replay_admission("retained")
                .unwrap();
            vm.record_process_output("retained", crate::protocol::StreamChannel::Stdout, b"one");
            vm.record_process_output("retained", crate::protocol::StreamChannel::Stdout, b"two");
        }
        let request = RequestFrame::new(
            1,
            cleanup_test_ownership(&vm_id),
            RequestPayload::ReadProcessOutput(crate::protocol::ReadProcessOutputRequest {
                process_id: "retained".into(),
                after: None,
                max_events: 0,
                max_bytes: 0,
            }),
        );
        let RequestPayload::ReadProcessOutput(payload) = request.payload.clone() else {
            unreachable!()
        };
        let result = sidecar
            .read_process_output(&request, payload.clone())
            .await
            .unwrap();
        let ResponsePayload::ProcessOutputPage(page) = result.response.payload else {
            panic!("replay response")
        };
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.events[0].sequence, 0);
        assert_eq!(page.next_cursor, Some(0));
        assert!(page.has_more);
        for (max_events, max_bytes, path, unit, limit, requested) in [
            (
                2,
                0,
                "limits.process.outputReplayPageEvents",
                "events",
                1,
                2,
            ),
            (0, 4, "limits.process.outputReplayPageBytes", "bytes", 3, 4),
            (0, 1, "maxBytes", "bytes", 1, 3),
        ] {
            let result = sidecar
                .read_process_output(
                    &request,
                    crate::protocol::ReadProcessOutputRequest {
                        max_events,
                        max_bytes,
                        ..payload.clone()
                    },
                )
                .await
                .unwrap();
            let ResponsePayload::Rejected(rejected) = result.response.payload else {
                panic!("expected typed replay limit")
            };
            assert_eq!(rejected.code, "ERR_AGENTOS_RESOURCE_LIMIT");
            assert_eq!(rejected.configuration_path.as_deref(), Some(path));
            assert_eq!(rejected.unit.as_deref(), Some(unit));
            assert_eq!(rejected.configured_limit, Some(limit));
            assert_eq!(rejected.requested, Some(requested));
            assert_eq!(rejected.operation.as_deref(), Some("process.output.read"));
        }
        let missing = sidecar
            .read_process_output(
                &request,
                crate::protocol::ReadProcessOutputRequest {
                    process_id: "expired".into(),
                    ..payload
                },
            )
            .await
            .unwrap();
        assert!(
            matches!(missing.response.payload, ResponsePayload::Rejected(rejected) if rejected.code == "ESRCH")
        );
    }

    fn extension_tracks_process(
        sidecar: &VmManager<LocalBridge>,
        extension_session_id: &str,
        process_id: &str,
    ) -> bool {
        sidecar
            .extension_sessions
            .get(&(
                String::from("test.cleanup"),
                extension_session_id.to_owned(),
            ))
            .is_some_and(|resources| resources.process_ids.contains(process_id))
    }

    fn insert_bound_resident_cleanup_fixture(
        sidecar: &mut VmManager<LocalBridge>,
        vm_id: &str,
        execution_id: &str,
        process_id: &str,
        context: bool,
        completed: bool,
        expires_at_ms: Option<u64>,
    ) -> String {
        let extension_session_id = format!("ext-{process_id}");
        let mut vm = sidecar.vms.get_mut(vm_id).expect("cleanup test VM");
        let guest_env = vm.guest_env.clone();
        let kernel_handle = vm
            .kernel
            .create_virtual_process(
                EXECUTION_DRIVER_NAME,
                EXECUTION_DRIVER_NAME,
                crate::state::JAVASCRIPT_COMMAND,
                vec![String::from(crate::state::JAVASCRIPT_COMMAND)],
                agentos_vm_kernel::kernel::VirtualProcessOptions {
                    env: guest_env,
                    ..Default::default()
                },
            )
            .expect("spawn cleanup fixture kernel process");
        let kernel_pid = kernel_handle.pid();
        let runtime_context = vm.runtime_context.clone();
        let limits = vm.limits.clone();
        vm.active_processes.insert(
            process_id.to_owned(),
            crate::state::ActiveProcess::new(
                kernel_pid,
                kernel_handle,
                runtime_context,
                limits,
                agentos_driver_tokio::DEFAULT_PROTOCOL_MAX_PROCESS_EVENTS,
                crate::protocol::GuestRuntimeKind::JavaScript,
                crate::state::ActiveExecution::HostFunction(
                    crate::state::HostFunctionExecution::default(),
                ),
            ),
        );
        let descriptor = crate::protocol::ExecutionDescriptor {
            execution_id: execution_id.to_owned(),
            generation: 0,
            state: crate::protocol::ExecutionState::Idle,
            retained_language: None,
            process_id: None,
            pid: None,
            created_at_ms: 1,
            last_started_at_ms: None,
            last_completed_at_ms: completed.then_some(2),
            last_outcome: completed.then_some(crate::protocol::ExecutionOutcome::Succeeded),
            last_exit_code: completed.then_some(0),
        };
        let result = completed.then(|| crate::protocol::ExecutionCompletedResponse {
            execution: Some(descriptor.clone()),
            outcome: crate::protocol::ExecutionOutcome::Succeeded,
            exit_code: Some(0),
            error: None,
            stdout: None,
            stderr: None,
            stdout_truncated: None,
            stderr_truncated: None,
            evaluation_value: None,
            type_script_check_result: None,
        });
        vm.executions.insert(
            execution_id.to_owned(),
            crate::state::ManagedLanguageExecution {
                public: true,
                context,
                descriptor,
                result,
                events: VecDeque::new(),
                retained_event_bytes: 0,
                output_truncated: false,
                next_sequence: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                output_limit_bytes: 0,
                output_limit_setting: "limits.execution.maxCompletedExecutions",
                capture: crate::protocol::ExecutionOutputCapture::None,
                retain_events: false,
                event_limit: 1,
                event_bytes_limit: 1,
                uses_pty: false,
                value_kind: crate::state::ExecutionValueKind::None,
                semantic_result_path: None,
                pending_outcome: None,
                deadline_ms: None,
                expires_at_ms,
                deadline_task: None,
                resident_process_id: Some(process_id.to_owned()),
            },
        );
        vm.execution_processes
            .insert(process_id.to_owned(), execution_id.to_owned());
        drop(vm);
        sidecar
            .bind_extension_process_resource(
                cleanup_test_ownership(vm_id),
                String::from("test.cleanup"),
                extension_session_id.clone(),
                process_id.to_owned(),
            )
            .expect("bind resident process to extension session");
        extension_session_id
    }

    async fn execute_cleanup_request(
        sidecar: &mut VmManager<LocalBridge>,
        request_id: RequestId,
        vm_id: &str,
        payload: RequestPayload,
    ) -> CompletedRequest {
        sidecar
            .prepare_request_wire(wire_request(RequestFrame::new(
                request_id,
                cleanup_test_ownership(vm_id),
                payload,
            )))
            .expect("prepare cleanup request")
            .expect("cleanup request is detachable")
            .execute()
            .await
    }

    fn complete_cleanup_request(
        sidecar: &mut VmManager<LocalBridge>,
        completed: CompletedRequest,
    ) -> ResponseFrame {
        compat_response(
            sidecar
                .complete_request(completed)
                .expect("complete cleanup request"),
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retention_expiry_completion_prunes_extension_resident_process() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let process_id = "retention-expired-process";
        let extension_session_id = insert_bound_resident_cleanup_fixture(
            &mut sidecar,
            &vm_id,
            "retention-expired-execution",
            process_id,
            false,
            true,
            Some(0),
        );

        let completed = execute_cleanup_request(
            &mut sidecar,
            301,
            &vm_id,
            RequestPayload::ListExecutions(crate::protocol::ListExecutionsRequest {}),
        )
        .await;
        assert!(extension_tracks_process(
            &sidecar,
            &extension_session_id,
            process_id
        ));
        complete_cleanup_request(&mut sidecar, completed);
        assert!(!extension_tracks_process(
            &sidecar,
            &extension_session_id,
            process_id
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retention_eviction_completion_prunes_extension_resident_process() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        sidecar
            .vms
            .get_mut(&vm_id)
            .expect("cleanup test VM")
            .limits
            .execution
            .max_completed_executions = 0;
        let process_id = "retention-evicted-process";
        let extension_session_id = insert_bound_resident_cleanup_fixture(
            &mut sidecar,
            &vm_id,
            "retention-evicted-execution",
            process_id,
            false,
            true,
            None,
        );

        let completed = execute_cleanup_request(
            &mut sidecar,
            302,
            &vm_id,
            RequestPayload::ListExecutions(crate::protocol::ListExecutionsRequest {}),
        )
        .await;
        assert!(extension_tracks_process(
            &sidecar,
            &extension_session_id,
            process_id
        ));
        complete_cleanup_request(&mut sidecar, completed);
        assert!(!extension_tracks_process(
            &sidecar,
            &extension_session_id,
            process_id
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reset_execution_completion_prunes_extension_resident_process() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let execution_id = "reset-context";
        let process_id = "reset-resident-process";
        let extension_session_id = insert_bound_resident_cleanup_fixture(
            &mut sidecar,
            &vm_id,
            execution_id,
            process_id,
            true,
            false,
            None,
        );

        let completed = execute_cleanup_request(
            &mut sidecar,
            303,
            &vm_id,
            RequestPayload::ResetExecution(crate::protocol::ResetExecutionRequest {
                execution_id: execution_id.to_owned(),
            }),
        )
        .await;
        assert!(extension_tracks_process(
            &sidecar,
            &extension_session_id,
            process_id
        ));
        complete_cleanup_request(&mut sidecar, completed);
        assert!(!extension_tracks_process(
            &sidecar,
            &extension_session_id,
            process_id
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn delete_execution_completion_prunes_extension_resident_process() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let execution_id = "delete-context";
        let process_id = "delete-resident-process";
        let extension_session_id = insert_bound_resident_cleanup_fixture(
            &mut sidecar,
            &vm_id,
            execution_id,
            process_id,
            true,
            false,
            None,
        );

        let completed = execute_cleanup_request(
            &mut sidecar,
            304,
            &vm_id,
            RequestPayload::DeleteExecution(crate::protocol::DeleteExecutionRequest {
                execution_id: execution_id.to_owned(),
            }),
        )
        .await;
        assert!(extension_tracks_process(
            &sidecar,
            &extension_session_id,
            process_id
        ));
        complete_cleanup_request(&mut sidecar, completed);
        assert!(!extension_tracks_process(
            &sidecar,
            &extension_session_id,
            process_id
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reused_resident_start_failure_completion_prunes_extension_process() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let execution_id = "failed-reuse-context";
        let process_id = "failed-reuse-resident-process";
        let extension_session_id = insert_bound_resident_cleanup_fixture(
            &mut sidecar,
            &vm_id,
            execution_id,
            process_id,
            true,
            false,
            None,
        );

        let completed = execute_cleanup_request(
            &mut sidecar,
            305,
            &vm_id,
            RequestPayload::JavaScriptExecution(crate::protocol::JavaScriptExecutionRequest {
                process: crate::protocol::ProcessExecutionOptions {
                    identity: crate::protocol::ExecutionIdentityOptions {
                        context_id: Some(execution_id.to_owned()),
                    },
                    output: crate::protocol::ExecutionOutputOptions {
                        capture: None,
                        retain_events: None,
                    },
                    operation_id: None,
                    background: None,
                    cwd: None,
                    env: None,
                    args: Vec::new(),
                    stdin: None,
                    timeout_ms: None,
                    pty: None,
                },
                source: String::from("globalThis.unreachable = true"),
                format: Some(crate::protocol::JavaScriptModuleFormat::CommonJs),
                file_path: None,
                inputs: None,
            }),
        )
        .await;
        assert!(extension_tracks_process(
            &sidecar,
            &extension_session_id,
            process_id
        ));
        complete_cleanup_request(&mut sidecar, completed);
        assert!(!extension_tracks_process(
            &sidecar,
            &extension_session_id,
            process_id
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn legacy_rpc_rejects_stale_process_identity() {
        use crate::executor::backend::{
            DirectHostReplyHandle, DirectHostReplyTarget, HostCallIdentity, HostCallReply,
            HostServiceError,
        };
        #[derive(Default)]
        struct Replies(Mutex<Vec<Result<HostCallReply, HostServiceError>>>);
        impl DirectHostReplyTarget for Replies {
            fn claim(&self, _: u64) -> Result<bool, HostServiceError> {
                Ok(true)
            }
            fn respond(
                &self,
                _: u64,
                _: bool,
                reply: Result<HostCallReply, HostServiceError>,
            ) -> Result<(), HostServiceError> {
                self.0.lock().unwrap().push(reply);
                Ok(())
            }
        }
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let process_id = "replacement-process";
        insert_bound_resident_cleanup_fixture(
            &mut sidecar,
            &vm_id,
            "stale-rpc",
            process_id,
            true,
            false,
            None,
        );
        let vm = sidecar.vms.handle(&vm_id).expect("VM handle");
        let (generation, pid) = vm
            .try_read("get live process identity", |vm| {
                (vm.generation, vm.active_processes[process_id].kernel_pid)
            })
            .unwrap();
        let replies = Arc::new(Replies::default());
        let reply = DirectHostReplyHandle::new(
            HostCallIdentity {
                generation: generation + 1,
                pid,
                call_id: 1,
            },
            replies.clone(),
            65536,
        )
        .unwrap();
        sidecar
            .prepare_owned_host_rpc(
                &vm_id,
                process_id,
                &[],
                vm,
                ExecutionHostCall {
                    request: crate::executor::HostRpcRequest {
                        id: 1,
                        method: "__bench.noop".to_owned(),
                        args: vec![],
                        raw_bytes_args: Default::default(),
                    },
                    reply,
                },
            )
            .await
            .expect("settle stale reply");
        let replies = replies.0.lock().unwrap();
        assert_eq!(replies.len(), 1);
        assert_eq!(
            replies[0]
                .as_ref()
                .expect_err("stale RPC must not run")
                .code,
            "ESTALE"
        );
    }

    #[test]
    fn request_completion_transfers_extension_ownership_to_detached_children() {
        let mut sidecar = test_sidecar();
        let key = (String::from("test.extension"), String::from("session"));
        sidecar.extension_sessions.insert(
            key.clone(),
            ExtensionSessionResources {
                ownership: OwnershipScope::vm("connection", "session", "vm"),
                process_ids: BTreeSet::from([String::from("parent")]),
                vm_ids: BTreeSet::new(),
            },
        );
        let effects = RequestCompletionEffects::default();
        effects
            .record_process_exit("parent", vec![String::from("detached-child")])
            .expect("record process completion");
        sidecar.apply_request_completion_effects(&effects);
        let resources = sidecar
            .extension_sessions
            .get(&key)
            .expect("extension still owns its detached child");
        assert!(!resources.process_ids.contains("parent"));
        assert!(resources.process_ids.contains("detached-child"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn prepared_completion_prunes_extension_process_effect_even_on_error() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let process_id = "failed-request-effect-process";
        let extension_session_id = insert_bound_resident_cleanup_fixture(
            &mut sidecar,
            &vm_id,
            "failed-request-effect-execution",
            process_id,
            true,
            false,
            None,
        );
        let request = RequestFrame::new(
            306,
            cleanup_test_ownership(&vm_id),
            RequestPayload::ListExecutions(crate::protocol::ListExecutionsRequest {}),
        );
        let effects = sidecar.request_completion_effects(&request);
        effects
            .record_exited_process(process_id)
            .expect("record exited process effect");
        let prepared = PreparedRequest::from_future_with_effects(
            request,
            async {
                Err(VmError::InvalidState(String::from(
                    "failure after process cleanup",
                )))
            },
            effects,
        );

        let completed = prepared.execute().await;
        assert!(completed.failed());
        let response = complete_cleanup_request(&mut sidecar, completed);
        assert!(matches!(response.payload, ResponsePayload::Rejected(_)));
        assert!(!extension_tracks_process(
            &sidecar,
            &extension_session_id,
            process_id
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn owned_prepared_request_waits_without_borrowing_coordinator_and_preserves_identity() {
        fn assert_owned<T: 'static>() {}
        assert_owned::<PreparedRequest>();

        let mut sidecar = test_sidecar();
        let request = RequestFrame::new(
            41,
            OwnershipScope::vm("conn-1", "session-1", "vm-1"),
            RequestPayload::GetZombieTimerCount(
                crate::protocol::GetZombieTimerCountRequest::default(),
            ),
        );
        let response = crate::core::zombie_timer_count_response(&request, 7);
        let (release, wait) = tokio::sync::oneshot::channel();
        let prepared = PreparedRequest::from_future(request.clone(), async move {
            wait.await
                .map_err(|_| VmError::Execution(String::from("test gate dropped")))?;
            Ok(DispatchResult {
                response,
                events: Vec::new(),
            })
        });

        let mut task = Box::pin(prepared.execute());
        assert!(
            poll_future_once(task.as_mut()).is_none(),
            "the owned operation should remain independently pending"
        );
        release.send(()).expect("release prepared request");
        let completed = task.await;
        assert_eq!(completed.request.request_id, request.request_id);
        assert_eq!(completed.request.ownership, request.ownership);
        assert!(!completed.failed());

        let response = compat_response(
            sidecar
                .complete_request(completed)
                .expect("complete prepared request"),
        );
        assert_eq!(response.request_id, request.request_id);
        assert_eq!(response.ownership, request.ownership);
        assert!(matches!(
            response.payload,
            ResponsePayload::ZombieTimerCount(crate::protocol::ZombieTimerCountResponse {
                count: 7
            })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gated_vm_a_operation_does_not_delay_vm_b_command() {
        let mut sidecar = test_sidecar();
        let vm_ids = create_test_vms(&mut sidecar, 2).await;
        let vm_a = sidecar.vms.handle(&vm_ids[0]).expect("VM A handle");
        let vm_b = sidecar.vms.handle(&vm_ids[1]).expect("VM B handle");
        let request = RequestFrame::new(
            101,
            OwnershipScope::vm(
                "vm-handle-test-connection",
                "vm-handle-test-session",
                vm_ids[0].clone(),
            ),
            RequestPayload::GetZombieTimerCount(
                crate::protocol::GetZombieTimerCountRequest::default(),
            ),
        );
        let response = crate::core::zombie_timer_count_response(&request, 0);
        let (release, wait) = tokio::sync::oneshot::channel();
        let prepared = PreparedRequest::from_future(request, async move {
            wait.await
                .map_err(|_| VmError::Execution(String::from("test gate dropped")))?;
            vm_a.try_command("gated VM A test command", |vm| {
                vm.attached_child_event_cursor = 1;
                Ok(())
            })?;
            Ok(DispatchResult {
                response,
                events: Vec::new(),
            })
        });
        let mut vm_a_operation = Box::pin(prepared.execute());

        assert!(
            poll_future_once(vm_a_operation.as_mut()).is_none(),
            "VM A operation must be waiting at the deterministic gate"
        );
        vm_b.try_command("independent VM B test command", |vm| {
            vm.attached_child_event_cursor = 7;
            Ok(())
        })
        .expect("VM B command completes while VM A is gated");
        assert_eq!(
            vm_b.try_read("verify VM B command", |vm| vm.attached_child_event_cursor)
                .expect("read VM B"),
            7
        );

        release.send(()).expect("release VM A gate");
        let completed = vm_a_operation.await;
        assert!(!completed.failed());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn vm_execution_engines_are_partitioned_by_vm() {
        let mut sidecar = test_sidecar();
        let vm_ids = create_test_vms(&mut sidecar, 2).await;
        let vm_a = sidecar.vms.handle(&vm_ids[0]).expect("VM A handle");
        let vm_b = sidecar.vms.handle(&vm_ids[1]).expect("VM B handle");
        let engines_a = vm_a
            .try_read("clone VM A engines", |vm| vm.execution_engines.clone())
            .expect("VM A engines");
        let engines_b = vm_b
            .try_read("clone VM B engines", |vm| vm.execution_engines.clone())
            .expect("VM B engines");

        let _held_a = engines_a
            .javascript("gate VM A JavaScript engine")
            .expect("borrow VM A JavaScript engine");
        vm_a.try_command("mutate VM A state outside its held engine", |vm| {
            vm.attached_child_event_cursor = 11;
            Ok(())
        })
        .expect("VM A state remains accessible while its engine is held");
        assert_eq!(
            vm_a.try_read("verify VM A state outside its held engine", |vm| {
                vm.attached_child_event_cursor
            })
            .expect("read VM A state while its engine is held"),
            11
        );
        let _independent_b = engines_b
            .javascript("enter VM B JavaScript engine")
            .expect("VM B engine remains independently available");
        let conflict = engines_a
            .javascript("re-enter VM A JavaScript engine")
            .expect_err("same-VM engine re-entry must be a typed conflict");
        assert!(conflict
            .to_string()
            .contains("ERR_AGENTOS_VM_EXECUTION_CONFLICT"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_and_language_routes_are_prepared_without_central_fallback() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1)
            .await
            .pop()
            .expect("test VM id");
        let ownership = OwnershipScope::vm(
            "vm-handle-test-connection",
            "vm-handle-test-session",
            vm_id.clone(),
        );
        let execute = RequestFrame::new(
            111,
            ownership.clone(),
            RequestPayload::Execute(ExecuteRequest {
                retain_output: false,

                process_id: String::from("prepared-execute"),
                wasm_backend: None,
                command: Some(String::from("node")),
                runtime: None,
                entrypoint: None,
                args: vec![String::from("-e"), String::from("0")],
                env: Default::default(),
                cwd: None,
                wasm_permission_tier: None,
            }),
        );
        let prepared_execute = sidecar
            .prepare_request_wire(wire_request(execute))
            .expect("prepare execute")
            .expect("Execute must not use coordinated fallback");
        drop(prepared_execute);

        let lifecycle = RequestFrame::new(
            112,
            ownership,
            RequestPayload::ListExecutions(crate::protocol::ListExecutionsRequest {}),
        );
        let prepared_lifecycle = sidecar
            .prepare_request_wire(wire_request(lifecycle))
            .expect("prepare language lifecycle")
            .expect("ExecutionLifecycle must not use coordinated fallback");
        drop(prepared_lifecycle);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn prepared_filesystem_command_avoids_central_fallback() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1)
            .await
            .pop()
            .expect("test VM id");
        let request = RequestFrame::new(
            102,
            OwnershipScope::vm("vm-handle-test-connection", "vm-handle-test-session", vm_id),
            RequestPayload::GuestFilesystemCall(GuestFilesystemCallRequest {
                operation: crate::protocol::GuestFilesystemOperation::Stat,
                path: String::from("/"),
                destination_path: None,
                target: None,
                content: None,
                encoding: None,
                recursive: false,
                max_depth: None,
                mode: None,
                uid: None,
                gid: None,
                atime_ms: None,
                mtime_ms: None,
                len: None,
                offset: None,
            }),
        );

        let prepared = sidecar
            .prepare_request_wire(wire_request(request))
            .expect("prepare filesystem command")
            .expect("filesystem command is detachable");
        drop(prepared);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn invalid_owned_guest_calls_become_prepared_rejections() {
        let mut sidecar = test_sidecar();
        let filesystem = RequestFrame::new(
            103,
            OwnershipScope::connection("not-vm-owned"),
            RequestPayload::GuestFilesystemCall(GuestFilesystemCallRequest {
                operation: crate::protocol::GuestFilesystemOperation::Stat,
                path: String::from("/"),
                destination_path: None,
                target: None,
                content: None,
                encoding: None,
                recursive: false,
                max_depth: None,
                mode: None,
                uid: None,
                gid: None,
                atime_ms: None,
                mtime_ms: None,
                len: None,
                offset: None,
            }),
        );
        let filesystem = sidecar
            .prepare_request_wire(wire_request(filesystem))
            .expect("invalid filesystem ownership is not a router failure")
            .expect("filesystem call is always prepared")
            .execute()
            .await;
        assert!(filesystem.failed());

        let kernel = RequestFrame::new(
            104,
            OwnershipScope::connection("not-vm-owned"),
            RequestPayload::GuestKernelCall(crate::protocol::GuestKernelCallRequest {
                execution_id: String::from("missing-execution"),
                operation: String::from("noop"),
                payload: Vec::new(),
            }),
        );
        let kernel = sidecar
            .prepare_request_wire(wire_request(kernel))
            .expect("invalid kernel ownership is not a router failure")
            .expect("kernel call is always prepared")
            .execute()
            .await;
        assert!(kernel.failed());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn overlapping_vm_commands_return_typed_conflict_without_panicking() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1)
            .await
            .pop()
            .expect("test VM id");
        let outer = sidecar.vms.handle(&vm_id).expect("outer VM handle");
        let nested = outer.clone();

        let error = outer
            .try_command("outer test command", |_| {
                nested.try_command("nested test command", |_| Ok(()))
            })
            .expect_err("nested command must report a typed conflict");
        assert!(error
            .to_string()
            .contains("ERR_AGENTOS_VM_COORDINATOR_CONFLICT"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn live_vm_handle_makes_disposal_conflict_typed_and_non_destructive() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1)
            .await
            .pop()
            .expect("test VM id");
        let live_handle = sidecar.vms.handle(&vm_id).expect("live VM handle");

        let error = match sidecar.vms.try_remove(&vm_id, "dispose") {
            Ok(_) => panic!("live handle must exclude disposal"),
            Err(error) => error,
        };
        assert!(error
            .to_string()
            .contains("ERR_AGENTOS_VM_COORDINATOR_CONFLICT"));
        assert!(sidecar.vms.contains_key(&vm_id));

        drop(live_handle);
        assert!(sidecar
            .vms
            .try_remove(&vm_id, "dispose")
            .expect("removal after handle release")
            .is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn preparation_keeps_cross_connection_ownership_rejection_terminal() {
        let mut sidecar = test_sidecar();
        sidecar.connections.insert(
            String::from("owner"),
            ConnectionState {
                auth_token: String::new(),
                sessions: BTreeSet::from([String::from("session-1")]),
            },
        );
        sidecar.connections.insert(
            String::from("attacker"),
            ConnectionState {
                auth_token: String::new(),
                sessions: BTreeSet::new(),
            },
        );
        sidecar.sessions.insert(
            String::from("session-1"),
            SessionState {
                connection_id: String::from("owner"),
                placement: crate::protocol::SidecarPlacement::SidecarPlacementShared(
                    crate::protocol::SidecarPlacementShared { pool: None },
                ),
                metadata: BTreeMap::new(),
                vm_ids: BTreeSet::from([String::from("vm-1")]),
            },
        );
        let request = RequestFrame::new(
            77,
            OwnershipScope::vm("attacker", "session-1", "vm-1"),
            RequestPayload::ProvidedCommands(crate::protocol::ProvidedCommandsRequest {}),
        );

        let prepared = sidecar
            .prepare_request_wire(wire_request(request.clone()))
            .expect("prepare query route")
            .expect("query route is detachable");
        let completed = prepared.execute().await;
        assert!(completed.failed());
        let response = compat_response(
            sidecar
                .complete_request(completed)
                .expect("ownership error becomes a canonical response"),
        );
        assert_eq!(response.request_id, request.request_id);
        assert_eq!(response.ownership, request.ownership);
        match response.payload {
            ResponsePayload::Rejected(rejected) => {
                assert!(rejected
                    .message
                    .contains("session session-1 is not owned by connection attacker"));
            }
            payload => panic!("expected ownership rejection, got {payload:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn short_coordinator_routes_and_unknown_extension_never_fall_back() {
        let mut sidecar = test_sidecar();
        let invalid_authenticate = RequestFrame::new(
            200,
            OwnershipScope::connection("client-hint"),
            RequestPayload::Authenticate(crate::protocol::AuthenticateRequest {
                client_name: String::from("invalid-prepared-route-test"),
                auth_token: String::new(),
                protocol_version: crate::protocol::PROTOCOL_VERSION + 1,
                bridge_version: agentos_vm_host_interface::bridge_contract().version,
            }),
        );
        let invalid_authenticate = sidecar
            .prepare_request_wire(wire_request(invalid_authenticate))
            .expect("invalid authenticate is a prepared rejection")
            .expect("authenticate is always prepared")
            .execute()
            .await;
        assert!(invalid_authenticate.failed());

        let unauthenticated_open = RequestFrame::new(
            201,
            OwnershipScope::connection("unknown-connection"),
            RequestPayload::OpenSession(OpenSessionRequest {
                placement: crate::protocol::SidecarPlacement::SidecarPlacementShared(
                    crate::protocol::SidecarPlacementShared { pool: None },
                ),
                metadata: std::collections::HashMap::new(),
            }),
        );
        let unauthenticated_open = sidecar
            .prepare_request_wire(wire_request(unauthenticated_open))
            .expect("unauthenticated open is a prepared rejection")
            .expect("open session is always prepared")
            .execute()
            .await;
        assert!(unauthenticated_open.failed());

        let authenticate = RequestFrame::new(
            202,
            OwnershipScope::connection("client-hint"),
            RequestPayload::Authenticate(crate::protocol::AuthenticateRequest {
                client_name: String::from("prepared-route-test"),
                auth_token: String::new(),
                protocol_version: crate::protocol::PROTOCOL_VERSION,
                bridge_version: agentos_vm_host_interface::bridge_contract().version,
            }),
        );
        let authenticated = sidecar
            .prepare_request_wire(wire_request(authenticate))
            .expect("prepare authenticate")
            .expect("authenticate is always prepared");
        assert!(sidecar.connections.is_empty());
        assert_eq!(sidecar.next_connection_id, 0);
        let authenticated = authenticated.execute().await;
        assert!(
            sidecar.connections.is_empty(),
            "executing the owned response must not mutate central membership before completion"
        );
        let authenticated = compat_response(
            sidecar
                .complete_request(authenticated)
                .expect("complete authenticate"),
        );
        let ResponsePayload::Authenticated(authenticated) = authenticated.payload else {
            panic!("authenticate returned a different response");
        };
        assert!(sidecar
            .connections
            .contains_key(&authenticated.connection_id));

        let open_session = RequestFrame::new(
            203,
            OwnershipScope::connection(authenticated.connection_id.clone()),
            RequestPayload::OpenSession(OpenSessionRequest {
                placement: crate::protocol::SidecarPlacement::SidecarPlacementShared(
                    crate::protocol::SidecarPlacementShared { pool: None },
                ),
                metadata: std::collections::HashMap::new(),
            }),
        );
        let opened = sidecar
            .prepare_request_wire(wire_request(open_session))
            .expect("prepare open session")
            .expect("open session is always prepared");
        assert!(sidecar.sessions.is_empty());
        assert_eq!(sidecar.next_session_id, 0);
        let opened = opened.execute().await;
        assert!(
            sidecar.sessions.is_empty(),
            "owned response execution stages central session membership until completion"
        );
        let opened = compat_response(
            sidecar
                .complete_request(opened)
                .expect("complete open session"),
        );
        assert!(matches!(opened.payload, ResponsePayload::SessionOpened(_)));

        let unknown_extension = RequestFrame::new(
            204,
            OwnershipScope::connection(authenticated.connection_id),
            RequestPayload::Ext(ExtEnvelope {
                namespace: String::from("missing.prepared.extension"),
                payload: Vec::new(),
            }),
        );
        let rejected = sidecar
            .prepare_request_wire(wire_request(unknown_extension))
            .expect("prepare unknown extension")
            .expect("unknown extension has a prepared canonical rejection")
            .execute()
            .await;
        let rejected = compat_response(
            sidecar
                .complete_request(rejected)
                .expect("complete unknown extension rejection"),
        );
        assert!(matches!(
            rejected.payload,
            ResponsePayload::Rejected(RejectedResponse { ref code, .. }) if code == "unknown_extension"
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn host_callback_registration_and_snapshot_queries_never_fall_back() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1)
            .await
            .pop()
            .expect("test VM id");
        let ownership = OwnershipScope::vm(
            "vm-handle-test-connection",
            "vm-handle-test-session",
            &vm_id,
        );
        let registration = RequestFrame::new(
            204,
            ownership.clone(),
            RequestPayload::RegisterHostCallbacks(crate::protocol::RegisterHostCallbacksRequest {
                name: String::from("prepared-host-functions"),
                description: String::from("prepared route test host_functions"),
                command_aliases: vec![String::from("agentos-prepared-host-functions")],
                registry_command_aliases: vec![String::from("agentos")],
                callbacks: std::collections::HashMap::from([(
                    String::from("call"),
                    crate::protocol::RegisteredHostCallbackDefinition {
                        description: String::from("prepared route test callback"),
                        input_schema: String::from(
                            r#"{"type":"object","additionalProperties":false}"#,
                        ),
                        timeout_ms: None,
                        examples: Vec::new(),
                    },
                )]),
            }),
        );
        let prepared_registration = sidecar
            .prepare_request_wire(wire_request(registration))
            .expect("prepare host callback registration")
            .expect("host callback registration is prepared");
        assert!(
            !sidecar
                .vms
                .get(&vm_id)
                .expect("test VM")
                .host_functions
                .contains_key("prepared-host-functions"),
            "preparing host callback registration must not mutate VM state"
        );
        let completed_registration = prepared_registration.execute().await;
        assert!(
            !completed_registration.failed(),
            "owned registration failed: {:?}",
            completed_registration.result.as_ref().err()
        );
        assert!(
            sidecar
                .vms
                .get(&vm_id)
                .expect("test VM")
                .host_functions
                .contains_key("prepared-host-functions"),
            "owned host callback registration runs only when its prepared future executes"
        );
        sidecar
            .complete_request(completed_registration)
            .expect("complete host callback registration");

        let queries = [
            RequestPayload::GetProcessSnapshot(crate::protocol::GetProcessSnapshotRequest {}),
            RequestPayload::GetResourceSnapshot(crate::protocol::GetResourceSnapshotRequest {}),
            RequestPayload::GetZombieTimerCount(
                crate::protocol::GetZombieTimerCountRequest::default(),
            ),
            RequestPayload::ProvidedCommands(crate::protocol::ProvidedCommandsRequest {}),
            RequestPayload::ListMounts(crate::protocol::ListMountsRequest {}),
        ];
        for (index, payload) in queries.into_iter().enumerate() {
            let request = RequestFrame::new(210 + index as RequestId, ownership.clone(), payload);
            assert!(sidecar
                .prepare_request_wire(wire_request(request))
                .expect("prepare snapshot/list query")
                .is_some());
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configure_vm_never_elevates_guest_permissions() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let guest_policy = deny_all_policy();
        sidecar
            .bridge
            .set_vm_permissions(&vm_id, &guest_policy)
            .expect("apply guest denial");
        sidecar
            .vms
            .get_mut(&vm_id)
            .expect("test VM")
            .configuration
            .permissions = guest_policy.clone();
        {
            let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
            let filesystem = vm.kernel.filesystem_mut().inner_mut();
            filesystem.mkdir("/__agentos/commands/0", true).unwrap();
            filesystem
                .write_file("/__agentos/commands/0/operator-discovered", Vec::new())
                .unwrap();
        }
        let history_start = sidecar
            .bridge
            .set_vm_permissions_history
            .lock()
            .expect("permission history")
            .len();
        let mut payload = crate::protocol::ConfigureVmRequest {
            mounts: vec![crate::protocol::MountDescriptor {
                guest_path: String::from("/operator-mount"),
                guest_source: String::from("operator-test"),
                guest_fstype: String::from("operator-test"),
                read_only: true,
                plugin: crate::protocol::MountPluginDescriptor {
                    id: String::from("agentos_packages"),
                    config: String::from(
                        r#"{"kind":"singleSymlink","target":"/bin/node","readOnly":true}"#,
                    ),
                },
            }],
            software: Vec::new(),
            permissions: None,
            module_access_cwd: None,
            instructions: Vec::new(),
            projected_modules: Vec::new(),
            command_permissions: Default::default(),
            loopback_exempt_ports: Vec::new(),
            packages: Vec::new(),
            packages_mount_at: String::new(),
            bootstrap_commands: Vec::new(),
            host_function_shim_commands: Vec::new(),
        };
        let request = RequestFrame::new(
            230,
            cleanup_test_ownership(&vm_id),
            RequestPayload::ConfigureVm(payload.clone()),
        );
        sidecar
            .configure_vm(&request, payload.clone())
            .await
            .expect("trusted configuration under guest denial");
        assert!(sidecar
            .vms
            .get(&vm_id)
            .unwrap()
            .kernel
            .commands()
            .contains_key("operator-discovered"));
        let prior_guest_env = {
            let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
            vm.guest_env
                .insert(String::from("PRESERVED_ON_REJECTION"), String::from("yes"));
            vm.guest_env.clone()
        };
        let mut invalid = payload.clone();
        invalid.mounts[0].plugin.config = String::from("{");
        let error = sidecar.configure_vm(&request, invalid).await.unwrap_err();
        assert!(error.to_string().contains("not valid JSON"));
        let mut invalid = payload.clone();
        invalid.mounts[0].plugin.id = String::from("unregistered-plugin");
        let error = sidecar.configure_vm(&request, invalid).await.unwrap_err();
        assert!(error.to_string().contains("not registered"));
        let mut invalid = payload.clone();
        invalid.mounts[0].guest_path = String::from("/");
        let error = sidecar.configure_vm(&request, invalid).await.unwrap_err();
        assert!(error
            .to_string()
            .contains("invalid or duplicate VM mount path"));
        // A valid replacement must also be reversible when a later plugin
        // fails to open after the old mount and one new mount were changed.
        let mut invalid = payload.clone();
        invalid.mounts[0].guest_path = String::from("/replacement-mount");
        let mut broken = invalid.mounts[0].clone();
        broken.guest_path = String::from("/broken-mount");
        broken.plugin.config = String::from(r#"{"kind":"singleSymlink"}"#);
        invalid.mounts.push(broken);
        let error = sidecar.configure_vm(&request, invalid).await.unwrap_err();
        assert!(error.to_string().contains("target"), "{error}");
        assert!(
            !sidecar
                .vms
                .get(&vm_id)
                .unwrap()
                .kernel
                .exists_for_operator("/replacement-mount")
                .unwrap(),
            "failed replacements must not leave an empty mountpoint"
        );
        {
            let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
            vm.kernel
                .filesystem_mut()
                .inner_mut()
                .mkdir("/hidden/keep", true)
                .unwrap();
        }
        let mut nested = payload.clone();
        let mut parent = payload.mounts[0].clone();
        parent.guest_path = "/hidden".into();
        parent.read_only = false;
        parent.plugin.id = "memory".into();
        parent.plugin.config = "{}".into();
        let mut child = parent.clone();
        child.guest_path = "/hidden/keep/new".into();
        let mut broken = payload.mounts[0].clone();
        broken.guest_path = "/failure/invalid/deep/leaf".into();
        broken.plugin.config = r#"{"kind":"singleSymlink"}"#.into();
        nested.mounts = vec![parent, child, broken];
        let error = sidecar.configure_vm(&request, nested).await.unwrap_err();
        assert!(error.to_string().contains("target"), "{error}");
        assert!(!error.to_string().contains("rollback failed"), "{error}");
        {
            let vm = sidecar.vms.get(&vm_id).unwrap();
            assert!(
                vm.kernel.exists_for_operator("/hidden/keep").unwrap(),
                "rollback must preserve empty directories hidden below temporary parents"
            );
            assert!(!vm.kernel.exists_for_operator("/hidden/keep/new").unwrap());
        }
        assert!(sidecar
            .vms
            .get(&vm_id)
            .unwrap()
            .kernel
            .mounted_filesystems()
            .iter()
            .any(|mount| mount.path == "/operator-mount"));
        assert!(!sidecar
            .vms
            .get(&vm_id)
            .unwrap()
            .kernel
            .mounted_filesystems()
            .iter()
            .any(|mount| mount.path == "/replacement-mount"));
        assert_eq!(
            sidecar.vms.get(&vm_id).unwrap().configuration.mounts,
            payload.mounts
        );
        assert_eq!(sidecar.vms.get(&vm_id).unwrap().guest_env, prior_guest_env);
        payload.mounts.clear();
        sidecar
            .configure_vm(&request, payload)
            .await
            .expect("trusted unmount under guest denial");
        let registration = crate::protocol::RegisterHostCallbacksRequest {
            name: String::from("operator-bindings"),
            description: String::from("operator test"),
            command_aliases: vec![String::from("operator-callback")],
            registry_command_aliases: Vec::new(),
            callbacks: std::collections::HashMap::from([(
                String::from("call"),
                crate::protocol::RegisteredHostCallbackDefinition {
                    description: String::from("operator callback"),
                    input_schema: String::from(r#"{"type":"object"}"#),
                    timeout_ms: None,
                    examples: Vec::new(),
                },
            )]),
        };
        let registration_request = RequestFrame::new(
            231,
            cleanup_test_ownership(&vm_id),
            RequestPayload::RegisterHostCallbacks(registration.clone()),
        );
        crate::host_functions::register_host_callbacks(
            &mut sidecar,
            &registration_request,
            registration,
        )
        .await
        .expect("trusted binding stubs under guest denial");
        {
            let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
            assert!(vm.kernel.commands().contains_key("operator-callback"));
            assert!(vm.kernel.read_dir("/__agentos/commands/0").is_err());
            assert!(vm.kernel.read_file("/bin/operator-callback").is_err());
        }
        let history = sidecar
            .bridge
            .set_vm_permissions_history
            .lock()
            .expect("permission history");
        assert!(history[history_start..]
            .iter()
            .all(|(id, policy)| id != &vm_id || policy == &guest_policy));
        assert_eq!(
            sidecar.bridge.permissions.lock().unwrap().get(&vm_id),
            Some(&guest_policy)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unlink_software_cleans_operator_mountpoints_under_guest_denial() {
        use agentos_vm_kernel::mount_table::{MountOptions, MountedVirtualFileSystem};
        use agentos_vm_kernel::vfs::MemoryFileSystem;

        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let guest_policy = deny_all_policy();
        sidecar
            .bridge
            .set_vm_permissions(&vm_id, &guest_policy)
            .unwrap();
        let paths = [
            "/opt/agentos/pkgs/operator-cleanup/1.0.0",
            "/opt/agentos/pkgs/operator-cleanup/current",
            "/opt/agentos/bin/operator-software",
        ];
        {
            let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
            vm.configuration.permissions = guest_policy.clone();
            vm.configuration.defaults_profile = agentos_vm_config::VmDefaultsProfile::AgentOs;
            for path in paths {
                vm.kernel
                    .mount_boxed_filesystem_for_operator(
                        path,
                        Box::new(MountedVirtualFileSystem::new(MemoryFileSystem::new())),
                        MountOptions::new("operator-test"),
                    )
                    .unwrap();
                vm.configuration
                    .mounts
                    .push(crate::protocol::MountDescriptor {
                        guest_path: path.into(),
                        guest_source: String::from("operator-test"),
                        guest_fstype: String::from("operator-test"),
                        read_only: true,
                        plugin: crate::protocol::MountPluginDescriptor {
                            id: String::from("operator-test"),
                            config: String::from("{}"),
                        },
                    });
            }
            vm.package_descriptors.push((
                String::from("operator-package"),
                crate::package_projection::PackageDescriptor {
                    name: String::from("operator-cleanup"),
                    version: String::from("1.0.0"),
                    dir: String::from("/operator-fixture-not-read-during-unlink"),
                    tar_path: None,
                    provides: None,
                    commands: vec![crate::package_projection::PackageCommandTarget {
                        command: String::from("operator-software"),
                        entry: String::from("bin/run"),
                    }],
                    man_pages: Vec::new(),
                },
            ));
            vm.package_created_mountpoints.insert(
                String::from("operator-package"),
                paths.map(String::from).into_iter().collect(),
            );
            vm.package_mount_roots.insert(
                String::from("operator-package"),
                String::from("/opt/agentos"),
            );
            vm.package_mount_paths.insert(
                String::from("operator-package"),
                paths.map(String::from).into_iter().collect(),
            );
        }
        let payload = crate::protocol::UnlinkPackageRequest {
            package_id: String::from("operator-package"),
        };
        let request = RequestFrame::new(
            233,
            cleanup_test_ownership(&vm_id),
            RequestPayload::UnlinkPackage(payload.clone()),
        );
        sidecar
            .unlink_package(&request, payload)
            .await
            .expect("unlink under guest denial");
        let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
        assert!(vm.package_descriptors.is_empty());
        assert!(vm.package_created_mountpoints.is_empty());
        for path in paths {
            assert!(
                !vm.kernel.exists_for_operator(path).unwrap(),
                "left mountpoint {path}"
            );
        }
        for command in ["node", "python", "python3", "wasm", "npm", "npx"] {
            assert!(
                vm.kernel.commands().contains_key(command),
                "lost profile command {command}"
            );
        }
        assert!(vm.kernel.read_dir("/opt/agentos").is_err());
        assert_eq!(vm.configuration.permissions, guest_policy);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn link_package_rejects_duplicate_owned_mount_paths_before_mutation() {
        let package_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(package_dir.path().join("bin")).unwrap();
        std::fs::create_dir_all(package_dir.path().join("share/config")).unwrap();
        std::fs::write(
            package_dir.path().join("agentos-package.json"),
            r#"{"name":"duplicate-path","version":"1.0.0","provides":{"files":[{"source":"share/config","target":"/opt/agentos/pkgs/duplicate-path/1.0.0/"}]}}"#,
        )
        .unwrap();
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let initial_mount_count = sidecar.vms.get(&vm_id).unwrap().configuration.mounts.len();
        let link = crate::protocol::LinkPackageRequest {
            package: crate::protocol::PackageDescriptor {
                path: package_dir.path().to_string_lossy().into_owned(),
            },
            package_id: String::from("duplicate-path-test"),
        };
        let request = RequestFrame::new(
            262,
            cleanup_test_ownership(&vm_id),
            RequestPayload::LinkPackage(link.clone()),
        );
        let error = sidecar.link_package(&request, link).await.unwrap_err();
        assert!(error.to_string().contains("duplicate package mount path"));
        let vm = sidecar.vms.get(&vm_id).unwrap();
        assert!(vm.package_descriptors.is_empty());
        assert!(vm.package_mount_paths.is_empty());
        assert_eq!(vm.configuration.mounts.len(), initial_mount_count);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn link_package_rolls_back_earlier_leaves_when_a_later_mount_fails() {
        let package_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(package_dir.path().join("share/config")).unwrap();
        std::fs::write(package_dir.path().join("blocked"), b"not a directory").unwrap();
        std::fs::write(package_dir.path().join("agentos-package.json"),
            r#"{"name":"failed-leaf","version":"1.0.0","provides":{"files":[{"source":"share/config","target":"/opt/agentos/pkgs/failed-leaf/1.0.0/blocked/child"}]}}"#).unwrap();
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let initial_mounts = sidecar
            .vms
            .get(&vm_id)
            .unwrap()
            .kernel
            .mounted_filesystems();
        let link = crate::protocol::LinkPackageRequest {
            package: crate::protocol::PackageDescriptor {
                path: package_dir.path().to_string_lossy().into_owned(),
            },
            package_id: "failed-leaf".into(),
        };
        let request = RequestFrame::new(
            263,
            cleanup_test_ownership(&vm_id),
            RequestPayload::LinkPackage(link.clone()),
        );
        let error = sidecar.link_package(&request, link).await.unwrap_err();
        assert!(error.to_string().contains("ENOTDIR"), "{error}");
        assert!(!error.to_string().contains("rollback failed"), "{error}");
        let vm = sidecar.vms.get(&vm_id).unwrap();
        assert_eq!(vm.kernel.mounted_filesystems(), initial_mounts);
        assert!(vm.package_descriptors.is_empty());
        assert!(vm.package_mount_paths.is_empty());
        assert!(!vm
            .kernel
            .exists_for_operator("/opt/agentos/pkgs/failed-leaf")
            .unwrap());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn package_mount_limit_covers_boot_provides_and_live_links() {
        let first_dir = tempfile::tempdir().expect("first package directory");
        std::fs::create_dir_all(first_dir.path().join("share/config")).unwrap();
        std::fs::write(
            first_dir.path().join("agentos-package.json"),
            r#"{"name":"first-limit-test","version":"1.0.0","provides":{"files":[{"source":"share/config","target":"/etc/first-limit-test"}]}}"#,
        )
        .unwrap();
        let second_dir = tempfile::tempdir().expect("second package directory");
        std::fs::write(
            second_dir.path().join("agentos-package.json"),
            r#"{"name":"second-limit-test","version":"1.0.0"}"#,
        )
        .unwrap();

        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let ownership = cleanup_test_ownership(&vm_id);
        let first = crate::protocol::PackageDescriptor {
            path: first_dir.path().to_string_lossy().into_owned(),
        };
        let configure = crate::protocol::ConfigureVmRequest {
            mounts: vec![crate::protocol::MountDescriptor {
                guest_path: String::from("/unrelated-mount"),
                guest_source: String::from("operator-test"),
                guest_fstype: String::from("operator-test"),
                read_only: true,
                plugin: crate::protocol::MountPluginDescriptor {
                    id: String::from("agentos_packages"),
                    config: String::from(
                        r#"{"kind":"singleSymlink","target":"/tmp","readOnly":true}"#,
                    ),
                },
            }],
            software: Vec::new(),
            permissions: None,
            module_access_cwd: None,
            instructions: Vec::new(),
            projected_modules: Vec::new(),
            command_permissions: Default::default(),
            loopback_exempt_ports: Vec::new(),
            packages: vec![first.clone()],
            packages_mount_at: String::new(),
            bootstrap_commands: Vec::new(),
            host_function_shim_commands: Vec::new(),
        };
        let configure_request = RequestFrame::new(
            260,
            ownership.clone(),
            RequestPayload::ConfigureVm(configure.clone()),
        );
        sidecar
            .vms
            .get_mut(&vm_id)
            .unwrap()
            .limits
            .agentos_packages
            .max_mounts = 2;
        let error = sidecar
            .configure_vm(&configure_request, configure.clone())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            VmError::PackageMountLimit {
                used: 0,
                requested: 3,
                limit: 2
            }
        ));
        assert!(sidecar
            .vms
            .get(&vm_id)
            .unwrap()
            .package_descriptors
            .is_empty());

        sidecar
            .vms
            .get_mut(&vm_id)
            .unwrap()
            .limits
            .agentos_packages
            .max_mounts = 4;
        sidecar
            .configure_vm(&configure_request, configure)
            .await
            .expect("boot package within limit");
        // Existing usage is the installed projection, not whatever its source
        // happens to contain now. Reopening the old provides path would fail.
        std::fs::rename(
            first_dir.path().join("share/config"),
            first_dir.path().join("share/config-moved"),
        )
        .unwrap();
        let live = crate::protocol::LinkPackageRequest {
            package: crate::protocol::PackageDescriptor {
                path: second_dir.path().to_string_lossy().into_owned(),
            },
            package_id: String::from("second-limit-test"),
        };
        let live_request =
            RequestFrame::new(261, ownership, RequestPayload::LinkPackage(live.clone()));
        let error = sidecar
            .link_package(&live_request, live.clone())
            .await
            .unwrap_err();
        assert!(matches!(
            &error,
            VmError::PackageMountLimit {
                used: 3,
                requested: 2,
                limit: 4
            }
        ));
        let rejected = sidecar.reject_error(&live_request, &error);
        let ResponsePayload::Rejected(rejected) = rejected.payload else {
            panic!("expected structured package limit rejection");
        };
        assert_eq!(rejected.code, "ERR_AGENTOS_RESOURCE_LIMIT");
        assert_eq!(rejected.limit_name.as_deref(), Some("packageMounts"));
        assert_eq!(rejected.current_usage, Some(3));
        assert_eq!(rejected.requested, Some(2));
        assert_eq!(rejected.configured_limit, Some(4));
        assert_eq!(
            rejected.configuration_path.as_deref(),
            Some("limits.agentosPackages.maxMounts")
        );
        assert_eq!(rejected.errno.as_deref(), Some("ENOSPC"));
        assert_eq!(
            sidecar.vms.get(&vm_id).unwrap().package_descriptors.len(),
            1
        );

        sidecar
            .vms
            .get_mut(&vm_id)
            .unwrap()
            .limits
            .agentos_packages
            .max_mounts = 5;
        sidecar
            .link_package(&live_request, live)
            .await
            .expect("raised limit admits live link");
        assert_eq!(
            sidecar.vms.get(&vm_id).unwrap().package_descriptors.len(),
            2
        );

        // Removal uses the installed path set, remains available below the
        // old package size, and releases capacity for the next link.
        sidecar
            .vms
            .get_mut(&vm_id)
            .unwrap()
            .limits
            .agentos_packages
            .max_mounts = 1;
        for package_id in [
            format!("path:{}", first.path),
            String::from("second-limit-test"),
        ] {
            let unlink = crate::protocol::UnlinkPackageRequest { package_id };
            sidecar.unlink_package(&live_request, unlink).await.unwrap();
        }
        {
            let vm = sidecar.vms.get(&vm_id).unwrap();
            assert!(vm.package_mount_paths.is_empty());
            assert_eq!(vm.configuration.mounts.len(), 1, "unrelated mount remains");
        }
        std::fs::rename(
            first_dir.path().join("share/config-moved"),
            first_dir.path().join("share/config"),
        )
        .unwrap();
        sidecar
            .vms
            .get_mut(&vm_id)
            .unwrap()
            .limits
            .agentos_packages
            .max_mounts = 3;
        sidecar
            .link_package(
                &live_request,
                crate::protocol::LinkPackageRequest {
                    package: first,
                    package_id: String::from("relinked-first"),
                },
            )
            .await
            .expect("unlink releases the complete package budget");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn verified_install_pins_immutable_bytes_until_unlink() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("installed.aospkg");
        let mut tar = tar::Builder::new(Vec::new());
        let manifest = br#"{"name":"installed-test","version":"1.0.0"}"#;
        let mut header = tar::Header::new_gnu();
        header.set_size(manifest.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "agentos-package.json", &manifest[..])
            .unwrap();
        let bytes = agentos_vfs_core::package_format::pack::pack_aospkg_from_tar_bytes(
            &tar.into_inner().unwrap(),
        )
        .unwrap()
        .0;
        std::fs::write(&source, bytes).unwrap();

        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let ownership = cleanup_test_ownership(&vm_id);
        let payload = crate::protocol::InstallPackageRequest {
            acquisition: crate::protocol::AcquirePackageRequest {
                source: crate::protocol::PackageAcquisitionSource::PackagePathSource(
                    crate::protocol::PackagePathSource {
                        path: source.to_string_lossy().into_owned(),
                        expected_digest: None,
                    },
                ),
                advisory: false,
                timeout_ms: None,
                max_package_bytes: None,
                download_timeout_ms: None,
                connect_timeout_ms: None,
                max_redirects: None,
                allow_insecure_local_http: false,
            },
        };
        let request = RequestFrame::new(
            273,
            ownership.clone(),
            RequestPayload::InstallPackage(payload.clone()),
        );
        let installed = sidecar
            .install_package(&request, payload.clone())
            .await
            .expect("install verified package");
        let ResponsePayload::PackageInstalled(installed) = installed.response.payload else {
            panic!("expected installed package response");
        };
        let package_id = installed.package.package_id;
        let cached_path = sidecar
            .vms
            .get(&vm_id)
            .unwrap()
            .installed_package_pins
            .get(&package_id)
            .expect("VM must pin installed bytes")
            .path()
            .to_path_buf();
        assert_ne!(cached_path, source);
        std::fs::remove_file(&source).unwrap();
        assert!(
            cached_path.exists(),
            "installed bytes survive source removal"
        );

        let mut missing_payload = payload;
        if let crate::protocol::PackageAcquisitionSource::PackagePathSource(source) =
            &mut missing_payload.acquisition.source
        {
            source.expected_digest = Some(format!("sha256:{}", "0".repeat(64)));
        }
        let missing_request = RequestFrame::new(
            275,
            ownership.clone(),
            RequestPayload::InstallPackage(missing_payload.clone()),
        );
        let rejected = sidecar
            .install_package(&missing_request, missing_payload)
            .await
            .expect("acquisition failure is a typed response");
        assert!(matches!(
            rejected.response.payload,
            ResponsePayload::Rejected(_)
        ));
        assert!(sidecar
            .vms
            .get(&vm_id)
            .unwrap()
            .installed_package_pins
            .contains_key(&package_id));

        let unlink = crate::protocol::UnlinkPackageRequest {
            package_id: package_id.clone(),
        };
        let request = RequestFrame::new(
            274,
            ownership,
            RequestPayload::UnlinkPackage(unlink.clone()),
        );
        sidecar
            .unlink_package(&request, unlink)
            .await
            .expect("unlink installed package");
        let vm = sidecar.vms.get(&vm_id).unwrap();
        assert!(!vm.installed_package_pins.contains_key(&package_id));
        assert!(!vm.runtime_linked_package_ids.contains(&package_id));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configuring_mounts_preserves_runtime_linked_package_until_unlink() {
        let package_dir = tempfile::tempdir().expect("package fixture directory");
        std::fs::create_dir_all(package_dir.path().join("bin")).unwrap();
        std::fs::write(
            package_dir.path().join("agentos-package.json"),
            r#"{"name":"dynamic-test","version":"1.0.0"}"#,
        )
        .unwrap();
        std::fs::write(package_dir.path().join("bin/dynamic-command"), b"command").unwrap();

        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let ownership = cleanup_test_ownership(&vm_id);
        let package_id = format!("path:{}", package_dir.path().display());
        let link = crate::protocol::LinkPackageRequest {
            package: crate::protocol::PackageDescriptor {
                path: package_dir.path().to_string_lossy().into_owned(),
            },
            package_id: package_id.clone(),
        };
        let request = RequestFrame::new(
            234,
            ownership.clone(),
            RequestPayload::LinkPackage(link.clone()),
        );
        sidecar
            .link_package(&request, link.clone())
            .await
            .expect("link package");

        // Reusing an ID must not acknowledge commands from a different package
        // while retaining the old projection. Exercise a changed path-backed
        // manifest as well as the ConfigureVm boot/live identity collision.
        std::fs::write(
            package_dir.path().join("agentos-package.json"),
            r#"{"name":"replacement-test","version":"2.0.0"}"#,
        )
        .unwrap();
        let error = sidecar
            .link_package(&request, link.clone())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("different descriptor"));

        let mut configure = crate::protocol::ConfigureVmRequest {
            mounts: vec![crate::protocol::MountDescriptor {
                guest_path: String::from("/operator-mount"),
                guest_source: String::from("operator-test"),
                guest_fstype: String::from("operator-test"),
                read_only: true,
                plugin: crate::protocol::MountPluginDescriptor {
                    id: String::from("agentos_packages"),
                    config: String::from(
                        r#"{"kind":"singleSymlink","target":"/bin/node","readOnly":true}"#,
                    ),
                },
            }],
            software: Vec::new(),
            permissions: None,
            module_access_cwd: None,
            instructions: Vec::new(),
            projected_modules: Vec::new(),
            command_permissions: Default::default(),
            loopback_exempt_ports: Vec::new(),
            packages: Vec::new(),
            packages_mount_at: String::new(),
            bootstrap_commands: Vec::new(),
            host_function_shim_commands: Vec::new(),
        };
        let request = RequestFrame::new(
            235,
            ownership.clone(),
            RequestPayload::ConfigureVm(configure.clone()),
        );
        configure.packages.push(link.package.clone());
        let error = sidecar
            .configure_vm(&request, configure.clone())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("different descriptor"));
        configure.packages.clear();
        std::fs::write(
            package_dir.path().join("agentos-package.json"),
            r#"{"name":"dynamic-test","version":"1.0.0"}"#,
        )
        .unwrap();

        // Lexically different paths can identify the same mount. Preflight
        // must reject them before unmounting the already-linked package.
        let mut duplicate = configure.mounts[0].clone();
        duplicate.guest_path.push('/');
        configure.mounts.push(duplicate);
        let error = sidecar
            .configure_vm(&request, configure.clone())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("duplicate VM mount path"));
        configure.mounts.pop();
        {
            let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
            assert_eq!(
                vm.kernel
                    .read_file("/opt/agentos/bin/dynamic-command")
                    .unwrap(),
                b"command",
            );
            assert_eq!(vm.package_descriptors[0].1.name, "dynamic-test");
        }
        sidecar
            .configure_vm(&request, configure.clone())
            .await
            .expect("add unrelated mount");
        configure.mounts.clear();
        sidecar
            .configure_vm(&request, configure)
            .await
            .expect("remove unrelated mount");

        {
            let vm = sidecar.vms.get(&vm_id).unwrap();
            assert!(vm.runtime_linked_package_ids.contains(&package_id));
            assert!(vm
                .package_descriptors
                .iter()
                .any(|(id, _)| id == &package_id));
            assert!(vm.kernel.commands().contains_key("dynamic-command"));
            assert!(vm
                .configuration
                .mounts
                .iter()
                .any(|mount| { mount.guest_path == "/opt/agentos/pkgs/dynamic-test/1.0.0" }));
        }

        let unlink = crate::protocol::UnlinkPackageRequest {
            package_id: package_id.clone(),
        };
        let request = RequestFrame::new(
            236,
            ownership,
            RequestPayload::UnlinkPackage(unlink.clone()),
        );
        sidecar
            .unlink_package(&request, unlink)
            .await
            .expect("unlink package");
        let vm = sidecar.vms.get(&vm_id).unwrap();
        assert!(!vm.runtime_linked_package_ids.contains(&package_id));
        assert!(!vm.package_mount_roots.contains_key(&package_id));
        assert!(!vm
            .package_descriptors
            .iter()
            .any(|(id, _)| id == &package_id));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pinned_boot_package_retains_custom_projection_root_and_provides() {
        let package_dir = tempfile::tempdir().unwrap();
        let manifest = r#"{"name":"custom-root-test","version":"1.0.0","provides":{"env":{"PACKAGE_RETAINED":"yes"},"files":[{"source":"share/config","target":"/etc/custom-package"}]}}"#;
        std::fs::create_dir_all(package_dir.path().join("bin")).unwrap();
        std::fs::create_dir_all(package_dir.path().join("share/config")).unwrap();
        std::fs::write(package_dir.path().join("bin/custom-command"), b"custom").unwrap();
        std::fs::write(package_dir.path().join("share/config/value"), b"retained").unwrap();
        std::fs::write(package_dir.path().join("agentos-package.json"), manifest).unwrap();
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
        let ownership = cleanup_test_ownership(&vm_id);
        let package = crate::protocol::PackageDescriptor {
            path: package_dir.path().to_string_lossy().into_owned(),
        };
        let package_id = format!("path:{}", package.path);
        let mut configure = crate::protocol::ConfigureVmRequest {
            mounts: Vec::new(),
            software: Vec::new(),
            permissions: None,
            module_access_cwd: None,
            instructions: Vec::new(),
            projected_modules: Vec::new(),
            command_permissions: Default::default(),
            loopback_exempt_ports: Vec::new(),
            packages: vec![package.clone()],
            packages_mount_at: String::from("/custom-software"),
            bootstrap_commands: Vec::new(),
            host_function_shim_commands: Vec::new(),
        };
        let request = RequestFrame::new(
            237,
            ownership.clone(),
            RequestPayload::ConfigureVm(configure.clone()),
        );
        let result = sidecar
            .configure_vm(&request, configure.clone())
            .await
            .unwrap();
        let ResponsePayload::VmConfigured(response) = result.response.payload else {
            panic!("unexpected configure response");
        };
        assert!(response.projected_commands.iter().any(|command| {
            command.name == "custom-command"
                && command.guest_path == "/custom-software/bin/custom-command"
        }));
        let link = crate::protocol::LinkPackageRequest {
            package,
            package_id: package_id.clone(),
        };
        let request = RequestFrame::new(
            238,
            ownership.clone(),
            RequestPayload::LinkPackage(link.clone()),
        );
        let result = sidecar.link_package(&request, link.clone()).await.unwrap();
        let ResponsePayload::PackageLinked(response) = result.response.payload else {
            panic!("unexpected link response");
        };
        assert_eq!(
            response.projected_commands[0].guest_path,
            "/custom-software/bin/custom-command"
        );

        // Separate roots do not make duplicate logical package/command names
        // valid: both providedCommands and command dispatch require uniqueness.
        let mut duplicate = link.clone();
        duplicate.package_id = String::from("another-identity");
        let error = sidecar
            .link_package(&request, duplicate.clone())
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("already projected under another identity"));
        std::fs::write(
            package_dir.path().join("agentos-package.json"),
            r#"{"name":"different-name","version":"1.0.0"}"#,
        )
        .unwrap();
        let error = sidecar.link_package(&request, duplicate).await.unwrap_err();
        assert!(error
            .to_string()
            .contains("already provided by another package"));
        std::fs::write(package_dir.path().join("agentos-package.json"), manifest).unwrap();

        let request = RequestFrame::new(
            239,
            ownership.clone(),
            RequestPayload::ConfigureVm(configure.clone()),
        );
        sidecar
            .configure_vm(&request, configure.clone())
            .await
            .unwrap();
        configure.packages_mount_at = String::from("/different-root");
        let error = sidecar
            .configure_vm(&request, configure.clone())
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("different descriptor or projection root"));
        configure.packages.clear();
        sidecar.configure_vm(&request, configure).await.unwrap();
        {
            let mut vm = sidecar.vms.get_mut(&vm_id).unwrap();
            assert_eq!(vm.package_mount_roots[&package_id], "/custom-software");
            assert_eq!(
                vm.command_guest_paths["custom-command"],
                "/custom-software/bin/custom-command"
            );
            assert_eq!(vm.guest_env["PACKAGE_RETAINED"], "yes");
            assert_eq!(
                vm.kernel
                    .read_file("/custom-software/bin/custom-command")
                    .unwrap(),
                b"custom"
            );
            assert_eq!(
                vm.kernel.read_file("/etc/custom-package/value").unwrap(),
                b"retained"
            );
            assert!(!vm
                .kernel
                .exists_for_operator("/opt/agentos/pkgs/custom-root-test/1.0.0")
                .unwrap());
        }
        let unlink = crate::protocol::UnlinkPackageRequest {
            package_id: package_id.clone(),
        };
        let request = RequestFrame::new(
            240,
            ownership,
            RequestPayload::UnlinkPackage(unlink.clone()),
        );
        sidecar.unlink_package(&request, unlink).await.unwrap();
        let vm = sidecar.vms.get(&vm_id).unwrap();
        assert!(!vm.package_mount_roots.contains_key(&package_id));
        assert!(!vm.guest_env.contains_key("PACKAGE_RETAINED"));
        assert!(!vm.kernel.commands().contains_key("custom-command"));
        assert!(!vm
            .kernel
            .exists_for_operator("/custom-software/pkgs/custom-root-test/1.0.0")
            .unwrap());
        assert!(!vm
            .kernel
            .exists_for_operator("/etc/custom-package")
            .unwrap());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configure_vm_permission_commit_failure_restores_policy_or_fails_closed() {
        for fail_rollback in [false, true] {
            let mut sidecar = test_sidecar();
            let vm_id = create_test_vms(&mut sidecar, 1).await.remove(0);
            let mut original_permissions = deny_all_policy();
            original_permissions.fs = crate::core::permissions::allow_all_policy().fs;
            sidecar
                .bridge
                .set_vm_permissions(&vm_id, &original_permissions)
                .expect("set original policy");
            sidecar
                .vms
                .get_mut(&vm_id)
                .expect("test VM")
                .configuration
                .permissions = original_permissions.clone();
            sidecar
                .bridge
                .queue_set_vm_permissions_result(Err(VmError::Bridge(String::from(
                    "injected configure permission commit failure",
                ))))
                .expect("queue commit failure");
            if fail_rollback {
                sidecar
                    .bridge
                    .queue_set_vm_permissions_result(Err(VmError::Bridge(String::from(
                        "injected configure permission rollback failure",
                    ))))
                    .expect("queue rollback failure");
            }
            let payload = crate::protocol::ConfigureVmRequest {
                mounts: Vec::new(),
                software: Vec::new(),
                permissions: Some(crate::wire::PermissionsPolicy::deny_all()),
                module_access_cwd: None,
                instructions: Vec::new(),
                projected_modules: Vec::new(),
                command_permissions: Default::default(),
                loopback_exempt_ports: Vec::new(),
                packages: Vec::new(),
                packages_mount_at: String::new(),
                bootstrap_commands: Vec::new(),
                host_function_shim_commands: Vec::new(),
            };
            let request = RequestFrame::new(
                231,
                cleanup_test_ownership(&vm_id),
                RequestPayload::ConfigureVm(payload.clone()),
            );
            let error = sidecar
                .configure_vm(&request, payload)
                .await
                .expect_err("commit must fail");
            assert!(error
                .to_string()
                .contains("injected configure permission commit failure"));
            if fail_rollback {
                assert!(error
                    .to_string()
                    .contains("injected configure permission rollback failure"));
                assert!(error.to_string().contains("deny-all fallback"));
            }
            let expected = if fail_rollback {
                deny_all_policy()
            } else {
                original_permissions
            };
            assert_eq!(
                sidecar
                    .vms
                    .get(&vm_id)
                    .expect("test VM")
                    .configuration
                    .permissions,
                expected
            );
            assert_eq!(
                sidecar
                    .bridge
                    .permissions
                    .lock()
                    .expect("bridge permissions")
                    .get(&vm_id),
                Some(&expected),
                "a failed configure must not leave the guest temporarily privileged",
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn owned_host_callback_restore_failure_rolls_back_registry_and_permissions() {
        let mut sidecar = test_sidecar();
        let vm_id = create_test_vms(&mut sidecar, 1)
            .await
            .pop()
            .expect("test VM id");
        let (original_permissions, original_host_functions, original_paths) = {
            let vm = sidecar.vms.get(&vm_id).expect("test VM");
            (
                vm.configuration.permissions.clone(),
                vm.host_functions.clone(),
                vm.command_guest_paths.clone(),
            )
        };
        sidecar
            .bridge
            .queue_set_vm_permissions_result(Err(VmError::Bridge(String::from(
                "injected owned registration restore failure",
            ))))
            .expect("queue original permission restore failure");

        let request = RequestFrame::new(
            230,
            cleanup_test_ownership(&vm_id),
            RequestPayload::RegisterHostCallbacks(crate::protocol::RegisterHostCallbacksRequest {
                name: String::from("rollback-host-functions"),
                description: String::from("rollback host_function collection"),
                command_aliases: vec![String::from("rollback-command")],
                registry_command_aliases: vec![String::from("agentos")],
                callbacks: std::collections::HashMap::from([(
                    String::from("call"),
                    crate::protocol::RegisteredHostCallbackDefinition {
                        description: String::from("rollback callback"),
                        input_schema: String::from(
                            r#"{"type":"object","additionalProperties":false}"#,
                        ),
                        timeout_ms: None,
                        examples: Vec::new(),
                    },
                )]),
            }),
        );
        let completed = sidecar
            .prepare_request_wire(wire_request(request))
            .expect("prepare callback registration")
            .expect("callback registration is detachable")
            .execute()
            .await;
        assert!(completed.failed());
        let response = compat_response(
            sidecar
                .complete_request(completed)
                .expect("complete failed callback registration"),
        );
        let ResponsePayload::Rejected(rejected) = response.payload else {
            panic!("expected callback registration rejection");
        };
        assert!(rejected
            .message
            .contains("injected owned registration restore failure"));

        let vm = sidecar.vms.get(&vm_id).expect("test VM");
        assert_eq!(vm.host_functions, original_host_functions);
        assert_eq!(vm.command_guest_paths, original_paths);
        assert!(!vm.kernel.commands().contains_key("rollback-command"));
        drop(vm);
        assert_eq!(
            sidecar
                .bridge
                .permissions
                .lock()
                .expect("read bridge permissions")
                .get(&vm_id),
            Some(&original_permissions),
        );
    }

    #[test]
    fn mutating_vm_request_uses_detached_vm_handle_path() {
        let mut sidecar = test_sidecar();
        let request = RequestFrame::new(
            91,
            OwnershipScope::vm("conn-1", "session-1", "vm-1"),
            RequestPayload::WriteStdin(WriteStdinRequest {
                process_id: String::from("process-1"),
                chunk: vec![1, 2, 3],
            }),
        );
        assert!(
            sidecar
                .prepare_request_wire(wire_request(request))
                .expect("classify mutating request")
                .is_some(),
            "mutating VM state must use the detached VM-handle path"
        );
    }
}

#[cfg(test)]
mod legacy_child_spawn_options_tests {
    use super::*;

    #[test]
    fn legacy_v8_string_bridge_preserves_canonical_spawn_options() {
        let vm_guest_env = BTreeMap::from([
            (
                String::from("AGENTOS_ALLOWED_NODE_BUILTINS"),
                String::from("node:path"),
            ),
            (String::from("AGENTOS_NOT_ALLOWED"), String::from("drop-me")),
        ]);
        let parsed = parse_legacy_javascript_child_process_spawn_options(
            &vm_guest_env,
            r#"{
                "argv0":"custom-zero",
                "cloexecFds":[9,10],
                "localReplacement":true,
                "executableFd":11,
                "cwd":"/work",
                "env":{"VISIBLE":"yes"},
                "internalBootstrapEnv":{
                    "AGENTOS_WASM_INITIAL_SIGNAL_MASK":"[10]",
                    "AGENTOS_WASM_INITIAL_SIGNAL_IGNORES":"[13]",
                    "AGENTOS_WASM_INITIAL_PENDING_SIGNALS":"[12]",
                    "AGENTOS_NOT_ALLOWED":"drop-me-too"
                },
                "spawnAttrFlags":70,
                "spawnExactPath":false,
                "spawnSearchPath":"/custom/bin:/bin",
                "spawnSchedPolicy":0,
                "spawnSchedPriority":0,
                "spawnPgroup":42,
                "spawnSignalDefaults":[13],
                "spawnSignalMask":[10,12],
                "spawnFileActions":[{
                    "command":2,
                    "guestFd":41,
                    "fd":8,
                    "sourceFd":7,
                    "guestSourceFd":40,
                    "oflag":0,
                    "mode":420,
                    "path":"/tmp/unused"
                }],
                "spawnFdMappings":[[40,7],[50,8]],
                "input":{"type":"Buffer","data":[97]},
                "shell":true,
                "detached":true,
                "stdio":["pipe","inherit","ignore"],
                "maxBuffer":1234,
                "timeout":5678,
                "killSignal":"SIGUSR2"
            }"#,
        )
        .expect("parse V8 three-string options payload");

        assert_eq!(parsed.max_buffer, Some(1234));
        let options = parsed.options;
        assert_eq!(options.argv0.as_deref(), Some("custom-zero"));
        assert_eq!(options.cloexec_fds, vec![9, 10]);
        assert!(options.local_replacement);
        assert_eq!(options.executable_fd, Some(11));
        assert_eq!(options.cwd.as_deref(), Some("/work"));
        assert_eq!(options.env.get("VISIBLE").map(String::as_str), Some("yes"));
        assert_eq!(
            options
                .internal_bootstrap_env
                .get("AGENTOS_ALLOWED_NODE_BUILTINS")
                .map(String::as_str),
            Some("node:path")
        );
        // Signal state belongs to the kernel and typed spawn attributes below.
        // Obsolete guest-visible bootstrap variables must not override it.
        for key in [
            "AGENTOS_WASM_INITIAL_SIGNAL_MASK",
            "AGENTOS_WASM_INITIAL_SIGNAL_IGNORES",
            "AGENTOS_WASM_INITIAL_PENDING_SIGNALS",
        ] {
            assert!(!options.internal_bootstrap_env.contains_key(key));
        }
        assert!(!options
            .internal_bootstrap_env
            .contains_key("AGENTOS_NOT_ALLOWED"));
        assert_eq!(options.spawn_attr_flags, 70);
        assert!(!options.spawn_exact_path);
        assert_eq!(
            options.spawn_search_path.as_deref(),
            Some("/custom/bin:/bin")
        );
        assert_eq!(options.spawn_sched_policy, Some(0));
        assert_eq!(options.spawn_sched_priority, Some(0));
        assert_eq!(options.spawn_pgroup, Some(42));
        assert_eq!(options.spawn_signal_defaults, vec![13]);
        assert_eq!(options.spawn_signal_mask, vec![10, 12]);
        assert_eq!(options.spawn_fd_mappings, vec![[40, 7], [50, 8]]);
        assert_eq!(options.spawn_file_actions.len(), 1);
        let action = &options.spawn_file_actions[0];
        assert_eq!(action.command, 2);
        assert_eq!(action.guest_fd, Some(41));
        assert_eq!(action.fd, 8);
        assert_eq!(action.source_fd, 7);
        assert_eq!(action.guest_source_fd, Some(40));
        assert_eq!(action.oflag, 0);
        assert_eq!(action.mode, 420);
        assert_eq!(action.path, "/tmp/unused");
        assert_eq!(options.input, Some(json!({"type":"Buffer","data":[97]})));
        assert!(options.shell);
        assert!(options.detached);
        assert_eq!(options.stdio, vec!["pipe", "inherit", "ignore"]);
        assert_eq!(options.timeout, Some(5678));
        assert_eq!(options.kill_signal.as_deref(), Some("SIGUSR2"));
    }
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
        assert!(hint.contains("agentos can't load mounted node_modules"));
        assert!(!hint.contains("/root/node_modules/.pnpm/"));
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
mod guest_limit_diagnostic_tests {
    use super::guest_limit_diagnostic;
    use agentos_driver_tokio::accounting::{LimitError, ResourceClass};

    fn limit(scope: &str, used: usize) -> LimitError {
        LimitError {
            scope: scope.to_owned(),
            resource: ResourceClass::AsyncCompletions,
            used,
            requested: 1,
            limit: 8,
            config_path: String::from("runtime.resources.maxAsyncCompletions"),
        }
    }

    #[test]
    fn vm_limit_reports_only_the_requesting_vm_usage() {
        let diagnostic = guest_limit_diagnostic(&limit("vm=vm-1 generation=7", 6));
        assert_eq!(diagnostic.scope, "vm");
        assert_eq!(diagnostic.current_usage, Some(6));
        assert!(diagnostic.message.contains("used=6"));
    }

    #[test]
    fn process_limit_hides_cross_vm_aggregate_usage() {
        let diagnostic = guest_limit_diagnostic(&limit("sidecar-process", 7));
        assert_eq!(diagnostic.scope, "process");
        assert_eq!(diagnostic.current_usage, None);
        assert!(!diagnostic.message.contains("used=7"));
        assert!(diagnostic.message.contains("requested=1 limit=8"));
        assert!(diagnostic
            .message
            .contains("runtime.resources.maxAsyncCompletions"));
    }
}

#[cfg(test)]
mod dispose_lifecycle_tests {
    use super::*;
    use crate::extension::ExtensionResponse;
    use agentos_vm_host_interface::LocalVmHost as LocalBridge;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("dispose lifecycle test runtime")
            .block_on(future)
    }

    fn test_sidecar() -> VmManager<LocalBridge> {
        VmManager::new(LocalBridge::default()).expect("build test sidecar")
    }

    // Register a connection + session directly so the dispose paths can be
    // exercised without spinning up a V8-backed VM.
    fn insert_session(
        sidecar: &mut VmManager<LocalBridge>,
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
                metadata: BTreeMap::new(),
                vm_ids,
            },
        );
    }

    #[test]
    fn package_acquisition_is_detached_and_rejects_foreign_sessions_before_io() {
        let mut sidecar = test_sidecar();
        insert_session(&mut sidecar, "conn-a", "session-a", BTreeSet::new());
        insert_session(&mut sidecar, "conn-b", "session-b", BTreeSet::new());
        let request = crate::wire::RequestFrame {
            schema: crate::wire::ProtocolSchema::current(),
            request_id: 87,
            ownership: crate::wire::OwnershipScope::SessionOwnership(
                crate::wire::SessionOwnership {
                    connection_id: String::from("conn-b"),
                    session_id: String::from("session-a"),
                },
            ),
            payload: crate::wire::RequestPayload::AcquirePackageRequest(
                crate::wire::AcquirePackageRequest {
                    source: crate::wire::PackageAcquisitionSource::PackagePathSource(
                        crate::wire::PackagePathSource {
                            path: String::from("/does-not-exist.aospkg"),
                            expected_digest: None,
                        },
                    ),
                    advisory: false,
                    timeout_ms: None,
                    max_package_bytes: None,
                    download_timeout_ms: None,
                    connect_timeout_ms: None,
                    max_redirects: None,
                    allow_insecure_local_http: false,
                },
            ),
        };
        let prepared = sidecar
            .prepare_request_wire(request.clone())
            .expect("prepare package acquisition")
            .expect("package acquisition is detached");
        let completed = block_on(prepared.execute());
        let rejected = completed
            .result
            .expect_err("foreign session must be rejected before acquiring a package");
        assert!(rejected.to_string().contains("not owned by connection"));
        let result = block_on(sidecar.dispatch_wire(request)).expect("dispatch foreign request");
        assert!(matches!(
            result.response.payload,
            crate::wire::ResponsePayload::RejectedResponse(rejected)
                if rejected.message.contains("not owned by connection")
        ));
        assert!(sidecar.vms.is_empty());
    }

    #[test]
    fn package_acquisition_rejections_preserve_error_kinds_and_limit_details() {
        let timeout = package_acquisition_rejection(&ClientError::OperationTimedOut {
            message: "raise PackageResolverOptions.download_timeout_ms".into(),
            details: Box::new(agentos_client::error::ResourceLimitDetails {
                configured_limit: Some(15),
                configuration_path: Some("PackageResolverOptions.download_timeout_ms".into()),
                errno: Some("ETIMEDOUT".into()),
                ..Default::default()
            }),
        });
        assert_eq!(timeout.code, "timeout");
        assert_eq!(timeout.configured_limit, Some(15));
        assert_eq!(
            timeout.configuration_path.as_deref(),
            Some("AcquirePackageRequest.downloadTimeoutMs")
        );
        assert_eq!(
            timeout.message,
            "raise AcquirePackageRequest.downloadTimeoutMs"
        );
        for (error, code) in [
            (
                ClientError::InvalidPackageSource("bad source".into()),
                "invalid_package_source",
            ),
            (
                ClientError::InvalidPackageFormat("bad archive".into()),
                "invalid_package_format",
            ),
            (
                ClientError::PackageDigestMismatch {
                    expected: "wanted".into(),
                    actual: "got".into(),
                },
                "package_digest_mismatch",
            ),
            (
                ClientError::PackageDownload("fetch failed".into()),
                "package_download_failed",
            ),
            (
                ClientError::PackageIo("read failed".into()),
                "package_io_failed",
            ),
            (
                ClientError::PackageCacheConfiguration("conflict".into()),
                "package_cache_configuration",
            ),
        ] {
            let rejection = package_acquisition_rejection(&error);
            assert_eq!(rejection.code, code);
            assert_eq!(rejection.message, error.to_string());
            assert_eq!(rejection.operation.as_deref(), Some("package.acquire"));
        }
        for (error, name, limit, current, requested, path) in [
            (
                ClientError::PackageTooLarge {
                    observed: 11,
                    limit: 10,
                },
                "packageBytes",
                10,
                None,
                Some(11),
                "AcquirePackageRequest.maxPackageBytes",
            ),
            (
                ClientError::PackageCacheCapacity {
                    requested: 4,
                    current: 8,
                    limit: 10,
                },
                "packageCacheBytes",
                10,
                Some(8),
                Some(4),
                "ProcessPackageCacheOptions.max_bytes",
            ),
            (
                ClientError::PackageCacheEntryCapacity {
                    current: 2,
                    limit: 2,
                },
                "packageCacheEntries",
                2,
                Some(2),
                Some(1),
                "ProcessPackageCacheOptions.max_entries",
            ),
            (
                ClientError::PackageCachePendingLimit { limit: 3 },
                "packageCachePendingAcquisitions",
                3,
                None,
                Some(1),
                "ProcessPackageCacheOptions.max_pending_acquisitions",
            ),
        ] {
            let rejection = package_acquisition_rejection(&error);
            assert_eq!(rejection.code, "ERR_AGENTOS_RESOURCE_LIMIT");
            assert_eq!(rejection.limit_name.as_deref(), Some(name));
            assert_eq!(rejection.configured_limit, Some(limit));
            assert_eq!(rejection.current_usage, current);
            assert_eq!(rejection.requested, requested);
            assert_eq!(rejection.configuration_path.as_deref(), Some(path));
        }
    }

    #[test]
    fn package_acquisition_deadline_clamps_to_operator_cap_and_drops_waiter() {
        struct Dropped(Arc<AtomicUsize>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        block_on(async {
            for (timeout, cap, expected, path) in [
                (
                    None,
                    2,
                    2,
                    "ProcessPackageCacheOptions.acquisition_timeout_ms",
                ),
                (
                    Some(u64::MAX),
                    2,
                    2,
                    "ProcessPackageCacheOptions.acquisition_timeout_ms",
                ),
                (Some(1), 2, 1, "AcquirePackageRequest.timeoutMs"),
            ] {
                let dropped = Arc::new(AtomicUsize::new(0));
                let guard = Dropped(dropped.clone());
                let rejection = await_package_acquisition(timeout, cap, async move {
                    let _guard = guard;
                    std::future::pending::<Result<(), ClientError>>().await
                })
                .await
                .unwrap_err();
                assert_eq!(rejection.code, "timeout");
                assert_eq!(rejection.configured_limit, Some(expected));
                assert_eq!(rejection.configuration_path.as_deref(), Some(path));
                assert_eq!(rejection.errno.as_deref(), Some("ETIMEDOUT"));
                assert_eq!(dropped.load(Ordering::SeqCst), 1);
            }
            let rejection = await_package_acquisition(Some(0), 2, async {
                panic!("invalid deadline must not poll acquisition");
                #[allow(unreachable_code)]
                Ok::<(), ClientError>(())
            })
            .await
            .unwrap_err();
            assert_eq!(rejection.code, "invalid_package_source");
        });
    }

    #[test]
    fn package_acquisition_owned_dispatch_verifies_bytes_without_allocating_vm() {
        let mut sidecar = test_sidecar();
        insert_session(&mut sidecar, "conn", "session", BTreeSet::new());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("demo.aospkg");
        let mut tar = tar::Builder::new(Vec::new());
        let manifest = br#"{"name":"acquisition-review","version":"1.0.0"}"#;
        let mut header = tar::Header::new_gnu();
        header.set_size(manifest.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "agentos-package.json", &manifest[..])
            .unwrap();
        let bytes = agentos_vfs_core::package_format::pack::pack_aospkg_from_tar_bytes(
            &tar.into_inner().unwrap(),
        )
        .unwrap()
        .0;
        std::fs::write(&path, &bytes).unwrap();
        let request = |max_package_bytes, expected_digest| crate::wire::RequestFrame {
            schema: crate::wire::ProtocolSchema::current(),
            request_id: 88,
            ownership: crate::wire::OwnershipScope::SessionOwnership(
                crate::wire::SessionOwnership {
                    connection_id: "conn".into(),
                    session_id: "session".into(),
                },
            ),
            payload: crate::wire::RequestPayload::AcquirePackageRequest(
                crate::wire::AcquirePackageRequest {
                    source: crate::wire::PackageAcquisitionSource::PackagePathSource(
                        crate::wire::PackagePathSource {
                            path: path.to_string_lossy().into_owned(),
                            expected_digest,
                        },
                    ),
                    advisory: true,
                    timeout_ms: None,
                    max_package_bytes,
                    download_timeout_ms: None,
                    connect_timeout_ms: None,
                    max_redirects: None,
                    allow_insecure_local_http: false,
                },
            ),
        };
        block_on(async {
            for (limit, expected, code) in [
                (Some(0), None, "invalid_package_source"),
                (Some(1), None, "ERR_AGENTOS_RESOURCE_LIMIT"),
                (
                    None,
                    Some(format!("sha256:{}", "0".repeat(64))),
                    "package_digest_mismatch",
                ),
            ] {
                let result = sidecar
                    .dispatch_wire(request(limit, expected))
                    .await
                    .unwrap();
                let crate::wire::ResponsePayload::RejectedResponse(rejection) =
                    result.response.payload
                else {
                    panic!("expected package rejection");
                };
                assert_eq!(rejection.code, code);
                if code == "ERR_AGENTOS_RESOURCE_LIMIT" {
                    assert_eq!(rejection.configured_limit, Some(1));
                    assert_eq!(rejection.requested, Some(bytes.len() as u64));
                }
            }
            let prepared = sidecar
                .prepare_request_wire(request(None, None))
                .unwrap()
                .unwrap();
            let completed = prepared.execute().await.result.unwrap();
            let ResponsePayload::PackageAcquired(package) = completed.response.payload else {
                panic!("expected acquired package metadata");
            };
            use sha2::Digest;
            assert_eq!(
                package.digest,
                format!("sha256:{:x}", sha2::Sha256::digest(&bytes))
            );
            assert_eq!(package.package_id, package.digest);
            assert_eq!(package.size, bytes.len() as u64);
            assert_eq!(package.package_name, "acquisition-review");
            assert_eq!(package.version, "1.0.0");
        });
        assert!(sidecar.vms.is_empty());
    }

    #[test]
    fn vm_config_comparison_dispatch_is_session_owned_and_does_not_allocate_vm() {
        let mut sidecar = test_sidecar();
        insert_session(&mut sidecar, "conn-a", "session-a", BTreeSet::new());
        insert_session(&mut sidecar, "conn-b", "session-b", BTreeSet::new());
        let next_vm_id = sidecar.next_vm_id;
        let next_session_id = sidecar.next_session_id;
        let ownership = |connection: &str, session: &str| {
            crate::wire::OwnershipScope::SessionOwnership(crate::wire::SessionOwnership {
                connection_id: connection.into(),
                session_id: session.into(),
            })
        };
        let request = |request_id, ownership, after: &str| crate::wire::RequestFrame {
            schema: crate::wire::ProtocolSchema::current(),
            request_id,
            ownership,
            payload: crate::wire::RequestPayload::CompareVmConfigRequest(
                crate::wire::CompareVmConfigRequest {
                    before_mounts: Vec::new(),
                    after_mounts: Vec::new(),
                    before_restart_identity: Vec::new(),
                    after_restart_identity: Vec::new(),

                    before: r#"{"defaultsProfile":"agent_os"}"#.into(),
                    after: after.into(),
                },
            ),
        };
        for (request_id, high_resolution_time, expected) in [(1, false, true), (2, true, false)] {
            let after = format!(
                r#"{{"defaultsProfile":"agent_os","jsRuntime":{{"highResolutionTime":{high_resolution_time}}}}}"#
            );
            let result = block_on(sidecar.dispatch_wire(request(
                request_id,
                ownership("conn-a", "session-a"),
                &after,
            )))
            .expect("compare through wire dispatch");
            assert!(matches!(
                result.response.payload,
                crate::wire::ResponsePayload::VmConfigComparedResponse(compared)
                    if compared.equivalent == expected
            ));
            assert!(result.events.is_empty());
        }
        let mut mount_request = request(
            9,
            ownership("conn-a", "session-a"),
            r#"{"defaultsProfile":"agent_os"}"#,
        );
        let crate::wire::RequestPayload::CompareVmConfigRequest(comparison) =
            &mut mount_request.payload
        else {
            unreachable!("comparison request");
        };
        comparison.after_mounts.push(crate::wire::MountDescriptor {
            guest_path: String::from("/workspace"),
            guest_source: String::from("agentos-packages"),
            guest_fstype: String::from("agentos-packages"),
            read_only: true,
            plugin: crate::wire::MountPluginDescriptor {
                id: String::from("agentos_packages"),
                config: String::from("{}"),
            },
        });
        let result = block_on(sidecar.dispatch_wire(mount_request.clone()))
            .expect("compare configured mounts through wire dispatch");
        assert!(matches!(
            result.response.payload,
            crate::wire::ResponsePayload::VmConfigComparedResponse(compared)
                if !compared.equivalent
        ));

        let crate::wire::RequestPayload::CompareVmConfigRequest(comparison) =
            &mut mount_request.payload
        else {
            unreachable!("comparison request");
        };
        comparison.before_mounts = comparison.after_mounts.clone();
        comparison.before_mounts[0].guest_path = String::from("/workspace/./nested/..");
        comparison.before_mounts[0].plugin.config = String::from(r#"{ "b": 2, "a": 1 }"#);
        comparison.after_mounts[0].plugin.config = String::from(r#"{"a":1,"b":2}"#);
        let result = block_on(sidecar.dispatch_wire(mount_request))
            .expect("compare normalized mount paths and parsed plugin configuration");
        assert!(matches!(
            result.response.payload,
            crate::wire::ResponsePayload::VmConfigComparedResponse(compared)
                if compared.equivalent
        ));

        let mut identity_request = request(
            10,
            ownership("conn-a", "session-a"),
            r#"{"defaultsProfile":"agent_os"}"#,
        );
        let crate::wire::RequestPayload::CompareVmConfigRequest(comparison) =
            &mut identity_request.payload
        else {
            unreachable!("comparison request");
        };
        comparison
            .after_restart_identity
            .push(String::from("sha256:changed"));
        let result = block_on(sidecar.dispatch_wire(identity_request))
            .expect("compare actor runtime identity through wire dispatch");
        assert!(matches!(
            result.response.payload,
            crate::wire::ResponsePayload::VmConfigComparedResponse(compared)
                if !compared.equivalent
        ));
        for (request_id, owner, after) in [
            (3, ownership("conn-b", "session-a"), "{}"),
            (4, ownership("unknown", "session-a"), "{}"),
            (5, ownership("conn-a", "unknown"), "{}"),
            (6, ownership("conn-a", "session-a"), r#"{"unknown":true}"#),
            (
                7,
                ownership("conn-a", "session-a"),
                r#"{"jsRuntime":{"allowedBuiltins":["not-a-builtin"]}}"#,
            ),
            (
                8,
                crate::wire::OwnershipScope::VmOwnership(crate::wire::VmOwnership {
                    connection_id: "conn-a".into(),
                    session_id: "session-a".into(),
                    vm_id: "vm-missing".into(),
                }),
                "{}",
            ),
        ] {
            let result = block_on(sidecar.dispatch_wire(request(request_id, owner, after)))
                .expect("invalid comparison has a rejected response");
            assert!(matches!(
                result.response.payload,
                crate::wire::ResponsePayload::RejectedResponse(_)
            ));
            assert!(result.events.is_empty());
        }
        assert!(sidecar.vms.is_empty());
        assert!(sidecar.quarantined_vms.is_empty());
        assert_eq!(sidecar.next_vm_id, next_vm_id);
        assert_eq!(sidecar.next_session_id, next_session_id);
        assert_eq!(sidecar.connections.len(), 2);
        assert_eq!(sidecar.sessions.len(), 2);
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
            _ctx: ExtensionContext,
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

    fn register_recording_extension(sidecar: &mut VmManager<LocalBridge>) -> Arc<AtomicUsize> {
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
    // Extensions can release per-connection state on client disconnect.
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
