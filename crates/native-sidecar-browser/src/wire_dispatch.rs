use crate::{
    BrowserExecutionOptions, BrowserExtensionRequest, BrowserSidecar, BrowserSidecarBridge,
    BrowserSidecarConfig,
};
use agentos_bridge::{
    CreateJavascriptContextRequest, CreateWasmContextRequest, ExecutionEvent,
    ExecutionHandleRequest, GuestRuntime, KillExecutionRequest, PollExecutionEventRequest,
    StartExecutionRequest, WriteExecutionStdinRequest,
};
use agentos_kernel::kernel::KernelVmConfig;
use agentos_native_sidecar_core::{
    authenticated_response, bound_udp_snapshot_response, connection_id_of,
    execution_signal_from_number, guest_environment_with_overrides, layer_created_response,
    layer_sealed_response, listener_snapshot_response, overlay_created_response,
    permissions_from_policy, permissions_with_allow_all_defaults, process_exited_event_with_result,
    process_killed_response, process_output_event, process_snapshot_response,
    process_started_response, protocol_process_snapshot_entry, protocol_root_filesystem_mode,
    reject, resolve_command_line, respond, root_filesystem_bootstrapped_response,
    root_filesystem_snapshot_response, root_snapshot_entry, route_request_payload,
    session_opened_response, session_scope_of, signal_state_response, snapshot_exported_response,
    snapshot_imported_response, stdin_closed_response, stdin_written_response,
    unsupported_guest_kernel_call_event, unsupported_host_callback_direction_dispatch,
    validate_authenticate_versions, validate_process_id, vm_configured_response,
    vm_created_response, vm_disposed_response, vm_id_of, vm_lifecycle_event,
    zombie_timer_count_response, CaptureChunkOutcome, CapturedOutputBudget, CapturedOutputState,
    CronAction, CronScheduler, DispatchResult, RequestRoute, VmLimits,
};
use agentos_sidecar_protocol::protocol::{
    AuthenticateRequest, BootstrapRootFilesystemRequest, CancelCronJobRequest, CloseStdinRequest,
    CompleteCronRunRequest, ConfigureVmRequest, CreateLayerRequest, CreateOverlayRequest,
    CreateVmRequest, CronAlarm, CronDispatchEvent, CronEventKind, CronEventRecord, CronRun,
    DisposeVmRequest, EventFrame, EventPayload, ExecuteRequest, ExportSnapshotRequest, ExtEnvelope,
    FindBoundUdpRequest, FindListenerRequest, GetProcessSnapshotRequest, GetSignalStateRequest,
    GetZombieTimerCountRequest, GuestRuntimeKind, HostCallbacksRegisteredResponse,
    ImportCronStateRequest, ImportSnapshotRequest, KillProcessRequest, OpenSessionRequest,
    OwnershipScope, RegisterHostCallbacksRequest, RequestFrame, ResponsePayload,
    ScheduleCronRequest, SealLayerRequest, SnapshotRootFilesystemRequest, SocketStateEntry,
    StreamChannel, StructuredEvent, VmFetchRequest, VmFetchResponse, VmLifecycleState,
    WakeCronRequest, WriteStdinRequest,
};
use agentos_sidecar_protocol::wire::{
    request_frame_to_compat, CompatDispatchResult, ProtocolCodecError, ProtocolFrame,
    WireFrameCodec,
};
use agentos_vm_config::CreateVmConfig;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const BROWSER_SIDECAR_ID: &str = "agentos-native-sidecar-browser";
pub const BROWSER_MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
const MAX_PENDING_REQUEST_EVENTS: usize = 256;
const MAX_REQUEST_EVENTS_PER_DISPATCH: usize = 2;

#[derive(Debug)]
struct ExecutionRecord {
    vm_id: String,
    process_id: String,
    ownership: OwnershipScope,
    captured_output: Option<CapturedOutputState>,
}

type ProcessExecutionKey = (String, String);

pub struct BrowserWireDispatcher<B: BrowserSidecarBridge> {
    codec: WireFrameCodec,
    sidecar: BrowserSidecar<B>,
    next_connection: usize,
    next_session: usize,
    next_vm: usize,
    next_process: u64,
    active_vms: BTreeSet<String>,
    vm_limits: BTreeMap<String, VmLimits>,
    vm_capture_budgets: BTreeMap<String, Arc<CapturedOutputBudget>>,
    cron_schedulers: BTreeMap<String, CronScheduler>,
    cron_process_runs: BTreeMap<ProcessExecutionKey, String>,
    executions: BTreeMap<String, ExecutionRecord>,
    process_executions: BTreeMap<ProcessExecutionKey, String>,
    pending_events: VecDeque<EventFrame>,
}

impl<B> BrowserWireDispatcher<B>
where
    B: BrowserSidecarBridge,
    <B as agentos_bridge::BridgeTypes>::Error: fmt::Debug,
{
    pub fn new(bridge: B) -> Self {
        Self {
            codec: WireFrameCodec::new(BROWSER_MAX_FRAME_BYTES),
            sidecar: BrowserSidecar::new(bridge, BrowserSidecarConfig::default()),
            next_connection: 0,
            next_session: 0,
            next_vm: 0,
            next_process: 0,
            active_vms: BTreeSet::new(),
            vm_limits: BTreeMap::new(),
            vm_capture_budgets: BTreeMap::new(),
            cron_schedulers: BTreeMap::new(),
            cron_process_runs: BTreeMap::new(),
            executions: BTreeMap::new(),
            process_executions: BTreeMap::new(),
            pending_events: VecDeque::new(),
        }
    }

    pub fn vm_count(&self) -> usize {
        self.sidecar.vm_count()
    }

    pub fn sidecar_mut(&mut self) -> &mut BrowserSidecar<B> {
        &mut self.sidecar
    }

    pub fn handle_request_bytes(&mut self, bytes: &[u8]) -> Result<Vec<u8>, ProtocolCodecError> {
        let generated_request = match self.codec.decode_message(bytes)? {
            ProtocolFrame::RequestFrame(request) => request,
            _ => {
                return Err(ProtocolCodecError::SerializeFailure(String::from(
                    "browser sidecar expected a request frame",
                )));
            }
        };
        let request = request_frame_to_compat(generated_request)?;
        let dispatch = if MAX_REQUEST_EVENTS_PER_DISPATCH > self.request_event_capacity() {
            rejected(
                &request,
                "event_queue_limit_exceeded",
                &format!(
                    "browser sidecar request event queue limit of {MAX_PENDING_REQUEST_EVENTS} events reached; call pollEvent before issuing more requests"
                ),
            )
        } else {
            self.dispatch(request)
        };
        debug_assert!(dispatch.events.len() <= MAX_REQUEST_EVENTS_PER_DISPATCH);
        self.pending_events.extend(dispatch.events.iter().cloned());
        let generated =
            agentos_sidecar_protocol::wire::dispatch_result_from_compat(CompatDispatchResult {
                response: dispatch.response,
                events: Vec::new(),
            })?;
        self.codec
            .encode_message(&ProtocolFrame::ResponseFrame(generated.response))
    }

    pub fn poll_event_bytes(&mut self) -> Result<Option<Vec<u8>>, ProtocolCodecError> {
        let event = match self.pending_events.pop_front() {
            Some(event) => Some(event),
            None => self.poll_one_execution_event()?,
        };
        let Some(event) = event else {
            return Ok(None);
        };
        let generated = agentos_sidecar_protocol::wire::event_frame_from_compat(event)?;
        self.codec
            .encode_message(&ProtocolFrame::EventFrame(generated))
            .map(Some)
    }

    fn request_event_capacity(&self) -> usize {
        MAX_PENDING_REQUEST_EVENTS.saturating_sub(self.pending_events.len())
    }

    fn dispatch(&mut self, request: RequestFrame) -> DispatchResult {
        match route_request_payload(&request) {
            RequestRoute::Authenticate(payload) => self.authenticate(&request, payload),
            RequestRoute::OpenSession(payload) => self.open_session(&request, payload),
            RequestRoute::CreateVm(payload) => self.create_vm(&request, payload),
            RequestRoute::InitializeVm(payload) => self.initialize_vm(&request, payload),
            RequestRoute::DisposeVm(payload) => self.dispose_vm(&request, payload),
            RequestRoute::BootstrapRootFilesystem(payload) => {
                self.bootstrap_root_filesystem(&request, payload)
            }
            RequestRoute::ConfigureVm(payload) => self.configure_vm(&request, payload),
            RequestRoute::RegisterHostCallbacks(payload) => {
                self.register_host_callbacks(&request, payload)
            }
            RequestRoute::CreateLayer(payload) => self.create_layer(&request, payload),
            RequestRoute::SealLayer(payload) => self.seal_layer(&request, payload),
            RequestRoute::ImportSnapshot(payload) => self.import_snapshot(&request, payload),
            RequestRoute::ExportSnapshot(payload) => self.export_snapshot(&request, payload),
            RequestRoute::CreateOverlay(payload) => self.create_overlay(&request, payload),
            RequestRoute::GuestFilesystemCall(payload) => {
                self.guest_filesystem_call(&request, payload)
            }
            RequestRoute::GuestKernelCall(payload) => self.guest_kernel_call(&request, payload),
            RequestRoute::SnapshotRootFilesystem(payload) => {
                self.snapshot_root_filesystem(&request, payload)
            }
            RequestRoute::GetProcessSnapshot(payload) => {
                self.get_process_snapshot(&request, payload)
            }
            RequestRoute::GetResourceSnapshot(payload) => {
                // Resource snapshots surface the native sidecar's queue/limit
                // trackers, which the converged browser runtime does not run.
                let _ = payload;
                rejected(
                    &request,
                    "unsupported",
                    "get_resource_snapshot is not available in the converged browser runtime",
                )
            }
            RequestRoute::GetSignalState(payload) => self.get_signal_state(&request, payload),
            RequestRoute::GetZombieTimerCount(payload) => {
                self.get_zombie_timer_count(&request, payload)
            }
            RequestRoute::Execute(payload) => self.execute(&request, payload),
            RequestRoute::WriteStdin(payload) => self.write_stdin(&request, payload),
            RequestRoute::ResizePty(payload) => {
                // The converged browser path resizes the PTY through the driver's
                // master-side resize, not via a native wire ResizePty op, so this
                // route is not exercised by the in-browser terminal.
                let _ = payload;
                rejected(
                    &request,
                    "unsupported",
                    "resize_pty is handled by the converged browser driver, not the native wire op",
                )
            }
            RequestRoute::CloseStdin(payload) => self.close_stdin(&request, payload),
            RequestRoute::KillProcess(payload) => self.kill_process(&request, payload),
            RequestRoute::FindListener(payload) => self.find_listener(&request, payload),
            RequestRoute::FindBoundUdp(payload) => self.find_bound_udp(&request, payload),
            RequestRoute::VmFetch(payload) => self.vm_fetch(&request, payload),
            RequestRoute::Ext(payload) => self.ext(&request, payload),
            RequestRoute::LinkPackage(payload) => {
                // Package linking projects host-filesystem package trees into the
                // VM, which the converged browser runtime does not provide.
                let _ = payload;
                rejected(
                    &request,
                    "unsupported",
                    "link_package is not available in the converged browser runtime",
                )
            }
            RequestRoute::ProvidedCommands(payload) => {
                // Provided command metadata is retained by the native sidecar's
                // host-backed package projection, which the converged browser
                // runtime does not provide.
                let _ = payload;
                rejected(
                    &request,
                    "unsupported",
                    "provided_commands is not available in the converged browser runtime",
                )
            }
            RequestRoute::ScheduleCron(payload) => self.schedule_cron(&request, payload),
            RequestRoute::ListCronJobs(_) => self.list_cron_jobs(&request),
            RequestRoute::CancelCronJob(payload) => self.cancel_cron_job(&request, payload),
            RequestRoute::WakeCron(payload) => self.wake_cron(&request, payload),
            RequestRoute::CompleteCronRun(payload) => self.complete_cron_run(&request, payload),
            RequestRoute::ExportCronState(_) => self.export_cron_state(&request),
            RequestRoute::ImportCronState(payload) => self.import_cron_state(&request, payload),
            RequestRoute::UnsupportedHostCallbackDirection => {
                unsupported_host_callback_direction_dispatch(&request)
            }
        }
    }

    fn bootstrap_root_filesystem(
        &mut self,
        request: &RequestFrame,
        payload: BootstrapRootFilesystemRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "bootstrap_root_filesystem requires VM ownership",
            );
        };
        let entry_count = match self
            .sidecar
            .bootstrap_root_filesystem_entries(&vm_id, &payload.entries)
        {
            Ok(entry_count) => entry_count,
            Err(error) => return rejected(request, "bootstrap_root_failed", &error.to_string()),
        };
        DispatchResult {
            response: root_filesystem_bootstrapped_response(request, entry_count),
            events: Vec::new(),
        }
    }

    fn schedule_cron(
        &mut self,
        request: &RequestFrame,
        payload: ScheduleCronRequest,
    ) -> DispatchResult {
        let Some(vm_id) = self.cron_vm_id(request) else {
            return rejected(
                request,
                "invalid_ownership",
                "schedule_cron requires an active VM",
            );
        };
        let response = match self
            .cron_schedulers
            .entry(vm_id)
            .or_default()
            .schedule(payload, unix_time_ms())
        {
            Ok(response) => response,
            Err(error) => return rejected(request, "cron_schedule_failed", &error.to_string()),
        };
        DispatchResult {
            response: respond(request, ResponsePayload::CronScheduled(response)),
            events: Vec::new(),
        }
    }

    fn list_cron_jobs(&mut self, request: &RequestFrame) -> DispatchResult {
        let Some(vm_id) = self.cron_vm_id(request) else {
            return rejected(
                request,
                "invalid_ownership",
                "list_cron_jobs requires an active VM",
            );
        };
        let response = self.cron_schedulers.entry(vm_id).or_default().list();
        DispatchResult {
            response: respond(request, ResponsePayload::CronJobs(response)),
            events: Vec::new(),
        }
    }

    fn cancel_cron_job(
        &mut self,
        request: &RequestFrame,
        payload: CancelCronJobRequest,
    ) -> DispatchResult {
        let Some(vm_id) = self.cron_vm_id(request) else {
            return rejected(
                request,
                "invalid_ownership",
                "cancel_cron_job requires an active VM",
            );
        };
        let response = match self
            .cron_schedulers
            .entry(vm_id)
            .or_default()
            .cancel(payload)
        {
            Ok(response) => response,
            Err(error) => return rejected(request, "cron_cancel_failed", &error.to_string()),
        };
        DispatchResult {
            response: respond(request, ResponsePayload::CronCancelled(response)),
            events: Vec::new(),
        }
    }

    fn wake_cron(&mut self, request: &RequestFrame, payload: WakeCronRequest) -> DispatchResult {
        let Some(vm_id) = self.cron_vm_id(request) else {
            return rejected(
                request,
                "invalid_ownership",
                "wake_cron requires an active VM",
            );
        };
        let mut response = match self
            .cron_schedulers
            .entry(vm_id.clone())
            .or_default()
            .wake(payload, unix_time_ms())
        {
            Ok(response) => response,
            Err(error) => return rejected(request, "cron_wake_failed", &error.to_string()),
        };
        response.runs = self.start_cron_runs(
            request,
            &vm_id,
            response.runs,
            &mut response.alarm,
            &mut response.events,
        );
        DispatchResult {
            response: respond(request, ResponsePayload::CronWake(response)),
            events: Vec::new(),
        }
    }

    fn start_cron_runs(
        &mut self,
        request: &RequestFrame,
        vm_id: &str,
        runs: Vec<CronRun>,
        alarm: &mut CronAlarm,
        events: &mut Vec<CronEventRecord>,
    ) -> Vec<CronRun> {
        let mut pending = VecDeque::from(runs);
        let mut host_runs = Vec::new();
        while let Some(run) = pending.pop_front() {
            let action = match agentos_native_sidecar_core::decode_cron_action(&run.action) {
                Ok(action) => action,
                Err(error) => {
                    self.complete_failed_cron_run(
                        vm_id,
                        run.run_id,
                        run.job_id,
                        error.to_string(),
                        alarm,
                        events,
                        &mut pending,
                    );
                    continue;
                }
            };
            match action {
                CronAction::Callback { .. } => host_runs.push(run),
                CronAction::Session { .. } => self.complete_failed_cron_run(
                    vm_id,
                    run.run_id,
                    run.job_id,
                    String::from(
                        "cron session actions require an ACP adapter with background execution support; this browser ACP adapter does not provide it",
                    ),
                    alarm,
                    events,
                    &mut pending,
                ),
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
                        capture_output: None,
                    };
                    let internal = RequestFrame::new(
                        0,
                        request.ownership.clone(),
                        agentos_sidecar_protocol::protocol::RequestPayload::Execute(
                            payload.clone(),
                        ),
                    );
                    let launch = self.execute(&internal, payload);
                    let launch_error = match launch.response.payload {
                        ResponsePayload::ProcessStarted(_) => None,
                        ResponsePayload::Rejected(rejected) => {
                            Some(format!("{}: {}", rejected.code, rejected.message))
                        }
                        other => Some(format!(
                            "cron exec returned unexpected response: {other:?}"
                        )),
                    };
                    if let Some(error) = launch_error {
                        self.complete_failed_cron_run(
                            vm_id,
                            run.run_id,
                            run.job_id,
                            error,
                            alarm,
                            events,
                            &mut pending,
                        );
                    } else {
                        self.cron_process_runs.insert(
                            (vm_id.to_string(), process_id),
                            run.run_id,
                        );
                    }
                }
            }
        }
        host_runs
    }

    fn complete_failed_cron_run(
        &mut self,
        vm_id: &str,
        run_id: String,
        job_id: String,
        error: String,
        alarm: &mut CronAlarm,
        events: &mut Vec<CronEventRecord>,
        pending: &mut VecDeque<CronRun>,
    ) {
        let completion = self
            .cron_schedulers
            .entry(vm_id.to_string())
            .or_default()
            .complete(
                CompleteCronRunRequest {
                    run_id: run_id.clone(),
                    error: Some(error.clone()),
                },
                unix_time_ms(),
            );
        let completion = match completion {
            Ok(completion) => completion,
            Err(completion_error) => {
                events.push(CronEventRecord {
                    kind: CronEventKind::Error,
                    job_id,
                    time_ms: unix_time_ms(),
                    duration_ms: None,
                    error: Some(format!(
                        "sidecar could not complete cron run {run_id}: {completion_error}; original error: {error}"
                    )),
                });
                return;
            }
        };
        *alarm = completion.alarm;
        events.extend(completion.events);
        pending.extend(completion.runs);
    }

    fn complete_cron_run(
        &mut self,
        request: &RequestFrame,
        payload: CompleteCronRunRequest,
    ) -> DispatchResult {
        let Some(vm_id) = self.cron_vm_id(request) else {
            return rejected(
                request,
                "invalid_ownership",
                "complete_cron_run requires an active VM",
            );
        };
        let response = match self
            .cron_schedulers
            .entry(vm_id)
            .or_default()
            .complete(payload, unix_time_ms())
        {
            Ok(response) => response,
            Err(error) => return rejected(request, "cron_complete_failed", &error.to_string()),
        };
        DispatchResult {
            response: respond(request, ResponsePayload::CronRunCompleted(response)),
            events: Vec::new(),
        }
    }

    fn export_cron_state(&mut self, request: &RequestFrame) -> DispatchResult {
        let Some(vm_id) = self.cron_vm_id(request) else {
            return rejected(
                request,
                "invalid_ownership",
                "export_cron_state requires an active VM",
            );
        };
        let state = match self
            .cron_schedulers
            .entry(vm_id)
            .or_default()
            .export_state()
        {
            Ok(state) => state,
            Err(error) => return rejected(request, "cron_export_failed", &error.to_string()),
        };
        DispatchResult {
            response: respond(
                request,
                ResponsePayload::CronStateExported(
                    agentos_sidecar_protocol::protocol::CronStateExportedResponse { state },
                ),
            ),
            events: Vec::new(),
        }
    }

    fn import_cron_state(
        &mut self,
        request: &RequestFrame,
        payload: ImportCronStateRequest,
    ) -> DispatchResult {
        let Some(vm_id) = self.cron_vm_id(request) else {
            return rejected(
                request,
                "invalid_ownership",
                "import_cron_state requires an active VM",
            );
        };
        let response = match self
            .cron_schedulers
            .entry(vm_id)
            .or_default()
            .import_state(&payload.state)
        {
            Ok(response) => response,
            Err(error) => return rejected(request, "cron_import_failed", &error.to_string()),
        };
        DispatchResult {
            response: respond(request, ResponsePayload::CronStateImported(response)),
            events: Vec::new(),
        }
    }

    fn cron_vm_id(&self, request: &RequestFrame) -> Option<String> {
        vm_id_of(&request.ownership).filter(|vm_id| self.active_vms.contains(vm_id))
    }

    fn configure_vm(
        &mut self,
        request: &RequestFrame,
        payload: ConfigureVmRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "configure_vm requires VM ownership",
            );
        };
        if payload
            .mounts
            .as_ref()
            .is_some_and(|mounts| !mounts.is_empty())
        {
            return rejected(
                request,
                "unsupported_request",
                "browser ConfigureVm does not support host mounts",
            );
        }
        if payload
            .packages
            .as_ref()
            .is_some_and(|packages| !packages.is_empty())
            || payload.packages_mount_at.is_some()
        {
            return rejected(
                request,
                "unsupported_request",
                "browser ConfigureVm does not support package projection",
            );
        }

        let permissions = match payload.permissions {
            Some(policy) => {
                let policy = permissions_with_allow_all_defaults(Some(
                    agentos_sidecar_protocol::wire::permissions_policy_config_from_wire(policy),
                ));
                if let Err(error) =
                    agentos_native_sidecar_core::validate_permissions_policy(&policy)
                {
                    return rejected(request, "invalid_config", &error.to_string());
                }
                Some(permissions_from_policy(policy))
            }
            None => None,
        };
        if let Err(error) = self.sidecar.configure_vm(
            &vm_id,
            permissions,
            payload
                .command_permissions
                .map(|permissions| permissions.into_iter().collect()),
            payload.loopback_exempt_ports,
        ) {
            return rejected(request, "configure_vm_failed", &error.to_string());
        }
        DispatchResult {
            response: vm_configured_response(request, 0, Vec::new(), Vec::new()),
            events: Vec::new(),
        }
    }

    fn register_host_callbacks(
        &mut self,
        request: &RequestFrame,
        payload: RegisterHostCallbacksRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "register_host_callbacks requires VM ownership",
            );
        };
        let (registration, command_count) =
            match self.sidecar.register_host_callbacks(&vm_id, payload) {
                Ok(result) => result,
                Err(error) => {
                    return rejected(
                        request,
                        "register_host_callbacks_failed",
                        &error.to_string(),
                    )
                }
            };
        DispatchResult {
            response: respond(
                request,
                ResponsePayload::HostCallbacksRegistered(HostCallbacksRegisteredResponse {
                    registration,
                    command_count,
                }),
            ),
            events: Vec::new(),
        }
    }

    fn guest_filesystem_call(
        &mut self,
        request: &RequestFrame,
        payload: agentos_sidecar_protocol::protocol::GuestFilesystemCallRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "guest_filesystem_call requires VM ownership",
            );
        };
        let result = match self.sidecar.guest_filesystem_call(&vm_id, payload) {
            Ok(result) => result,
            Err(error) => return rejected(request, "guest_filesystem_failed", &error.to_string()),
        };
        DispatchResult {
            response: respond(request, ResponsePayload::GuestFilesystemResult(result)),
            events: Vec::new(),
        }
    }

    fn guest_kernel_call(
        &mut self,
        request: &RequestFrame,
        payload: agentos_sidecar_protocol::protocol::GuestKernelCallRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "guest_kernel_call requires VM ownership",
            );
        };
        let result = match self.sidecar.guest_kernel_call(&vm_id, payload) {
            Ok(result) => result,
            Err(error) => return rejected(request, "guest_kernel_failed", &error.to_string()),
        };
        DispatchResult {
            response: respond(request, ResponsePayload::GuestKernelResult(result)),
            events: Vec::new(),
        }
    }

    fn create_layer(
        &mut self,
        request: &RequestFrame,
        _payload: CreateLayerRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "create_layer requires VM ownership",
            );
        };
        let layer_id = match self.sidecar.create_layer(&vm_id) {
            Ok(layer_id) => layer_id,
            Err(error) => return rejected(request, "create_layer_failed", &error.to_string()),
        };
        DispatchResult {
            response: layer_created_response(request, layer_id),
            events: Vec::new(),
        }
    }

    fn seal_layer(&mut self, request: &RequestFrame, payload: SealLayerRequest) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "seal_layer requires VM ownership",
            );
        };
        let layer_id = match self.sidecar.seal_layer(&vm_id, &payload.layer_id) {
            Ok(layer_id) => layer_id,
            Err(error) => return rejected(request, "seal_layer_failed", &error.to_string()),
        };
        DispatchResult {
            response: layer_sealed_response(request, layer_id),
            events: Vec::new(),
        }
    }

    fn import_snapshot(
        &mut self,
        request: &RequestFrame,
        payload: ImportSnapshotRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "import_snapshot requires VM ownership",
            );
        };
        let layer_id = match self.sidecar.import_snapshot(&vm_id, &payload.entries) {
            Ok(layer_id) => layer_id,
            Err(error) => return rejected(request, "import_snapshot_failed", &error.to_string()),
        };
        DispatchResult {
            response: snapshot_imported_response(request, layer_id),
            events: Vec::new(),
        }
    }

    fn export_snapshot(
        &mut self,
        request: &RequestFrame,
        payload: ExportSnapshotRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "export_snapshot requires VM ownership",
            );
        };
        let snapshot = match self.sidecar.export_snapshot(&vm_id, &payload.layer_id) {
            Ok(snapshot) => snapshot,
            Err(error) => return rejected(request, "export_snapshot_failed", &error.to_string()),
        };
        DispatchResult {
            response: snapshot_exported_response(
                request,
                payload.layer_id,
                snapshot.entries.iter().map(root_snapshot_entry).collect(),
            ),
            events: Vec::new(),
        }
    }

    fn create_overlay(
        &mut self,
        request: &RequestFrame,
        payload: CreateOverlayRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "create_overlay requires VM ownership",
            );
        };
        let mode = protocol_root_filesystem_mode(payload.mode);
        let layer_id = match self.sidecar.create_overlay(
            &vm_id,
            mode,
            payload.upper_layer_id,
            payload.lower_layer_ids,
        ) {
            Ok(layer_id) => layer_id,
            Err(error) => return rejected(request, "create_overlay_failed", &error.to_string()),
        };
        DispatchResult {
            response: overlay_created_response(request, layer_id),
            events: Vec::new(),
        }
    }

    fn snapshot_root_filesystem(
        &mut self,
        request: &RequestFrame,
        _payload: SnapshotRootFilesystemRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "snapshot_root_filesystem requires VM ownership",
            );
        };
        let snapshot = match self.sidecar.snapshot_root_filesystem(&vm_id) {
            Ok(snapshot) => snapshot,
            Err(error) => return rejected(request, "snapshot_root_failed", &error.to_string()),
        };
        DispatchResult {
            response: root_filesystem_snapshot_response(
                request,
                snapshot.entries.iter().map(root_snapshot_entry).collect(),
            ),
            events: Vec::new(),
        }
    }

    fn get_process_snapshot(
        &mut self,
        request: &RequestFrame,
        _payload: GetProcessSnapshotRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "get_process_snapshot requires VM ownership",
            );
        };
        let mut processes = match self.sidecar.process_snapshot_entries(&vm_id) {
            Ok(processes) => processes,
            Err(error) => return rejected(request, "process_snapshot_failed", &error.to_string()),
        };
        for process in &mut processes {
            if let Some(record) = self.executions.get(&process.process_id) {
                process.process_id = record.process_id.clone();
            }
        }
        DispatchResult {
            response: process_snapshot_response(
                request,
                processes
                    .into_iter()
                    .map(protocol_process_snapshot_entry)
                    .collect(),
            ),
            events: Vec::new(),
        }
    }

    fn get_signal_state(
        &mut self,
        request: &RequestFrame,
        payload: GetSignalStateRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "get_signal_state requires VM ownership",
            );
        };
        let Some(execution_id) = self.execution_id_for(&vm_id, &payload.process_id) else {
            return rejected(
                request,
                "unknown_process",
                "get_signal_state process is not active",
            );
        };
        let handlers = match self.sidecar.signal_state(&vm_id, &execution_id) {
            Ok(handlers) => handlers,
            Err(error) => return rejected(request, "signal_state_failed", &error.to_string()),
        };
        DispatchResult {
            response: signal_state_response(request, payload.process_id, handlers),
            events: Vec::new(),
        }
    }

    fn get_zombie_timer_count(
        &mut self,
        request: &RequestFrame,
        _payload: GetZombieTimerCountRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "get_zombie_timer_count requires VM ownership",
            );
        };
        let count = match self.sidecar.zombie_timer_count(&vm_id) {
            Ok(count) => count,
            Err(error) => {
                return rejected(request, "zombie_timer_count_failed", &error.to_string())
            }
        };
        DispatchResult {
            response: zombie_timer_count_response(request, count),
            events: Vec::new(),
        }
    }

    fn find_listener(
        &mut self,
        request: &RequestFrame,
        payload: FindListenerRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "find_listener requires VM ownership",
            );
        };
        let listener = match self.sidecar.find_listener(&vm_id, &payload) {
            Ok(listener) => listener,
            Err(error) => return rejected(request, "find_listener_failed", &error.to_string()),
        }
        .map(|entry| self.client_socket_state_entry(entry));
        DispatchResult {
            response: listener_snapshot_response(request, listener),
            events: Vec::new(),
        }
    }

    fn find_bound_udp(
        &mut self,
        request: &RequestFrame,
        payload: FindBoundUdpRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "find_bound_udp requires VM ownership",
            );
        };
        let socket = match self.sidecar.find_bound_udp(&vm_id, &payload) {
            Ok(socket) => socket,
            Err(error) => return rejected(request, "find_bound_udp_failed", &error.to_string()),
        }
        .map(|entry| self.client_socket_state_entry(entry));
        DispatchResult {
            response: bound_udp_snapshot_response(request, socket),
            events: Vec::new(),
        }
    }

    fn vm_fetch(&mut self, request: &RequestFrame, payload: VmFetchRequest) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "vm.fetch requires VM ownership",
            );
        };
        if let Err(error) = serde_json::from_str::<serde_json::Value>(&payload.headers_json) {
            return rejected(
                request,
                "invalid_request",
                &format!("vm.fetch headers_json must be valid JSON: {error}"),
            );
        }

        let response_json = match self.sidecar.vm_fetch(&vm_id, &payload) {
            Ok(response_json) => response_json,
            Err(error) => return rejected(request, "vm_fetch_failed", &error.to_string()),
        };
        DispatchResult {
            response: respond(
                request,
                ResponsePayload::VmFetchResult(VmFetchResponse { response_json }),
            ),
            events: Vec::new(),
        }
    }

    fn authenticate(
        &mut self,
        request: &RequestFrame,
        payload: AuthenticateRequest,
    ) -> DispatchResult {
        if let Err(error) = validate_authenticate_versions(&payload) {
            return rejected(request, error.code(), error.message());
        }

        self.next_connection += 1;
        let connection_id = format!("browser-connection-{}", self.next_connection);
        DispatchResult {
            response: authenticated_response(
                request.request_id,
                BROWSER_SIDECAR_ID,
                connection_id,
                BROWSER_MAX_FRAME_BYTES as u32,
            ),
            events: Vec::new(),
        }
    }

    fn client_socket_state_entry(&self, mut entry: SocketStateEntry) -> SocketStateEntry {
        if let Some(record) = self.executions.get(&entry.process_id) {
            entry.process_id = record.process_id.clone();
        }
        entry
    }

    fn open_session(
        &mut self,
        request: &RequestFrame,
        _payload: OpenSessionRequest,
    ) -> DispatchResult {
        let Some(connection_id) = connection_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "open_session requires connection ownership",
            );
        };
        self.next_session += 1;
        let session_id = format!("browser-session-{}", self.next_session);
        DispatchResult {
            response: session_opened_response(request.request_id, connection_id, session_id),
            events: Vec::new(),
        }
    }

    fn create_vm(&mut self, request: &RequestFrame, payload: CreateVmRequest) -> DispatchResult {
        let Some((connection_id, session_id)) = session_scope_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "create_vm requires session ownership",
            );
        };
        let create_config: CreateVmConfig = match serde_json::from_str(&payload.config) {
            Ok(config) => config,
            Err(error) => {
                return rejected(
                    request,
                    "invalid_config",
                    &format!("invalid create VM config JSON: {error}"),
                );
            }
        };
        if let Err(error) = create_config.validate(BROWSER_MAX_FRAME_BYTES) {
            return rejected(
                request,
                "invalid_config",
                &format!("invalid create VM config: {error}"),
            );
        }

        self.next_vm += 1;
        let vm_id = format!("vm-{}", self.next_vm);
        let mut kernel_config = KernelVmConfig::new(vm_id.clone());
        kernel_config.env =
            guest_environment_with_overrides(&create_config.env.clone().unwrap_or_default());
        if let Some(cwd) = create_config.cwd.clone() {
            kernel_config.cwd = cwd;
        }
        kernel_config.loopback_exempt_ports = create_config
            .loopback_exempt_ports
            .as_deref()
            .unwrap_or_default()
            .iter()
            .copied()
            .collect();
        let limits = match agentos_native_sidecar_core::vm_limits_from_config(
            create_config.limits.as_ref(),
            BROWSER_MAX_FRAME_BYTES,
        ) {
            Ok(limits) => limits,
            Err(error) => {
                return rejected(request, "invalid_config", &error.to_string());
            }
        };
        kernel_config.resources = limits.resources.clone();
        let permissions = permissions_with_allow_all_defaults(create_config.permissions.clone());
        if let Err(error) = agentos_native_sidecar_core::validate_permissions_policy(&permissions) {
            return rejected(request, "invalid_config", &error.to_string());
        }
        kernel_config.permissions = permissions_from_policy(permissions);

        let guest_cwd = kernel_config.cwd.clone();
        let guest_env = kernel_config.env.clone().into_iter().collect();

        if let Err(error) = self.sidecar.create_vm_with_root_filesystem(
            kernel_config,
            create_config.root_filesystem.unwrap_or_default(),
        ) {
            return rejected(request, "create_vm_failed", &error.to_string());
        }
        if let Err(error) = self
            .sidecar
            .set_agent_additional_instructions(&vm_id, create_config.agent_additional_instructions)
        {
            let _ = self.sidecar.dispose_vm(&vm_id);
            return rejected(request, "create_vm_failed", &error.to_string());
        }
        self.active_vms.insert(vm_id.clone());
        self.vm_capture_budgets
            .insert(vm_id.clone(), CapturedOutputBudget::for_vm(&limits));
        self.vm_limits.insert(vm_id.clone(), limits);

        let ownership = OwnershipScope::vm(&connection_id, &session_id, &vm_id);
        DispatchResult {
            response: vm_created_response(request, vm_id.clone(), guest_cwd, guest_env),
            events: vec![
                vm_lifecycle_event(
                    &connection_id,
                    &session_id,
                    &vm_id,
                    VmLifecycleState::Creating,
                ),
                EventFrame::new(
                    ownership,
                    agentos_sidecar_protocol::protocol::EventPayload::VmLifecycle(
                        agentos_sidecar_protocol::protocol::VmLifecycleEvent {
                            state: VmLifecycleState::Ready,
                        },
                    ),
                ),
            ],
        }
    }

    fn initialize_vm(
        &mut self,
        request: &RequestFrame,
        payload: agentos_sidecar_protocol::protocol::InitializeVmRequest,
    ) -> DispatchResult {
        let Some((connection_id, session_id)) = session_scope_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "initialize_vm requires session ownership",
            );
        };
        let created_dispatch = self.create_vm(
            request,
            CreateVmRequest {
                runtime: payload.runtime,
                config: payload.config,
            },
        );
        let ResponsePayload::VmCreated(created) = created_dispatch.response.payload.clone() else {
            return created_dispatch;
        };
        let vm_id = created.vm_id.clone();
        let ownership = OwnershipScope::vm(&connection_id, &session_id, &vm_id);
        let configure_payload = ConfigureVmRequest {
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
            payload: agentos_sidecar_protocol::protocol::RequestPayload::ConfigureVm(
                configure_payload.clone(),
            ),
        };
        let configured_dispatch = self.configure_vm(&configure_request, configure_payload);
        let ResponsePayload::VmConfigured(configured) =
            configured_dispatch.response.payload.clone()
        else {
            let message = match configured_dispatch.response.payload {
                ResponsePayload::Rejected(rejected) => rejected.message,
                _ => String::from("initialize_vm configure step returned an unexpected response"),
            };
            self.cleanup_failed_initialization(&vm_id);
            return rejected(request, "initialize_vm_failed", &message);
        };

        let registrations = payload.host_callbacks.unwrap_or_default();
        let mut host_callbacks = Vec::with_capacity(registrations.len());
        for registration in registrations {
            let registration_request = RequestFrame {
                schema: request.schema.clone(),
                request_id: request.request_id,
                ownership: ownership.clone(),
                payload: agentos_sidecar_protocol::protocol::RequestPayload::RegisterHostCallbacks(
                    registration.clone(),
                ),
            };
            let registered_dispatch =
                self.register_host_callbacks(&registration_request, registration);
            match registered_dispatch.response.payload {
                ResponsePayload::HostCallbacksRegistered(registered) => {
                    host_callbacks.push(registered)
                }
                ResponsePayload::Rejected(rejected_response) => {
                    self.cleanup_failed_initialization(&vm_id);
                    return rejected(request, "initialize_vm_failed", &rejected_response.message);
                }
                _ => {
                    self.cleanup_failed_initialization(&vm_id);
                    return rejected(
                        request,
                        "initialize_vm_failed",
                        "initialize_vm host-callback step returned an unexpected response",
                    );
                }
            }
        }

        DispatchResult {
            response: respond(
                request,
                ResponsePayload::VmInitialized(
                    agentos_sidecar_protocol::protocol::VmInitializedResponse {
                        vm_id,
                        guest_cwd: created.guest_cwd,
                        guest_env: created.guest_env,
                        applied_mounts: configured.applied_mounts,
                        projected_commands: configured.projected_commands,
                        agents: configured.agents,
                        host_callbacks,
                    },
                ),
            ),
            events: created_dispatch.events,
        }
    }

    fn cleanup_failed_initialization(&mut self, vm_id: &str) {
        if let Err(error) = self.sidecar.dispose_vm(vm_id) {
            eprintln!("failed to clean up partially initialized browser VM {vm_id}: {error}");
        }
        self.purge_vm_state(vm_id);
    }

    fn purge_vm_state(&mut self, vm_id: &str) {
        self.active_vms.remove(vm_id);
        self.vm_limits.remove(vm_id);
        self.vm_capture_budgets.remove(vm_id);
        self.cron_schedulers.remove(vm_id);
        self.cron_process_runs
            .retain(|(process_vm_id, _), _| process_vm_id != vm_id);
        self.executions.retain(|_, record| record.vm_id != vm_id);
        self.process_executions
            .retain(|(process_vm_id, _), _| process_vm_id != vm_id);
        self.pending_events
            .retain(|event| vm_id_of(&event.ownership).as_deref() != Some(vm_id));
    }

    fn dispose_vm(&mut self, request: &RequestFrame, _payload: DisposeVmRequest) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "dispose_vm requires VM ownership",
            );
        };
        let dispose_result = self.sidecar.dispose_vm(&vm_id);
        self.purge_vm_state(&vm_id);
        if let Err(error) = dispose_result {
            return rejected(request, "dispose_vm_failed", &error.to_string());
        }
        DispatchResult {
            response: vm_disposed_response(request, vm_id),
            events: Vec::new(),
        }
    }

    fn ext(&mut self, request: &RequestFrame, payload: ExtEnvelope) -> DispatchResult {
        let response = match self
            .sidecar
            .dispatch_extension_request(BrowserExtensionRequest {
                namespace: payload.namespace,
                payload: payload.payload,
                vm_id: vm_id_of(&request.ownership),
                connection_id: connection_id_of(&request.ownership),
            }) {
            Ok(response) => response,
            Err(error) => return rejected(request, "extension_failed", &error.to_string()),
        };
        DispatchResult {
            response: respond(
                request,
                ResponsePayload::ExtResult(ExtEnvelope {
                    namespace: response.namespace,
                    payload: response.payload,
                }),
            ),
            events: Vec::new(),
        }
    }

    fn execute(&mut self, request: &RequestFrame, mut payload: ExecuteRequest) -> DispatchResult {
        agentos_native_sidecar_core::apply_execute_defaults(&mut payload);
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "execute requires VM ownership",
            );
        };
        if let Some(shell_command) = payload.shell_command.take() {
            if payload.command.is_some()
                || payload.runtime.is_some()
                || payload.entrypoint.is_some()
            {
                return rejected(
                    request,
                    "invalid_request",
                    "execute shellCommand cannot be combined with command, runtime, or entrypoint",
                );
            }
            let Some(resolved) = resolve_command_line(&shell_command) else {
                return rejected(
                    request,
                    "invalid_request",
                    "execute shellCommand must not be empty",
                );
            };
            payload.command = Some(resolved.command);
            payload.args = resolved.args;
        }
        if payload.pty.is_some() {
            return rejected(
                request,
                "unsupported",
                "PTY execution is not supported by the browser sidecar",
            );
        }
        if payload.timeout_ms.is_some() {
            return rejected(
                request,
                "unsupported",
                "execution timeouts are not supported by the browser sidecar",
            );
        }
        let process_id = match payload.process_id.take() {
            Some(process_id) => process_id,
            None => loop {
                let Some(next_process) = self.next_process.checked_add(1) else {
                    return rejected(
                        request,
                        "process_id_exhausted",
                        "sidecar process id space exhausted",
                    );
                };
                self.next_process = next_process;
                let candidate = format!("sidecar-process-{next_process}");
                if !self
                    .process_executions
                    .contains_key(&(vm_id.clone(), candidate.clone()))
                {
                    break candidate;
                }
            },
        };
        if let Err(error) = validate_process_id(&process_id) {
            return rejected(request, "invalid_request", &error.to_string());
        }
        let process_key = (vm_id.clone(), process_id.clone());
        if self.process_executions.contains_key(&process_key) {
            return rejected(
                request,
                "process_already_active",
                "process_id is already active",
            );
        }
        let requested_runtime = payload
            .runtime
            .clone()
            .unwrap_or(GuestRuntimeKind::JavaScript);
        let runtime = match requested_runtime {
            GuestRuntimeKind::JavaScript | GuestRuntimeKind::Python => GuestRuntime::JavaScript,
            GuestRuntimeKind::WebAssembly => GuestRuntime::WebAssembly,
        };
        let context = match runtime {
            GuestRuntime::JavaScript => {
                self.sidecar
                    .create_javascript_context(CreateJavascriptContextRequest {
                        vm_id: vm_id.clone(),
                        bootstrap_module: payload.entrypoint.clone(),
                    })
            }
            GuestRuntime::WebAssembly => {
                self.sidecar.create_wasm_context(CreateWasmContextRequest {
                    vm_id: vm_id.clone(),
                    module_path: payload.entrypoint.clone(),
                })
            }
        };
        let context = match context {
            Ok(context) => context,
            Err(error) => return rejected(request, "execute_failed", &error.to_string()),
        };

        let mut argv = Vec::new();
        if let Some(command) = payload.command.clone() {
            argv.push(command);
        }
        argv.extend(payload.args.clone());
        let guest_cwd = match payload.cwd.clone() {
            Some(cwd) => cwd,
            None => match self.sidecar.guest_cwd(&vm_id) {
                Ok(cwd) => cwd,
                Err(error) => return rejected(request, "execute_failed", &error.to_string()),
            },
        };
        let started = match self.sidecar.start_execution_with_options(
            StartExecutionRequest {
                vm_id: vm_id.clone(),
                context_id: context.context_id,
                argv,
                env: payload
                    .env
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
                cwd: guest_cwd,
            },
            BrowserExecutionOptions {
                command_name: payload.command.clone(),
                wasm_permission_tier: payload.wasm_permission_tier,
            },
        ) {
            Ok(started) => started,
            Err(error) => return rejected(request, "execute_failed", &error.to_string()),
        };

        let captured_output = payload.capture_output.unwrap_or(false).then(|| {
            CapturedOutputState::for_runtime(
                self.vm_limits
                    .get(&vm_id)
                    .expect("active browser VM must retain its limits"),
                payload
                    .runtime
                    .clone()
                    .unwrap_or(GuestRuntimeKind::JavaScript),
                Arc::clone(
                    self.vm_capture_budgets
                        .get(&vm_id)
                        .expect("active browser VM must retain its capture budget"),
                ),
            )
        });
        self.executions.insert(
            started.execution_id.clone(),
            ExecutionRecord {
                vm_id: vm_id.clone(),
                process_id: process_id.clone(),
                ownership: request.ownership.clone(),
                captured_output,
            },
        );
        self.process_executions
            .insert(process_key, started.execution_id.clone());
        DispatchResult {
            response: process_started_response(request, process_id, None),
            events: Vec::new(),
        }
    }

    fn write_stdin(
        &mut self,
        request: &RequestFrame,
        payload: WriteStdinRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "write_stdin requires VM ownership",
            );
        };
        let Some(execution_id) = self.execution_id_for(&vm_id, &payload.process_id) else {
            return rejected(
                request,
                "unknown_process",
                "write_stdin process is not active",
            );
        };
        let accepted_bytes = payload.chunk.len() as u64;
        if let Err(error) = self.sidecar.write_stdin(WriteExecutionStdinRequest {
            vm_id,
            execution_id,
            chunk: payload.chunk,
        }) {
            return rejected(request, "write_stdin_failed", &error.to_string());
        }
        DispatchResult {
            response: stdin_written_response(request, payload.process_id, accepted_bytes),
            events: Vec::new(),
        }
    }

    fn close_stdin(
        &mut self,
        request: &RequestFrame,
        payload: CloseStdinRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "close_stdin requires VM ownership",
            );
        };
        let Some(execution_id) = self.execution_id_for(&vm_id, &payload.process_id) else {
            return rejected(
                request,
                "unknown_process",
                "close_stdin process is not active",
            );
        };
        if let Err(error) = self.sidecar.close_stdin(ExecutionHandleRequest {
            vm_id,
            execution_id,
        }) {
            return rejected(request, "close_stdin_failed", &error.to_string());
        }
        DispatchResult {
            response: stdin_closed_response(request, payload.process_id),
            events: Vec::new(),
        }
    }

    fn kill_process(
        &mut self,
        request: &RequestFrame,
        payload: KillProcessRequest,
    ) -> DispatchResult {
        let Some(vm_id) = vm_id_of(&request.ownership) else {
            return rejected(
                request,
                "invalid_ownership",
                "kill_process requires VM ownership",
            );
        };
        let Some(execution_id) = self.execution_id_for(&vm_id, &payload.process_id) else {
            return DispatchResult {
                response: process_killed_response(request, payload.process_id),
                events: Vec::new(),
            };
        };
        let signal = match agentos_native_sidecar_core::parse_posix_signal(&payload.signal) {
            Some(signal) => signal,
            None => {
                return rejected(
                    request,
                    "kill_process_failed",
                    &format!("unsupported kill_process signal {}", payload.signal),
                );
            }
        };
        if signal == 0 {
            return DispatchResult {
                response: process_killed_response(request, payload.process_id),
                events: Vec::new(),
            };
        }
        if let Err(error) =
            self.sidecar
                .signal_execution_kernel_process(&vm_id, &execution_id, signal)
        {
            return rejected(request, "kill_process_failed", &error.to_string());
        }
        if let Some(bridge_signal) = execution_signal_from_number(signal) {
            if let Err(error) = self
                .sidecar
                .bridge_mut()
                .kill_execution(KillExecutionRequest {
                    vm_id,
                    execution_id,
                    signal: bridge_signal,
                })
            {
                return rejected(request, "kill_process_failed", &format!("{error:?}"));
            }
        }
        DispatchResult {
            response: process_killed_response(request, payload.process_id),
            events: Vec::new(),
        }
    }

    fn poll_one_execution_event(&mut self) -> Result<Option<EventFrame>, ProtocolCodecError> {
        for vm_id in self.active_vms.iter().cloned().collect::<Vec<_>>() {
            loop {
                match self
                    .sidecar
                    .poll_execution_event(PollExecutionEventRequest {
                        vm_id: vm_id.clone(),
                    }) {
                    Ok(Some(event)) => {
                        if let Some(frame) = self.execution_event_to_frame(event) {
                            return Ok(Some(frame));
                        }
                        // Suppressed capture chunks and internal cron events are not public frames.
                        // Keep draining the bridge's already-bounded queue so `None` means every VM
                        // was actually empty, never merely that an internal event was consumed.
                    }
                    Ok(None) => break,
                    Err(error) => {
                        return Err(ProtocolCodecError::SerializeFailure(format!(
                            "browser sidecar failed to poll an execution event: {error:?}"
                        )));
                    }
                }
            }
        }
        Ok(None)
    }

    fn execution_event_to_frame(&mut self, event: ExecutionEvent) -> Option<EventFrame> {
        match event {
            ExecutionEvent::Stdout(chunk) => {
                let (vm_id, process_id, ownership, outcome) = {
                    let record = self.executions.get_mut(&chunk.execution_id)?;
                    if self
                        .cron_process_runs
                        .contains_key(&(record.vm_id.clone(), record.process_id.clone()))
                    {
                        return None;
                    }
                    let outcome = record
                        .captured_output
                        .as_mut()
                        .map(|capture| {
                            capture.record_chunk(
                                &record.process_id,
                                StreamChannel::Stdout,
                                &chunk.chunk,
                            )
                        })
                        .unwrap_or(CaptureChunkOutcome::Forward);
                    (
                        record.vm_id.clone(),
                        record.process_id.clone(),
                        record.ownership.clone(),
                        outcome,
                    )
                };
                if outcome == CaptureChunkOutcome::LimitExceeded {
                    if let Err(error) =
                        self.sidecar
                            .bridge_mut()
                            .kill_execution(KillExecutionRequest {
                                vm_id,
                                execution_id: chunk.execution_id,
                                signal: agentos_bridge::ExecutionSignal::Kill,
                            })
                    {
                        eprintln!("failed to kill browser execution after captured-output overflow: {error:?}");
                    }
                }
                if outcome != CaptureChunkOutcome::Forward {
                    return None;
                }
                Some(process_output_event(
                    ownership,
                    &process_id,
                    StreamChannel::Stdout,
                    chunk.chunk,
                ))
            }
            ExecutionEvent::Stderr(chunk) => {
                let (vm_id, process_id, ownership, outcome) = {
                    let record = self.executions.get_mut(&chunk.execution_id)?;
                    if self
                        .cron_process_runs
                        .contains_key(&(record.vm_id.clone(), record.process_id.clone()))
                    {
                        return None;
                    }
                    let outcome = record
                        .captured_output
                        .as_mut()
                        .map(|capture| {
                            capture.record_chunk(
                                &record.process_id,
                                StreamChannel::Stderr,
                                &chunk.chunk,
                            )
                        })
                        .unwrap_or(CaptureChunkOutcome::Forward);
                    (
                        record.vm_id.clone(),
                        record.process_id.clone(),
                        record.ownership.clone(),
                        outcome,
                    )
                };
                if outcome == CaptureChunkOutcome::LimitExceeded {
                    if let Err(error) =
                        self.sidecar
                            .bridge_mut()
                            .kill_execution(KillExecutionRequest {
                                vm_id,
                                execution_id: chunk.execution_id,
                                signal: agentos_bridge::ExecutionSignal::Kill,
                            })
                    {
                        eprintln!("failed to kill browser execution after captured-output overflow: {error:?}");
                    }
                }
                if outcome != CaptureChunkOutcome::Forward {
                    return None;
                }
                Some(process_output_event(
                    ownership,
                    &process_id,
                    StreamChannel::Stderr,
                    chunk.chunk,
                ))
            }
            ExecutionEvent::Exited(exited) => {
                let record = self.executions.remove(&exited.execution_id)?;
                self.process_executions
                    .remove(&(record.vm_id.clone(), record.process_id.clone()));
                let process_key = (record.vm_id.clone(), record.process_id.clone());
                let Some(run_id) = self.cron_process_runs.remove(&process_key) else {
                    return Some(process_exited_event_with_result(
                        record.ownership,
                        &record.process_id,
                        exited.exit_code,
                        record.captured_output.map(CapturedOutputState::into_result),
                    ));
                };
                let completion = self
                    .cron_schedulers
                    .entry(record.vm_id.clone())
                    .or_default()
                    .complete(
                        CompleteCronRunRequest {
                            run_id: run_id.clone(),
                            error: (exited.exit_code != 0).then(|| {
                                format!("cron exec exited with status {}", exited.exit_code)
                            }),
                        },
                        unix_time_ms(),
                    );
                let mut completion = match completion {
                    Ok(completion) => completion,
                    Err(error) => {
                        return Some(EventFrame::new(
                            record.ownership,
                            EventPayload::Structured(StructuredEvent {
                                name: String::from("cron_dispatch_error"),
                                detail: HashMap::from([
                                    (String::from("run_id"), run_id),
                                    (String::from("error"), error.to_string()),
                                ]),
                            }),
                        ));
                    }
                };
                let internal = RequestFrame::new(
                    0,
                    record.ownership.clone(),
                    agentos_sidecar_protocol::protocol::RequestPayload::WakeCron(WakeCronRequest {
                        generation: completion.alarm.generation,
                    }),
                );
                completion.runs = self.start_cron_runs(
                    &internal,
                    &record.vm_id,
                    completion.runs,
                    &mut completion.alarm,
                    &mut completion.events,
                );
                Some(EventFrame::new(
                    record.ownership,
                    EventPayload::CronDispatch(CronDispatchEvent {
                        alarm: completion.alarm,
                        runs: completion.runs,
                        events: completion.events,
                    }),
                ))
            }
            ExecutionEvent::GuestRequest(call) => {
                let record = self.executions.get(&call.execution_id)?;
                Some(unsupported_guest_kernel_call_event(
                    record.ownership.clone(),
                    &record.process_id,
                    &call.execution_id,
                    &call.operation,
                    call.payload.len(),
                ))
            }
            ExecutionEvent::SignalState(_) => None,
        }
    }

    fn execution_id_for(&self, vm_id: &str, process_id: &str) -> Option<String> {
        self.process_executions
            .get(&(vm_id.to_string(), process_id.to_string()))
            .cloned()
    }
}

fn rejected(request: &RequestFrame, code: &str, message: &str) -> DispatchResult {
    DispatchResult {
        response: reject(request, code, message),
        events: Vec::new(),
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
