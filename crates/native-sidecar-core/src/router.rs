use crate::frames::{reject, DispatchResult};
use agentos_sidecar_protocol::protocol::{
    AuthenticateRequest, BootstrapRootFilesystemRequest, CancelCronJobRequest, CloseSessionRequest,
    CloseStdinRequest, CompleteCronRunRequest, ConfigureVmRequest, CreateLayerRequest,
    CreateOverlayRequest, CreateVmRequest, DisposeVmRequest, ExecuteRequest,
    ExportCronStateRequest, ExportSnapshotRequest, ExtEnvelope, FindBoundUdpRequest,
    FindListenerRequest, GetProcessSnapshotRequest, GetResourceSnapshotRequest,
    GetSignalStateRequest, GetZombieTimerCountRequest, GuestFilesystemCallRequest,
    GuestKernelCallRequest, ImportCronStateRequest, ImportSnapshotRequest, InitializeVmRequest,
    KillProcessRequest, LinkPackageRequest, ListCronJobsRequest, OpenSessionRequest,
    OwnershipScope, ProvidedCommandsRequest, RegisterHostCallbacksRequest, RequestFrame,
    RequestPayload, ResizePtyRequest, ScheduleCronRequest, SealLayerRequest,
    SnapshotRootFilesystemRequest, VmFetchRequest, WakeCronRequest, WriteStdinRequest,
};
use agentos_sidecar_protocol::wire as generated_wire;
use std::collections::{BTreeMap, VecDeque};

pub const UNSUPPORTED_HOST_CALLBACK_DIRECTION_CODE: &str = "unsupported_direction";
pub const UNSUPPORTED_HOST_CALLBACK_DIRECTION_MESSAGE: &str =
    "host callback request categories are sidecar-to-host only in this scaffold";
pub const DEFAULT_MAX_SESSIONS_PER_CONNECTION: usize = 1_024;
pub const SESSION_LIMIT_ERROR_CODE: &str = "session_limit_exceeded";
const SESSION_LIMIT_WARNING_PERCENT: usize = 80;
pub const CLOSE_SESSION_INVALID_OWNERSHIP_ERROR_CODE: &str = "invalid_ownership";
pub const CLOSE_SESSION_UNAUTHENTICATED_ERROR_CODE: &str = "unauthenticated";
pub const CLOSE_SESSION_OWNERSHIP_MISMATCH_ERROR_CODE: &str = "ownership_mismatch";
pub const CLOSE_SESSION_FAILED_ERROR_CODE: &str = "close_session_failed";
pub const CLOSE_SESSION_HISTORY_EXPIRED_ERROR_CODE: &str = "close_session_history_expired";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCloseOutcome {
    pub error_message: Option<String>,
}

pub fn session_limit_near_capacity(active: usize, limit: usize) -> bool {
    limit > 0 && active.saturating_mul(100) >= limit.saturating_mul(SESSION_LIMIT_WARNING_PERCENT)
}

pub fn session_limit_rejection_message(limit: usize) -> String {
    format!(
        "maximum sessions per connection reached ({limit}); raise the sidecar max_sessions_per_connection setting (AGENTOS_MAX_SESSIONS_PER_CONNECTION for stdio)"
    )
}

/// Keep enough terminal close outcomes to cover two complete active-session
/// generations. The configured session bound is also the operator-facing knob
/// for this history: raising it increases both admission and retry history.
pub fn session_close_history_capacity(max_sessions_per_connection: usize) -> usize {
    max_sessions_per_connection.saturating_mul(2).max(1)
}

/// Record a terminal close result and evict the oldest result when the bounded
/// history is full. Returns true when an old result was evicted.
pub fn record_session_close_outcome(
    outcomes: &mut BTreeMap<String, SessionCloseOutcome>,
    order: &mut VecDeque<String>,
    session_id: String,
    outcome: SessionCloseOutcome,
    capacity: usize,
) -> bool {
    if outcomes.contains_key(&session_id) {
        outcomes.insert(session_id, outcome);
        return false;
    }

    let capacity = capacity.max(1);
    let mut evicted = false;
    while outcomes.len() >= capacity {
        let Some(oldest) = order.pop_front() else {
            break;
        };
        evicted |= outcomes.remove(&oldest).is_some();
    }
    order.push_back(session_id.clone());
    outcomes.insert(session_id, outcome);
    evicted
}

/// Session IDs are allocated monotonically by each sidecar shell. This lets a
/// bounded history distinguish a genuinely unknown future ID from a previously
/// allocated ID whose terminal outcome has expired, without retaining another
/// unbounded tombstone collection.
pub fn session_id_was_allocated(session_id: &str, prefix: &str, next_session_id: usize) -> bool {
    session_id
        .strip_prefix(prefix)
        .and_then(|suffix| suffix.parse::<usize>().ok())
        .is_some_and(|id| id > 0 && id <= next_session_id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestDispatchMode {
    Immediate,
    Async,
}

// Request payload variants intentionally vary widely in size (small acks next
// to bulky create/exec payloads); boxing is a wire-adjacent refactor.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum RequestRoute {
    Authenticate(AuthenticateRequest),
    OpenSession(OpenSessionRequest),
    CloseSession(CloseSessionRequest),
    CreateVm(CreateVmRequest),
    DisposeVm(DisposeVmRequest),
    BootstrapRootFilesystem(BootstrapRootFilesystemRequest),
    ConfigureVm(ConfigureVmRequest),
    RegisterHostCallbacks(RegisterHostCallbacksRequest),
    CreateLayer(CreateLayerRequest),
    SealLayer(SealLayerRequest),
    ImportSnapshot(ImportSnapshotRequest),
    ExportSnapshot(ExportSnapshotRequest),
    CreateOverlay(CreateOverlayRequest),
    GuestFilesystemCall(GuestFilesystemCallRequest),
    GuestKernelCall(GuestKernelCallRequest),
    SnapshotRootFilesystem(SnapshotRootFilesystemRequest),
    Execute(ExecuteRequest),
    WriteStdin(WriteStdinRequest),
    ResizePty(ResizePtyRequest),
    CloseStdin(CloseStdinRequest),
    KillProcess(KillProcessRequest),
    GetProcessSnapshot(GetProcessSnapshotRequest),
    GetResourceSnapshot(GetResourceSnapshotRequest),
    FindListener(FindListenerRequest),
    FindBoundUdp(FindBoundUdpRequest),
    VmFetch(VmFetchRequest),
    GetSignalState(GetSignalStateRequest),
    GetZombieTimerCount(GetZombieTimerCountRequest),
    LinkPackage(LinkPackageRequest),
    ProvidedCommands(ProvidedCommandsRequest),
    ScheduleCron(ScheduleCronRequest),
    ListCronJobs(ListCronJobsRequest),
    CancelCronJob(CancelCronJobRequest),
    WakeCron(WakeCronRequest),
    CompleteCronRun(CompleteCronRunRequest),
    ExportCronState(ExportCronStateRequest),
    ImportCronState(ImportCronStateRequest),
    InitializeVm(InitializeVmRequest),
    Ext(ExtEnvelope),
    UnsupportedHostCallbackDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockingExtensionInterrupt<'a> {
    ExtensionPayload(&'a [u8]),
    KillProcess,
    CloseSession,
}

pub fn route_request_payload(request: &RequestFrame) -> RequestRoute {
    match request.payload.clone() {
        RequestPayload::Authenticate(payload) => RequestRoute::Authenticate(payload),
        RequestPayload::OpenSession(payload) => RequestRoute::OpenSession(payload),
        RequestPayload::CloseSession(payload) => RequestRoute::CloseSession(payload),
        RequestPayload::CreateVm(payload) => RequestRoute::CreateVm(payload),
        RequestPayload::DisposeVm(payload) => RequestRoute::DisposeVm(payload),
        RequestPayload::BootstrapRootFilesystem(payload) => {
            RequestRoute::BootstrapRootFilesystem(payload)
        }
        RequestPayload::ConfigureVm(payload) => RequestRoute::ConfigureVm(payload),
        RequestPayload::RegisterHostCallbacks(payload) => {
            RequestRoute::RegisterHostCallbacks(payload)
        }
        RequestPayload::CreateLayer(payload) => RequestRoute::CreateLayer(payload),
        RequestPayload::SealLayer(payload) => RequestRoute::SealLayer(payload),
        RequestPayload::ImportSnapshot(payload) => RequestRoute::ImportSnapshot(payload),
        RequestPayload::ExportSnapshot(payload) => RequestRoute::ExportSnapshot(payload),
        RequestPayload::CreateOverlay(payload) => RequestRoute::CreateOverlay(payload),
        RequestPayload::GuestFilesystemCall(payload) => RequestRoute::GuestFilesystemCall(payload),
        RequestPayload::GuestKernelCall(payload) => RequestRoute::GuestKernelCall(payload),
        RequestPayload::SnapshotRootFilesystem(payload) => {
            RequestRoute::SnapshotRootFilesystem(payload)
        }
        RequestPayload::Execute(payload) => RequestRoute::Execute(payload),
        RequestPayload::WriteStdin(payload) => RequestRoute::WriteStdin(payload),
        RequestPayload::ResizePty(payload) => RequestRoute::ResizePty(payload),
        RequestPayload::CloseStdin(payload) => RequestRoute::CloseStdin(payload),
        RequestPayload::KillProcess(payload) => RequestRoute::KillProcess(payload),
        RequestPayload::GetProcessSnapshot(payload) => RequestRoute::GetProcessSnapshot(payload),
        RequestPayload::GetResourceSnapshot(payload) => RequestRoute::GetResourceSnapshot(payload),
        RequestPayload::FindListener(payload) => RequestRoute::FindListener(payload),
        RequestPayload::FindBoundUdp(payload) => RequestRoute::FindBoundUdp(payload),
        RequestPayload::VmFetch(payload) => RequestRoute::VmFetch(payload),
        RequestPayload::GetSignalState(payload) => RequestRoute::GetSignalState(payload),
        RequestPayload::GetZombieTimerCount(payload) => RequestRoute::GetZombieTimerCount(payload),
        RequestPayload::LinkPackage(payload) => RequestRoute::LinkPackage(payload),
        RequestPayload::ProvidedCommands(payload) => RequestRoute::ProvidedCommands(payload),
        RequestPayload::ScheduleCron(payload) => RequestRoute::ScheduleCron(payload),
        RequestPayload::ListCronJobs(payload) => RequestRoute::ListCronJobs(payload),
        RequestPayload::CancelCronJob(payload) => RequestRoute::CancelCronJob(payload),
        RequestPayload::WakeCron(payload) => RequestRoute::WakeCron(payload),
        RequestPayload::CompleteCronRun(payload) => RequestRoute::CompleteCronRun(payload),
        RequestPayload::ExportCronState(payload) => RequestRoute::ExportCronState(payload),
        RequestPayload::ImportCronState(payload) => RequestRoute::ImportCronState(payload),
        RequestPayload::InitializeVm(payload) => RequestRoute::InitializeVm(payload),
        RequestPayload::HostFilesystemCall(_)
        | RequestPayload::PersistenceLoad(_)
        | RequestPayload::PersistenceFlush(_) => RequestRoute::UnsupportedHostCallbackDirection,
        RequestPayload::Ext(payload) => RequestRoute::Ext(payload),
    }
}

pub fn generated_wire_blocking_extension_interrupt<'a>(
    active_request: &generated_wire::RequestFrame,
    blocking_namespace: &str,
    interrupting_request: &'a generated_wire::RequestFrame,
) -> Option<BlockingExtensionInterrupt<'a>> {
    if let generated_wire::RequestPayload::CloseSessionRequest(close) =
        &interrupting_request.payload
    {
        let (active_connection_id, active_session_id) = match &active_request.ownership {
            generated_wire::OwnershipScope::SessionOwnership(scope) => {
                (&scope.connection_id, &scope.session_id)
            }
            generated_wire::OwnershipScope::VmOwnership(scope) => {
                (&scope.connection_id, &scope.session_id)
            }
            generated_wire::OwnershipScope::ConnectionOwnership(_) => return None,
        };
        let generated_wire::OwnershipScope::ConnectionOwnership(connection) =
            &interrupting_request.ownership
        else {
            return None;
        };
        return (connection.connection_id == *active_connection_id
            && close.session_id == *active_session_id)
            .then_some(BlockingExtensionInterrupt::CloseSession);
    }

    if interrupting_request.ownership != active_request.ownership {
        return None;
    }

    match &interrupting_request.payload {
        generated_wire::RequestPayload::ExtEnvelope(envelope)
            if envelope.namespace == blocking_namespace =>
        {
            Some(BlockingExtensionInterrupt::ExtensionPayload(
                &envelope.payload,
            ))
        }
        generated_wire::RequestPayload::ExtEnvelope(_) => None,
        generated_wire::RequestPayload::KillProcessRequest(_) => {
            Some(BlockingExtensionInterrupt::KillProcess)
        }
        _ => None,
    }
}

pub fn request_dispatch_mode(request: &RequestFrame) -> RequestDispatchMode {
    match request.payload {
        RequestPayload::DisposeVm(_)
        | RequestPayload::CloseSession(_)
        | RequestPayload::InitializeVm(_)
        | RequestPayload::WakeCron(_)
        | RequestPayload::Ext(_) => RequestDispatchMode::Async,
        RequestPayload::Authenticate(_)
        | RequestPayload::OpenSession(_)
        | RequestPayload::CreateVm(_)
        | RequestPayload::BootstrapRootFilesystem(_)
        | RequestPayload::ConfigureVm(_)
        | RequestPayload::RegisterHostCallbacks(_)
        | RequestPayload::CreateLayer(_)
        | RequestPayload::SealLayer(_)
        | RequestPayload::ImportSnapshot(_)
        | RequestPayload::ExportSnapshot(_)
        | RequestPayload::CreateOverlay(_)
        | RequestPayload::GuestFilesystemCall(_)
        | RequestPayload::GuestKernelCall(_)
        | RequestPayload::SnapshotRootFilesystem(_)
        | RequestPayload::Execute(_)
        | RequestPayload::WriteStdin(_)
        | RequestPayload::ResizePty(_)
        | RequestPayload::CloseStdin(_)
        | RequestPayload::KillProcess(_)
        | RequestPayload::GetProcessSnapshot(_)
        | RequestPayload::GetResourceSnapshot(_)
        | RequestPayload::FindListener(_)
        | RequestPayload::FindBoundUdp(_)
        | RequestPayload::VmFetch(_)
        | RequestPayload::GetSignalState(_)
        | RequestPayload::GetZombieTimerCount(_)
        | RequestPayload::LinkPackage(_)
        | RequestPayload::ProvidedCommands(_)
        | RequestPayload::ScheduleCron(_)
        | RequestPayload::ListCronJobs(_)
        | RequestPayload::CancelCronJob(_)
        | RequestPayload::CompleteCronRun(_)
        | RequestPayload::ExportCronState(_)
        | RequestPayload::ImportCronState(_)
        | RequestPayload::HostFilesystemCall(_)
        | RequestPayload::PersistenceLoad(_)
        | RequestPayload::PersistenceFlush(_) => RequestDispatchMode::Immediate,
    }
}

pub fn request_is_unsupported_host_callback_direction(request: &RequestFrame) -> bool {
    matches!(
        request.payload,
        RequestPayload::HostFilesystemCall(_)
            | RequestPayload::PersistenceLoad(_)
            | RequestPayload::PersistenceFlush(_)
    )
}

pub fn unsupported_host_callback_direction_dispatch(request: &RequestFrame) -> DispatchResult {
    debug_assert!(request_is_unsupported_host_callback_direction(request));
    DispatchResult {
        response: reject(
            request,
            UNSUPPORTED_HOST_CALLBACK_DIRECTION_CODE,
            UNSUPPORTED_HOST_CALLBACK_DIRECTION_MESSAGE,
        ),
        events: Vec::new(),
    }
}

pub fn connection_id_of(ownership: &OwnershipScope) -> Option<String> {
    match ownership {
        OwnershipScope::ConnectionOwnership(ownership) => Some(ownership.connection_id.clone()),
        OwnershipScope::SessionOwnership(ownership) => Some(ownership.connection_id.clone()),
        OwnershipScope::VmOwnership(ownership) => Some(ownership.connection_id.clone()),
    }
}

pub fn session_scope_of(ownership: &OwnershipScope) -> Option<(String, String)> {
    match ownership {
        OwnershipScope::SessionOwnership(ownership) => Some((
            ownership.connection_id.clone(),
            ownership.session_id.clone(),
        )),
        OwnershipScope::VmOwnership(ownership) => Some((
            ownership.connection_id.clone(),
            ownership.session_id.clone(),
        )),
        OwnershipScope::ConnectionOwnership(_) => None,
    }
}

pub fn vm_id_of(ownership: &OwnershipScope) -> Option<String> {
    match ownership {
        OwnershipScope::VmOwnership(ownership) => Some(ownership.vm_id.clone()),
        OwnershipScope::ConnectionOwnership(_) | OwnershipScope::SessionOwnership(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_sidecar_protocol::protocol::{
        AuthenticateRequest, ExtEnvelope, FilesystemOperation, HostFilesystemCallRequest,
        OwnershipScope, PersistenceFlushRequest, PersistenceLoadRequest, ResponsePayload,
        PROTOCOL_VERSION,
    };
    use agentos_sidecar_protocol::wire as generated_wire;

    fn request(payload: RequestPayload) -> RequestFrame {
        RequestFrame::new(7, OwnershipScope::connection("conn"), payload)
    }

    fn generated_request(
        request_id: i64,
        ownership: generated_wire::OwnershipScope,
        payload: generated_wire::RequestPayload,
    ) -> generated_wire::RequestFrame {
        generated_wire::RequestFrame {
            schema: generated_wire::protocol_schema(),
            request_id,
            ownership,
            payload,
        }
    }

    fn reverse_host_callback_payloads() -> Vec<RequestPayload> {
        vec![
            RequestPayload::HostFilesystemCall(HostFilesystemCallRequest {
                operation: FilesystemOperation::Read,
                path: String::from("/state"),
                payload_size_bytes: 0,
            }),
            RequestPayload::PersistenceLoad(PersistenceLoadRequest {
                key: String::from("state"),
            }),
            RequestPayload::PersistenceFlush(PersistenceFlushRequest {
                key: String::from("state"),
                payload_size_bytes: 0,
            }),
        ]
    }

    #[test]
    fn dispose_close_and_ext_requests_are_async() {
        let ext = request(RequestPayload::Ext(ExtEnvelope {
            namespace: String::from("test"),
            payload: Vec::new(),
        }));
        assert_eq!(request_dispatch_mode(&ext), RequestDispatchMode::Async);
        let close = request(RequestPayload::CloseSession(CloseSessionRequest {
            session_id: String::from("session-1"),
        }));
        assert_eq!(request_dispatch_mode(&close), RequestDispatchMode::Async);
    }

    #[test]
    fn normal_requests_are_immediate() {
        let authenticate = request(RequestPayload::Authenticate(AuthenticateRequest {
            client_name: String::from("test"),
            auth_token: String::from("token"),
            protocol_version: PROTOCOL_VERSION,
            bridge_version: 1,
        }));
        assert_eq!(
            request_dispatch_mode(&authenticate),
            RequestDispatchMode::Immediate
        );
    }

    #[test]
    fn host_callback_requests_are_identified_as_reverse_direction_only() {
        for payload in reverse_host_callback_payloads() {
            let host_call = request(payload);

            assert!(request_is_unsupported_host_callback_direction(&host_call));
            assert_eq!(
                request_dispatch_mode(&host_call),
                RequestDispatchMode::Immediate
            );
        }
    }

    #[test]
    fn routes_protocol_payloads_through_shared_enum() {
        let authenticate = request(RequestPayload::Authenticate(AuthenticateRequest {
            client_name: String::from("test"),
            auth_token: String::from("token"),
            protocol_version: PROTOCOL_VERSION,
            bridge_version: 1,
        }));
        assert!(matches!(
            route_request_payload(&authenticate),
            RequestRoute::Authenticate(_)
        ));

        let extension = request(RequestPayload::Ext(ExtEnvelope {
            namespace: String::from("test"),
            payload: vec![1, 2, 3],
        }));
        assert!(matches!(
            route_request_payload(&extension),
            RequestRoute::Ext(_)
        ));

        for payload in reverse_host_callback_payloads() {
            let host_call = request(payload);
            assert!(matches!(
                route_request_payload(&host_call),
                RequestRoute::UnsupportedHostCallbackDirection
            ));
        }
    }

    #[test]
    fn unsupported_host_callback_dispatch_rejects_with_shared_code() {
        for payload in reverse_host_callback_payloads() {
            let host_call = request(payload);

            let dispatch = unsupported_host_callback_direction_dispatch(&host_call);

            assert!(dispatch.events.is_empty());
            assert_eq!(dispatch.response.request_id, host_call.request_id);
            assert_eq!(dispatch.response.ownership, host_call.ownership);
            match dispatch.response.payload {
                ResponsePayload::Rejected(rejected) => {
                    assert_eq!(rejected.code, UNSUPPORTED_HOST_CALLBACK_DIRECTION_CODE);
                    assert_eq!(
                        rejected.message,
                        UNSUPPORTED_HOST_CALLBACK_DIRECTION_MESSAGE
                    );
                }
                other => panic!("unexpected response payload: {other:?}"),
            }
        }
    }

    #[test]
    fn generated_wire_prompt_interrupt_classifier_matches_only_same_scope_interrupts() {
        let ownership = generated_wire::OwnershipScope::VmOwnership(generated_wire::VmOwnership {
            connection_id: String::from("conn"),
            session_id: String::from("session"),
            vm_id: String::from("vm"),
        });
        let active = generated_request(
            1,
            ownership.clone(),
            generated_wire::RequestPayload::ExtEnvelope(generated_wire::ExtEnvelope {
                namespace: String::from("prompt"),
                payload: b"active".to_vec(),
            }),
        );

        let same_namespace = generated_request(
            2,
            ownership.clone(),
            generated_wire::RequestPayload::ExtEnvelope(generated_wire::ExtEnvelope {
                namespace: String::from("prompt"),
                payload: b"cancel".to_vec(),
            }),
        );
        assert_eq!(
            generated_wire_blocking_extension_interrupt(&active, "prompt", &same_namespace),
            Some(BlockingExtensionInterrupt::ExtensionPayload(b"cancel"))
        );

        let kill = generated_request(
            3,
            ownership.clone(),
            generated_wire::RequestPayload::KillProcessRequest(
                generated_wire::KillProcessRequest {
                    process_id: String::from("proc"),
                    signal: String::from("SIGTERM"),
                },
            ),
        );
        assert_eq!(
            generated_wire_blocking_extension_interrupt(&active, "prompt", &kill),
            Some(BlockingExtensionInterrupt::KillProcess)
        );

        let close = generated_request(
            4,
            generated_wire::OwnershipScope::ConnectionOwnership(
                generated_wire::ConnectionOwnership {
                    connection_id: String::from("conn"),
                },
            ),
            generated_wire::RequestPayload::CloseSessionRequest(
                generated_wire::CloseSessionRequest {
                    session_id: String::from("session"),
                },
            ),
        );
        assert_eq!(
            generated_wire_blocking_extension_interrupt(&active, "prompt", &close),
            Some(BlockingExtensionInterrupt::CloseSession)
        );

        let other_namespace = generated_request(
            4,
            ownership.clone(),
            generated_wire::RequestPayload::ExtEnvelope(generated_wire::ExtEnvelope {
                namespace: String::from("other"),
                payload: b"cancel".to_vec(),
            }),
        );
        assert_eq!(
            generated_wire_blocking_extension_interrupt(&active, "prompt", &other_namespace),
            None
        );

        let other_scope = generated_request(
            5,
            generated_wire::OwnershipScope::VmOwnership(generated_wire::VmOwnership {
                connection_id: String::from("conn"),
                session_id: String::from("session"),
                vm_id: String::from("other-vm"),
            }),
            generated_wire::RequestPayload::KillProcessRequest(
                generated_wire::KillProcessRequest {
                    process_id: String::from("proc"),
                    signal: String::from("SIGTERM"),
                },
            ),
        );
        assert_eq!(
            generated_wire_blocking_extension_interrupt(&active, "prompt", &other_scope),
            None
        );
    }

    #[test]
    fn ownership_scope_helpers_extract_shared_ids() {
        let connection = OwnershipScope::connection("conn-1");
        let session = OwnershipScope::session("conn-1", "session-1");
        let vm = OwnershipScope::vm("conn-1", "session-1", "vm-1");

        assert_eq!(connection_id_of(&connection).as_deref(), Some("conn-1"));
        assert_eq!(connection_id_of(&session).as_deref(), Some("conn-1"));
        assert_eq!(connection_id_of(&vm).as_deref(), Some("conn-1"));
        assert_eq!(
            session_scope_of(&session),
            Some((String::from("conn-1"), String::from("session-1")))
        );
        assert_eq!(
            session_scope_of(&vm),
            Some((String::from("conn-1"), String::from("session-1")))
        );
        assert_eq!(session_scope_of(&connection), None);
        assert_eq!(vm_id_of(&vm).as_deref(), Some("vm-1"));
        assert_eq!(vm_id_of(&session), None);
    }

    #[test]
    fn terminal_session_close_history_is_bounded_and_recognizes_expired_ids() {
        let mut outcomes = BTreeMap::new();
        let mut order = VecDeque::new();
        assert!(!record_session_close_outcome(
            &mut outcomes,
            &mut order,
            String::from("session-1"),
            SessionCloseOutcome {
                error_message: Some(String::from("first failure")),
            },
            2,
        ));
        assert!(!record_session_close_outcome(
            &mut outcomes,
            &mut order,
            String::from("session-2"),
            SessionCloseOutcome {
                error_message: None,
            },
            2,
        ));
        assert!(record_session_close_outcome(
            &mut outcomes,
            &mut order,
            String::from("session-3"),
            SessionCloseOutcome {
                error_message: None,
            },
            2,
        ));
        assert!(!outcomes.contains_key("session-1"));
        assert_eq!(outcomes.len(), 2);
        assert!(session_id_was_allocated("session-1", "session-", 3));
        assert!(!session_id_was_allocated("session-4", "session-", 3));
        assert!(!session_id_was_allocated("other-1", "session-", 3));
        assert_eq!(session_close_history_capacity(2), 4);
    }
}
